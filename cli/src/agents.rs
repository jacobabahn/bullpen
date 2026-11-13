//! A task's agents (bullpen/agents.py): its worker and its reviewer, where each one's
//! pane is, and the one the task waits on. A session or block Python has as None is "".

use bullpen_core::json::{obj, py_str, s, truthy, Obj};
use bullpen_core::task::{is_waiting, reviewers_stop};
use bullpen_core::Res;
use serde_json::Value;

/// A value as the str a pane's session or block is ("" for none).
fn text(v: Option<&Value>) -> String {
    v.filter(|v| truthy(Some(v))).map_or(String::new(), py_str)
}

/// (session, block) of the task's worker or reviewer pane; block "" when it has none.
/// A reviewer runs in its worker's session unless its own is recorded.
pub fn pane(t: &Obj, who: &str) -> (String, String) {
    if who == "reviewer" {
        let r = obj(t, "review");
        let session = if truthy(r.get("session")) {
            text(r.get("session"))
        } else {
            text(t.get("session"))
        };
        return (session, text(r.get("block")));
    }
    (text(t.get("session")), text(t.get("block")))
}

/// The placement the task's worker or reviewer pane was made with ("float", "tabs",
/// "background"), or "" for one made before bullpen recorded it.
pub fn placed(t: &Obj, who: &str) -> String {
    let o = if who == "reviewer" { obj(t, "review") } else { t };
    o.get("pane").filter(|v| !v.is_null()).map_or(String::new(), py_str)
}

/// Is the agent the task is waiting on stopped at the usage limit?
pub fn is_limited(t: &Obj) -> bool {
    let lim = obj(t, "limit");
    if !truthy(t.get("limit")) {
        return false;
    }
    let waits = if s(lim, "who") == "reviewer" {
        "review"
    } else {
        "working"
    };
    s(t, "status") == waits
}

/// Is the task stopped on its reviewer (a prompt, or no verdict)?
pub fn on_reviewer(t: &Obj) -> bool {
    is_waiting(s(t, "status")) && reviewers_stop(t)
}

/// "reviewer" while the task is in review or its stop is the reviewer's (but not a
/// usage-limit stop, seen to as its worker's), "worker" while it's working or stopped
/// on anything else, else None. The usage limit makes no difference here.
pub fn whose_turn(t: &Obj) -> Option<&'static str> {
    let st = s(t, "status");
    if st == "review" || on_reviewer(t) {
        Some("reviewer")
    } else if st == "working" || is_waiting(st) {
        Some("worker")
    } else {
        None
    }
}

/// "worker" or "reviewer": the agent the task waits on now (whose_turn), or None when it
/// waits on the usage limit, or on no agent.
pub fn waits_on(t: &Obj) -> Option<&'static str> {
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

/// Is the pane of the task's worker or reviewer (by default the one it waits on) alive?
pub fn running(t: &Obj, who: Option<&str>) -> Res<bool> {
    let Some(who) = who.or_else(|| waits_on(t)) else {
        return Ok(false);
    };
    let (session, block) = pane(t, who);
    crate::rex::alive(&session, &block)
}

#[cfg(test)]
#[path = "test_agents.rs"]
mod tests;
