//! `check`: each task's status against what its agent is doing now,
//! and the events it missed.
//!
//! A task's status follows its agent's events; a missed one (a watcher or Rex restart, a
//! bug) leaves it out of step. `check` compares each task that has an agent in a pane
//! with what Rex says that agent is doing right now, and `--fix` applies the event that
//! was missed, through the same paths (and news) as the watcher (`events::event`,
//! `events::gone`). It also
//! labels a ready task that has no risk label (risk_late.rs).

use crate::agents::waited_on;
use crate::args::{flag, pos, Arg, Args, Nargs};
use crate::events::{event, gone, Event};
use crate::modes::task_base_ref;
use crate::rex::{alive, program_status, screen};
use crate::screen::{compacting, screen_state, shell_running};
use crate::stops::{escalate, overdue};
use bullpen_core::json::{self, py_str, s, truthy, Obj};
use bullpen_core::proc::git;
use bullpen_core::repo::{known_repos, repo_root};
use bullpen_core::task::{at_prompt, clip, is_closed, is_waiting, stamp};
use bullpen_core::text::strip;
use bullpen_core::timeutil::seconds_since;
use bullpen_core::{Repo, Res};
use serde_json::Value;

/// Seconds a new idle, blocked or error state is left alone: its event may still be on the way.
const SETTLE: f64 = 60.0;

const CHECK: &[Arg] = &[
    pos("ID", Nargs::Any, "only these tasks"),
    flag(
        "--fix",
        "apply the updates the missed events would have made (news included)",
    ),
    flag("--quiet", "print nothing"),
    flag("--all-repos", "every repo bullpen knows (the watcher's)"),
];

/// What the agent in a pane is doing now. state: gone, working, blocked, error, idle or
/// shell (its turn ended, but a background shell of its own runs, so it takes another).
/// age: seconds in that state, None if unknown (the screen doesn't say).
pub struct Agent {
    pub state: &'static str,
    pub kind: String,
    pub msg: String,
    pub age: Option<f64>,
}

/// A record's value as text: "" when it's falsy.
fn text_of(r: &Obj, key: &str) -> String {
    r.get(key).filter(|v| truthy(Some(v))).map_or(String::new(), py_str)
}

/// The agent's state by its status record, else its screen. None: unsure, or it just changed.
fn agent_state(session: &str, block: &str) -> Res<Option<Agent>> {
    if !alive(session, block)? {
        return Ok(Some(Agent {
            state: "gone",
            kind: String::new(),
            msg: String::new(),
            age: None,
        }));
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
                // its event may still be on the way (the watcher holds an end of turn back).
                // Not working: the watcher forwards that at once, and a busy agent's record
                // is new every few seconds, as its message changes.
                return Ok(None);
            }
            Agent {
                state,
                kind: text_of(&r, "kind"),
                msg: text_of(&r, "msg"),
                age,
            }
        }
        None => match screen_state(&screen(session, block)?) {
            Some(state) => Agent {
                state,
                kind: String::new(),
                msg: String::new(),
                age: None,
            },
            None => return Ok(None),
        },
    };
    if a.state == "idle" && shell_running(&screen(session, block)?) {
        a.state = "shell";
    }
    Ok(Some(a))
}

/// "permission" for "Needs you (permission): ...", else the why in a few words.
pub fn needs_label(why: Option<&Value>) -> String {
    let why = why.filter(|v| truthy(Some(v))).map_or(String::new(), py_str);
    let rest = why.strip_prefix("Reviewer ").unwrap_or(&why);
    if let Some(inside) = rest
        .strip_prefix("Needs you (")
        .or_else(|| rest.strip_prefix("needs you ("))
    {
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
        "blocked" => format!(
            "its {who} is waiting on you ({})",
            if a.kind.is_empty() { "input" } else { &a.kind }
        ),
        "error" => format!("its {who}'s turn failed"),
        _ => format!("its {who} is working again"),
    }
}

