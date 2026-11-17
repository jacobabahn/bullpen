//! A task an accept left in review (rebase.rs: a reviewer was on it) is checked against
//! its base when that review ends with an approval. It goes by `show`'s test, a trial
//! merge: a task that conflicts gets what a ready task that conflicts gets after an
//! accept (rebased keeping both sides of a trivial conflict, else sent back to its
//! worker to rebase), with the same news; one that doesn't is left as it is and is ready
//! as usual. Findings leave it to its worker, as always.

use super::rebase::{conflicts_with, rebase_task};
use crate::modes::{is_local, quiet_git, task_base_ref};
use crate::news::{news, Opts};
use bullpen_core::json::{py_str, s, truthy, write_obj, Obj};
use bullpen_core::proc::git;
use bullpen_core::text::strip;
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::path::Path;

/// The task's key for the accepts that left it in review, in order.
const KEY: &str = "left_in_review";

/// Note on the task that the accept of aid left it in review (not a save: the status
/// doesn't change). Each accept during one review adds itself.
pub fn mark(repo: &Repo, t: &mut Obj, aid: &str) -> Res<()> {
    let mut by = ids(t);
    if !by.iter().any(|b| b == aid) {
        by.push(aid.to_string());
    }
    t.insert(KEY.into(), json!(by));
    write_obj(&repo.task_path(s(t, "id")), t)?;
    Ok(())
}

/// The accepts that left the task in review, taken off it: a verdict ends that review.
pub fn take(t: &mut Obj) -> Vec<String> {
    let by = ids(t);
    t.shift_remove(KEY);
    by
}

fn ids(t: &Obj) -> Vec<String> {
    match t.get(KEY) {
        Some(Value::Array(a)) => a.iter().map(py_str).collect(),
        _ => Vec::new(),
    }
}

/// "BP-3", "BP-3 and BP-5", "BP-3, BP-5 and BP-6".
pub fn and_list(ids: &[String]) -> String {
    match ids {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// An approved task (saved ready) that the accepts by left in review: when its branch
/// conflicts with its base now, bring it up to date as the after-accept step does, with
/// its `After BP-3: ...` news. Returns whether the task's ready news is still to be said
/// (and an auto-accept still to run): true unless it was sent back or is in review again.
pub fn after_approval(repo: &Repo, t: &mut Obj, by: &[String]) -> Res<bool> {
    if by.is_empty() || !truthy(repo.cfg.get("rebase_others")) {
        return Ok(true);
    }
    let Some(base_ref) = conflicting(repo, t)? else {
        return Ok(true); // rebases cleanly: left as it is (rebase only when there are conflicts)
    };
    let (id, words) = (s(t, "id").to_string(), and_list(by));
    let line = rebase_task(repo, t, &words, &base_ref).unwrap_or_else(|e| format!("{id} not rebased ({e})"));
    let everyone = Opts {
        everyone: true,
        ..Opts::default()
    };
    news(repo, t, "rebased", &format!("After {words}: {line}."), everyone)?;
    Ok(s(t, "status") == "ready")
}

/// The task's base ref when its committed branch conflicts with it now (fetched first,
/// with a remote), else None, or when that can't be told or it can't be rebased here
/// (no worktree, uncommitted changes).
fn conflicting(repo: &Repo, t: &Obj) -> Res<Option<String>> {
    let wt = s(t, "worktree");
    if wt.is_empty() || !Path::new(wt).is_dir() {
        return Ok(None);
    }
    if !is_local(repo, t)? {
        let base = py_str(t.get("base").unwrap_or(&Value::Null));
        quiet_git(repo, &["fetch", "-q", "origin", &base], 60)?; // unfetched: it goes by what it has
    }
    let base_ref = task_base_ref(repo, t)?;
    if conflicts_with(wt, &base_ref, "HEAD")?.is_none()
        || !strip(&git(wt, &["status", "--porcelain"], true)?.stdout).is_empty()
    {
        return Ok(None);
    }
    Ok(Some(base_ref))
}

#[cfg(test)]
#[path = "test_left_in_review.rs"]
mod tests;
