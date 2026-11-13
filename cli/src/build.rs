//! Building bullpen's two programs from its own checkout: the command line and the
//! board (bullpen/board_build.py). `build` (install.sh runs it), `_build_board` (the
//! background build `up` starts), and `up`'s rebuild_if_stale. Python keeps its copy
//! (a twin) for accept's rebuild and the catch-up of a command an old build hands it.
//!
//! bullpen's own checkout (HOME) is one cargo workspace (Cargo.toml): core/, cli/ (the
//! command line, bin/bullpen runs it) and board/. Each program's source has a hash, and
//! its build records the hash it was built from (its stamp). `up`, and an accept in
//! HOME's own repo, start a build in the background when a source no longer matches. A
//! running board sees its program replaced and execs the new one; a command runs the
//! new build from its next call. A build that fails, or one skipped because the source
//! has uncommitted changes, is one news line per source hash, and the old program keeps
//! running.

use crate::args::{usage_error, value, Arg, Args, SUPPRESS};
use crate::jobs::{background, Job};
use crate::news::{news, Opts};
use crate::watcher::restart_watcher;
use bullpen_core::json::{read_obj, write_obj, Obj};
use bullpen_core::paths::{home, self_bin, user_home};
use bullpen_core::proc::{file_hash, git, run_with, which};
use bullpen_core::repo::repo_root;
use bullpen_core::text::{splitlines, strip, upper_first};
use bullpen_core::timeutil::iso_now;
use bullpen_core::{fail, Fail, Repo, Res};
use serde_json::{json, Value};
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::path::{Path, PathBuf};

/// The workspace's files: both programs are built from them.
const SHARED: [&str; 2] = ["Cargo.toml", "Cargo.lock"];

/// One of the workspace's programs: its crates, and where its build goes. What runs is
/// a copy of cargo's output (installed), renamed into place once it's whole, never
/// cargo's own, which it rewrites in place.
#[derive(PartialEq)]
pub struct Program {
    pub name: &'static str,
    package: &'static str,
    pub crates: &'static [&'static str],
    /// "bullpen's command line", "the board"
    pub what: &'static str,
    keeps: &'static str,
    /// the news log's task column and kind for its news
    news_as: (&'static str, &'static str),
}

pub static CLI: Program = Program {
    name: "bullpen",
    package: "bullpen-cli",
    crates: &["core", "cli"],
    what: "bullpen's command line",
    keeps: "Commands keep running the old build",
    news_as: ("bullpen", "build"),
};

pub static BOARD: Program = Program {
    name: "bullpen-board",
    package: "bullpen-board",
    crates: &["core", "board"],
    what: "the board",
    keeps: "Boards keep running the old one",
    news_as: ("board", "board"),
};

/// The cargo workspace's build folder.
pub fn target() -> PathBuf {
    home().join("target")
}

pub fn build_lock() -> PathBuf {
    target().join("build.lock")
}

pub fn build_log() -> PathBuf {
    target().join("build.log")
}

impl Program {
    /// What runs: cargo's output, copied and renamed into place.
    pub fn installed(&self) -> PathBuf {
        target().join(self.name)
    }

    fn cargo_out(&self) -> PathBuf {
        target().join("release").join(self.name)
    }

    /// {"source": the hash it was built from, "failed": a hash whose build failed,
    /// "skipped": one not built because the source had uncommitted changes}
    fn stamp_path(&self) -> PathBuf {
        target().join(format!("{}.json", self.name))
    }

    pub fn stamp(&self) -> Res<Obj> {
        Ok(read_obj(&self.stamp_path())?.unwrap_or_default())
    }

    /// What its source is, relative to HOME: each crate's Cargo.toml and src/, and the workspace's files.
    fn paths(&self) -> Vec<String> {
        let mut out: Vec<String> = SHARED.iter().map(|s| s.to_string()).collect();
        for c in self.crates {
            out.extend([format!("{c}/Cargo.toml"), format!("{c}/src")]);
        }
        out
    }

