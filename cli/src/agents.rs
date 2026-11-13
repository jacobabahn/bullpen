//! A task's agents (bullpen/agents.py): its worker and its reviewer, where each one's
//! pane is, and the one the task waits on.

use crate::core::reviewers_stop;
use crate::json::{obj, py_str, truthy, Obj};
use crate::task_line::is_limited;
use serde_json::Value;

/// core.WAITING: a task stopped on something, waiting on its coordinator or the person.
pub fn is_waiting(status: &str) -> bool {
    matches!(status, "needs" | "triage")
}

/// A value as the str a pane's session or block is ("" for none).
fn text(v: Option<&Value>) -> String {
    v.filter(|v| truthy(Some(v))).map_or(String::new(), py_str)
}

/// (session, block) of the task's worker or reviewer pane; block "" when it has none.
/// A reviewer runs in its worker's session unless its own is recorded.
pub fn pane(t: &Obj, who: &str) -> (String, String) {
    if who == "reviewer" {
        let r = obj(t, "review");
        let session = if truthy(r.get("session")) { text(r.get("session")) } else { text(t.get("session")) };
        return (session, text(r.get("block")));
    }
    (text(t.get("session")), text(t.get("block")))
}

/// "reviewer" while the task is in review or stopped on its reviewer, "worker" while it's
/// working or stopped on anything else, else None.
fn whose_turn(t: &Obj) -> Option<&'static str> {
    let st = crate::json::s(t, "status");
    if st == "review" || (is_waiting(st) && reviewers_stop(t)) {
        Some("reviewer")
    } else if st == "working" || is_waiting(st) {
        Some("worker")
    } else {
        None
    }
}

/// The agent the task waits on now, or None: the usage limit, or no agent.
fn waits_on(t: &Obj) -> Option<&'static str> {
    if is_limited(t) {
        None
    } else {
        whose_turn(t)
    }
}

/// (who, session, block) of the agent the task waits on now, or None: it waits on none,
/// or that agent has no pane.
pub fn waited_on(t: &Obj) -> Option<(&'static str, String, String)> {
    let who = waits_on(t)?;
    let (session, block) = pane(t, who);
    (!block.is_empty()).then_some((who, session, block))
}
