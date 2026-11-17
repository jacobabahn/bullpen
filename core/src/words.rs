//! What the board says about a task, for `ls`, `show` and the board: its mark, the board's order, its
//! age, and the board's sentence for where it stands. The board (board/src/model/detail.rs)
//! says these too, and keeps its own words only where they differ on purpose.
//!
//! A backlog task's `↻` (`from_mark`, `from_detail`); the board's sentence is `board_detail`,
//! made of `status_detail`, `with_gh`, `auto_detail`, `model_detail` and `doing_said`.

use crate::json::{self, get, obj, py_str, s, truthy, Obj};
use crate::paths::{safe, state};
use crate::task::{
    is_closed, is_limited, keeps_branch, nothing_to_merge, on_waited, own_model, pr_short, stamp, task_num,
};
use crate::text::{splitlines, squash, strip, upper_first};
use crate::timeutil::{self, clock, seconds_since, Stamp};
use crate::{Fail, Res};
use serde_json::Value;
use std::collections::HashMap;

/// Every task of a repo by ID, for what a task waits on.
pub type Every = HashMap<String, Obj>;

pub fn every(tasks: &[Obj]) -> Every {
    tasks.iter().map(|t| (s(t, "id").to_string(), t.clone())).collect()
}

/// Seconds a task can need you before its age turns red, as on the board.
pub const STALE: f64 = 300.0;
/// How old a note of what an agent is doing can be and still show.
pub const DOING_FOR: f64 = 30.0 * 60.0;

/// The status's mark, on the board and in `ls`.
pub fn mark(status: &str) -> &'static str {
    match status {
        "needs" => "●",
        "stopped" => "■",
        "triage" => "◎",
        "ready" => "✔",
        "finished" => "◆",
        "done" => "·", // dim, unlike ready's bold ✔
        "review" => "◐",
        "working" => "○",
        "todo" => "◌", // a dotted circle: not started yet
        "dropped" => "–",
        _ => " ",
    }
}

/// Every status with a mark, in task_line.MARKS' order.
pub const MARKED: [&str; 10] = [
    "needs", "stopped", "triage", "ready", "finished", "done", "review", "working", "todo", "dropped",
];

/// A count of tasks in words, as the board's top line, mission control and `repos` say
/// it: "1 needs you", "2 need you" (what: "needs"), else "3 ready", "4 running" (what
/// is the word).
pub fn count_phrase(n: usize, what: &str) -> String {
    match what {
        "needs" if n == 1 => "1 needs you".into(),
        "needs" => format!("{n} need you"),
        _ => format!("{n} {what}"),
    }
}

/// The board's groups, in order: what you do next first.
fn board_group(status: &str) -> u8 {
    match status {
        "needs" | "stopped" => 0,
        "triage" => 1,
        "ready" => 2,
        "finished" => 3,
        "review" => 4,
        "working" => 5,
        "todo" => 6,
        "done" | "dropped" => 7,
        _ => 8,
    }
}

/// Tasks as the board lists them: by group (needs you first), then by number.
pub fn board_order(mut tasks: Vec<Obj>) -> Vec<Obj> {
    tasks.sort_by_key(|t| (board_group(s(t, "status")), task_num(t)));
    tasks
}

/// The SGR colour of a task's mark: a quiet agent's in the colour of one that needs you.
pub fn mark_color(t: &Obj) -> &'static str {
    if is_stuck(t) {
        return "1;33";
    }
    match s(t, "status") {
        "needs" => "1;33",
        "stopped" => "1;31",
        "ready" | "finished" => "1;32",
        "review" => "1;34",
        _ => "2",
    }
}

/// "3m", "2h", "1d", "now", or "" with no time.
pub fn ago(t: &Obj, at: Option<Stamp>) -> String {
    let Some(st) = stamp(t) else { return String::new() };
    let secs = (seconds_since(st, at) as i64).max(0);
    for (unit, n) in [("d", 86400), ("h", 3600), ("m", 60)] {
        if secs >= n {
            return format!("{}{unit}", secs / n);
        }
    }
    "now".into()
}

