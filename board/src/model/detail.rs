//! Time, and the sentences under each task: what it needs, where it stands, what
//! its agent is doing.

use chrono::{DateTime, FixedOffset, Local};
use super::*;

pub fn pr_short(url: Option<&str>) -> String {
    let url = url.unwrap_or("");
    if let Some(i) = url.find("/pull/") {
        let n: String = url[i + 6..].chars().take_while(char::is_ascii_digit).collect();
        if !n.is_empty() {
            return format!("PR #{n}");
        }
    }
    url.to_string()
}

fn parse(stamp: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(stamp).ok()
}

pub fn seconds_in_status(t: &Task, at: DateTime<FixedOffset>) -> i64 {
    t.stamp().and_then(parse).map_or(0, |s| (at - s).num_seconds())
}

pub fn ago(stamp: Option<&str>, at: DateTime<FixedOffset>) -> String {
    let Some(s) = stamp.and_then(parse) else { return String::new() };
    let s = (at - s).num_seconds().max(0);
    for (unit, n) in [("d", 86400), ("h", 3600), ("m", 60)] {
        if s >= n {
            return format!("{}{unit}", s / n);
        }
    }
    "now".into()
}

pub fn now() -> DateTime<FixedOffset> {
    Local::now().fixed_offset()
}

/// "no progress for 25 min", or "" for a task that isn't stuck.
pub fn stuck_phrase(t: &Task, at: DateTime<FixedOffset>) -> String {
    let Some(s) = t.stuck.as_ref().filter(|_| t.is_stuck()) else { return String::new() };
    let m = parse(&s.since).map_or(0, |since| (at - since).num_seconds() / 60);
    if m < 120 {
        format!("no progress for {m} min")
    } else {
        format!("no progress for {} h", m / 60)
    }
}

/// "3:00 PM", with the day when it isn't today: "Mon 12:00 AM", "Oct 9 3:00 PM".
pub fn clock(when: DateTime<FixedOffset>, at: DateTime<FixedOffset>) -> String {
    let (when, at) = (when.with_timezone(&Local), at.with_timezone(&Local));
    let text = when.format("%-I:%M %p").to_string();
    match (when.date_naive() - at.date_naive()).num_days() {
        0 => text,
        1..=6 => format!("{} {text}", when.format("%a")),
        _ => format!("{} {text}", when.format("%b %-d")),
    }
}

/// The board's line for a task stopped at the usage limit.
fn limit_detail(lim: &Limit, at: DateTime<FixedOffset>) -> String {
    let reviewer = lim.who == "reviewer";
    let who = if reviewer { "Reviewer stopped at the usage limit" } else { "Usage limit" };
    if let Some(resets) = lim.resets.as_deref().and_then(parse) {
        return format!("{who}, resets {}. Continues by itself then.", clock(resets, at));
    }
    let next = if reviewer { "⏎ opens it." } else { "t tells it to continue." };
    format!("{who}, reset time unknown. {next}")
}

/// The PR's latest state in a few words, or "". A working task only shows what
/// GitHub sent it back for: anything else is about the old push.
pub fn pr_phrase(t: &Task) -> String {
    let Some(g) = &t.gh else { return String::new() };
    if let Some(state) = g.state.as_deref().filter(|s| matches!(*s, "merged" | "closed")) {
        return format!("{state} on GitHub");
    }
    if t.status == "working" && !t.sent_back_by_github() {
        return String::new();
    }
    let mut parts = Vec::new();
    if let Some(c) = g.checks.as_deref().filter(|c| !c.is_empty()) {
        parts.push(format!("checks {c}"));
    }
    if let Some(n) = g.new.filter(|n| *n > 0) {
        parts.push(format!("{n} new comment{}", if n == 1 { "" } else { "s" }));
    }
    parts.join(", ")
}

/// What the task needs or where it stands, as a plain sentence, then what GitHub
/// last said about its PR.
pub fn task_detail(t: &Task, rounds: u32, at: DateTime<FixedOffset>) -> String {
    let text = status_detail(t, rounds, at);
    let gh = if t.closed() { String::new() } else { pr_phrase(t) };
    if gh.is_empty() || t.note.as_deref().is_some_and(|n| !n.is_empty()) {
        return text;
    }
    format!("{text} {}.", capitalize(&gh)).trim().to_string()
}

/// What auto-accept does with the task, when it isn't what the config says for all,
/// or it's waiting: "" otherwise. `on` is the config's auto_accept.
pub fn auto_phrase(t: &Task, on: bool) -> &'static str {
    if t.closed() {
        ""
    } else if t.auto_wait == Some(true) && t.status == "ready" {
        "Auto-accepts once its checks pass."
    } else if on && t.auto_accept == Some(false) {
        "Kept for you: no auto-accept."
    } else if !on && t.auto_accept == Some(true) {
        "Auto-accepts once approved."
    } else {
        ""
    }
}

/// The worker model the task chose, when it isn't the config's: "Model: opus." or "".
pub fn model_phrase(t: &Task, cfg: &Config) -> String {
    match t.worker_model.as_deref() {
        Some(m) if !m.is_empty() && !t.closed() && Some(m) != cfg.worker_model.as_deref() => format!("Model: {m}."),
        _ => String::new(),
    }
}

