//! Answering a task's prompt from the board, and replying to what the coordinator asked:
//! `prompt`, `answer` and `reply`.
//!
//! A task whose agent is at a prompt can be answered from the board: `prompt` asks its
//! host for the question and its options (Rex reads them off the pane's screen,
//! screen/prompt.rs), and `answer` gives one of them, once it has asked again and found
//! the same question there. A coordinator's own pane is read and answered the same way
//! (`--coordinator`, whose_pane.rs). A task the coordinator asked the person about (`ask`) is answered with `reply`:
//! the answer goes to the coordinator, never to the worker.

use crate::agents::{agent, running};
use crate::args::{pos, usage_error, value, Arg, Args, Nargs};
use crate::coordinators::sees_to;
use crate::host::Agent;
use crate::news::kind::Kind;
use crate::news::{news, Opts};
use crate::saves::save;
use crate::start::peek::py_int;
use crate::whose_pane::{named, Pane, COORDINATOR, REPO};
use bullpen_core::json::{dumps_line, obj, s, truthy, Obj};
use bullpen_core::repo::repo_root;
use bullpen_core::task::{clip, ok, set_status, waits_on};
use bullpen_core::text::squash;
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};

const ID: Arg = pos("id", Nargs::One, "the task, like BP-3");
const TASK: Arg = pos("id", Nargs::Maybe, "the task, like BP-3 (none with --coordinator)");

const ANSWER: &[Arg] = &[
    TASK,
    pos("key", Nargs::One, "the option: its number, or y or n"),
    value(
        "--prompt",
        "ID",
        "refuse unless it is still this prompt (its id from `prompt`)",
    ),
    COORDINATOR,
    REPO,
];
const REPLY: &[Arg] = &[ID, pos("answer", Nargs::Some, "the answer")];
const PROMPT: &[Arg] = &[
    TASK,
    value("--lines", "LINES", "how many lines of screen (default 40)"),
    COORDINATOR,
    REPO,
];

/// (who, agent) the task is stopped on at a prompt. Fails when it isn't at one, or its
/// agent isn't running.
pub fn prompt_pane(t: &Obj) -> Res<(&'static str, Agent)> {
    let id = s(t, "id");
    let who = waits_on(t);
    let Some(who) = who.filter(|_| s(t, "status") == "needs" && s(obj(t, "stop"), "kind") == "prompt") else {
        return fail(format!("{id} isn't waiting at a prompt"));
    };
    let agent = agent(t, who);
    if !agent.alive()? {
        return fail(format!("{id}'s {who} isn't running"));
    }
    Ok((who, agent))
}

/// The pane cmd's arguments name, at a prompt: who is there (worker, reviewer or
/// coordinator), whose it is in the messages (BP-3's worker, c1 in proj) and its agent.
/// A task fails unless it is stopped at a prompt; a coordinator's pane is read as it is.
fn at_prompt(cmd: &str, spec: &[Arg], a: &Args) -> Res<(&'static str, String, Agent)> {
    Ok(match named(cmd, spec, a)? {
        (_, Pane::Task(t)) => {
            let (who, agent) = prompt_pane(&t)?;
            (who, format!("{}'s {who}", s(&t, "id")), agent)
        }
        (repo, Pane::Coordinator(cid, agent)) => ("coordinator", format!("{cid} in {}", repo.name()), agent),
    })
}

