//! A task's totals from its timeline (bullpen_core::timeline): the time it spent in
//! each phase, its review rounds, its trips back and its stalls.

use bullpen_core::json::{obj, s, Obj};
use bullpen_core::task::is_closed;
use bullpen_core::timeutil::{parse, Stamp};
use serde_json::{json, Value};

/// The phases a task's time goes to, in the order stats shows them: key and words.
pub const PHASES: [(&str, &str); 8] = [
    ("working", "working"),
    ("rebasing", "rebasing"),
    ("review_wait", "waiting for review"),
    ("review", "in review"),
    ("person", "waiting for the person"),
    ("coordinator", "waiting for the coordinator"),
    ("stalled", "stalled"),
    ("queued", "queued"),
];

/// Where a task is, as its timeline goes.
#[derive(Default)]
struct State {
    status: String,
    stop_kind: String,
    rebasing: bool,
    reviewing: bool,
}

impl State {
    /// The phase its time goes to now.
    fn phase(&self) -> Option<&'static str> {
        Some(match self.status.as_str() {
            "todo" => "queued",
            "working" if self.rebasing => "rebasing",
            "working" => "working",
            "review" if self.reviewing => "review",
            "review" => "review_wait",
            "ready" | "finished" | "needs" => "person",
            "triage" if self.stop_kind == "ended" => "coordinator",
            "triage" | "stopped" => "stalled",
            _ => return None, // closed, or not known
        })
    }

    fn apply(&mut self, e: &Obj) {
        match s(e, "event") {
            "added" => self.status = s(e, "status").into(),
            "status" => {
                self.status = s(e, "to").into();
                self.stop_kind = s(obj(e, "stop"), "kind").into();
                if s(e, "back") == "rebase" {
                    self.rebasing = true;
                } else if !matches!(self.status.as_str(), "working" | "triage" | "needs" | "stopped") {
                    self.rebasing = false; // ready again, or closed
                }
                if self.status == "working" {
                    self.reviewing = false; // the next review is a new one
                }
            }
            "review_started" => self.reviewing = true,
            "review_finished" => self.reviewing = false,
            _ => {}
        }
    }
}

fn at(e: &Obj) -> Option<Stamp> {
    parse(s(e, "at"))
}

fn seconds(a: Stamp, b: Stamp) -> i64 {
    (b - a).num_seconds().max(0)
}

/// Add n to counts[key].
fn bump(counts: &mut Obj, key: &str) {
    let n = counts.get(key).and_then(Value::as_i64).unwrap_or(0);
    counts.insert(key.into(), json!(n + 1));
}

/// Was this status entry a trip back to work: why, or None. A ready (or in-review,
/// finished) task whose worker took a new turn of its own goes back too ("other").
/// Carrying on after the usage limit is a stall, not a trip back.
fn trip_back(e: &Obj) -> Option<String> {
    if s(e, "to") != "working" {
        return None;
    }
    match s(e, "back") {
        "limit" => None,
        "" if matches!(s(e, "from"), "review" | "ready" | "finished") => Some("other".into()),
        "" => None,
        why => Some(why.into()),
    }
}

/// The totals of a task with a timeline (events, oldest first), up to now for an open
/// one: time per phase (seconds), turns, questions, reviews, trips back, stalls and
/// refused accepts. A timeline that doesn't start with the task's adding is partial:
/// its times count from its first entry.
pub fn totals(events: &[Obj], now: Stamp) -> Obj {
    let mut time = Obj::new();
    for (k, _) in PHASES {
        time.insert(k.into(), json!(0));
    }
    let (mut reviews, mut trips, mut refused) = (Obj::new(), Obj::new(), Obj::new());
    let (mut turns, mut questions, mut stalls, mut rounds) = (0, 0, 0, 0);
    let mut state = State::default();
    let mut last: Option<Stamp> = None;
    for e in events {
        let Some(t) = at(e) else { continue };
        if let (Some(from), Some(phase)) = (last, state.phase()) {
            let n = time[phase].as_i64().unwrap_or(0) + seconds(from, t);
            time.insert(phase.into(), json!(n));
        }
        last = Some(t);
        match s(e, "event") {
            "turn_ended" => turns += 1,
            "review_started" => {
                let kind = s(e, "kind");
                bump(&mut reviews, if kind.is_empty() { "review" } else { kind });
                if !matches!(kind, "rebase" | "tour") {
                    rounds += 1; // a rebase check or a tour's review keeps the round it follows
                }
            }
            "accept_refused" => bump(&mut refused, s(e, "reason")),
            "status" => {
                let stop = s(obj(e, "stop"), "kind");
                if s(e, "to") == "needs" {
                    questions += 1;
                } else if matches!(s(e, "to"), "triage" | "stopped") && stop != "ended" {
                    stalls += 1;
                }
                if let Some(why) = trip_back(e) {
                    bump(&mut trips, &why);
                }
            }
            _ => {}
        }
        state.apply(e);
    }
    if let (Some(from), Some(phase)) = (last, state.phase()) {
        let n = time[phase].as_i64().unwrap_or(0) + seconds(from, now);
        time.insert(phase.into(), json!(n));
    }
    let mut out = Obj::new();
    out.insert("time".into(), Value::Object(time));
    out.insert("turns".into(), json!(turns));
    out.insert("questions".into(), json!(questions));
    out.insert("review_rounds".into(), json!(rounds));
    out.insert("reviews".into(), Value::Object(reviews));
    out.insert("trips_back".into(), Value::Object(trips));
    out.insert("stalls".into(), json!(stalls));
    out.insert("accept_refused".into(), Value::Object(refused));
    out
}

/// How a closed task ended: "accepted", "branch kept", "closed" or "dropped"; None
/// for an open one.
pub fn outcome(t: &Obj) -> Option<&'static str> {
    match s(t, "status") {
        "dropped" => Some("dropped"),
        "done" => Some(match bullpen_core::timeline::outcome(t) {
            "kept" => "branch kept",
            o => o,
        }),
        _ => None,
    }
}

/// When the task closed (when it entered done or dropped), if it did.
pub fn closed_at(t: &Obj) -> Option<Stamp> {
    if !is_closed(s(t, "status")) {
        return None;
    }
    bullpen_core::task::stamp(t).and_then(parse)
}

/// (added, removed) lines from a task's diffstat ("3 files, +40 −12"), as its last
/// ready saw them.
pub fn lines(t: &Obj) -> Option<(i64, i64)> {
    let stat = s(t, "diffstat");
    let num = |mark: char| -> Option<i64> {
        let i = stat.find(mark)?;
        let digits: String = stat[i + mark.len_utf8()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    };
    Some((num('+')?, num('−').or_else(|| num('-'))?))
}

#[cfg(test)]
#[path = "test_totals.rs"]
mod tests;
