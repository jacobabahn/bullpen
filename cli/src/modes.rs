//! Local mode and remote mode (bullpen/modes.py), as far as what a task's branch is
//! compared with, publishing it (`ready`), and the one way to push (quiet_git,
//! push_base). The adapters' other verbs (merge, withdraw, ...) move in T8; Python keeps
//! its own modes.py (a twin).

use crate::jobs::{background, Job};
use bullpen_core::json::{get, py_str, s, truthy, Obj};
use bullpen_core::paths::self_bin;
use bullpen_core::proc::{git, text, Done};
use bullpen_core::{Repo, Res};
use std::io::Read;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Is the task in local mode? Its record from when it was published, else the repo's.
pub fn is_local(repo: &Repo, t: &Obj) -> Res<bool> {
    Ok(!remote(repo, t)?)
}

/// Is the task in remote mode? Its record from when it was published, else the repo's.
fn remote(repo: &Repo, t: &Obj) -> Res<bool> {
    if let Some(local) = get(t, "local") {
        return Ok(!truthy(Some(local)));
    }
    if truthy(t.get("pr")) {
        return Ok(true); // published before the record was kept
    }
    repo.has_origin()
}

/// What the task's branch is compared with: origin's copy of its base branch in remote
/// mode (when origin has it), else the local base branch.
pub fn task_base_ref(repo: &Repo, t: &Obj) -> Res<String> {
    let base = t.get("base").map_or("None".into(), py_str);
    if remote(repo, t)? && truthy(t.get("worktree")) {
        let on_origin = format!("origin/{base}");
        if git(s(t, "worktree"), &["rev-parse", "-q", "--verify", &on_origin], false)?.ok() {
            return Ok(on_origin);
        }
    }
    Ok(base)
}

/// The base branch new work starts from: origin's copy when there is one.
pub fn repo_base_ref(repo: &Repo) -> Res<String> {
    let base = repo.base()?;
    let on_origin = format!("origin/{base}");
    if repo.has_origin()? && git(&repo.root, &["rev-parse", "-q", "--verify", &on_origin], false)?.ok() {
        return Ok(on_origin);
    }
    Ok(base)
}

// ---------- one way to push ----------

/// git that never waits for a password or a key's passphrase: it fails instead.
fn push_env(repo: &Repo, cmd: &mut Command) -> Res<()> {
    cmd.env("GIT_TERMINAL_PROMPT", "0").env("GCM_INTERACTIVE", "never");
    let own_ssh = std::env::var("GIT_SSH_COMMAND").is_ok_and(|v| !v.is_empty());
    if !own_ssh && !git(&repo.root, &["config", "core.sshCommand"], false)?.ok() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    Ok(())
}

/// git on the network, away from any terminal (no prompt can reach the person), in a
/// session of its own; after timeout seconds it is killed and says so.
pub fn quiet_git(repo: &Repo, args: &[&str], timeout: u64) -> Res<Done> {
    quiet_git_in(repo, &repo.root, args, timeout)
}

/// quiet_git, in cwd (a task's worktree) rather than the repo's root.
pub fn quiet_git_in(repo: &Repo, cwd: &Path, args: &[&str], timeout: u64) -> Res<Done> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(cwd).args(args);
    push_env(repo, &mut cmd)?;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // SAFETY: only setsid between fork and exec
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let read_out = std::thread::spawn(move || read_all(out.as_mut()));
    let read_err = std::thread::spawn(move || read_all(err.as_mut()));
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break Some(st);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = read_out.join().unwrap_or_default();
    let stderr = read_err.join().unwrap_or_default();
    Ok(match status {
        None => Done {
            code: 1,
            stdout: String::new(),
            stderr: format!("no answer from the remote in {timeout}s"),
        },
        Some(st) => Done {
            code: st.code().unwrap_or_else(|| -st.signal().unwrap_or(0)),
            stdout: text(&stdout),
            stderr: text(&stderr),
        },
    })
}