    fn files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for rel in self.paths() {
            let p = home().join(rel);
            if p.is_dir() {
                let mut found = Vec::new();
                walk(&p, &mut found);
                found.sort();
                out.extend(found);
            } else if p.is_file() {
                out.push(p);
            }
        }
        out
    }

    /// A hash of its source, by path: the same as board_build.py's, so either side's
    /// stamp says what the other built.
    pub fn source_hash(&self) -> Res<String> {
        let h = home();
        let mut parts: Vec<(String, String)> = Vec::new();
        for f in self.files() {
            let rel = f.strip_prefix(&h).unwrap_or(&f).display().to_string();
            parts.push((rel, file_hash(&f)?));
        }
        parts.sort();
        let mut map = Obj::new();
        for (rel, hash) in parts {
            map.insert(rel, Value::String(hash));
        }
        Ok(bullpen_core::sha256::hex(
            bullpen_core::json::dumps_line(&Value::Object(map)).as_bytes(),
        ))
    }

    /// Does HOME have uncommitted changes to its source (new files too)?
    pub fn dirty(&self) -> Res<bool> {
        let mut args = vec!["status", "--porcelain", "--"];
        let paths = self.paths();
        args.extend(paths.iter().map(String::as_str));
        let p = git(home(), &args, false)?;
        Ok(p.ok() && !strip(&p.stdout).is_empty())
    }

    /// Its name to start a sentence with: "The board", "bullpen's command line".
    pub fn subject(&self) -> String {
        if self.what.starts_with("the ") {
            upper_first(self.what)
        } else {
            self.what.into()
        }
    }

    /// Set these keys of its stamp (a key set to "" goes).
    fn set_stamp(&self, key: &str, v: &str) -> Res<()> {
        let mut st = self.stamp()?;
        st.insert(key.into(), Value::String(v.into()));
        st.retain(|_, v| bullpen_core::json::truthy(Some(v)));
        write_obj(&self.stamp_path(), &st)?;
        Ok(())
    }

    /// Its source is neither what it was built from nor a source whose build failed.
    pub fn stale(&self) -> Res<bool> {
        let st = self.stamp()?;
        let src = Value::String(self.source_hash()?);
        Ok(st.get("source") != Some(&src) && st.get("failed") != Some(&src))
    }
}

/// Every file under dir, as Path.rglob("*") finds them: links to folders aren't followed.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let real_dir = e.file_type().is_ok_and(|t| t.is_dir());
        if real_dir {
            walk(&p, out);
        } else if p.is_file() {
            out.push(p);
        }
    }
}

/// The programs HOME has the source of. In the background (`up`, accept), not the board
/// when BULLPEN_BOARD names a board of the person's own choosing.
pub fn programs(background: bool) -> Vec<&'static Program> {
    let h = home();
    if !h.join("Cargo.toml").exists() {
        return vec![];
    }
    let own_board = std::env::var_os("BULLPEN_BOARD").is_some_and(|b| !b.is_empty());
    [&CLI, &BOARD]
        .into_iter()
        .filter(|p| h.join(p.crates[p.crates.len() - 1]).join("Cargo.toml").exists())
        .filter(|p| !(background && own_board && **p == BOARD))
        .collect()
}

/// The build lock's open file, locked, or None when another build holds it (wait=false).
fn lock_build(wait: bool) -> Res<Option<File>> {
    Ok(bullpen_core::lock::take(&build_lock(), wait)?)
}

/// Is a build running (holding the lock)?
pub fn building() -> Res<bool> {
    if !build_lock().exists() {
        return Ok(false);
    }
    Ok(lock_build(false)?.is_none())
}

fn names(progs: &[&Program]) -> String {
    progs.iter().map(|p| p.what).collect::<Vec<_>>().join(" and ")
}

/// `up`: when a program's source isn't what it was built from, build it in the
/// background. Says what it started; never fails.
pub fn rebuild_if_stale(repo: &Repo) -> Option<String> {
    match start_rebuild(repo) {
        Ok(said) => said,
        Err(e) => {
            eprintln!("bullpen: couldn't check bullpen's build: {e}");
            None
        }
    }
}

fn start_rebuild(repo: &Repo) -> Res<Option<String>> {
    let mut stale = Vec::new();
    for p in programs(true) {
        if !p.stale()? {
            continue; // current, or its build failed already: doctor says so
        }
        if p.dirty()? {
            let src = p.source_hash()?;
            if p.stamp()?.get("skipped").and_then(Value::as_str) != Some(src.as_str()) {
                p.set_stamp("skipped", &src)?;
                let text = format!(
                    "{} wasn't rebuilt: {} has uncommitted changes to {}, and a build would take them in. {}. \
                     Commit or drop them, then run `bullpen up` (or `bullpen build` to build them as they are).",
                    p.subject(),
                    home().display(),
                    p.crates.join(", "),
                    p.keeps
                );
                say(repo, p, &text)?;
            }
            continue;
        }
        stale.push(p);
    }
    if stale.is_empty() {
        return Ok(None);
    }
    let Some(lock) = lock_build(false)? else {
        return Ok(None); // a build is running: it checks the sources again when it's done
    };
    let fd = lock.as_raw_fd();
    let log = repo.dir.join("jobs.log");
    let job = Job {
        argv: vec![
            self_bin().display().to_string(),
            "_build_board".into(),
            "--lock-fd".into(),
            fd.to_string(),
        ],
        cwd: &repo.root,
        log: &log,
        what: format!("Rebuilding {}", names(&stale)),
        task: None,
        pass_fds: vec![fd],
    };
    background(&job, false);
    drop(lock); // the job keeps it; this command's copy closes here
    let their = if stale.len() > 1 { "their source" } else { "its source" };
    let boards = if stale.contains(&&BOARD) {
        "; running boards reload when it's done"
    } else {
        ""
    };
    Ok(Some(format!(
        "rebuilding {} in the background ({their} changed){boards}",
        names(&stale)
    )))
}