/// Where a ready task's tour stands, as a nudge (a tour is never needed to accept),
/// and a tour being written of one in review: "" otherwise, and when a failed one's
/// note says why already.
pub fn tour_phrase(t: &Task) -> &'static str {
    if t.tour_writing() {
        return "Writing its tour…";
    }
    if t.status != "ready" || t.why.as_deref().unwrap_or("").starts_with("Nothing to merge") {
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
pub fn yours_phrase(t: &Task, minutes: u64, at: DateTime<FixedOffset>) -> String {
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
/// auto-accept does with it and the model it chose.
pub fn detail_with_auto(t: &Task, cfg: &Config, at: DateTime<FixedOffset>) -> String {
    let text = task_detail(t, cfg.review_rounds, at);
    let yours = yours_phrase(t, cfg.triage_minutes, at);
    let model = model_phrase(t, cfg);
    let parts = [text.as_str(), yours.as_str(), auto_phrase(t, cfg.auto_accept), model.as_str()];
    parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" ").trim().to_string()
}

pub fn capitalize(text: &str) -> String {
    let mut c = text.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn ready_detail(pr: &str, r: &Review) -> String {
    match r.verdict.as_deref() {
        Some("approved") => format!("{pr}, approved by {}.", r.kind_words()),
        Some("changes") => format!("{pr}. Review still asks for changes."),
        _ if !pr.is_empty() => format!("{pr}."),
        _ => "Done.".into(),
    }
}

fn status_detail(t: &Task, rounds: u32, at: DateTime<FixedOffset>) -> String {
    let r = t.review.clone().unwrap_or_default();
    let round = r.round.unwrap_or(1);
    let verdict = r.verdict.as_deref();
    let why = t.why.as_deref().unwrap_or("");
    let has_pr = t.pr.as_deref().is_some_and(|p| !p.is_empty());
    let mut pr = pr_short(t.pr.as_deref());
    if pr.is_empty() && t.is_local() {
        pr = format!("Local branch {}", t.branch.as_deref().unwrap_or(""));
    }
    if let Some(note) = t.note.as_deref().filter(|n| !n.is_empty()) {
        return note.into();
    }
    if t.is_stuck() {
        let quiet = stuck_phrase(t, at);
        if t.stuck.as_ref().is_some_and(|s| s.who == "reviewer") {
            return format!("Reviewer has shown {quiet}. ⏎ opens it.");
        }
        return format!("{}. p shows its screen.", capitalize(&quiet));
    }
    if let Some(lim) = t.limit.as_ref().filter(|_| t.is_limited()) {
        return limit_detail(lim, at);
    }
    match t.status.as_str() {
        "needs" if why == "Ended its turn without saying ready" => {
            "Stopped mid-task, probably with a question. p shows its screen.".into()
        }
        "needs" if why.is_empty() => "Waiting for you.".into(),
        "needs" => why.into(),
        "triage" if why == "Ended its turn without saying ready" => "Ended its turn without ready.".into(),
        "triage" if why.is_empty() => "Waiting for its coordinator.".into(),
        "triage" => why.into(),
        "stopped" => "Its agent exited. r resumes it.".into(),
        "review" if r.kind.is_some() => {
            format!("{} on round {round} of {rounds}.", capitalize(r.kind_words()))
        }
        "review" => format!("Reviewer on round {round} of {rounds}."),
        "ready" if why.starts_with("Nothing to merge") => "Done, with nothing to merge.".into(),
        "finished" => match first_line(t.summary.as_deref()).as_str() {
            "" => "Finished, nothing to merge.".into(),
            line => format!("Finished, nothing to merge: {line}"),
        },
        "ready" => match &t.rebased {
            Some(base) => format!("{} Rebased onto {base}.", ready_detail(&pr, &r)),
            None => ready_detail(&pr, &r),
        },
        "working" if t.sent_back_by_github() => "Fixing what GitHub said.".into(),
        "working" if verdict == Some("changes") => format!("Fixing what review round {round} found."),
        "todo" if !t.waits.is_empty() => format!("Waits on {}.", t.waits.join(", ")),
        "todo" => "Not started. s starts it.".into(),
        "done" if t.is_local() && !has_pr => match (&t.merged, &t.base) {
            (Some(_), Some(base)) => format!("Accepted. Merged into {base}."),
            _ => "Accepted.".into(),
        },
        "done" if t.closed == Some(true) => "Closed, nothing to merge.".into(),
        "done" if !pr.is_empty() => format!("Accepted. {pr} merged."),
        "done" => "Accepted.".into(),
        "dropped" => "Dropped.".into(),
        _ => String::new(),
    }
}

/// The first line of a summary as a sentence: "Recorded the demo." from "# Recorded
/// the demo", or "" when there's none.
fn first_line(summary: Option<&str>) -> String {
    let line = summary.unwrap_or("").lines().map(|l| l.trim().trim_start_matches('#').trim()).find(|l| !l.is_empty());
    match line {
        Some(l) if l.ends_with(['.', '!', '?']) => l.to_string(),
        Some(l) => format!("{l}."),
        None => String::new(),
    }
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

/// How old a note of what an agent is doing can be and still show.
pub const DOING_FOR: i64 = 30 * 60;

/// What the agent the task waits on is doing now, on one line: "" when it hasn't
/// said, has stopped working, said it before the task's current status (with a
/// little slack: the first words come before bullpen records the status), said it
/// over DOING_FOR ago, or has gone quiet or hit the usage limit.
pub fn doing_now(t: &Task, at: DateTime<FixedOffset>) -> String {
    let Some(d) = t.doing.as_ref().filter(|d| d.state == "working") else { return String::new() };
    if t.is_stuck() || t.is_limited() || t.note.as_deref().is_some_and(|n| !n.is_empty()) {
        return String::new();
    }
    let since = t.stamp().and_then(parse).map_or(i64::MIN, |s| s.timestamp() - 30);
    let now = at.timestamp();
    if d.at < since || now - d.at > DOING_FOR {
        return String::new();
    }
    d.text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "test_detail.rs"]
mod tests;
