//! The run queue (`start_queued`): when a task frees a slot or closes, the queue
//! starts each todo task that has dependencies, all done, within max_running. Only
//! working tasks take a slot: a reviewer is short and cheap. A task sent back from
//! review works, and counts, again, even if that puts the count over max_running: it's
//! finishing work already started, and the queue just starts nothing more until the
//! count is under the limit.
//!
//! A save that frees a slot moves it at later-time (saves.rs).

use super::by_id;
use super::launch::start_task;
use crate::coordinators::session_for;
use crate::limit::{limit_active, limit_phrase};
use crate::news::{news, say, Opts};
use bullpen_core::json::{s, truthy, Obj};
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
    let mut out: Vec<Obj> = tasks
        .values()
        .filter(|t| s(t, "status") == "todo" && truthy(t.get("after")) && open_deps(t, tasks).is_empty())
        .cloned()
        .collect();
    out.sort_by_key(task_num);
    out
}

/// After by was accepted, closed or dropped, or went to review, ready or finished
/// (how): start each queued task, in ID order, while fewer than max_running work. One
/// news line per start. Plain todo tasks wait for the coordinator.
pub fn start_queued(repo: &Repo, by: &Obj, how: &str) -> Res<()> {
    let tasks = by_id(repo)?;
    let queued = queued(&tasks);
    if queued.is_empty() {
        return Ok(());
    }
    let why = why(s(by, "id"), how);
    if let Some(lim) = limit_active(None)? {
        let ids: Vec<&str> = queued.iter().map(|t| s(t, "id")).collect();
        let line = format!(
            "{} can start ({why}), but the usage limit is reached ({}): `bullpen start` them after the reset",
            ids.join(", "),
            limit_phrase(lim.get("resets"), None)
        );
        let o = Opts {
            everyone: true,
            ..Opts::default()
        };
        return news(repo, by, "queue", &line, o);
    }
    let max = repo
        .cfg
        .get("max_running")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let room = max - running_count(&repo.tasks()?);
    let session = session_for(repo, None)?;
    for mut t in queued.into_iter().take(room.max(0) as usize) {
        let id = s(&t, "id").to_string();
        let started = if session.is_empty() {
            Err(Fail("no Rex session: run `bullpen up`".into()))
        } else {
            start_task(repo, &mut t, &session, None)
        };
        match started {
            Ok(false) => continue, // its setup failed: that has its own news
            Ok(true) => say(repo, &t, "started", &format!("{id} started ({why})"))?,
            Err(Fail(e)) => say(
                repo,
                &t,
                "queue",
                &format!(
                    "{id} couldn't start ({why}): {}. `bullpen start {id}` tries again",
                    clip(&e, 200)
                ),
            )?,
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "test_queue.rs"]
mod tests;