/// A program's news: for every coordinator.
fn say(repo: &Repo, p: &Program, text: &str) -> Res<()> {
    let (tid, kind) = p.news_as;
    let task: Obj = json!({"id": tid}).as_object().cloned().unwrap_or_default();
    news(
        repo,
        &task,
        kind,
        text,
        Opts {
            everyone: true,
            ..Opts::default()
        },
    )
}

const MISE_NO_VERSION: &str = "No version is set for shim";

/// rustup's cargo, in ~/.cargo/bin.
fn rustup_cargo() -> PathBuf {
    PathBuf::from(user_home()).join(".cargo/bin/cargo")
}

fn executable(p: &Path) -> bool {
    std::ffi::CString::new(p.as_os_str().as_encoded_bytes())
        .is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0)
}

/// The cargo to build with, or why there's none.
fn cargo_command() -> Result<String, String> {
    if let Some(cargo) = which("cargo") {
        return Ok(cargo);
    }
    let rustup = rustup_cargo();
    if executable(&rustup) {
        return Ok(rustup.display().to_string());
    }
    Err("cargo isn't on PATH (install Rust: https://rustup.rs)".into())
}

/// Build these programs with cargo, its output appended to log: Ok, or why not.
fn run_cargo(cargo: &str, log: &mut File, progs: &[&Program], nice: bool) -> Res<Result<(), String>> {
    let manifest = home().join("Cargo.toml").display().to_string();
    let mut argv: Vec<String> = vec![
        cargo.into(),
        "build".into(),
        "--release".into(),
        "--manifest-path".into(),
        manifest,
    ];
    for p in progs {
        argv.extend(["-p".into(), p.package.into()]);
    }
    let run_argv: Vec<String> = if nice {
        std::iter::once("nice".to_string())
            .chain(argv.iter().cloned())
            .collect()
    } else {
        argv.clone()
    };
    let proc = run_with(&run_argv, Some(&home()), None, false)?;
    write!(log, "$ {}\n{}{}", argv.join(" "), proc.stdout, proc.stderr)?;
    log.flush()?;
    if proc.ok() {
        return Ok(Ok(()));
    }
    let out = format!("{}{}", proc.stdout, proc.stderr);
    if out.contains(MISE_NO_VERSION) {
        let rustup = rustup_cargo();
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        if executable(&rustup) && canon(&rustup) != canon(Path::new(cargo)) {
            writeln!(
                log,
                "{cargo} is a mise shim with no version set here: trying {}",
                rustup.display()
            )?;
            return run_cargo(&rustup.display().to_string(), log, progs, nice);
        }
        return Ok(Err(format!(
            "{cargo} is a mise shim with no Rust version set for {h}: set one (for example `mise use -g rust`, \
             or a mise.toml in {h}), or install rustup's cargo",
            h = home().display()
        )));
    }
    let lines: Vec<&str> = splitlines(&out).into_iter().filter(|l| !strip(l).is_empty()).collect();
    let err = lines
        .iter()
        .find(|l| l.starts_with("error"))
        .or(lines.last())
        .map(|l| strip(l).to_string())
        .unwrap_or_else(|| format!("exit {}", proc.code));
    Ok(Err(format!("cargo failed: {err}")))
}

/// What a build did: the programs that failed, why, and the ones built.
pub struct Built {
    pub failed: Vec<&'static Program>,
    pub why: Option<String>,
    pub built: Vec<&'static Program>,
}

