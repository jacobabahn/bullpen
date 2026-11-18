//! Saving a task, and what a save sets off: core writes it (`Repo::save_task`, which
//! returns the task as it was), then the hooks run in order (`save`): the run queue and
//! then pickup (both at later-time), the
//! person's notification (notifications.rs), the PR to follow (for prs.rs's `_prs`).
//! Work that waits for the command's own work to be done (the run queue starting the
//! next task) goes in `later`, and main runs it when the command returns (`run_later`).

use bullpen_core::fail::Res;
use bullpen_core::json::{obj, s, truthy, Obj};
use bullpen_core::paths::state;
use bullpen_core::repo::Repo;
use std::cell::RefCell;

/// Save the task, then run what the save sets off, in one fixed order.
pub fn save(repo: &Repo, t: &mut Obj) -> Res<()> {
    let old = repo.save_task(t)?;
    crate::copies::note_command(repo);
    move_queue(repo, t, &old);
    crate::notifications::notify(repo, t, &old);
    follow_saved(t)?;
    Ok(())
}

/// move_queue: the run queue hears of every status change. When a task frees a slot or
/// closes, the queue starts the next queued tasks once the command's own work is done,
/// reading the tasks fresh then; then pickup may start a backlog item (backlog/pickup.rs).
fn move_queue(repo: &Repo, t: &Obj, old: &Obj) {
    let Some(how) = crate::start::slot_freed(old, t) else {
        return;
    };
    let (root, by) = (repo.root_str(), t.clone());
    later(&format!("queue {root}"), move || {
        // the queue first: tasks waiting on this one go before the backlog
        let moved = Repo::new(&root).and_then(|repo| {
            crate::start::queue::start_queued(&repo, &by, how)?;
            crate::backlog::pickup::pickup(&Repo::new(&root)?).map(|_| ())
        });
        if let Err(e) = moved {
            eprintln!("bullpen: {e}");
        }
    });
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

/// follow_saved: a saved task whose PR the watcher looks at marks it to follow
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
#[path = "tests/test_saves.rs"]
mod tests;
