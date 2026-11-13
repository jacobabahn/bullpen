//! Saving a task, and what a save sets off: core writes it (`Repo::save_task`, which
//! returns the task as it was), then the hooks run in cli.SAVERS' order (the run queue,
//! the person's notification, the PR to follow). Work that waits for the command's own
//! work to be done (the run queue starting the next task) goes in `later`, and main runs
//! it when the command returns.
//!
//! No Rust command saves a task yet: the hooks are stubs, and T3 makes them real.
#![allow(dead_code)]

use bullpen_core::fail::Res;
use bullpen_core::json::Obj;
use bullpen_core::repo::Repo;
use std::cell::RefCell;

/// Save the task, then run what the save sets off, in one fixed order.
pub fn save(repo: &Repo, t: &mut Obj) -> Res<()> {
    let old = repo.save_task(t)?;
    move_queue(repo, t, &old);
    notify(repo, t, &old);
    follow_saved(repo, t, &old);
    Ok(())
}

/// start.move_queue: a save that frees a slot starts queued tasks (T3: through `bullpen _queue`).
fn move_queue(_repo: &Repo, _t: &Obj, _old: &Obj) {}

/// notifications.notify: the person's notification for a change (T3).
fn notify(_repo: &Repo, _t: &Obj, _old: &Obj) {}

/// prs.follow_saved: mark a PR for the watcher to follow (T3).
fn follow_saved(_repo: &Repo, _t: &Obj, _old: &Obj) {}

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
