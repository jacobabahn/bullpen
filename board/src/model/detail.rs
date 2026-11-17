//! Time, and the sentences under each task: what it needs, where it stands, what
//! its agent is doing. Most are bullpen-core's (words.rs), which `ls` and `show` say
//! too; the board's own are those it says differently on purpose: a finished task's
//! (its key is on the bottom line), when a task its coordinator is on comes to you,
//! and where a ready task's tour stands.

use bullpen_core::timeutil::{parse, seconds_since, Stamp};
use bullpen_core::words;
use super::*;

pub use bullpen_core::timeutil::now;

/// "PR #12" for a pull request's URL; anything else as it is ("" for none).
pub fn pr_short(url: Option<&str>) -> String {
    bullpen_core::task::pr_short(url.unwrap_or(""))
}

/// Seconds since it entered its current status (0 when that isn't known).
pub fn seconds_in_status(t: &Task, at: Stamp) -> f64 {
    t.stamp().map_or(0.0, |s| seconds_since(s, Some(at)))
}

/// "3m", "2h", "1d", "now", or "" with no time.
pub fn ago(t: &Task, at: Stamp) -> String {
    words::ago(t.obj(), Some(at))
}

/// The PR's latest state in a few words, or "". A working task only shows what
/// GitHub sent it back for: anything else is about the old push.
pub fn pr_phrase(t: &Task) -> String {
    words::pr_phrase(t.obj())
}

/// What the task needs or where it stands, as a plain sentence, then what GitHub
/// last said about its PR.
pub fn task_detail(t: &Task, cfg: &Config, at: Stamp) -> String {
    words::with_gh(t.obj(), status_detail(t, cfg, at))
}

/// Where a ready task's tour stands, as a nudge (a tour is never needed to accept),
/// and a tour being written of one in review: "" otherwise, and when a failed one's
/// note says why already.
pub fn tour_phrase(t: &Task) -> &'static str {
    if t.tour_writing() {
        return "Writing its tour…";
    }
    if t.status != "ready" || t.nothing_to_merge() {
        return "";
    }
    match t.tour.as_ref().map(|r| r.state.as_str()) {
        _ if t.toured() => "Toured.",
        Some("written") if t.tour.as_ref().and_then(|r| r.stale) != Some(true) => "Tour written, not opened yet.",
        Some("failed") => "",
        _ => "Not toured yet.",
    }
}

/// When a task its coordinator is on comes to you, if its coordinator doesn't settle
/// it first: "Yours in 7 min.", "Yours any moment." (bullpen moves it then), or ""
/// for a task that isn't waiting on its coordinator. `minutes` is the config's
/// triage_minutes.
pub fn yours_phrase(t: &Task, minutes: u64, at: Stamp) -> String {
    let Some(since) = t.stamp().and_then(parse).filter(|_| t.triage()) else { return String::new() };
    let left = minutes as i64 * 60 - (at - since).num_seconds();
    let m = (left + 59) / 60; // a part of a minute counts as one
    match m {
        ..=0 => "Yours any moment.".into(),
        1..=119 => format!("Yours in {m} min."),
        _ => format!("Yours in {} h.", m / 60),
    }
}

/// task_detail, then when a task its coordinator is on comes to you, what
/// auto-accept does with it, the model it chose and where it came from (the backlog).
pub fn detail_with_auto(t: &Task, cfg: &Config, at: Stamp) -> String {
    let text = task_detail(t, cfg, at);
    let yours = yours_phrase(t, cfg.triage_minutes, at);
    let model = words::model_detail(cfg.obj(), t.obj());
    let parts = [text.as_str(), yours.as_str(), words::auto_detail(cfg.obj(), t.obj()), model.as_str()];
    words::from_around(t.obj(), &parts).trim().to_string()
}

/// Where the task stands: bullpen-core's sentence, but a finished task's without
/// "a closes it." (the bottom line shows its keys).
fn status_detail(t: &Task, cfg: &Config, at: Stamp) -> String {
    if t.status == "finished" && t.note.as_deref().unwrap_or("").is_empty() {
        return words::finished_detail(t.obj());
    }
    words::status_detail(cfg.obj(), t.obj(), &t.waits, Some(at))
}

/// The size of a task's change, for one in review or ready: "3 files, +120 −14".
pub fn task_stat(t: &Task) -> &str {
    match t.status.as_str() {
        "review" | "ready" => t.diffstat.as_deref().unwrap_or(""),
        _ => "",
    }
}

/// The line under a task in review or ready: the size of its change, then where its
/// tour stands: "3 files, +120 −14 · Not toured yet".
pub fn stat_line(t: &Task) -> String {
    let tour = tour_phrase(t).trim_end_matches('.');
    [task_stat(t), tour].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" · ")
}

/// What the agent the task waits on is doing now, on one line: "" when it hasn't
/// said, has stopped working, said it before the task's current status (with a
/// little slack: the first words come before bullpen records the status), said it
/// over DOING_FOR ago, or has gone quiet or hit the usage limit.
pub fn doing_now(t: &Task, at: Stamp) -> String {
    t.doing.as_ref().map_or(String::new(), |d| words::doing_said(t.obj(), d, Some(at)))
}

#[cfg(test)]
#[path = "test_detail.rs"]
mod tests;
