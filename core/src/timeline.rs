//! A task's timeline: one line of JSON per event that matters, for `bullpen stats`,
//! in <state>/repos/<repo>/timeline/<ID>.jsonl. Nearly every entry comes from
//! `Repo::save_task`, which compares the task as it was with the task it saves
//! (`changes`): nothing that only reads tasks (ls, the board) writes here. Each entry
//! has `at` (an ISO stamp) and `event`:
//!
//!   added            the task was added (status: its first status)
//!   status           its status changed: from, to; stop (who stopped and how) and why
//!                    for a stopped one; ask for a question to the person; back (why it
//!                    was sent back to work: review, rebase, person, pr, limit); outcome
//!                    for a done one (accepted, closed, kept); risk (level, why) for a
//!                    ready one with a risk label
//!   turn_ended       its worker ended a turn
//!   review_started   a reviewer started: round, kind (full, light, followup, rebase,
//!                    rebased, tour)
//!   review_finished  a reviewer gave its verdict: round, kind, verdict
//!   accept_refused   an auto-accept didn't merge it: reason (conflict, checks, comments, other), why
//!
//! accept_refused changes nothing a save sees, so the auto-accept records it itself:
//! `record` is for an event no save shows. A step that sends a task back sets its
//! trip-back mark with `send_back` (the cli's `hand_back` takes its reason).

use crate::json::{obj, s, truthy, Obj};
use crate::task::clip;
use crate::timeutil::iso_now;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The task key a caller sets before a save that sends the task back to work, saying
/// why ("review", "rebase", "person", "pr", "limit"). The save takes it off the task.
pub const BACK: &str = "back";

/// Where a task's timeline is, in a repo's state folder.
pub fn path(dir: &Path, id: &str) -> PathBuf {
    dir.join("timeline").join(format!("{id}.jsonl"))
}

/// Mark the next save as sending the task back to work, for why.
pub fn send_back(t: &mut Obj, why: &str) {
    t.insert(BACK.into(), json!(why));
}

/// An entry stamped now.
pub fn entry(event: &str, fields: Value) -> Obj {
    let mut e = Obj::new();
    e.insert("at".into(), json!(iso_now()));
    e.insert("event".into(), json!(event));
    if let Value::Object(f) = fields {
        e.extend(f);
    }
    e
}

/// Add entries to a task's timeline. A timeline that can't be written isn't worth
/// failing the command over: it is left as it was.
pub fn append(dir: &Path, id: &str, entries: &[Obj]) {
    if entries.is_empty() || id.is_empty() {
        return;
    }
    let p = path(dir, id);
    let text: String = entries
        .iter()
        .map(|e| serde_json::to_string(e).unwrap_or_default() + "\n")
        .collect();
    let _ = p
        .parent()
        .map(std::fs::create_dir_all)
        .unwrap_or(Ok(()))
        .and_then(|_| std::fs::OpenOptions::new().create(true).append(true).open(&p))
        .and_then(|mut f| f.write_all(text.as_bytes()));
}

/// Record one event that no save shows.
pub fn record(dir: &Path, id: &str, event: &str, fields: Value) {
    append(dir, id, &[entry(event, fields)]);
}

/// A task's timeline, oldest first; None when it has none (a task from before
/// timelines). A line that doesn't read is skipped.
pub fn read(dir: &Path, id: &str) -> Option<Vec<Obj>> {
    let text = std::fs::read_to_string(path(dir, id)).ok()?;
    Some(
        text.lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| match v {
                Value::Object(o) => Some(o),
                _ => None,
            })
            .collect(),
    )
}

/// What a save changes, as timeline entries: old is the task as it was saved before
/// ({} for a new one), t the one being saved. Takes the back mark off t.
pub fn changes(old: &Obj, t: &mut Obj) -> Vec<Obj> {
    let back = t.shift_remove(BACK);
    let mut out = Vec::new();
    if old.is_empty() {
        out.push(entry("added", json!({"status": s(t, "status")})));
        return out;
    }
    if turn_ended(old, t) {
        out.push(entry("turn_ended", json!({})));
    }
    let (r0, r1) = (obj(old, "review"), obj(t, "review"));
    if review_finished(r0, r1) {
        out.push(entry(
            "review_finished",
            json!({"round": r1.get("round"), "kind": r1.get("kind"), "verdict": r1.get("verdict")}),
        ));
    }
    let (from, to) = (s(old, "status"), s(t, "status"));
    if from != to {
        out.push(status_entry(from, to, t, back.filter(|_| to == "working")));
    }
    if review_started(r0, r1) {
        out.push(entry(
            "review_started",
            json!({"round": r1.get("round"), "kind": r1.get("kind")}),
        ));
    }
    out
}

fn status_entry(from: &str, to: &str, t: &Obj, back: Option<Value>) -> Obj {
    let mut e = entry("status", json!({"from": from, "to": to}));
    if let Some(stop) = t.get("stop").filter(|v| truthy(Some(v))) {
        e.insert("stop".into(), stop.clone());
    }
    if truthy(t.get("ask")) {
        e.insert("ask".into(), json!(true));
    }
    if let Some(why) = t.get("why").and_then(Value::as_str).filter(|w| !w.is_empty()) {
        e.insert("why".into(), json!(clip(why, 160)));
    }
    if let Some(back) = back.filter(|b| truthy(Some(b))) {
        e.insert("back".into(), back);
    }
    if to == "done" {
        e.insert("outcome".into(), json!(outcome(t)));
    }
    if let Some((level, why)) = crate::risk::shown(t) {
        e.insert("risk".into(), json!({"level": level, "why": why}));
    }
    e
}

/// How a done task ended: "kept" (close --keep-branch), "closed" (finished, nothing
/// to merge) or "accepted".
pub fn outcome(t: &Obj) -> &'static str {
    if truthy(t.get("kept")) {
        "kept"
    } else if truthy(t.get("closed")) {
        "closed"
    } else {
        "accepted"
    }
}

/// The worker's last event went from working to the end of a turn. The end of a
/// compaction Claude Code did by itself is no end of a turn.
fn turn_ended(old: &Obj, t: &Obj) -> bool {
    let ended = |last: &str| last.starts_with("done|") || last.starts_with("idle|");
    s(old, "last").starts_with("working|") && ended(s(t, "last")) && !truthy(old.get("compacting"))
}

/// A new review: one with no verdict yet, of another round, kind or commit than the
/// last, or after it got its verdict, or a reviewer in a pane where there was none.
/// (Finding a running reviewer's pane again after a Rex restart is no new review.)
fn review_started(r0: &Obj, r1: &Obj) -> bool {
    if r1.is_empty() || truthy(r1.get("verdict")) || !truthy(r1.get("block")) {
        return false;
    }
    let other = ["round", "kind", "commit"].iter().any(|k| r0.get(*k) != r1.get(*k));
    other || truthy(r0.get("verdict")) || !truthy(r0.get("block"))
}

/// The review got its verdict.
fn review_finished(r0: &Obj, r1: &Obj) -> bool {
    truthy(r1.get("verdict"))
        && (r0.get("verdict") != r1.get("verdict")
            || r0.get("round") != r1.get("round")
            || r0.get("kind") != r1.get("kind"))
}

#[cfg(test)]
#[path = "test_timeline.rs"]
mod tests;
