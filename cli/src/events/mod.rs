//! What the watcher (lua/watch.lua) reports: an agent's events, and a
//! pane that closed. An agent that shows no progress and the usage limit's reset are in
//! stuck.rs. `check --fix` applies the events the watcher missed through the same paths.

pub mod stuck;

use crate::agents::{on_reviewer, pane, whose_turn};
use crate::args::{pos, Arg, Args, Nargs};
use crate::coordinators::{coordinator_at, coordinator_gone};
use crate::limit::{limit_hit, limit_over, limited, LIMIT_STATES};
use crate::modes::task_base_ref;
use crate::news::{ready_news, say};
use crate::review::{keep_approval, rebase_check, Check};
use crate::rex::screen;
use crate::saves::save;
use crate::screen::{compacting, shell_running};
use crate::stops::stop;
use bullpen_core::json::{s, truthy, Obj};
use bullpen_core::proc::git;
use bullpen_core::repo::{lookup_block, mark_compacting, unindex_block};
use bullpen_core::task::{back_to_work, is_closed, is_stopped, is_waiting, set_status};
use bullpen_core::text::{split, strip};
use bullpen_core::timeutil::iso_now;
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};

/// What the watcher saw a pane's agent do: its state (working, idle, done, blocked or
/// error), the kind of prompt it's blocked on and Rex's message ("" for none). `check
/// --fix` also gives shell_min, how long the agent's own background shell has run.
pub struct Event {
    pub block: String,
    pub state: String,
    pub kind: String,
    pub msg: String,
    pub shell_min: Option<i64>,
}

const EVENT: &[Arg] = &[
    pos("block", Nargs::One, ""),
    pos("state", Nargs::One, ""),
    pos("kind", Nargs::Maybe, ""),
    pos("msg", Nargs::Maybe, ""),
];
const GONE: &[Arg] = &[pos("block", Nargs::One, "")];

/// `_event BLOCK STATE [KIND] [MSG]` (the watcher).
pub fn cmd_event(argv: &[String]) -> Res<i32> {
    let a = Args::parse("_event", argv, EVENT);
    let arg = |name| a.arg(name).unwrap_or("").to_string();
    let ev = Event {
        block: arg("block"),
        state: arg("state"),
        kind: arg("kind"),
        msg: arg("msg"),
        shell_min: None,
    };
    event(&ev)?;
    Ok(0)
}

/// The first word of a pane's role: the task's ID ("BP-3" of "BP-3 review").
fn role_task(role: &str) -> &str {
    split(role).first().copied().unwrap_or("")
}