/// Minutes an agent has been idle with a background shell running, once that's past
/// limit (minutes): a server or a watch it left running, not a test run. Else None.
pub fn shell_minutes(a: &Agent, limit: f64) -> Option<i64> {
    let age = a.age.filter(|age| a.state == "shell" && *age >= limit * 60.0)?;
    Some((age / 60.0).floor() as i64)
}

/// (line, fix) when the worker's task status disagrees with what it does, else None. fix
/// is the event the watcher missed: "gone", "shell" (idle for long with a shell running),
/// or the agent's state.
pub fn worker_drift(t: &Obj, a: &Agent, limit: f64) -> Option<(String, &'static str)> {
    let (tid, st) = (s(t, "id"), s(t, "status"));
    if a.state == "gone" {
        return Some((format!("{tid} is marked {st} but its pane is gone"), "gone"));
    }
    if a.state == "shell" {
        let m = shell_minutes(a, limit).filter(|_| st == "working")?;
        let line = format!(
            "{tid} is marked working but its agent has been idle at its prompt for {m} min, \
             with a background shell still running: `bullpen peek` it"
        );
        return Some((line, "shell")); // else busy: it takes another turn when its shell ends
    }
    if st == "working" && a.state != "working" {
        if truthy(t.get("ok")) && a.state != "blocked" {
            return None; // its coordinator said it's fine (`bullpen ok`)
        }
        let tail = if a.state == "idle" { ": `bullpen peek` it" } else { "" };
        return Some((
            format!("{tid} is marked working but {}{tail}", doing(a, "agent")),
            a.state,
        ));
    }
    if !is_waiting(st) || truthy(t.get("ask")) {
        return None; // an ask waits on the person, whatever its agent does
    }
    if a.state == "working" || (a.state == "blocked" && !at_prompt(t)) {
        let line = format!(
            "{tid} is marked {st} ({}) but {}",
            needs_label(t.get("why")),
            doing(a, "agent")
        );
        return Some((line, a.state));
    }
    None
}

