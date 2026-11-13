//! A task's worker or reviewer stopping (bullpen/stops.py): whether it goes to its
//! coordinator (triage) or to the person, and a triage passed on to the person once it
//! is overdue. Nothing in Rust stops a task yet: the watcher's events (T10) and the
//! commands that hand a task back (T6 on) call these. Python keeps its own (a twin).
//!
//! A task that stopped on something its coordinator can handle (a failed turn, a turn
//! ended without ready, a message that didn't reach its worker, a reviewer without a
//! verdict) is "triage": its coordinator hears of it, and settles it with tell, resume,
//! `review`, `ok` or `ask`. Still there after triage_minutes, or with no coordinator
//! running, it needs the person ("needs"), as prompts and `ask` do at once.
#![allow(dead_code)] // its callers move from T6 on

use crate::coordinators::sees_to;
use crate::news::say;
use crate::saves::save;
use bullpen_core::json::{py_str, s, Obj};
use bullpen_core::task::{set_status, stamp};
use bullpen_core::timeutil::{seconds_since, Stamp};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};

/// The task's worker or reviewer (by) stopped (kind; why in words). A prompt needs the
/// person. A worker whose pane is gone is stopped: resume restarts it. Anything else
/// waits on its coordinator, or on the person when no coordinator is running to see to
/// it. Saves the task, then sends each of lines as news (event), worded for whoever has
/// it now. Returns the task's new status.
pub fn stop(
    repo: &Repo,
    t: &mut Obj,
    by: &str,
    kind: &str,
    why: &str,
    lines: &[String],
    event: &str,
) -> Res<&'static str> {
    let m = py_str(triage_minutes(repo));
    let (status, say_it): (&'static str, Box<dyn Fn(&str) -> String>) = if kind == "prompt" {
        t.shift_remove("ask"); // the prompt is what it needs the person for now
        ("needs", Box::new(|line: &str| line.to_string()))
    } else if by == "worker" && matches!(kind, "exited" | "pane_gone") {
        ("stopped", Box::new(|line: &str| line.to_string()))
    } else if !sees_to(repo, t)?.is_empty() {
        let f = move |line: &str| format!("For the coordinator: {line} (still here in {m} min, it goes to the person)");
        ("triage", Box::new(f))
    } else {
        let f = |line: &str| format!("{line}. No coordinator is running to see to it, so it needs the person");
        ("needs", Box::new(f))
    };
    set_status(t, status, Some(why), Some(json!({"by": by, "kind": kind})));
    save(repo, t)?;
    for line in lines {
        say(repo, t, event, &say_it(line))?;
    }
    Ok(status)
}

/// Pass a task its coordinator didn't settle on to the person, with one news line.
/// late: (what its why gets, why it goes now), from overdue.
pub fn escalate(repo: &Repo, t: &mut Obj, late: &(String, String)) -> Res<()> {
    let why = format!("{}{}", Some(s(t, "why")).filter(|w| !w.is_empty()).unwrap_or("Stopped"), late.0);
    set_status(t, "needs", Some(&why), None);
    save(repo, t)?;
    say(repo, t, "needs-you", &format!("{} needs the person: {why}", s(t, "id")))
}

fn triage_minutes(repo: &Repo) -> &Value {
    repo.cfg.get("triage_minutes").unwrap_or(&Value::Null)
}

/// (what the reason gets, why) when a task waiting on its coordinator goes to the person
/// now, else None.
pub fn overdue(repo: &Repo, t: &Obj, at: Option<Stamp>) -> Res<Option<(String, String)>> {
    if s(t, "status") != "triage" {
        return Ok(None);
    }
    if sees_to(repo, t)?.is_empty() {
        return Ok(Some((" (no coordinator is running)".into(), "no coordinator is running".into())));
    }
    let m = triage_minutes(repo);
    let since = stamp(t).map_or_else(bullpen_core::timeutil::iso_now, String::from);
    if seconds_since(&since, at) >= m.as_f64().unwrap_or(0.0) * 60.0 {
        let why = format!("its coordinator didn't resolve it in {} min", py_str(m));
        return Ok(Some((" (coordinator didn't resolve it)".into(), why)));
    }
    Ok(None)
}