/// A pane's agent changed state (lua/watch.lua, or `check --fix` for one it missed).
pub fn event(ev: &Event) -> Res<()> {
    let Some((repo, role)) = lookup_block(Some(&ev.block))? else {
        return Ok(());
    };
    if role == "coordinator" || role == "board" {
        return Ok(());
    }
    if role.ends_with(" review") {
        let mut t = repo.task(role_task(&role))?;
        return review_event(&repo, &mut t, ev);
    }
    let mut t = repo.task(&role)?;
    if s(&t, "block") != ev.block || is_closed(s(&t, "status")) {
        return Ok(());
    }
    let (state, msg) = (ev.state.as_str(), strip(&ev.msg));
    let key = format!("{state}|{msg}");
    let woke = unstick(&mut t, "worker");
    if LIMIT_STATES.contains(&state)
        && (limited(&t, "worker") || limit_hit(&repo, &mut t, "worker", &ev.block, state, &ev.msg)?)
    {
        return Ok(());
    }
    let status = s(&t, "status").to_string();
    if state == "working" && status != "working" && compacting(s(&t, "session"), &ev.block, msg)? {
        // Claude Code compacting the conversation by itself: not a new turn
        if !truthy(t.get("compacting")) {
            t.insert("compacting".into(), json!(iso_now()));
            mark_compacting(&ev.block, true)?;
        }
        return save(&repo, &mut t);
    }
    if truthy(t.shift_remove("compacting").as_ref()) {
        mark_compacting(&ev.block, false)?;
        if (state == "done" || state == "idle") && status != "working" {
            // the compaction is over: its end is no end of a turn
            t.insert("last".into(), json!(key));
            return save(&repo, &mut t);
        }
    }
    let id = s(&t, "id").to_string();
    let after_working = s(&t, "last").starts_with("working|");
    if state == "working" {
        if is_stopped(&status) || limit_over(&mut t)? {
            set_status(&mut t, "working", None, None);
        } else if status == "ready" && !after_working {
            back_to_work(&mut t); // it had ended its turn: this is a new one
            let approved = s(bullpen_core::json::obj(&t, "review"), "verdict") == "approved";
            if approved {
                t.insert("from_ready".into(), json!(true)); // an end of turn that changed no code makes it ready again
            }
            let holds = if approved {
                "; its approval holds if the code doesn't change"
            } else {
                ""
            };
            say(
                &repo,
                &t,
                "working",
                &format!("{id} is working again (was ready): it must run `ready` again{holds}"),
            )?;
        } else if status == "finished" && !after_working {
            back_to_work(&mut t);
            say(
                &repo,
                &t,
                "working",
                &format!("{id} is working again (was finished): it must run `finish` (or `ready`) again"),
            )?;
        }
        t.shift_remove("ok");
        t.insert("last".into(), json!(key));
        return save(&repo, &mut t);
    }
    if t.get("last") == Some(&json!(key)) {
        if woke {
            save(&repo, &mut t)?;
        }
        return Ok(());
    }
    t.insert("last".into(), json!(key));
    let saying = |msg: &str| {
        if msg.is_empty() {
            String::new()
        } else {
            format!(": {msg}")
        }
    };
    match state {
        "blocked" => {
            let kind = if ev.kind.is_empty() { "input" } else { &ev.kind };
            let msg = prompt_msg(kind, msg);
            let why = format!("Needs you ({kind}){}", saying(&msg));
            stop(
                &repo,
                &mut t,
                "worker",
                "prompt",
                &why,
                &[prompt_news(&id, kind, &msg)],
                "needs-you",
            )?;
        }
        "error" => {
            let line = format!("{id}'s turn failed{}: read it with `bullpen peek {id}`", saying(msg));
            stop(
                &repo,
                &mut t,
                "worker",
                "failed",
                &format!("Failed{}", saying(msg)),
                &[line],
                "failed",
            )?;
        }
        "done" | "idle" => {
            let shell_min = ev.shell_min; // from `check`: its shell has run this long
            if s(&t, "status") != "working"
                || (shell_min.is_none() && shell_running(&screen(s(&t, "session"), s(&t, "block"))?))
            {
                // a background shell of its own still runs: it takes another turn when that ends
                return save(&repo, &mut t);
            }
            crate::overlap::check(&repo, &t); // the end of a turn: its branch may share files with another's now
            if ready_again(&repo, &mut t)? {
                return Ok(());
            }
            let line = match shell_min {
                None => format!("{id} ended its turn without `ready`: read it with `bullpen peek {id}`"),
                Some(m) => format!(
                    "{id} ended its turn without `ready` {m} min ago, and a background shell of its own still runs \
                     (a server, a watch?): read it with `bullpen peek {id}`"
                ),
            };
            stop(
                &repo,
                &mut t,
                "worker",
                "ended",
                "Ended its turn without saying ready",
                &[line],
                "quiet",
            )?;
        }
        _ => save(&repo, &mut t)?,
    }
    Ok(())
}

/// What Rex said of a prompt, unless it only names its kind ("permission prompt").
pub fn prompt_msg(kind: &str, msg: &str) -> String {
    let msg = strip(msg);
    let (low, kind) = (msg.to_lowercase(), kind.to_lowercase());
    if low == kind || low == format!("{kind} prompt") {
        String::new()
    } else {
        msg.into()
    }
}

