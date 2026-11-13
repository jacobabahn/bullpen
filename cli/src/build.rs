//! Building bullpen's two programs from its own checkout: the command line and the
//! board. `build` (install.sh runs it), `_build_board` (the background build `up` and
//! accept start), their rebuild_if_stale, and the catch-up of an internal command this
//! build doesn't have yet (catch_up).
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
use bullpen_core::paths::{home, self_bin};
use bullpen_core::proc::{file_hash, git, run_with};
use bullpen_core::repo::repo_root;
use bullpen_core::text::{splitlines, strip, upper_first};
use bullpen_core::timeutil::iso_now;
use bullpen_core::{fail, Fail, Repo, Res};
use serde_json::{json, Value};
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
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

    /// A hash of its source, by path (the same bytes as the Python's was, so a stamp
    /// from before T13 still says what it built).
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

/// A test's or a compare's fixture (tests/support.py, cli/check/) sets BULLPEN_NO_BUILD:
/// bullpen never builds, or takes the build lock in, the checkout it runs from.
pub fn no_build() -> bool {
    std::env::var_os("BULLPEN_NO_BUILD").is_some_and(|v| !v.is_empty())
}

const NO_BUILD: &str = "builds are off here (BULLPEN_NO_BUILD is set: a test's or a compare's fixture)";

/// The build lock's open file, locked, or None when another build holds it (wait=false).
fn lock_build(wait: bool) -> Res<Option<File>> {
    Ok(bullpen_core::lock::take(&build_lock(), wait)?)
}

/// Write the build's pid in the lock file it holds, for a call that waits on it to name.
fn mark(lock: &File, pid: u32) -> Res<()> {
    lock.set_len(0)?;
    let mut f = lock;
    writeln!(f, "{pid}")?;
    Ok(())
}

/// The pid in the lock file, when that process is running: the build that holds it.
fn holder() -> Option<i32> {
    let pid: i32 = strip(&std::fs::read_to_string(build_lock()).ok()?).parse().ok()?;
    // SAFETY: signal 0 only asks whether the process is there
    (pid > 0 && unsafe { libc::kill(pid, 0) } == 0).then_some(pid)
}