/// Has it needed you long enough for its age to turn red?
pub fn stale(t: &Obj, at: Option<Stamp>) -> bool {
    s(t, "status") == "needs" && stamp(t).is_some_and(|st| seconds_since(st, at) >= STALE)
}

/// Has the agent the task waits on (its worker, or its reviewer in review) shown no
/// progress for too long?
pub fn is_stuck(t: &Obj) -> bool {
    on_waited(t, "stuck")
}

/// "no progress for 25 min", or "" for a task that isn't stuck.
pub fn stuck_phrase(t: &Obj, at: Option<Stamp>) -> String {
    if !is_stuck(t) {
        return String::new();
    }
    let m = (seconds_since(s(obj(t, "stuck"), "since"), at) / 60.0).floor() as i64;
    if m < 120 {
        format!("no progress for {m} min")
    } else {
        format!("no progress for {} h", m.div_euclid(60))
    }
}

/// Is the worker fixing failed checks or comments from its PR?
pub fn sent_back_by_github(t: &Obj) -> bool {
    truthy(obj(t, "gh").get("sent"))
}

/// The PR's latest state in a few words, for the board and `show`, or "". A working
/// task only shows what GitHub sent it back for: anything else is about the old push.
pub fn pr_phrase(t: &Obj) -> String {
    let g = obj(t, "gh");
    let state = s(g, "state");
    if state == "merged" || state == "closed" {
        return format!("{state} on GitHub");
    }
    if s(t, "status") == "working" && !sent_back_by_github(t) {
        return String::new();
    }
    let mut parts = Vec::new();
    if let Some(c) = get(g, "checks").filter(|v| truthy(Some(v))) {
        parts.push(format!("checks {}", py_str(c)));
    }
    if let Some(n) = get(g, "new").filter(|v| truthy(Some(v))) {
        let one = n.as_f64() == Some(1.0) || n == &Value::Bool(true);
        parts.push(format!("{} new comment{}", py_str(n), if one { "" } else { "s" }));
    }
    parts.join(", ")
}

/// start.open_deps: the tasks t waits on that aren't done, with their status (None: no such task).
/// A task closed with its branch kept is open still: its work never reached the base branch.
pub fn open_deps<'a>(t: &'a Obj, every: &'a Every) -> Vec<(String, Option<&'a str>)> {
    let after = t
        .get("after")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    after
        .iter()
        .map(py_str)
        .map(|d| {
            let o = every.get(&d);
            let st = o.and_then(|o| o.get("status")).and_then(Value::as_str);
            (d, st, o.is_some_and(|o| truthy(o.get("kept"))))
        })
        .filter(|(_, st, kept)| *st != Some("done") || *kept)
        .map(|(d, st, _)| (d, st))
        .collect()
}

/// start.waits_phrase: "Waits on BP-8, BP-9 (dropped)" for a todo task with open
/// dependencies, else "".
pub fn waits_phrase(t: &Obj, every: &Every) -> String {
    if s(t, "status") != "todo" {
        return String::new();
    }
    let deps = open_deps(t, every);
    if deps.is_empty() {
        return String::new();
    }
    let words: Vec<String> = deps
        .iter()
        .map(|(d, st)| match st {
            Some("dropped") => format!("{d} (dropped)"),
            Some("done") => format!("{d} (branch kept, not merged)"),
            None => format!("{d} (missing)"),
            _ => d.clone(),
        })
        .collect();
    format!("Waits on {}", words.join(", "))
}

/// The pane of the agent the task waits on now: its reviewer's in review, its worker's
/// while working.
pub fn agent_block(t: &Obj) -> Option<String> {
    let block = match s(t, "status") {
        "review" => obj(t, "review").get("block"),
        "working" => t.get("block"),
        _ => None,
    };
    block.filter(|b| truthy(Some(b))).map(py_str)
}