/// The news that a task's agent is at a prompt, which needs the person: "BP-3 is at a
/// permission prompt: Allow rm?", "BP-3's reviewer is at a prompt (question): ...".
pub fn prompt_news(who: &str, kind: &str, msg: &str) -> String {
    let at = if kind == "permission" {
        "a permission prompt".to_string()
    } else {
        format!("a prompt ({kind})")
    };
    let tail = if msg.is_empty() {
        String::new()
    } else {
        format!(": {msg}")
    };
    format!("{who} is at {at}{tail}")
}

/// A ready task's worker took a turn and ended it without ready. If the code is as it
/// was approved (committed, the same patch), it is ready again with its approval. True
/// if so.
fn ready_again(repo: &Repo, t: &mut Obj) -> Res<bool> {
    if !truthy(t.shift_remove("from_ready").as_ref()) || !worktree_clean(t)? {
        return Ok(false);
    }
    let base_ref = task_base_ref(repo, t)?;
    let Some(Check::Kept(kept)) = rebase_check(t, &base_ref)? else {
        return Ok(false);
    };
    keep_approval(t, &base_ref)?;
    crate::risk::assess(repo, t, &base_ref, crate::risk::Said::Kept)?;
    set_status(t, "ready", None, None);
    save(repo, t)?;
    let then = format!(". `bullpen peek {}` if it said something", s(t, "id"));
    ready_news(
        repo,
        t,
        &[format!("its worker's turn left the code as approved: {kept}")],
        &then,
        true,
    )?;
    Ok(true)
}

fn worktree_clean(t: &Obj) -> Res<bool> {
    let wt = s(t, "worktree");
    if !truthy(t.get("worktree")) || !std::path::Path::new(wt).is_dir() {
        return Ok(false);
    }
    Ok(strip(&git(wt, &["status", "--porcelain"], false)?.stdout).is_empty())
}

/// The task's review, to change (it has one: its pane's block matched).
fn review_mut(t: &mut Obj) -> Option<&mut Obj> {
    t.get_mut("review").and_then(Value::as_object_mut)
}

/// A reviewer's status: its coordinator hears if it stops, the person if it's at a prompt.
fn review_event(repo: &Repo, t: &mut Obj, ev: &Event) -> Res<()> {
    let r = bullpen_core::json::obj(t, "review");
    let status = s(t, "status").to_string();
    if s(r, "block") != ev.block || !(status == "review" || is_waiting(&status)) {
        return Ok(());
    }
    let (state, msg) = (ev.state.as_str(), strip(&ev.msg));
    let stuck = on_reviewer(t);
    let mut woke = unstick(t, "reviewer");
    if LIMIT_STATES.contains(&state)
        && (limited(t, "reviewer") || limit_hit(repo, t, "reviewer", &ev.block, state, &ev.msg)?)
    {
        return Ok(());
    }
    let (session, block) = pane(t, "reviewer");
    if state == "working" && stuck && compacting(&session, &block, msg)? {
        // compacting by itself: it stays where its coordinator or the person has it
        let r = review_mut(t).expect("a review");
        if !truthy(r.get("compacting")) {
            r.insert("compacting".into(), json!(iso_now()));
            mark_compacting(&ev.block, true)?;
        }
        return save(repo, t);
    }
    if truthy(review_mut(t).and_then(|r| r.shift_remove("compacting")).as_ref()) {
        // the compaction is over. Its end changes nothing here: a reviewer stopped in
        // triage or needs stays there, and one in review (`ok`) ended a real turn.
        mark_compacting(&ev.block, false)?;
        woke = true; // save the task
    }
    let id = s(t, "id").to_string();
    match state {
        "working" => {
            let back = limit_over(t)?;
            let was_ok = truthy(t.shift_remove("ok").as_ref());
            if stuck || back {
                set_status(t, "review", None, None);
            }
            if stuck || woke || back || was_ok {
                save(repo, t)?;
            }
        }
        "blocked" => {
            let kind = if ev.kind.is_empty() { "input" } else { &ev.kind };
            let msg = prompt_msg(kind, msg);
            let tail = if msg.is_empty() {
                String::new()
            } else {
                format!(": {msg}")
            };
            let line = prompt_news(&format!("{id}'s reviewer"), kind, &msg);
            stop(
                repo,
                t,
                "reviewer",
                "prompt",
                &format!("Reviewer needs you ({kind}){tail}"),
                &[line],
                "needs-you",
            )?;
        }
        "done" | "idle" | "error" if status == "review" => {
            if state != "error" && ev.shell_min.is_none() && shell_running(&screen(&session, &block)?) {
                // a background shell of its own still runs (the tests?): it takes another turn
                if woke {
                    save(repo, t)?;
                }
                return Ok(());
            }
            let late = match ev.shell_min {
                Some(m) => format!(" {m} min ago, and a background shell of its own still runs"),
                None => String::new(),
            };
            let line = format!(
                "{id}'s reviewer stopped without a verdict{late}: `bullpen go {id} --review` shows it, \
                 `bullpen review {id}` starts a new one"
            );
            stop(
                repo,
                t,
                "reviewer",
                "ended",
                "Reviewer stopped without a verdict",
                &[line],
                "quiet",
            )?;
        }
        _ if woke => save(repo, t)?,
        _ => {}
    }
    Ok(())
}

