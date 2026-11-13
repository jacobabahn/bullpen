//! `check`: each task's status against what its agent is doing now (bullpen/check.py),
//! the report only. `--fix` applies the missed events through the watcher's paths and
//! news, so it stays with Python.

use crate::agents::{is_waiting, waited_on};
use crate::args::Args;
use crate::core::{git, is_closed, known_repos, repo_root, stamp, Fail, Repo, Res};
use crate::events::{compacting, shell_running, stripped_lines};
use crate::json::{py_str, s, truthy, Obj};
use crate::modes::task_base_ref;
use crate::rex::{self, program_status, screen};
use crate::stops::overdue;
use crate::text::{split, strip};
use crate::timeutil::seconds_since;
use serde_json::Value;

/// Seconds a new idle, blocked or error state is left alone: its event may still be on the way.
const SETTLE: f64 = 60.0;
/// core.DEFAULT_CONFIG's stuck_after, for a config that turns it off (0).
const STUCK_AFTER: f64 = 20.0;

/// What the agent in a pane is doing now. state: gone, working, blocked, error, idle or
/// shell (its turn ended, but a background shell of its own runs). age: seconds in that
/// state, None if unknown.
struct Agent {
    state: &'static str,
    kind: String,
    msg: String,
    age: Option<f64>,
}

/// What Claude Code's screen shows: working (a spinner), blocked (a question), idle
/// (its prompt), or None.
fn screen_state(text: &str) -> Option<&'static str> {
    let lines = stripped_lines(text, 12);
    if lines.iter().any(|l| l.contains("esc to interrupt")) {
        Some("working")
    } else if lines.iter().any(|l| l.starts_with("Do you want to")) {
        Some("blocked")
    } else if lines.iter().any(|l| l.starts_with('❯')) {
        Some("idle")
    } else {
        None
    }
}

/// A record's value as text: "" when it's falsy.
fn text_of(r: &Obj, key: &str) -> String {
    r.get(key).filter(|v| truthy(Some(v))).map_or(String::new(), py_str)
}

/// The agent's state by its status record, else its screen. None: unsure, or it just changed.
fn agent_state(session: &str, block: &str) -> Res<Option<Agent>> {
    if !rex::alive(session, block)? {
        return Ok(Some(Agent { state: "gone", kind: String::new(), msg: String::new(), age: None }));
    }
    let mut a = match program_status(session, block)?.filter(|r| !r.is_empty()) {
        Some(r) => {
            let age = truthy(r.get("updated_at")).then(|| seconds_since(&text_of(&r, "updated_at"), None));
            let state = match text_of(&r, "state").as_str() {
                "working" => "working",
                "blocked" => "blocked",
                "error" => "error",
                _ => "idle",
            };
            if state != "working" && age.is_some_and(|a| a < SETTLE) {
                return Ok(None); // its event may still be on the way
            }
            Agent { state, kind: text_of(&r, "kind"), msg: text_of(&r, "msg"), age }
        }
        None => match screen_state(&screen(session, block)?) {
            Some(state) => Agent { state, kind: String::new(), msg: String::new(), age: None },
            None => return Ok(None),
        },
    };
    if a.state == "idle" && shell_running(&screen(session, block)?) {
        a.state = "shell";
    }
    Ok(Some(a))
}

/// core.clip: the text on one line, cut to n characters with an ellipsis.
fn clip(text: &str, n: usize) -> String {
    let text = split(text).join(" ");
    if text.chars().count() <= n {
        text
    } else {
        text.chars().take(n - 1).collect::<String>() + "…"
    }
}

/// "permission" for "Needs you (permission): ...", else the why in a few words.
fn needs_label(why: Option<&Value>) -> String {
    let why = why.filter(|v| truthy(Some(v))).map_or(String::new(), py_str);
    let rest = why.strip_prefix("Reviewer ").unwrap_or(&why);
    if let Some(inside) = rest.strip_prefix("Needs you (").or_else(|| rest.strip_prefix("needs you (")) {
        if let Some(end) = inside.find(')') {
            return inside[..end].to_string();
        }
    }
    let text = if why.is_empty() { "waiting" } else { &why };
    let mut chars = text.chars();
    let first: String = chars.next().map_or(String::new(), |c| c.to_lowercase().collect());
    clip(&(first + chars.as_str()), 60)
}

/// What the agent is doing, in words: "its agent is idle at its prompt".
fn doing(a: &Agent, who: &str) -> String {
    match a.state {
        "idle" => format!("its {who} is idle at its prompt"),
        "blocked" => format!("its {who} is waiting on you ({})", if a.kind.is_empty() { "input" } else { &a.kind }),
        "error" => format!("its {who}'s turn failed"),
        _ => format!("its {who} is working again"),
    }
}

/// Minutes idle with a background shell running, once past limit (minutes); else None.
fn shell_minutes(a: &Agent, limit: f64) -> Option<i64> {
    let age = a.age.filter(|age| a.state == "shell" && *age >= limit * 60.0)?;
    Some((age / 60.0).floor() as i64)
}

/// Is the task's worker stopped at a prompt? (Its stop is exactly that.)
fn at_prompt(t: &Obj) -> bool {
    t.get("stop").and_then(Value::as_object).is_some_and(|o| o.len() == 2 && s(o, "by") == "worker" && s(o, "kind") == "prompt")
}