/// What the agent the task waits on last said it's doing, on one line, as the board
/// shows it in place of its sentence: "" when it hasn't said, isn't working, said it
/// before the task's status (less 30 s), over DOING_FOR ago, or is quiet or limited.
pub fn doing_now(t: &Obj, at: Option<Stamp>) -> Res<String> {
    let Some(block) = agent_block(t) else {
        return Ok(String::new());
    };
    if is_stuck(t) || is_limited(t) || truthy(t.get("note")) {
        return Ok(String::new());
    }
    let path = state().join("doing").join(format!("{}.json", safe(&block)));
    match json::read(&path) {
        Ok(Some(Value::Object(d))) => Ok(doing_said(t, &d, at)),
        Ok(_) => Ok(String::new()),
        Err(e) => Err(Fail(format!(
            "{} doesn't parse ({e}): fix it, or move it away",
            path.display()
        ))),
    }
}

/// doing_now with the agent's note read (<state>/doing/<block>.json: {"state", "text",
/// "at"}): what it says, or "" when it doesn't count.
pub fn doing_said(t: &Obj, d: &Obj, at: Option<Stamp>) -> String {
    if is_stuck(t) || is_limited(t) || truthy(t.get("note")) {
        return String::new();
    }
    let when = match d.get("at") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::Bool(b)) => *b as u8 as f64,
        _ => return String::new(),
    };
    if s(d, "state") != "working" {
        return String::new();
    }
    let secs = |x: Stamp| x.timestamp_micros() as f64 / 1e6;
    let since = stamp(t)
        .and_then(timeutil::parse)
        .map_or(f64::NEG_INFINITY, |x| secs(x) - 30.0);
    let now = secs(at.unwrap_or_else(timeutil::now));
    if when < since || now - when > DOING_FOR {
        return String::new();
    }
    squash(
        &get(d, "text")
            .filter(|v| truthy(Some(v)))
            .map(py_str)
            .unwrap_or_default(),
    )
}

/// The board's words for a review's kind.
pub fn board_kind(kind: Option<&Value>) -> Option<&'static str> {
    Some(match kind?.as_str()? {
        "full" => "full review",
        "light" => "light review",
        "followup" => "follow-up review",
        "tour" => "review of your comments",
        "rebase" => "rebase check",
        "rebased" => "review after a rebase",
        _ => return None,
    })
}

/// "Recorded the demo." from "# Recorded the demo", or "" when there's none.
pub fn first_sentence(summary: &str) -> String {
    for line in splitlines(summary) {
        let line = strip(strip(line).trim_start_matches('#'));
        if !line.is_empty() {
            return if line.ends_with(['.', '!', '?']) {
                line.into()
            } else {
                format!("{line}.")
            };
        }
    }
    String::new()
}

/// The sentence for a task stopped at the usage limit.
pub fn limit_detail(lim: &Obj, at: Option<Stamp>) -> String {
    let reviewer = s(lim, "who") == "reviewer";
    let who = if reviewer {
        "Reviewer stopped at the usage limit"
    } else {
        "Usage limit"
    };
    if let Some(resets) = get(lim, "resets").filter(|v| truthy(Some(v))) {
        let when = timeutil::parse(&py_str(resets)).unwrap_or_else(timeutil::now);
        return format!("{who}, resets {}. Continues by itself then.", clock(when, at));
    }
    format!(
        "{who}, reset time unknown. {}",
        if reviewer {
            "⏎ opens it."
        } else {
            "t tells it to continue."
        }
    )
}

/// A value as an f-string shows it: Python's str(), "" for a missing or false one (`x or ""`).
fn or_empty(t: &Obj, key: &str) -> String {
    t.get(key).filter(|v| truthy(Some(v))).map(py_str).unwrap_or_default()
}

