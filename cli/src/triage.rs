//! The coordinator's commands for a task that stopped: `ask` and `ok`; and `note`.
//! `review` (a new reviewer) is in review.rs.

use crate::agents::running;
use crate::args::{pos, Arg, Args, Nargs};
use crate::news::kind::Kind;
use crate::news::tell_coordinator;
use crate::saves::save;
use bullpen_core::json::{self, s};
use bullpen_core::repo::repo_root;
use bullpen_core::task::{ask, is_closed, is_status, ok, waits_on};
use bullpen_core::text::squash;
use bullpen_core::{fail, Repo, Res};
use serde_json::json;

const ID: Arg = pos("id", Nargs::One, "the task, like BP-3");

const ASK: &[Arg] = &[ID, pos("question", Nargs::Some, "what the person should decide")];
const OK: &[Arg] = &[ID];
const NOTE: &[Arg] = &[ID, pos("text", Nargs::Some, "the line")];

/// `ask`: the task needs the person: the question shows on the board.
pub fn cmd_ask(argv: &[String]) -> Res<i32> {
    let a = Args::parse("ask", argv, ASK);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    if is_closed(s(&t, "status")) {
        return fail(is_status(&t));
    }
    let question = squash(&a.args("question").join(" "));
    if question.is_empty() {
        return fail("give the question to ask");
    }
    ask(&mut t, &question);
    save(&repo, &mut t)?;
    let id = s(&t, "id").to_string();
    tell_coordinator(&repo, &t, Kind::Ask, &format!("{id} needs the person: {question}"))?;
    println!("{id} needs the person now: {question}");
    Ok(0)
}

/// `ok`: a task's stop is seen to: back to what it was, typing nothing.
pub fn cmd_ok(argv: &[String]) -> Res<i32> {
    let a = Args::parse("ok", argv, OK);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let id = s(&t, "id").to_string();
    if s(&t, "status") != "triage" {
        return fail(format!("{}, not waiting on its coordinator", is_status(&t)));
    }
    if waits_on(&t) == Some("reviewer") {
        if !running(&t, None)? {
            return fail(format!(
                "{id}'s reviewer isn't running: `bullpen review {id}` starts a new one, or `bullpen ask` the person"
            ));
        }
    } else if !running(&t, None)? {
        return fail(format!(
            "{id}'s worker isn't running: `bullpen resume {id}` restarts it"
        ));
    }
    ok(&mut t); // `check` leaves its agent idle; its next turn clears that
    save(&repo, &mut t)?;
    let status = s(&t, "status");
    let words = if status == "review" { "in review" } else { status };
    println!("{id} is {words} again (nothing typed to its agent)");
    Ok(0)
}

/// `note`: an informational line on the board about a task (cleared when its status
/// changes). A question for the person is `ask`.
pub fn cmd_note(argv: &[String]) -> Res<i32> {
    let a = Args::parse("note", argv, NOTE);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    t.insert("note".into(), json!(squash(&a.args("text").join(" "))));
    json::write_obj(&repo.task_path(s(&t, "id")), &t)?; // not a save: the status didn't change
    println!("noted on {}", s(&t, "id"));
    Ok(0)
}
