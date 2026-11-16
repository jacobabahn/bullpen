//! `close --keep-branch` (the person's call): end a task whose work stays on a side
//! branch and never merges (an experiment, a port tried out). Its branch is pushed first
//! (to origin in remote mode, else to the push remote), never forced, and a failed push
//! refuses, so the work can't be lost; with neither, the local branch stays. Then the
//! close step, keeping the branch: the task is done, with `kept` saying where.

use crate::close::{close, waiting_dep};
use crate::modes::{push_branch, push_remote, task_base_ref, withdraw};
use crate::news::tell_coordinator;
use crate::push_remote::push_reason;
use bullpen_core::json::{obj, s, Obj};
use bullpen_core::proc::git;
use bullpen_core::settings::CLOSED;
use bullpen_core::task::is_status;
use bullpen_core::text::strip;
use bullpen_core::words::kept_detail;
use bullpen_core::{fail, Repo, Res};
use serde_json::json;
use std::path::Path;

/// End t without merging, its branch kept (as name, if given), pushed first if there's
/// a remote to push to.
pub fn keep_branch(repo: &Repo, t: &mut Obj, name: Option<&str>) -> Res<()> {
    let id = s(t, "id").to_string();
    if CLOSED.contains(&s(t, "status")) {
        return fail(is_status(t));
    }
    let branch = s(t, "branch").to_string();
    let local_ref = format!("refs/heads/{branch}");
    if branch.is_empty() || !git(&repo.root, &["rev-parse", "-q", "--verify", &local_ref], false)?.ok() {
        return fail(format!("{id} has no branch to keep: `drop {id}` ends it"));
    }
    let wt = s(t, "worktree").to_string();
    if !wt.is_empty()
        && Path::new(&wt).exists()
        && !strip(&git(&wt, &["status", "--porcelain"], true)?.stdout).is_empty()
    {
        return fail(format!(
            "{id} has uncommitted changes in {wt}: commit them (they're kept with the branch) or discard them, \
             then run this again"
        ));
    }
    let base_ref = task_base_ref(repo, t)?;
    let ahead = git(
        &repo.root,
        &["rev-list", "--count", &format!("{base_ref}..{branch}")],
        false,
    )?;
    if ahead.ok() && strip(&ahead.stdout) == "0" {
        return fail(format!(
            "{id}'s branch has no commits on {base_ref}, so there's nothing to keep: `drop {id}` ends it"
        ));
    }
    let name = name.unwrap_or(&branch).to_string();
    if !git(&repo.root, &["check-ref-format", "--branch", &name], false)?.ok() {
        return fail(format!("{name} isn't a name git takes for a branch"));
    }
    let named = format!("refs/heads/{name}");
    if name != branch && git(&repo.root, &["rev-parse", "-q", "--verify", &named], false)?.ok() {
        return fail(format!(
            "there's a branch {name} here already: pick another name with --as"
        ));
    }
    let remote = keep_remote(repo)?;
    if let Some(remote) = &remote {
        push(repo, &id, &branch, &name, remote)?;
    }
    // the PR (remote mode) is closed; its branch on origin goes only when kept under another name
    withdraw(repo, t, name != branch)?;
    t.insert("kept".into(), json!({"branch": name, "remote": remote}));
    t.shift_remove("keep_branch"); // `finish --keep-branch` asked for this: done
    let said = |t: &Obj| -> Res<()> {
        if name != branch {
            git(&repo.root, &["branch", "-m", &branch, &name], true)?;
        }
        let kept = kept_detail(obj(t, "kept"));
        match &remote {
            Some(_) => println!("{id} closed. {kept}"),
            None => println!(
                "{id} closed. {kept} There's no remote to push it to: `bullpen config --push-remote <remote>` sets one"
            ),
        }
        tell_coordinator(repo, t, "kept", &format!("{id} closed without merging. {kept}"))?;
        waiting_dep(repo, t, "closed with its branch kept, not merged")
    };
    close(repo, t, "done", said, false, false, &[])
}

/// Where a kept branch goes: origin in remote mode, else the push remote, else nowhere.
fn keep_remote(repo: &Repo) -> Res<Option<String>> {
    if repo.has_origin()? {
        return Ok(Some("origin".into()));
    }
    push_remote(repo)
}

/// Push branch to remote as name, never forced. A failure refuses: nothing has changed.
fn push(repo: &Repo, id: &str, branch: &str, name: &str, remote: &str) -> Res<()> {
    if !git(&repo.root, &["remote", "get-url", remote], false)?.ok() {
        return fail(format!(
            "{id} isn't closed: the push remote {remote} isn't a remote here (`git remote -v`)"
        ));
    }
    let p = push_branch(repo, remote, branch, name)?;
    if p.ok() {
        return Ok(());
    }
    let out = if p.stderr.is_empty() { &p.stdout } else { &p.stderr };
    let rejected = ["[rejected]", "non-fast-forward", "fetch first"]
        .iter()
        .any(|w| out.contains(w));
    let why = if rejected {
        format!("{remote} has a branch {name} with commits this one doesn't (bullpen never force-pushes): pick another name with --as")
    } else {
        push_reason(out, remote, name)
    };
    fail(format!(
        "{id} isn't closed, so its work stays: {branch} wasn't pushed to {remote}: {why}"
    ))
}