/// The board's sentence for where the task stands, by the repo's settings (cfg). waits:
/// what a queued task waits on (waits_phrase).
pub fn status_detail(cfg: &Obj, t: &Obj, waits: &str, at: Option<Stamp>) -> String {
    let r = obj(t, "review");
    let rnd = r.get("round").filter(|v| truthy(Some(v))).map_or("1".into(), py_str);
    let why = or_empty(t, "why");
    let st = s(t, "status");
    let mut pr = pr_short(s(t, "pr"));
    if pr.is_empty() && truthy(t.get("local")) {
        pr = format!("Local branch {}", or_empty(t, "branch"));
    }
    if truthy(t.get("note")) {
        return py_str(&t["note"]);
    }
    if is_stuck(t) {
        let quiet = stuck_phrase(t, at);
        if s(obj(t, "stuck"), "who") == "reviewer" {
            return format!("Reviewer has shown {quiet}. ⏎ opens it.");
        }
        return format!("{}. p shows its screen.", upper_first(&quiet));
    }
    if is_limited(t) {
        return limit_detail(obj(t, "limit"), at);
    }
    let ended = why == "Ended its turn without saying ready";
    match st {
        "needs" if ended => "Stopped mid-task, probably with a question. p shows its screen.".into(),
        "triage" if ended => "Ended its turn without ready.".into(),
        "needs" if why.is_empty() => "Waiting for you.".into(),
        "triage" if why.is_empty() => "Waiting for its coordinator.".into(),
        "needs" | "triage" => why,
        "stopped" => "Its agent exited. r resumes it.".into(),
        "review" => {
            let kind = board_kind(r.get("kind")).unwrap_or(if truthy(r.get("kind")) { "review" } else { "reviewer" });
            let rounds = cfg.get("review_rounds").map_or("None".into(), py_str);
            format!("{} on round {rnd} of {rounds}.", upper_first(kind))
        }
        "ready" if nothing_to_merge(t) => "Done, with nothing to merge.".into(),
        "finished" => {
            let closes = if keeps_branch(t) {
                "a closes it, keeping the branch."
            } else {
                "a closes it."
            };
            format!("{} {closes}", finished_detail(t))
        }
        "ready" => {
            let text = match s(r, "verdict") {
                "approved" => format!("{pr}, approved by {}.", board_kind(r.get("kind")).unwrap_or("review")),
                "changes" => format!("{pr}. Review still asks for changes."),
                _ if !pr.is_empty() => format!("{pr}."),
                _ => "Done.".into(),
            };
            let text = match get(t, "rebased").filter(|v| truthy(Some(v))) {
                Some(b) => format!("{text} Rebased onto {}.", py_str(b)),
                None => text,
            };
            match crate::risk::shown(t) {
                Some((level, "")) => format!("{text} {} risk.", upper_first(level)),
                Some((level, why)) => format!("{text} {} risk: {why}.", upper_first(level)),
                None => text,
            }
        }
        "working" if obj(t, "job").contains_key("since") => match s(obj(t, "job"), "what") {
            "" => "Waiting on its background job: it takes another turn when that ends.".into(),
            what => format!("Waiting on its job ({what}): it takes another turn when that ends."),
        },
        "working" if sent_back_by_github(t) => "Fixing what GitHub said.".into(),
        "working" if s(r, "verdict") == "changes" => format!("Fixing what review round {rnd} found."),
        "working" if s(r, "verdict") == "approved" && truthy(r.get("conflict")) => {
            format!(
                "Approved, rebasing onto {}: its branch conflicted.",
                or_empty(t, "base")
            )
        }
        "todo" if waits.is_empty() => "Not started. s starts it.".into(),
        "todo" => format!("{waits}."),
        "done" if truthy(t.get("kept")) => kept_detail(obj(t, "kept")),
        "done" if truthy(t.get("local")) && !truthy(t.get("pr")) => {
            if truthy(t.get("merged")) && truthy(t.get("base")) {
                format!("Accepted. Merged into {}.", py_str(&t["base"]))
            } else {
                "Accepted.".into()
            }
        }
        "done" if truthy(t.get("closed")) => "Closed, nothing to merge.".into(),
        "done" if !pr.is_empty() => format!("Accepted. {pr} merged."),
        "done" => "Accepted.".into(),
        "dropped" => "Dropped.".into(),
        _ => String::new(),
    }
}

/// A finished task: nothing to merge, or its branch to be kept, then its summary's
/// first sentence.
pub fn finished_detail(t: &Obj) -> String {
    let what = if keeps_branch(t) {
        "Finished, branch to be kept"
    } else {
        "Finished, nothing to merge"
    };
    match first_sentence(&or_empty(t, "summary")).as_str() {
        "" => format!("{what}."),
        line => format!("{what}: {line}"),
    }
}

