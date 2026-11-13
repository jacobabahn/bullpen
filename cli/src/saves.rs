//! Saving a task, and what a save sets off: core writes it (`Repo::save_task`, which
//! returns the task as it was), then the hooks run in cli.SAVERS' order: the run queue
//! (start.move_queue), the person's notification (notifications.notify), the PR to
//! follow (prs.follow_saved). Work that waits for the command's own work to be done
//! (the run queue starting the next task) goes in `later`, and main runs it when the
//! command returns.
//!
//! The run queue is the one bridge to Python (until T6 moves it): a save that frees a
//! slot runs `bullpen _queue` at later-time, which reads the tasks fresh from disk.

use bullpen_core::fail::Res;
use bullpen_core::json::{obj, s, truthy, Obj};
use bullpen_core::paths::{self_bin, state};
use bullpen_core::repo::Repo;
use std::cell::RefCell;
use std::io::Write;

/// Save the task, then run what the save sets off, in one fixed order.
pub fn save(repo: &Repo, t: &mut Obj) -> Res<()> {
    let old = repo.save_task(t)?;
    move_queue(repo, t, &old);
    crate::notifications::notify(repo, t, &old);
    follow_saved(t)?;
    Ok(())
}

/// start.move_queue: the run queue hears of every status change. When a task frees a
/// slot or closes, the queue starts the next queued tasks once the command's own work
/// is done: through the Python's `_queue` until T6.
fn move_queue(repo: &Repo, t: &Obj, old: &Obj) {
    let Some(how) = crate::start::slot_freed(old, t) else {
        return;
    };
    let (root, by) = (repo.root_str(), s(t, "id").to_string());
    later(&format!("queue {root}"), move || run_queue(&root, &by, how));
}

/// `bullpen _queue`: start what the queue can, now that task `by` freed a slot (how).
fn run_queue(root: &str, by: &str, how: &str) {
    let _ = std::io::stdout().flush(); // its output comes after ours
    let ran = std::process::Command::new(self_bin())
        .args(["_queue", "--repo", root, "--by", by, "--how", how])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .status();
    if let Err(e) = ran {
        eprintln!("bullpen: couldn't move the run queue: {e}");
    }
}

/// prs.followed: does the watcher look at this task's PR? Only an open PR of a task that
/// waits on review or the person: a working task is about to push anyway.
pub fn followed(t: &Obj) -> bool {
    let open = match obj(t, "gh").get("state") {
        None => true,
        Some(v) => v.as_str() == Some("open"),
    };
    truthy(t.get("pr"))
        && !truthy(t.get("local"))
        && matches!(s(t, "status"), "review" | "ready" | "needs" | "triage")
        && open
}

/// prs.follow_saved: a saved task whose PR the watcher looks at marks it to follow
/// (<state>/prs exists while some task's PR may need a look: the watcher only calls
/// `_prs` then).
fn follow_saved(t: &Obj) -> Res<()> {
    if followed(t) {
        std::fs::create_dir_all(state())?;
        crate::news::touch(&state().join("prs"))?;
    }
    Ok(())
}

type Later = Box<dyn FnOnce()>;

thread_local! {
    static LATER: RefCell<Vec<(String, Later)>> = const { RefCell::new(Vec::new()) };
}

/// Run f once the command's work is done (run_later). A later f with the same key
/// replaces it, and runs once, in the place of the newest.
pub fn later(key: &str, f: impl FnOnce() + 'static) {
    LATER.with(|l| {
        let mut l = l.borrow_mut();
        l.retain(|(k, _)| k != key);
        l.push((key.to_string(), Box::new(f)));
    });
}

/// Run what `later` holds, first in first out, including what those add.
pub fn run_later() {
    while let Some((_, f)) = LATER.with(|l| {
        let mut l = l.borrow_mut();
        (!l.is_empty()).then(|| l.remove(0))
    }) {
        f();
    }
}

#[cfg(test)]
#[path = "test_saves.rs"]
mod tests;
