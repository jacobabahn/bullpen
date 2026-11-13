//! The usage limit (bullpen/limit.py), as far as `start` and the run queue need it: is
//! one in force, and when it resets. Reading a limit off an agent's screen moves with
//! the watcher's events (T10); Python keeps its own limit.py (a twin) for them.
//!
//! Every agent shares one subscription. <state>/limit.json holds the limit for every
//! repo while it lasts; `start` refuses meanwhile, and the queue starts nothing.

use bullpen_core::json::{self, s, truthy, Obj};
use bullpen_core::paths::state;
use bullpen_core::timeutil::{clock, parse, seconds_since, Stamp};
use bullpen_core::Res;
use serde_json::Value;

/// Seconds a limit with no known reset time holds `start` back.
pub const LIMIT_FORGET: f64 = 5.0 * 3600.0;

/// The usage limit in force ({"at", "resets", "text"}), or None. It holds until its
/// reset time; one with no known reset time, for LIMIT_FORGET seconds.
pub fn limit_active(at: Option<Stamp>) -> Res<Option<Obj>> {
    let lim = match json::read(&state().join("limit.json")) {
        Ok(Some(Value::Object(o))) => o,
        Ok(_) => return Ok(None),
        Err(e) => return bullpen_core::fail(e),
    };
    if lim.is_empty() {
        return Ok(None);
    }
    let holds = if truthy(lim.get("resets")) {
        seconds_since(s(&lim, "resets"), at) < 0.0
    } else {
        seconds_since(s(&lim, "at"), at) < LIMIT_FORGET
    };
    Ok(holds.then_some(lim))
}

/// "resets 3:00 PM" for a stored reset time, "reset time unknown" for none.
pub fn limit_phrase(resets: Option<&Value>, at: Option<Stamp>) -> String {
    match resets.and_then(Value::as_str).filter(|r| !r.is_empty()).and_then(parse) {
        Some(when) => format!("resets {}", clock(when, at)),
        None => "reset time unknown".into(),
    }
}

#[cfg(test)]
#[path = "test_limit.rs"]
mod tests;
