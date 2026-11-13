//! Answering a task's prompt from the board, and replying to what the coordinator asked
//!: `prompt`, `answer` and `reply`.
//!
//! A task whose agent is at a prompt can be answered from the board: `prompt` reads the
//! question and its options off the pane's screen (screen/prompt.rs), and `answer`
//! presses one of them, once it has read the screen again and found the same question
//! there. A task the coordinator asked the person about (`ask`) is answered with `reply`:
//! the answer goes to the coordinator, never to the worker.

use crate::agents::{pane, running, waits_on};
use crate::args::{pos, usage_error, value, Arg, Args, Nargs};
use crate::coordinators::sees_to;
use crate::news::{news, Opts};
use crate::rex::{alive, screen, screen_end};
use crate::saves::save;
use crate::screen::prompt::read_prompt;
use crate::start::peek::py_int;
use bullpen_core::json::{dumps_line, obj, s, truthy, Obj};
use bullpen_core::paths::rex;
use bullpen_core::proc::run;
use bullpen_core::repo::repo_root;
use bullpen_core::task::{clip, ok, set_status};
use bullpen_core::text::squash;
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};

const ID: Arg = pos("id", Nargs::One, "the task, like BP-3");

const ANSWER: &[Arg] = &[
    ID,
    pos("key", Nargs::One, "the option: its number, or y or n"),
    value(
        "--prompt",
        "ID",
        "refuse unless it is still this prompt (its id from `prompt`)",
    ),
];
const REPLY: &[Arg] = &[ID, pos("answer", Nargs::Some, "the answer")];
const PROMPT: &[Arg] = &[ID, value("--lines", "LINES", "how many lines of screen (default 40)")];

/// (who, session, block) of the agent the task is stopped on at a prompt. Fails when it
/// isn't at one, or its pane is gone.
pub fn prompt_pane(t: &Obj) -> Res<(&'static str, String, String)> {
    let id = s(t, "id");
    let who = waits_on(t);
    let Some(who) = who.filter(|_| s(t, "status") == "needs" && s(obj(t, "stop"), "kind") == "prompt") else {
        return fail(format!("{id} isn't waiting at a prompt"));
    };
    let (session, block) = pane(t, who);
    if !alive(&session, &block)? {
        return fail(format!("{id}'s {who} isn't running"));
    }
    Ok((who, session, block))
}

/// `prompt`: the prompt a task's agent is at, with the end of its screen (in colour), as
/// JSON for the board. "question" is null when the screen shows none now.
pub fn cmd_prompt(argv: &[String]) -> Res<i32> {
    let a = Args::parse("prompt", argv, PROMPT);
    let n = match a.opt("--lines") {
        None => 40,
        Some(v) => py_int(v)
            .unwrap_or_else(|| usage_error("prompt", PROMPT, &format!("argument --lines: invalid int value: '{v}'"))),
    };
    let repo = Repo::new(repo_root()?)?;
    let t = repo.task(a.arg("id").unwrap_or(""))?;
    let (who, session, block) = prompt_pane(&t)?;
    let mut out = json!({"who": who});
    let o = out.as_object_mut().expect("an object");
    match read_prompt(&screen(&session, &block)?) {
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
    o.insert("screen".into(), json!(screen_end(&session, &block, n, true)?));
    println!("{}", dumps_line(&out));
    Ok(0)
}

/// `answer`: press one of the options of the prompt a task's agent is at, as the person
/// would in its pane. Refuses unless its screen still shows that prompt (--prompt, its
/// id from `prompt`).
pub fn cmd_answer(argv: &[String]) -> Res<i32> {
    let a = Args::parse("answer", argv, ANSWER);
    let repo = Repo::new(repo_root()?)?;
    let t = repo.task(a.arg("id").unwrap_or(""))?;
    let id = s(&t, "id");
    let (who, session, block) = prompt_pane(&t)?;
    let p = read_prompt(&screen(&session, &block)?);
    let Some(p) = p.filter(|p| a.opt("--prompt").is_none_or(|want| p.id == want)) else {
        return fail(format!("{id}'s {who} isn't at that prompt any more: nothing was sent"));
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
    let mut argv = vec![rex(), "send".into()];
    if !session.is_empty() {
        argv.extend(["-s".into(), session.clone()]);
    }
    argv.extend(["-b".into(), block, key.clone()]);
    run(&argv, true)?;
    println!("answered {id}'s {who}: {key} ({label})");
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
    news(&repo, &t, "answer", &line, o)?;
    println!("sent {id}'s answer to its coordinator");
    Ok(0)
}