/// Build these programs and put them in place. In the background, it builds only the
/// ones whose source still needs it and has no uncommitted changes.
pub fn build(progs: Vec<&'static Program>, background: bool, nice: bool) -> Res<Built> {
    let mut progs = progs;
    if background {
        let mut keep = Vec::new();
        for p in progs {
            if p.stale()? && !p.dirty()? {
                keep.push(p);
            }
        }
        progs = keep;
    }
    let none = Built {
        failed: vec![],
        why: None,
        built: vec![],
    };
    if progs.is_empty() {
        return Ok(none);
    }
    let mut srcs = Vec::new();
    for p in &progs {
        srcs.push(p.source_hash()?);
    }
    std::fs::create_dir_all(target())?;
    let mut log = File::create(build_log())?;
    writeln!(
        log,
        "{} building {} from {}",
        iso_now(),
        names(&progs),
        home().display()
    )?;
    let mut res = match cargo_command() {
        Ok(cargo) => run_cargo(&cargo, &mut log, &progs, nice)?,
        Err(why) => Err(why),
    };
    if res.is_ok() {
        if let Some(missing) = progs.iter().find(|p| !executable(&p.cargo_out())) {
            res = Err(format!("cargo made no {}", missing.cargo_out().display()));
        }
    }
    if let Err(why) = res {
        writeln!(log, "failed: {why}")?;
        for (p, src) in progs.iter().zip(&srcs) {
            p.set_stamp("failed", src)?;
        }
        return Ok(Built {
            failed: progs,
            why: Some(why),
            built: vec![],
        });
    }
    for (p, src) in progs.iter().zip(&srcs) {
        // A copy, renamed into place: a running program never sees a half-written file.
        let tmp = target().join(format!(".{}.{}", p.name, std::process::id()));
        std::fs::copy(p.cargo_out(), &tmp)?;
        std::fs::rename(&tmp, p.installed())?;
        write_obj(&p.stamp_path(), json!({"source": src}).as_object().expect("an object"))?;
        writeln!(log, "built {}", p.installed().display())?;
    }
    Ok(Built {
        failed: vec![],
        why: None,
        built: progs,
    })
}

/// `build`: build bullpen's command line and the board now, whatever their source
/// holds (install.sh runs this).
pub fn cmd_build(argv: &[String]) -> Res<i32> {
    Args::parse("build", argv, &[]);
    let progs = programs(false);
    if progs.is_empty() {
        return fail(format!("{} has no Cargo.toml: nothing to build", home().display()));
    }
    let lock = match lock_build(false)? {
        Some(lock) => lock,
        None => {
            println!("another build of bullpen is running: waiting for it");
            lock_build(true)?.expect("a lock, once waited for")
        }
    };
    let done = build(progs, false, false)?;
    drop(lock);
    if let Some(why) = done.why {
        let (were, log) = (if done.failed.len() > 1 { "weren't" } else { "wasn't" }, build_log());
        return fail(format!(
            "{} {were} built: {why} (log: {})",
            names(&done.failed),
            log.display()
        ));
    }
    for p in &done.built {
        println!("built {}: {}", p.what, p.installed().display());
    }
    watcher_after(&done.built);
    Ok(0)
}

const BUILD_BG: &[Arg] = &[value("--lock-fd", "LOCK_FD", SUPPRESS)];

/// `_build_board`: `up` or accept started this, holding the build lock for it
/// (--lock-fd), which it keeps until it's done, news included.
pub fn cmd_build_bg(argv: &[String]) -> Res<i32> {
    let a = Args::parse("_build_board", argv, BUILD_BG);
    let Some(v) = a.opt("--lock-fd") else {
        usage_error(
            "_build_board",
            BUILD_BG,
            "the following arguments are required: --lock-fd",
        )
    };
    let Ok(fd) = strip(v).parse::<i32>() else {
        usage_error(
            "_build_board",
            BUILD_BG,
            &format!("argument --lock-fd: invalid int value: '{v}'"),
        )
    };
    if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
        return fail(format!("[Errno 9] Bad file descriptor: {fd}"));
    }
    // SAFETY: the fd was passed to this program to own: the build lock, held for it
    let _lock = unsafe { File::from_raw_fd(fd) };
    let progs = programs(true);
    let done = match build(progs.clone(), true, false) {
        Ok(done) => done,
        Err(Fail(e)) => {
            let mut failed = Vec::new();
            for p in progs {
                if p.stale().unwrap_or(true) {
                    if let Ok(src) = p.source_hash() {
                        let _ = p.set_stamp("failed", &src);
                    }
                    failed.push(p);
                }
            }
            Built {
                failed,
                why: Some(e),
                built: vec![],
            }
        }
    };
    watcher_after(&done.built);
    if done.failed.is_empty() {
        return Ok(0);
    }
    let Ok(repo) = repo_root().and_then(Repo::new) else {
        return Ok(1);
    };
    let why = done.why.unwrap_or_default();
    for p in &done.failed {
        let text = format!(
            "{} wasn't rebuilt: {why}. {}. Log: {}. Fix it, then run `bullpen build`.",
            p.subject(),
            p.keeps,
            build_log().display()
        );
        say(&repo, p, &text)?;
    }
    Ok(1)
}

/// A new command line: a running watcher is replaced, so it runs the new build.
fn watcher_after(built: &[&Program]) {
    if built.contains(&&CLI) {
        restart_watcher();
    }
}

#[cfg(test)]
#[path = "test_build.rs"]
mod tests;