fn read_all(r: Option<&mut impl Read>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(r) = r {
        let _ = r.read_to_end(&mut buf);
    }
    buf
}

/// The remote accept pushes the base branch to in local mode, or None (unset, or the
/// repo has an origin, where the setting does nothing).
pub fn push_remote(repo: &Repo) -> Res<Option<String>> {
    let name = repo.cfg_str("push_remote");
    if name.is_empty() || repo.has_origin()? {
        return Ok(None);
    }
    Ok(Some(name.to_string()))
}

/// Push base to the push remote in the background (`_push`). Says what it started, or
/// None when there's no push remote.
pub fn push_base(repo: &Repo, base: &str, tid: Option<&str>) -> Res<Option<String>> {
    let Some(remote) = push_remote(repo)? else {
        return Ok(None);
    };
    let mut argv = vec![self_bin().display().to_string(), "_push".into(), base.into()];
    if let Some(tid) = tid {
        argv.extend(["--task".into(), tid.into()]);
    }
    let log = repo.dir.join("jobs.log");
    let job = Job {
        argv,
        cwd: &repo.root,
        log: &log,
        what: format!("Pushing {base} to {remote}"),
        task: tid,
        pass_fds: vec![],
    };
    // the tests: wait, to look at what it did
    background(&job, std::env::var("BULLPEN_PUSH_WAIT").is_ok_and(|v| !v.is_empty()));
    Ok(Some(format!("pushing {base} to {remote} in the background")))
}

/// What a failed git said, on one line.
pub fn failed(p: &Done) -> String {
    let said = if p.stderr.is_empty() { &p.stdout } else { &p.stderr };
    said.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `ready`'s publish: in remote mode push the branch and open its PR (once); in local
/// mode nothing leaves this machine. Says what it did.
pub fn publish(repo: &Repo, t: &mut Obj, ahead: i64, base_ref: &str, summary: &str) -> Res<String> {
    let branch = s(t, "branch").to_string();
    if is_local(repo, t)? {
        return Ok(format!(
            "local branch {branch} ({} ahead of {base_ref}; no remote, so nothing pushed)",
            bullpen_core::task::commits(ahead)
        ));
    }
    let wt = std::path::PathBuf::from(s(t, "worktree"));
    let to = format!("HEAD:refs/heads/{branch}");
    let p = quiet_git_in(
        repo,
        &wt,
        &["push", "-q", "--force-with-lease", "-u", "origin", &to],
        600,
    )?;
    if !p.ok() {
        return bullpen_core::fail(format!("pushing {branch} to origin failed: {}", failed(&p)));
    }
    if let Some(serde_json::Value::Object(gh)) = t.get_mut("gh").filter(|v| truthy(Some(v))) {
        gh.insert("new".into(), serde_json::json!(0)); // what it says about the new push comes later
        gh.insert("checks".into(), serde_json::Value::Null);
    }
    if !truthy(t.get("pr")) {
        let summary = bullpen_core::text::strip(summary);
        let body = format!(
            "{}\n\nBullpen task {}.\n",
            if summary.is_empty() { s(t, "title") } else { summary },
            s(t, "id")
        );
        let title = format!("{} ({})", s(t, "title"), s(t, "id"));
        let base = py_str(t.get("base").unwrap_or(&serde_json::Value::Null));
        let argv = [
            repo.gh(),
            "pr".into(),
            "create".into(),
            "--base".into(),
            base,
            "--head".into(),
            branch.clone(),
            "--title".into(),
            title,
            "--body-file".into(),
            "-".into(),
        ];
        let out = bullpen_core::proc::run_with(&argv, Some(&wt), Some(&body), true)?.stdout;
        let pr = bullpen_core::text::splitlines(bullpen_core::text::strip(&out))
            .last()
            .map_or(String::new(), |l| l.to_string());
        t.insert("pr".into(), serde_json::json!(pr));
    }
    Ok(format!("pushed {branch}; pull request: {}", s(t, "pr")))
}
