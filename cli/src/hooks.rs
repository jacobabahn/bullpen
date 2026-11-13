//! A worktree's hooks: the repo's config.json `setup` or `teardown`,
//! else .bullpen/<name>.sh as committed on the task's base, run in the worktree: setup
//! in a new one, teardown before it goes (remove_worktree, close's).

use crate::modes::task_base_ref;
use crate::stops::stop;
use bullpen_core::json::{py_str, s, truthy, Obj};
use bullpen_core::proc::git;
use bullpen_core::text::strip;
use bullpen_core::timeutil::iso_now;
use bullpen_core::{Repo, Res};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where the task's setup or teardown writes what it says.
pub fn hook_log(repo: &Repo, t: &Obj, name: &str) -> PathBuf {
    repo.dir.join("briefs").join(format!("{}.{name}.log", s(t, "id")))
}

/// A hook to run: its argv, where it came from, and the other source it wins over.
struct Hook {
    argv: Vec<String>,
    source: String,
    over: Option<String>,
}

/// The task's setup or teardown, or None: the repo's config.json first, then
/// .bullpen/<name>.sh as committed on the task's base.
fn hook(repo: &Repo, t: &Obj, name: &str) -> Res<Option<Hook>> {
    let git_ref = task_base_ref(repo, t)?;
    let path = format!(".bullpen/{name}.sh");
    let p = git(&repo.root, &["show", &format!("{git_ref}:{path}")], false)?;
    let committed = (p.ok() && !strip(&p.stdout).is_empty()).then_some(p.stdout);
    if let Some(cmd) = repo.cfg.get(name).filter(|v| truthy(Some(v))) {
        return Ok(Some(Hook {
            argv: vec!["sh".into(), "-c".into(), py_str(cmd)],
            source: format!("config.json's {name}"),
            over: committed.map(|_| format!("{path} on {git_ref}")),
        }));
    }
    let Some(committed) = committed else {
        return Ok(None);
    };
    let script = repo.dir.join("briefs").join(format!("{}.{name}.sh", s(t, "id")));
    std::fs::write(&script, &committed)?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let script = script.display().to_string();
    let argv = if committed.starts_with("#!") {
        vec![script]
    } else {
        vec!["sh".into(), script]
    };
    Ok(Some(Hook {
        argv,
        source: format!("{path} on {git_ref}"),
        over: None,
    }))
}

/// What a hook's run came to: its exit code, its log, where it came from, what it won over.
pub struct Ran {
    pub code: i32,
    pub log: PathBuf,
    pub source: String,
    pub over: Option<String>,
}

/// Run the task's setup or teardown in its worktree, its output in <ID>.<name>.log.
/// None when there is none.
pub fn run_hook(repo: &Repo, t: &Obj, name: &str) -> Res<Option<Ran>> {
    let Some(h) = hook(repo, t, name)? else {
        return Ok(None);
    };
    let log = hook_log(repo, t, name);
    let mut f = std::fs::File::create(&log)?;
    let over = h.over.as_ref().map_or(String::new(), |o| format!(" (over {o})"));
    write!(f, "# {name} for {} at {}: {}{over}\n", s(t, "id"), iso_now(), h.source)?;
    let ran = Command::new(&h.argv[0])
        .args(&h.argv[1..])
        .current_dir(s(t, "worktree"))
        .env("BULLPEN_TASK", s(t, "id"))
        .env("BULLPEN_WORKTREE", s(t, "worktree"))
        .env("BULLPEN_MAIN", repo.root_str())
        .env("BULLPEN_BRANCH", s(t, "branch"))
        .stdin(Stdio::null())
        .stdout(f.try_clone()?)
        .stderr(f.try_clone()?)
        .status();
    let code = match ran {
        Ok(st) => st.code().unwrap_or_else(|| -st.signal().unwrap_or(1)),
        Err(e) => {
            writeln!(f, "{}", os_error(&e, &h.argv[0]))?;
            127
        }
    };
    writeln!(f, "# exit {code}")?;
    Ok(Some(Ran {
        code,
        log,
        source: h.source,
        over: h.over,
    }))
}

/// Run the task's teardown before its worktree goes. A failure is logged and said,
/// never in the way. Returns whether one ran.
fn tear_down(repo: &Repo, t: &Obj) -> bool {
    let wt = s(t, "worktree");
    if wt.is_empty() || !Path::new(wt).exists() {
        return false;
    }
    match run_hook(repo, t, "teardown") {
        Err(e) => {
            eprintln!("{}: teardown couldn't run: {e}", s(t, "id"));
            false
        }
        Ok(None) => false,
        Ok(Some(r)) => {
            if r.code != 0 {
                eprintln!(
                    "{}: teardown failed (exit {}): see {}; removing the worktree anyway",
                    s(t, "id"),
                    r.code,
                    r.log.display()
                );
            }
            true
        }
    }
}

/// Remove the task's worktree (its teardown first) and its branch.
pub fn remove_worktree(repo: &Repo, t: &Obj, force: bool) -> Res<()> {
    // what a teardown leaves behind mustn't keep the worktree (drop checked it was clean before)
    let force = tear_down(repo, t) || force;
    let wt = s(t, "worktree");
    if !wt.is_empty() && Path::new(wt).exists() {
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(wt);
        git(&repo.root, &args, true)?;
    }
    let branch = s(t, "branch");
    if !branch.is_empty() && !truthy(t.get("kept")) {
        // a task closed with --keep-branch keeps it (close::keep_branch)
        git(&repo.root, &["branch", "-D", branch], false)?;
    }
    Ok(())
}

/// An OSError as Python says it: "[Errno 2] No such file or directory: 'x'".
fn os_error(e: &std::io::Error, path: &str) -> String {
    let n = e.raw_os_error().unwrap_or(0);
    let said = e.to_string();
    let said = said.trim_end_matches(&format!(" (os error {n})"));
    format!("[Errno {n}] {said}: '{path}'")
}

/// Run the task's setup in its new worktree. On failure the task stops (saved, news
/// sent) and this returns false; its next start or resume sets it up again.
pub fn set_up(repo: &Repo, t: &mut Obj) -> Res<bool> {
    let r = run_hook(repo, t, "setup")?;
    if let Some(r) = &r {
        if let Some(over) = &r.over {
            eprintln!("{}: running {}, over {over}", s(t, "id"), r.source);
        }
        if r.code != 0 {
            let (id, code, log) = (s(t, "id").to_string(), r.code, r.log.display());
            let why = format!("Setup failed (exit {code}): see {log}");
            let line = format!(
                "{id}'s setup failed (exit {code}), so its worker didn't start: see {log}. \
                 `bullpen start {id}` sets it up again"
            );
            stop(repo, t, "worker", "setup", &why, &[line], "stopped")?;
            return Ok(false);
        }
    }
    t.shift_remove("needs_setup");
    Ok(true)
}