/// The same for a task waiting on its reviewer.
pub fn reviewer_drift(t: &Obj, a: &Agent, limit: f64) -> Option<(String, &'static str)> {
    let (tid, st) = (s(t, "id"), s(t, "status"));
    let idle = format!("{tid} is in review but its reviewer is idle without a verdict");
    if a.state == "shell" {
        if st != "review" || shell_minutes(a, limit).is_none() || truthy(t.get("ok")) {
            return None; // busy: it takes another turn when its shell ends
        }
        return Some((idle, "shell")); // its shell has run too long to count as busy
    }
    if a.state == "gone" {
        let is = if st == "review" {
            "in review".to_string()
        } else {
            format!("marked {st}")
        };
        return Some((format!("{tid} is {is} but its reviewer's pane is gone"), "gone"));
    }
    if st == "review" && matches!(a.state, "idle" | "error") && !truthy(t.get("ok")) {
        return Some((idle, a.state));
    }
    if st == "review" && a.state == "blocked" {
        return Some((format!("{tid} is in review but {}", doing(a, "reviewer")), "blocked"));
    }
    if is_waiting(st) && a.state == "working" {
        let line = format!(
            "{tid} is marked {st} ({}) but {}",
            needs_label(t.get("why")),
            doing(a, "reviewer")
        );
        return Some((line, "working"));
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

/// Apply the event the watcher missed, through its own path (and news).
fn fix_drift(repo: &Repo, t: &Obj, who: &str, block: &str, fix: &str, a: &Agent) -> Res<()> {
    if fix == "gone" {
        return gone(block);
    }
    if who == "worker" {
        let mut t = repo.task(s(t, "id"))?;
        t.insert("last".into(), Value::Null); // the event did come: don't take it for a repeat
        json::write_obj(&repo.task_path(s(&t, "id")), &t)?;
    }
    let (state, shell_min) = if fix == "shell" {
        ("idle", shell_minutes(a, 0.0))
    } else {
        (fix, None)
    };
    event(&Event {
        block: block.into(),
        state: state.into(),
        kind: a.kind.clone(),
        msg: a.msg.clone(),
        shell_min,
    })
}

/// Check the repo's tasks (those in ids, if given). Returns how many disagreed.
fn check_repo(repo: &Repo, ids: &[String], fix: bool, quiet: bool) -> Res<usize> {
    let out = |line: String| {
        if !quiet {
            println!("{line}");
        }
    };
    let fixed = if fix { " (fixed)" } else { "" };
    let mut drift = 0;
    for mut t in repo.tasks()? {
        if is_closed(s(&t, "status")) || (!ids.is_empty() && !ids.iter().any(|i| i == s(&t, "id"))) {
            continue;
        }
        if let Some(late) = overdue(repo, &t, None)? {
            drift += 1;
            if fix {
                escalate(repo, &mut t, &late)?;
                t = repo.task(s(&t, "id"))?;
            }
            out(format!(
                "{} waits on its coordinator, but {}: it needs the person{fixed}",
                s(&t, "id"),
                late.1
            ));
        }
        if crate::risk_late::unlabelled(&t) {
            drift += 1;
            let tid = s(&t, "id");
            match fix.then(|| crate::risk_late::label_late(repo, tid)) {
                Some(Err(e)) => out(format!(
                    "{tid} is ready without a risk label, and it can't be worked out: {e}"
                )),
                _ => out(format!("{tid} is ready without a risk label{fixed}")),
            }
        }
        if stamp(&t).map_or(0.0, |st| seconds_since(st, None)) < SETTLE {
            continue; // bullpen just changed it (start, tell, ready): its agent has yet to say
        }
        let Some((who, session, block)) = waited_on(&t) else {
            continue;
        };
        let Some(a) = agent_state(&session, &block)? else {
            continue;
        };
        if a.state == "working" && compacting(&session, &block, &a.msg)? {
            continue; // compacting by itself: no turn, so no drift
        }
        // How long an idle agent's shell may run before it is drift. stuck_after 0 turns
        // off only the "no progress" news: this check still runs, on the default.
        let limit = match repo.cfg.get("stuck_after").filter(|v| truthy(Some(v))) {
            Some(v) => v.as_f64().unwrap_or(0.0),
            None => bullpen_core::settings::defaults()
                .get("stuck_after")
                .and_then(Value::as_f64)
                .unwrap_or(20.0),
        };
        let found = if who == "worker" {
            worker_drift(&t, &a, limit)
        } else {
            reviewer_drift(&t, &a, limit)
        };
        if let Some((line, what)) = found {
            drift += 1;
            if fix {
                fix_drift(repo, &t, who, &block, what, &a)?;
            }
            out(format!("{line}{fixed}"));
        }
        if who == "worker" {
            if let Some(hint) = unready_hint(repo, &repo.task(s(&t, "id"))?, &a)? {
                out(hint);
            }
        }
    }
    Ok(drift)
}

/// `check`: compare each task's status with what its agent is doing now; --fix catches
/// up. Exit 1 when some disagree (and weren't fixed).
pub fn cmd_check(argv: &[String]) -> Res<i32> {
    let a = Args::parse("check", argv, CHECK);
    let fix = a.flag("--fix");
    let repos = if a.flag("--all-repos") {
        known_repos()?
    } else {
        vec![Repo::new(repo_root()?)?]
    };
    let mut drift = 0;
    for repo in &repos {
        let mut ids: Vec<String> = Vec::new();
        for i in a.args("ID") {
            let id = repo.norm(i);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        for i in &ids {
            repo.task(i)?; // an unknown ID is an error, not "all agree"
        }
        drift += check_repo(repo, &ids, fix, a.flag("--quiet"))?;
    }
    if fix && a.flag("--all-repos") {
        crate::build_watch::catch_up(&repos); // the watcher's check: its own copy's build too
    }
    Ok(if drift > 0 && !fix { 1 } else { 0 })
}

#[cfg(test)]
#[path = "test_check.rs"]
mod tests;