/// The agent gave a sign of life: forget that it was quiet. True if it was.
pub fn unstick(t: &mut Obj, who: &str) -> bool {
    if s(bullpen_core::json::obj(t, "stuck"), "who") != who {
        return false;
    }
    t.shift_remove("stuck");
    true
}

/// `_gone BLOCK` (the watcher).
pub fn cmd_gone(argv: &[String]) -> Res<i32> {
    let a = Args::parse("_gone", argv, GONE);
    gone(a.arg("block").unwrap_or(""))?;
    Ok(0)
}

/// A pane bullpen knows closed (lua/watch.lua, or `check --fix` for one it missed).
pub fn gone(block: &str) -> Res<()> {
    let Some((repo, role)) = lookup_block(Some(block))? else {
        return Ok(());
    };
    unindex_block(block)?;
    if role == "coordinator" {
        if let Some(c) = coordinator_at(&repo, block, false) {
            coordinator_gone(&repo, c);
        }
        return Ok(());
    }
    if role == "board" {
        return Ok(());
    }
    if role.ends_with(" review") {
        let mut t = repo.task(role_task(&role))?;
        let Some(r) = review_mut(&mut t).filter(|r| s(r, "block") == block) else {
            return Ok(());
        };
        r.insert("block".into(), Value::Null);
        let verdict = truthy(r.get("verdict"));
        if limited(&t, "reviewer") {
            // started again when the limit resets
        } else if whose_turn(&t) == Some("reviewer") && !verdict {
            let id = s(&t, "id").to_string();
            let line = format!("{id}'s reviewer exited without a verdict: `bullpen review {id}` starts a new one");
            stop(
                &repo,
                &mut t,
                "reviewer",
                "exited",
                "Reviewer exited without a verdict",
                &[line],
                "stopped",
            )?;
        }
        return save(&repo, &mut t);
    }
    let mut t = repo.task(&role)?;
    if s(&t, "block") != block {
        return Ok(());
    }
    t.insert("block".into(), Value::Null);
    let status = s(&t, "status").to_string();
    if limited(&t, "worker") {
        save(&repo, &mut t)?; // resumed when the limit resets
    } else if status == "working" || is_waiting(&status) {
        let id = s(&t, "id").to_string();
        let line = format!("{id} stopped before saying ready: `bullpen resume {id}` continues it");
        stop(
            &repo,
            &mut t,
            "worker",
            "exited",
            "Agent exited before saying ready",
            &[line],
            "stopped",
        )?;
    } else {
        save(&repo, &mut t)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "test_events.rs"]
mod tests;