/// How long a call waits for a build that's running before it says it's stuck (the tests set it).
fn build_wait() -> f64 {
    std::env::var("BULLPEN_BUILD_WAIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600.0)
}

/// The build lock, once the build that's running is done: never longer than build_wait().
/// Past it, the build is stuck (or its holder isn't building): say so.
fn wait_for_build() -> Res<File> {
    let wait = build_wait();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(wait);
    loop {
        if let Some(lock) = lock_build(false)? {
            return Ok(lock);
        }
        if std::time::Instant::now() > deadline {
            let (who, stop) = match holder() {
                Some(pid) => (format!(" (pid {pid})"), format!("stop it (`kill {pid}`)")),
                None => (String::new(), "stop it".into()),
            };
            return fail(format!(
                "the build that's running{who} still isn't done after {wait}s: if it's stuck, {stop} and run \
                 this again (log: {})",
                build_log().display()
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Is a build running (holding the lock)?
pub fn building() -> Res<bool> {
    if no_build() || !build_lock().exists() {
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
    if no_build() {
        return Ok(None);
    }
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
    mark(&lock, std::process::id())?;
    if let Some(pid) = background(&job, false) {
        mark(&lock, pid)?;
    }
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

/// The one lookup of cargo every build uses (bin/bullpen-cargo): it runs the first cargo
/// that works here, with what it needs, and when none does says what it tried.
pub fn bullpen_cargo() -> PathBuf {
    home().join("bin").join("bullpen-cargo")
}

/// What bin/bullpen-cargo says when no cargo works: its last stderr line starts with it.
const NO_CARGO: &str = "bullpen-cargo: no cargo works here";

fn executable(p: &Path) -> bool {
    std::ffi::CString::new(p.as_os_str().as_encoded_bytes())
        .is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0)
}

/// Build these programs with cargo (bin/bullpen-cargo), its output appended to log: Ok, or why not.
fn run_cargo(log: &mut File, progs: &[&Program], nice: bool) -> Res<Result<(), String>> {
    let manifest = home().join("Cargo.toml").display().to_string();
    let mut argv: Vec<String> = vec![
        bullpen_cargo().display().to_string(),
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
    let lines: Vec<&str> = splitlines(&out).into_iter().filter(|l| !strip(l).is_empty()).collect();
    if let Some(none) = lines.iter().find(|l| l.starts_with(NO_CARGO)) {
        return Ok(Err(strip(&none["bullpen-cargo: ".len()..]).to_string()));
    }
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
    let mut res = run_cargo(&mut log, &progs, nice)?;
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
    if no_build() {
        return fail(NO_BUILD);
    }
    let progs = programs(false);
    if progs.is_empty() {
        return fail(format!("{} has no Cargo.toml: nothing to build", home().display()));
    }
    let lock = match lock_build(false)? {
        Some(lock) => lock,
        None => {
            println!("another build of bullpen is running: waiting for it");
            let _ = std::io::stdout().flush();
            wait_for_build()?
        }
    };
    mark(&lock, std::process::id())?;
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

/// A call for an internal command (a job's, or the watcher's from a newer watch.lua)
/// this build doesn't have, when the build is older than its source: the window between
/// an accept's merge and its new build, or a build that never ran. Wait for the build
/// that's running (never for ever), or build what's stale now, then run the call again
/// with the new build (exec, so the pid stays). Returns only when it can't: Ok(true)
/// when the build is behind its source (the call isn't in this build), Ok(false) when
/// the build is its source's (the name is simply unknown). A person's command gets the
/// unknown-command line at once: a typo never sets off a build.
pub fn catch_up(argv: &[std::ffi::OsString]) -> Res<bool> {
    let name = argv[0].to_string_lossy();
    if no_build() || !name.starts_with('_') || !programs(false).contains(&&CLI) || !executable(&CLI.installed()) {
        return Ok(false);
    }
    let src = Value::String(CLI.source_hash()?);
    let st = CLI.stamp()?;
    if st.get("source") == Some(&src) {
        return Ok(false);
    }
    // this very call, run again: not the jobs a call that caught up starts, which may need to catch up too
    let me = std::process::id().to_string();
    if st.get("failed") == Some(&src) || std::env::var("BULLPEN_CAUGHT_UP").is_ok_and(|v| v == me) {
        return Ok(true);
    }
    let inode = |p: &Path| std::fs::metadata(p).map(|m| std::os::unix::fs::MetadataExt::ino(&m));
    let was = inode(&CLI.installed())?; // a new build is a new file (renamed into place)
    let lock = match lock_build(false)? {
        Some(lock) => lock,
        None => {
            eprintln!("bullpen: `{name}` is in the build that's running: waiting for it");
            wait_for_build()?
        }
    };
    if CLI.stale()? && !CLI.dirty()? {
        eprintln!(
            "bullpen: `{name}` is newer than this build: building it first (log: {})",
            build_log().display()
        );
        mark(&lock, std::process::id())?;
        // the board too: an accept's rebuild that finds this build running leaves it to it
        let done = build(programs(true), true, false)?;
        watcher_after(&done.built);
        if done.failed.contains(&&CLI) {
            return Ok(true);
        }
    }
    drop(lock);
    let built = CLI.stamp()?.get("source") == Some(&Value::String(CLI.source_hash()?));
    if inode(&CLI.installed())? == was || !built {
        return Ok(true); // no new build: it would say the same again
    }
    let _ = std::io::stderr().flush();
    let err = std::process::Command::new(self_bin())
        .args(argv)
        .env("BULLPEN_CAUGHT_UP", &me)
        .exec();
    fail(format!("couldn't run {}: {err}", self_bin().display()))
}

/// What a call for a command this build doesn't have says, when the build is behind its source.
pub fn not_in_build(name: &str) -> String {
    let failed = CLI.stamp().is_ok_and(|st| st.contains_key("failed"));
    let log = if failed {
        format!(" (its last build failed: {})", build_log().display())
    } else {
        String::new()
    };
    format!("bullpen: `{name}` isn't in this build of bullpen: rebuild it with `bullpen build`{log}")
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
