//! The run queue (`start_queued`): when a task frees a slot or closes, the queue
//! starts each todo task that has dependencies, all done, within max_running. Only
//! working tasks take a slot: a reviewer is short and cheap. A task sent back from
//! review works, and counts, again, even if that puts the count over max_running: it's
//! finishing work already started, and the queue just starts nothing more until the
//! count is under the limit.
//!
//! A save that frees a slot moves it at later-time (saves.rs). `add` and `after` move it
//! too when they give a task dependencies that are all done already (start_if_done).

use super::by_id;
use super::launch::start_task;
use crate::coordinators::session_for;
use crate::limit::{limit_active, limit_phrase};
use crate::news::kind::Kind;
use crate::news::{news, say, Opts};
use bullpen_core::json::{s, strs, truthy, Obj};
use bullpen_core::task::{clip, task_num};
use bullpen_core::words::open_deps;
use bullpen_core::{Fail, Repo, Res};

/// The tasks that take a max_running slot: those working (not in review).
fn running_count(tasks: &[Obj]) -> i64 {
    tasks.iter().filter(|t| s(t, "status") == "working").count() as i64
}

/// Why the queue starts something now, in news: by freed a slot (how).
pub fn why(by: &str, how: &str) -> String {
    match how {
        "accepted" => format!("{by} was accepted"),
        "closed" => format!("{by} was closed"),
        "review" => format!("{by} went to review, freeing a slot"),
        "ready" => format!("{by} is ready, freeing a slot"),
        "finished" => format!("{by} finished, freeing a slot"),
        _ => format!("{by} was dropped, freeing a slot"),
    }
}

/// The queued tasks: todo, with dependencies, all done; in ID order.
pub fn queued(tasks: &super::Tasks) -> Vec<Obj> {
    let mut out: Vec<Obj> = tasks.values().filter(|t| is_queued(t, tasks)).cloned().collect();
    out.sort_by_key(task_num);
    out
}

/// Is t queued: todo, with dependencies, all done?
fn is_queued(t: &Obj, tasks: &super::Tasks) -> bool {
    s(t, "status") == "todo" && truthy(t.get("after")) && open_deps(t, tasks).is_empty()
}

/// What the queue did with a queued task.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Started,
    /// The usage limit is reached: when it resets (limit_phrase).
    Limit(String),
    /// max_running (this many) tasks work: it waits for a slot.
    NoRoom(i64),
    /// It couldn't start: why.
    Failed(String),
    /// Its setup failed (that has its own news).
    SetupFailed,
}

/// Why a queued task t can start, in news: by freed a slot (how), or, for "added" (by
/// was given dependencies that are all done already), what t waits on is done.
fn why_for(t: &Obj, tasks: &super::Tasks, by: &str, how: &str) -> String {
    if how == "added" {
        done_phrase(t, tasks)
    } else {
        why(by, how)
    }
}

/// "BP-1 is accepted already", "BP-1, BP-2 are closed already" (closed with nothing to
/// merge), or "are done already" for a mix: t's dependencies, all done.
pub fn done_phrase(t: &Obj, tasks: &super::Tasks) -> String {
    let deps = strs(t, "after");
    let closed = deps
        .iter()
        .filter(|d| tasks.get(*d).is_some_and(|o| truthy(o.get("closed"))))
        .count();
    let word = match closed {
        0 => "accepted",
        n if n == deps.len() => "closed",
        _ => "done",
    };
    let verb = if deps.len() == 1 { "is" } else { "are" };
    format!("{} {verb} {word} already", deps.join(", "))
}

/// After by was accepted, closed or dropped, or went to review, ready or finished, or
/// was given dependencies all done already ("added") (how): start each queued task, in
/// ID order, while fewer than max_running work. One news line per start. Plain todo
/// tasks wait for the coordinator. What became of each queued task, by ID.
pub fn start_queued(repo: &Repo, by: &Obj, how: &str) -> Res<Vec<(String, Outcome)>> {
    let tasks = by_id(repo)?;
    let queued = queued(&tasks);
    if queued.is_empty() {
        return Ok(Vec::new());
    }
    let by_name = s(by, "id");
    if let Some(lim) = limit_active(None)? {
        let ids: Vec<&str> = queued.iter().map(|t| s(t, "id")).collect();
        let when = limit_phrase(lim.get("resets"), None);
        let line = format!(
            "{} can start ({}), but the usage limit is reached ({when}): `bullpen start` them after the reset",
            ids.join(", "),
            why_for(by, &tasks, by_name, how)
        );
        let o = Opts {
            everyone: true,
            ..Opts::default()
        };
        news(repo, by, Kind::Queue, &line, o)?;
        return Ok(ids
            .into_iter()
            .map(|id| (id.to_string(), Outcome::Limit(when.clone())))
            .collect());
    }
    let max = repo
        .cfg
        .get("max_running")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let room = (max - running_count(&repo.tasks()?)).max(0) as usize;
    let session = session_for(repo, None)?;
    let mut out = Vec::new();
    for (i, mut t) in queued.into_iter().enumerate() {
        let id = s(&t, "id").to_string();
        if i >= room {
            out.push((id, Outcome::NoRoom(max)));
            continue;
        }
        let why = why_for(&t, &tasks, by_name, how);
        let started = if session.is_empty() {
            Err(Fail("no Rex session: run `bullpen up`".into()))
        } else {
            start_task(repo, &mut t, &session, None)
        };
        let outcome = match started {
            Ok(false) => Outcome::SetupFailed, // that has its own news
            Ok(true) => {
                say(repo, &t, Kind::Started, &format!("{id} started ({why})"))?;
                Outcome::Started
            }
            Err(Fail(e)) => {
                let e = clip(&e, 200);
                say(
                    repo,
                    &t,
                    Kind::QueueFailed,
                    &format!("{id} couldn't start ({why}): {e}. `bullpen start {id}` tries again"),
                )?;
                Outcome::Failed(e)
            }
        };
        out.push((id, outcome));
    }
    Ok(out)
}

/// `add` or `after` gave the task tid dependencies: if they're all done already, run
/// the queue as their accept would have (earlier queued tasks go first). A line for the
/// command's output saying what became of tid; None while it waits on something open.
pub fn start_if_done(repo: &Repo, tid: &str) -> Res<Option<String>> {
    let tasks = by_id(repo)?;
    let Some(t) = tasks.get(tid).filter(|t| is_queued(t, &tasks)) else {
        return Ok(None);
    };
    let done = done_phrase(t, &tasks);
    let outcomes = start_queued(repo, t, "added")?;
    let Some((_, outcome)) = outcomes.into_iter().find(|(id, _)| id == tid) else {
        return Ok(None);
    };
    Ok(Some(said(tid, &done, outcome)))
}

/// The line for start_if_done: what became of tid, whose dependencies are done.
fn said(tid: &str, done: &str, outcome: Outcome) -> String {
    match outcome {
        Outcome::Started => format!("{tid} started: {done}"),
        Outcome::Limit(when) => format!(
            "{tid} can start ({done}), but the usage limit is reached ({when}): `bullpen start {tid}` after the reset"
        ),
        Outcome::NoRoom(max) => format!(
            "{tid} is queued ({done}): max_running ({max}) tasks are working, so it starts when one frees a slot"
        ),
        Outcome::Failed(e) => format!("{tid} couldn't start ({done}): {e}. `bullpen start {tid}` tries again"),
        Outcome::SetupFailed => format!("{tid} couldn't start ({done}): its setup failed, as the news says"),
    }
}

#[cfg(test)]
#[path = "tests/test_queue.rs"]
mod tests;
