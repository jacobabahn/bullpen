//! A task waiting on its coordinator (triage) that goes to the person once it is
//! overdue (bullpen/stops.py), as far as telling that it is.

use crate::coordinators::anyone_sees_to;
use crate::core::{stamp, Repo, Res};
use crate::json::{py_str, s, Obj};
use crate::timeutil::{seconds_since, Stamp};
use serde_json::Value;

/// (what the reason gets, why) when a task waiting on its coordinator goes to the
/// person now, else None.
pub fn overdue(repo: &Repo, t: &Obj, at: Option<Stamp>) -> Res<Option<(String, String)>> {
    if s(t, "status") != "triage" {
        return Ok(None);
    }
    if !anyone_sees_to(repo, t)? {
        return Ok(Some((" (no coordinator is running)".into(), "no coordinator is running".into())));
    }
    let m = repo.cfg.get("triage_minutes").unwrap_or(&Value::Null);
    let minutes = m.as_f64().unwrap_or(0.0);
    if seconds_since(stamp(t), at) >= minutes * 60.0 {
        let why = format!("its coordinator didn't resolve it in {} min", py_str(m));
        return Ok(Some((" (coordinator didn't resolve it)".into(), why)));
    }
    Ok(None)
}