/// A task closed with its branch kept (`close --keep-branch`): where the branch is.
pub fn kept_detail(kept: &Obj) -> String {
    let branch = s(kept, "branch");
    match s(kept, "remote") {
        "" => format!("Closed, branch kept here: {branch}, not pushed."),
        remote => format!("Closed, branch kept: {branch} on {remote}."),
    }
}

/// A task's sentence, then what GitHub last said of its PR (not for a closed task, or
/// one with a note).
pub fn with_gh(t: &Obj, text: String) -> String {
    let gh = if is_closed(s(t, "status")) || truthy(t.get("note")) {
        String::new()
    } else {
        pr_phrase(t)
    };
    if gh.is_empty() {
        return text;
    }
    strip(&format!("{text} {}.", upper_first(&gh))).to_string()
}

/// "Model: opus." for an open task with a worker model of its own, else "".
pub fn model_detail(cfg: &Obj, t: &Obj) -> String {
    match own_model(cfg, t) {
        Some(m) if !is_closed(s(t, "status")) => format!("Model: {m}."),
        _ => String::new(),
    }
}

/// What auto-accept does with it, when that isn't what the config says for all.
pub fn auto_detail(cfg: &Obj, t: &Obj) -> &'static str {
    let on = truthy(cfg.get("auto_accept"));
    let st = s(t, "status");
    if is_closed(st) {
        ""
    } else if truthy(t.get("auto_wait")) && st == "ready" {
        "Auto-accepts once its checks pass."
    } else if on && t.get("auto_accept") == Some(&Value::Bool(false)) {
        "Kept for you: no auto-accept."
    } else if !on && t.get("auto_accept") == Some(&Value::Bool(true)) {
        "Auto-accepts once approved."
    } else {
        ""
    }
}

/// Did the task come from the backlog (`bullpen later L7 --start`, or pickup)? It has an
/// `origin` then.
pub fn from_backlog(t: &Obj) -> bool {
    !s(t, "origin").is_empty()
}

/// What follows a task's mark (before its ID) on the board, in `ls` and in `pick`: a
/// dim "↻" for one that came from the backlog, else the usual space.
pub fn from_mark(t: &Obj) -> &'static str {
    if from_backlog(t) {
        "↻"
    } else {
        " "
    }
}

/// Where an open task from the backlog came from: "From the backlog (L7).", or for one
/// pickup started, "Started by itself (L7)."; "" for any other.
pub fn from_detail(t: &Obj) -> String {
    if !from_backlog(t) || is_closed(s(t, "status")) {
        return String::new();
    }
    let item = match s(t, "from") {
        "" => String::new(),
        f => format!(" ({f})"),
    };
    match s(t, "started_by") {
        "itself" => format!("Started by itself{item}."),
        _ => format!("From the backlog{item}."),
    }
}

/// The board's sentence for a task: what its agent is doing now, else where it stands,
/// what GitHub said, what auto-accept does with it and its own model.
pub fn board_detail(cfg: &Obj, t: &Obj, every: &Every, at: Option<Stamp>) -> Res<String> {
    let doing = doing_now(t, at)?;
    Ok(board_sentence(cfg, t, every, at, doing))
}

/// board_detail with what its agent is doing already read (doing_said's, "" for nothing).
pub fn board_sentence(cfg: &Obj, t: &Obj, every: &Every, at: Option<Stamp>, doing: String) -> String {
    if !doing.is_empty() {
        return doing;
    }
    let text = with_gh(t, status_detail(cfg, t, &waits_phrase(t, every), at));
    let model = model_detail(cfg, t);
    from_around(t, &[text.as_str(), auto_detail(cfg, t), model.as_str()])
}

/// A task's sentence from its parts, with where it came from: first for one that
/// started itself (that's what to notice), else last.
pub fn from_around(t: &Obj, parts: &[&str]) -> String {
    let from = from_detail(t);
    let mut all: Vec<&str> = parts.to_vec();
    if s(t, "started_by") == "itself" {
        all.insert(0, &from);
    } else {
        all.push(&from);
    }
    all.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[path = "test_words.rs"]
mod tests;
