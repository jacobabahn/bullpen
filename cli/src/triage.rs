//! The coordinator's `ok` and `note` (bullpen/triage.py). `ask` and `review` stay with
//! Python: they notify the person, type news or start a reviewer.

use crate::args::Args;
use crate::core::{fail, is_status, repo_root, reviewers_stop, set_status, Repo, Res};
use crate::json::{self, obj, s, truthy, Obj};
use crate::rex;
use crate::text::squash;
use serde_json::{json, Value};

/// agents.running: is the pane of the agent the triaged task waits on alive?
fn running(t: &Obj, reviewer: bool) -> Res<bool> {
    if reviewer {
        let r = obj(t, "review");
        let session = if truthy(r.get("session")) { s(r, "session") } else { s(t, "session") };
        rex::alive(session, s(r, "block"))
    } else {
        rex::alive(s(t, "session"), s(t, "block"))
    }
}

/// A task's stop is seen to: back to what it was, typing nothing.
pub fn cmd_ok(a: &Args) -> Res<()> {
    let repo = Repo::new(&repo_root()?)?;
    let mut t = repo.task(a.arg(0).unwrap_or(""))?;
    if s(&t, "status") != "triage" {
        return fail(format!("{}, not waiting on its coordinator", is_status(&t)));
    }
    let id = s(&t, "id").to_string();
    // agents.waits_on, in triage: the reviewer when the stop is its own, else the worker
    // (the usage limit counts only while working or in review)
    let reviewer = reviewers_stop(&t);
    if reviewer && !running(&t, true)? {
        return fail(format!(
            "{id}'s reviewer isn't running: `bullpen review {id}` starts a new one, or `bullpen ask` the person"
        ));
    }
    if !reviewer && !running(&t, false)? {
        return fail(format!("{id}'s worker isn't running: `bullpen resume {id}` restarts it"));
    }
    // core.ok: `check` leaves its agent idle; its next turn clears that
    set_status(&mut t, if reviewer { "review" } else { "working" }, None, None);
    t.insert("ok".into(), json!(true));
    repo.save_task(&mut t)?;
    let status = s(&t, "status");
    let words = if status == "review" { "in review" } else { status };
    println!("{id} is {words} again (nothing typed to its agent)");
    Ok(())
}

/// An informational line on the board about a task (cleared when its status changes).
pub fn cmd_note(a: &Args) -> Res<()> {
    let repo = Repo::new(&repo_root()?)?;
    let mut t = repo.task(a.arg(0).unwrap_or(""))?;
    t.insert("note".into(), json!(squash(&a.pos[1..].join(" "))));
    // not save_task: the status didn't change
    json::write(&repo.task_path(s(&t, "id")), &Value::Object(t.clone()))?;
    println!("noted on {}", s(&t, "id"));
    Ok(())
}