/// `prompt`: the prompt a task's agent (or a coordinator) is at, with the end of its
/// screen (in colour), as JSON for the board. "question" is null when the screen shows
/// none now. "draft": something is half-typed in its input box (never while a question
/// shows: there is no input box then).
pub fn cmd_prompt(argv: &[String]) -> Res<i32> {
    let a = Args::parse("prompt", argv, PROMPT);
    let n = match a.opt("--lines") {
        None => 40,
        Some(v) => py_int(v)
            .unwrap_or_else(|| usage_error("prompt", PROMPT, &format!("argument --lines: invalid int value: '{v}'"))),
    };
    let (who, _, agent) = at_prompt("prompt", PROMPT, &a)?;
    let mut out = json!({"who": who});
    let o = out.as_object_mut().expect("an object");
    let question = agent.question()?;
    let draft = question.is_none() && agent.has_draft()?;
    match question {
        Some(p) => {
            let options: Vec<Value> = p.options.iter().map(|(k, l)| json!([k, l])).collect();
            o.insert("question".into(), json!(p.question));
            o.insert("options".into(), json!(options));
            o.insert("id".into(), json!(p.id));
        }
        None => {
            o.insert("question".into(), Value::Null);
            o.insert("options".into(), json!([]));
            o.insert("id".into(), Value::Null);
        }
    }
    o.insert("screen".into(), json!(agent.read_end(n, true)?));
    o.insert("draft".into(), json!(draft));
    println!("{}", dumps_line(&out));
    Ok(0)
}

/// `answer`: press one of the options of the prompt a task's agent (or a coordinator) is
/// at, as the person would in its pane. Refuses unless its screen still shows that
/// prompt (--prompt, its id from `prompt`).
pub fn cmd_answer(argv: &[String]) -> Res<i32> {
    let a = Args::parse("answer", argv, ANSWER);
    let (_, whose, agent) = at_prompt("answer", ANSWER, &a)?;
    let p = agent.question()?;
    let Some(p) = p.filter(|p| a.opt("--prompt").is_none_or(|want| p.id == want)) else {
        return fail(format!("{whose} isn't at that prompt any more: nothing was sent"));
    };
    let given = a.arg("key").unwrap_or("");
    let key = given.to_lowercase();
    let Some((_, label)) = p.options.iter().find(|(k, _)| *k == key) else {
        let keys: Vec<&str> = p.options.iter().map(|(k, _)| k.as_str()).collect();
        return fail(format!(
            "{given} isn't one of the prompt's options ({}): nothing was sent",
            keys.join(", ")
        ));
    };
    agent.answer(&key)?;
    println!("answered {whose}: {key} ({label})");
    Ok(0)
}

/// `reply`: the person's answer to what the coordinator asked about a task (`ask`). It
/// goes to the task's coordinator as "BP-3: <answer>": typed into its pane, or as news
/// with config --news monitor. A task asked about while it wasn't stopped goes back to
/// the status it had then. A stopped one leaves needs as `ok` would, or goes to its
/// coordinator when its agent isn't running.
pub fn cmd_reply(argv: &[String]) -> Res<i32> {
    let a = Args::parse("reply", argv, REPLY);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let id = s(&t, "id").to_string();
    if s(&t, "status") != "needs" || !truthy(t.get("ask")) {
        return fail(format!("{id} has no question from its coordinator to answer"));
    }
    let text = squash(&a.args("answer").join(" "));
    if text.is_empty() {
        return fail("give the answer");
    }
    if sees_to(&repo, &t)?.is_empty() {
        return fail(format!(
            "no coordinator is running to take the answer: `bullpen tell {id}` tells its worker instead"
        ));
    }
    let question = t
        .get("why")
        .filter(|v| truthy(Some(v)))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if truthy(t.get("asked_from")) {
        let was = obj(&t, "asked_from").clone();
        // it wasn't stopped: back to what it was doing
        set_status(&mut t, s(&was, "status"), was.get("why").and_then(Value::as_str), None);
    } else if running(&t, None)? {
        ok(&mut t);
    } else {
        set_status(
            &mut t,
            "triage",
            Some(&clip(&format!("The person answered: {text}"), 160)),
            None,
        );
    }
    save(&repo, &mut t)?;
    let typed = format!("{id}: {text}");
    let o = Opts {
        typed: Some(&typed),
        ..Opts::default()
    };
    let line = format!("{id}: the person answers \"{}\": {text}", clip(&question, 120));
    news(&repo, &t, Kind::Answer, &line, o)?;
    println!("sent {id}'s answer to its coordinator");
    Ok(0)
}
