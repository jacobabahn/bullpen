//! Local mode and remote mode: the steps that differ between them.
//! A task's mode is the one recorded when it was published (`local`), else the repo's
//! (an origin remote: remote mode). Here: what a task's branch is compared with,
//! publishing it (`ready`), merging it and what follows (`accept`), pushing it again
//! after a rebase (refresh), withdrawing it (close and drop), and the one way to push
//! (quiet_git, push_base).

use crate::jobs::{background, Job};
use bullpen_core::json::{get, obj, py_str, s, truthy, Obj};
use bullpen_core::paths::self_bin;
use bullpen_core::proc::{git, run_in, run_input, text, Done};
use bullpen_core::text::{split, splitlines, squash, strip};
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};
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
    match recorded_remote(t) {
        Some(remote) => Ok(remote),
        None => repo.has_origin(),
    }
}

/// The task's mode as it records it (true: remote), or None: it goes by the repo's.
pub fn recorded_remote(t: &Obj) -> Option<bool> {
    if let Some(local) = get(t, "local") {
        return Some(!truthy(Some(local)));
    }
    truthy(t.get("pr")).then_some(true) // published before the record was kept
}

/// Withdraw what was published of the task, unmerged: in remote mode close its PR (and
/// delete its branch on origin); in local mode nothing was published.
pub fn withdraw(repo: &Repo, t: &Obj, delete_branch: bool) -> Res<()> {
    if !remote(repo, t)? || !truthy(t.get("pr")) {
        return Ok(());
    }
    let pr = py_str(t.get("pr").unwrap_or(&serde_json::Value::Null));
    run_in(&[repo.gh(), "pr".into(), "close".into(), pr], &repo.root, false)?;
    let branch = s(t, "branch");
    if delete_branch && !branch.is_empty() {
        quiet_git(repo, &["push", "-q", "origin", "--delete", branch], 60)?;
    }
    Ok(())
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

/// Push a kept branch (`close --keep-branch`) to remote as name, never forced.
pub fn push_branch(repo: &Repo, remote: &str, branch: &str, name: &str) -> Res<Done> {
    let refspec = format!("refs/heads/{branch}:refs/heads/{name}");
    quiet_git(repo, &["push", "-q", remote, &refspec], 60)
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

// ---------- accept: merging, and what follows ----------

/// Merge the task: its PR on GitHub (unless it was merged there already) in remote
/// mode, its branch into the local base branch in local mode. Fails, leaving the task
/// as it was, when it can't.
pub fn merge(repo: &Repo, t: &mut Obj) -> Res<()> {
    if !remote(repo, t)? {
        if truthy(t.get("branch")) {
            merge_locally(repo, t)?;
        }
        return Ok(());
    }
    if !truthy(t.get("pr")) {
        return Ok(());
    }
    if s(obj(t, "gh"), "state") != "merged" {
        let how = repo.cfg_str("merge").to_string();
        let pr = py_str(t.get("pr").unwrap_or(&Value::Null));
        let argv = [repo.gh(), "pr".into(), "merge".into(), pr, format!("--{how}")];
        let p = run_in(&argv, &repo.root, false)?;
        if !p.ok() {
            let said = if p.stderr.is_empty() { &p.stdout } else { &p.stderr };
            return fail(format!("merge refused: {}", strip(said)));
        }
        t.insert("merged_how".into(), json!(how)); // one merged outside bullpen has none: who knows how
    }
    // for `revert`: the commit the merge left on the base and how many commits were merged,
    // when gh says them
    if let Some((oid, had)) = crate::revert::commits::pr_merge_commit(repo, t) {
        t.insert("merged".into(), json!(oid));
        if let Some(n) = had {
            t.insert("merged_commits".into(), json!(n));
        }
    }
    Ok(())
}

/// What follows a merge: in remote mode the merged branch is deleted on origin and
/// the base it moved is fetched; in local mode the base branch is backed up to the
/// push remote. Says what it started, or None.
pub fn after_merge(repo: &Repo, t: &Obj) -> Res<Option<String>> {
    let base = py_str(t.get("base").unwrap_or(&Value::Null));
    if !remote(repo, t)? {
        if !truthy(t.get("merged")) {
            return Ok(None);
        }
        return push_base(repo, &base, Some(s(t, "id")));
    }
    if truthy(t.get("pr")) && truthy(t.get("branch")) {
        quiet_git(repo, &["push", "-q", "origin", "--delete", s(t, "branch")], 60)?;
    }
    quiet_git(repo, &["fetch", "-q", "origin", &base], 60)?;
    Ok(None)
}

/// After bullpen rebased the task's branch: push it to its PR (remote mode). Says why
/// it failed, or None.
pub fn refresh(repo: &Repo, t: &Obj) -> Res<Option<String>> {
    if !remote(repo, t)? || !truthy(t.get("pr")) {
        return Ok(None);
    }
    let to = format!("HEAD:refs/heads/{}", s(t, "branch"));
    let wt = Path::new(s(t, "worktree"));
    let p = quiet_git_in(repo, wt, &["push", "-q", "--force-with-lease", "origin", &to], 600)?;
    Ok((!p.ok()).then(|| failed(&p)))
}

/// The worktree (the main checkout or another) that has branch checked out, or None.
fn checked_out_at(repo: &Repo, branch: &str) -> Res<Option<String>> {
    let p = git(&repo.root, &["worktree", "list", "--porcelain"], true)?;
    let mut path = None;
    let want = format!("branch refs/heads/{branch}");
    for line in splitlines(&p.stdout) {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(p.to_string());
        } else if line == want {
            return Ok(path);
        }
    }
    Ok(None)
}

/// git's stdout, stripped.
fn git_out(dir: impl AsRef<Path>, args: &[&str]) -> Res<String> {
    Ok(strip(&git(dir, args, true)?.stdout).to_string())
}

/// Local mode: merge the task's branch into its local base branch, as config --merge
/// says. On any failure, fail with the reason and leave the task, its worktree and its
/// branch as they were (a rebase that went through stays: the branch is still intact).
fn merge_locally(repo: &Repo, t: &mut Obj) -> Res<()> {
    let (id, base, branch) = (
        s(t, "id"),
        py_str(t.get("base").unwrap_or(&Value::Null)),
        s(t, "branch"),
    );
    let wt = s(t, "worktree");
    let how = repo.cfg_str("merge");
    let root = &repo.root;
    let head_ref = format!("refs/heads/{branch}");
    if !git(root, &["rev-parse", "-q", "--verify", &head_ref], false)?.ok() {
        return fail(format!("merge refused: {id}'s branch {branch} is gone"));
    }
    let old = git_out(root, &["rev-parse", &format!("refs/heads/{base}")])?;
    let count = git_out(root, &["rev-list", "--count", &format!("{old}..{head_ref}")])?;
    if count == "0" {
        return Ok(()); // nothing to merge
    }
    let new = if how == "rebase" {
        if wt.is_empty() || !Path::new(wt).exists() {
            return fail(format!("merge refused: {id} has no worktree to rebase in"));
        }
        if !strip(&git(wt, &["status", "--porcelain"], true)?.stdout).is_empty() {
            return fail(format!("merge refused: {id} has uncommitted changes in {wt}"));
        }
        if !git(wt, &["rebase", "-q", &old], false)?.ok() {
            git(wt, &["rebase", "--abort"], false)?;
            return fail(format!(
                "merge refused: {branch} doesn't rebase cleanly onto {base}; rebase it by hand in {wt}, then accept again"
            ));
        }
        git_out(root, &["rev-parse", &head_ref])?
    } else {
        let p = git(
            root,
            &["merge-tree", "--write-tree", "--no-messages", &old, &head_ref],
            false,
        )?;
        if !p.ok() {
            let wt = if wt.is_empty() { "None" } else { wt };
            return fail(format!(
                "merge refused: {branch} conflicts with {base}; rebase it onto {base} in {wt}, then accept again"
            ));
        }
        let tree = split(&p.stdout).first().map_or(String::new(), |w| w.to_string());
        let (msg, parents) = if how == "squash" {
            let mut msg = format!("{} ({id})", s(t, "title"));
            if truthy(t.get("summary")) {
                msg += &format!("\n\n{}", strip(s(t, "summary")));
            }
            (msg, vec!["-p", &old])
        } else {
            let msg = format!("Merge {branch}: {} ({id})", s(t, "title"));
            (msg, vec!["-p", &old, "-p", &head_ref])
        };
        let root_s = root.display().to_string();
        let mut argv = vec!["git", "-C", &root_s, "commit-tree", &tree];
        argv.extend(parents);
        argv.extend(["-F", "-"]);
        strip(&run_input(&argv, &format!("{msg}\n"), true)?.stdout).to_string()
    };
    move_base(repo, &base, &old, &new)?;
    t.insert("merged".into(), json!(new));
    // for `revert`: the task's commits are these, by how it was merged
    t.insert("merged_from".into(), json!(old));
    t.insert("merged_how".into(), json!(how));
    Ok(())
}

/// Fast-forward the local base branch from old to new. Where it's checked out, do it
/// with `merge --ff-only` there, which keeps uncommitted changes to other files and
/// refuses if the incoming commits touch one of them.
fn move_base(repo: &Repo, base: &str, old: &str, new: &str) -> Res<()> {
    if let Some(at) = checked_out_at(repo, base)? {
        let p = git(&at, &["merge", "-q", "--ff-only", new], false)?;
        if !p.ok() {
            let said = if p.stderr.is_empty() { &p.stdout } else { &p.stderr };
            return fail(format!(
                "merge refused: can't fast-forward {base} in {at}: {}",
                squash(said)
            ));
        }
        return Ok(());
    }
    if !git(&repo.root, &["merge-base", "--is-ancestor", old, new], false)?.ok() {
        return fail(format!("merge refused: {base} can't be fast-forwarded"));
    }
    let to = format!("refs/heads/{base}");
    git(&repo.root, &["update-ref", "-m", "bullpen accept", &to, new, old], true)?;
    Ok(())
}

#[cfg(test)]
#[path = "test_modes.rs"]
mod tests;
