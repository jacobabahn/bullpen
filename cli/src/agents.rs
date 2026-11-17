//! A task's agents: its worker and its reviewer, where each one's
//! pane is, and whether the one the task waits on (core's `waits_on`) is running.
//! A session or block Python has as None is "".

use bullpen_core::json::{obj, py_str, truthy, Obj};
use bullpen_core::task::waits_on;
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