/// The line when the worker's task status disagrees with what it does, else None.
fn worker_drift(t: &Obj, a: &Agent, limit: f64) -> Option<String> {
    let (tid, st) = (s(t, "id"), s(t, "status"));
    if a.state == "gone" {
        return Some(format!("{tid} is marked {st} but its pane is gone"));
    }
    if a.state == "shell" {
        let m = shell_minutes(a, limit).filter(|_| st == "working")?;
        return Some(format!(
            "{tid} is marked working but its agent has been idle at its prompt for {m} min, \
             with a background shell still running: `bullpen peek` it"
        ));
    }
    if st == "working" && a.state != "working" {
        if truthy(t.get("ok")) && a.state != "blocked" {
            return None; // its coordinator said it's fine (`bullpen ok`)
        }
        let tail = if a.state == "idle" { ": `bullpen peek` it" } else { "" };
        return Some(format!("{tid} is marked working but {}{tail}", doing(a, "agent")));
    }
    if !is_waiting(st) || truthy(t.get("ask")) {
        return None; // an ask waits on the person, whatever its agent does
    }
    if a.state == "working" || (a.state == "blocked" && !at_prompt(t)) {
        return Some(format!("{tid} is marked {st} ({}) but {}", needs_label(t.get("why")), doing(a, "agent")));
    }
    None
}

/// The same for a task waiting on its reviewer.
fn reviewer_drift(t: &Obj, a: &Agent, limit: f64) -> Option<String> {
    let (tid, st) = (s(t, "id"), s(t, "status"));
    let idle = format!("{tid} is in review but its reviewer is idle without a verdict");
    if a.state == "shell" {
        // busy, unless its shell has run too long to count as busy
        let long = st == "review" && shell_minutes(a, limit).is_some() && !truthy(t.get("ok"));
        return long.then_some(idle);
    }
    if a.state == "gone" {
        let is = if st == "review" { "in review".to_string() } else { format!("marked {st}") };
        return Some(format!("{tid} is {is} but its reviewer's pane is gone"));
    }
    if st == "review" && matches!(a.state, "idle" | "error") && !truthy(t.get("ok")) {
        return Some(idle);
    }
    if st == "review" && a.state == "blocked" {
        return Some(format!("{tid} is in review but {}", doing(a, "reviewer")));
    }
    if is_waiting(st) && a.state == "working" {
        return Some(format!("{tid} is marked {st} ({}) but {}", needs_label(t.get("why")), doing(a, "reviewer")));
    }
    None
}

/// A line for an idle worker whose work looks finished but never ran ready, or None.
fn unready_hint(repo: &Repo, t: &Obj, a: &Agent) -> Res<Option<String>> {
    let st = s(t, "status");
    if a.state != "idle" || !(st == "working" || is_waiting(st)) {
        return Ok(None);
    }
    let wt = s(t, "worktree");
    if !truthy(t.get("worktree")) || !std::path::Path::new(wt).is_dir() {
        return Ok(None);
    }
    if !strip(&git(wt, &["status", "--porcelain"], false)?.stdout).is_empty() {
        return Ok(None);
    }
    let range = format!("{}..HEAD", task_base_ref(repo, t)?);
    let ahead = git(wt, &["rev-list", "--count", &range], false)?.stdout;
    let ahead = strip(&ahead);
    if ahead.is_empty() || !ahead.bytes().all(|b| b.is_ascii_digit()) || ahead.bytes().all(|b| b == b'0') {
        return Ok(None);
    }
    Ok(Some(format!(
        "{} is idle with commits and a clean worktree but never ran ready: \
         `bullpen peek` it, then `bullpen tell` it to run ready if it's done",
        s(t, "id")
    )))
}

/// Check the repo's tasks (those in ids, if given), printing unless quiet. Returns how many disagreed.
fn check_repo(repo: &Repo, ids: &[String], quiet: bool) -> Res<usize> {
    let out = |line: String| {
        if !quiet {
            println!("{line}");
        }
    };
    let limit = repo.cfg.get("stuck_after").filter(|v| truthy(Some(v))).and_then(Value::as_f64).unwrap_or(STUCK_AFTER);
    let mut drift = 0;
    for t in repo.tasks()? {
        if is_closed(s(&t, "status")) || (!ids.is_empty() && !ids.iter().any(|i| i == s(&t, "id"))) {
            continue;
        }
        if let Some((_, late)) = overdue(repo, &t, None)? {
            drift += 1;
            out(format!("{} waits on its coordinator, but {late}: it needs the person", s(&t, "id")));
        }
        if seconds_since(stamp(&t), None) < SETTLE {
            continue; // bullpen just changed it (start, tell, ready): its agent has yet to say
        }
        let Some((who, session, block)) = waited_on(&t) else { continue };
        let Some(a) = agent_state(&session, &block)? else { continue };
        if a.state == "working" && compacting(&session, &block, &a.msg)? {
            continue; // compacting by itself: no turn, so no drift
        }
        let found = if who == "worker" { worker_drift(&t, &a, limit) } else { reviewer_drift(&t, &a, limit) };
        if let Some(line) = found {
            drift += 1;
            out(line);
        }
        if who == "worker" {
            if let Some(hint) = unready_hint(repo, &t, &a)? {
                out(hint);
            }
        }
    }
    Ok(drift)
}

/// Compare each task's status with what its agent is doing now. Exit 1 when some disagree.
pub fn cmd_check(a: &Args) -> Res<i32> {
    if a.flag("--fix") {
        return Err(Fail::Python);
    }
    let repos = if a.flag("--all-repos") { known_repos()? } else { vec![Repo::new(&repo_root()?)?] };
    let mut drift = 0;
    for repo in &repos {
        let mut ids: Vec<String> = Vec::new();
        for i in &a.pos {
            let id = repo.norm(i);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        for i in &ids {
            repo.task(i)?; // an unknown ID is an error, not "all agree"
        }
        drift += check_repo(repo, &ids, a.flag("--quiet"))?;
    }
    Ok(if drift > 0 { 1 } else { 0 })
}
