//! A ready task that has no risk label gets one, late (docs/review.md).
//!
//! `ready` and `verdict` label a task as they make it ready (risk.rs). One made ready by
//! a copy of bullpen from before the label (a task's brief names the `bin/bullpen` of
//! the copy that started it, and its reviewer's too) has no `risk` in its record at
//! all, and its ready news said none. `check` finds it (`unlabelled`), and `check --fix`
//! (`label_late`, with risk.rs's `assess`; the watcher,
//! every few minutes, from the copy the coordinator uses) labels it as risk.rs would
//! have, says the label in one news line, and notes the copy the task's brief names for
//! doctor's `copy` check (that copy is too old to note itself). A record that has `risk`, a label or null
//! (nothing to merge, no worktree), is left alone, so it happens once per task.

use crate::modes::task_base_ref;
use crate::news::say;
use crate::risk::{assess, Said};
use bullpen_core::json::{obj, s, write_obj, Obj};
use bullpen_core::risk::{label, stored};
use bullpen_core::task::nothing_to_merge;
use bullpen_core::{Repo, Res};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Is the task ready with a change, and with no word about its risk in its record?
pub fn unlabelled(t: &Obj) -> bool {
    s(t, "status") == "ready" && !t.contains_key("risk") && !nothing_to_merge(t)
}

/// Where the findings of the round that approved the task are, or None (no approval).
/// A rebase check keeps the round it follows, and that round's findings have the
/// reviewer's risk.
fn approval_path(briefs: &Path, t: &Obj) -> Option<PathBuf> {
    let r = obj(t, "review");
    if s(r, "verdict") != "approved" {
        return None;
    }
    if s(r, "kind") != "rebase" {
        return Some(PathBuf::from(s(r, "findings")));
    }
    let round = r.get("round").and_then(Value::as_i64)?;
    Some(briefs.join(format!("{}.review-{round}.findings.md", s(t, "id"))))
}

/// The copy of bullpen whose `ready` the task's worker was told to run: its brief names
/// that copy's `bin/bullpen` (`<copy>/bin/bullpen ready BP-3 --summary-file ...`).
fn told_copy(brief: &str, tid: &str) -> Option<PathBuf> {
    let after = format!(" ready {tid} ");
    let line = brief.lines().find(|l| l.contains(&after))?;
    let bin = line.split(&after).next()?.trim().trim_matches(['\'', '"']);
    let copy = bin.strip_suffix("/bin/bullpen").filter(|c| c.starts_with('/'))?;
    Some(PathBuf::from(copy))
}

/// Label the task now, if it is still unlabelled, and tell its coordinator the label.
/// True when it got one.
pub fn label_late(repo: &Repo, tid: &str) -> Res<bool> {
    let mut t = repo.task(tid)?;
    if !unlabelled(&t) {
        return Ok(false);
    }
    let base_ref = task_base_ref(repo, &t)?;
    let text = approval_path(&repo.dir.join("briefs"), &t).and_then(|p| std::fs::read_to_string(p).ok());
    let said = text.as_deref().map_or(Said::Nothing, Said::Round);
    assess(repo, &mut t, &base_ref, said)?;
    // the diff took a while: only the label goes onto the task as it is now
    let mut now = repo.task(tid)?;
    if !unlabelled(&now) {
        return Ok(false);
    }
    now.insert("risk".into(), t.get("risk").cloned().unwrap_or(Value::Null));
    write_obj(&repo.task_path(tid), &now)?; // not a save: its status didn't change
    let Some((level, why)) = stored(&now) else {
        return Ok(false); // no worktree to read a diff from: it has none, and that is kept
    };
    let text = format!(
        "{tid} became ready without a risk label, so here it is: {}. Tell the person",
        label(level, why)
    );
    say(repo, &now, "risk", &text)?;
    // the copy that made it ready knows no label: doctor's `copy` says which, if it isn't ours
    let brief = std::fs::read_to_string(repo.dir.join("briefs").join(format!("{tid}.full.md")));
    if let Some(copy) = brief.ok().and_then(|b| told_copy(&b, tid)) {
        // when it became ready, not now: the task may have waited long for its label
        let at = Some(s(&now, "since")).filter(|at| !at.is_empty());
        crate::copies::note_copy(repo, &copy, "ready", at.unwrap_or(&bullpen_core::timeutil::iso_now()));
    }
    Ok(true)
}

#[cfg(test)]
#[path = "test_risk_late.rs"]
mod tests;
