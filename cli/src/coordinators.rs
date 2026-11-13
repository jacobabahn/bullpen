//! A repo's coordinators, and who sees to a task (bullpen/coordinators.py), as far as
//! reading them: which one a pane is, which are alive, and whose a task is. Dropping a
//! gone one (`_coordinator_gone`) stays with Python until T10.
//!
//! cfg["coordinators"] lists them, oldest (the primary) first: {"id": "c1", "block",
//! "session", "board": {"block", "session"}, "board_others"}. A task belongs to the one
//! that added it (t["coordinator"]). Who sees to it is that one alone while its pane is
//! running, else every running coordinator.

use bullpen_core::json::{obj, s, Obj};
use bullpen_core::{Repo, Res};
use serde_json::Value;

/// The repo's coordinators, oldest (the primary) first (an entry that isn't an object is left out).
pub fn coordinators(repo: &Repo) -> Vec<&Obj> {
    let list = repo.cfg.get("coordinators").and_then(Value::as_array);
    list.into_iter().flatten().filter_map(Value::as_object).collect()
}

pub fn coordinator<'a>(repo: &'a Repo, cid: &str) -> Option<&'a Obj> {
    if cid.is_empty() {
        return None;
    }
    coordinators(repo).into_iter().find(|c| s(c, "id") == cid)
}

/// The coordinator whose pane is block (or whose board is, with boards).
pub fn coordinator_at<'a>(repo: &'a Repo, block: &str, boards: bool) -> Option<&'a Obj> {
    if block.is_empty() {
        return None;
    }
    coordinators(repo)
        .into_iter()
        .find(|c| s(c, "block") == block || (boards && s(obj(c, "board"), "block") == block))
}

/// Is the coordinator's pane running?
pub fn alive(c: Option<&Obj>) -> Res<bool> {
    match c {
        Some(c) if !c.is_empty() => crate::rex::alive(s(c, "session"), s(c, "block")),
        _ => Ok(false),
    }
}

/// The task's coordinator, if it is still one (an older task may have none).
pub fn owner<'a>(repo: &'a Repo, t: &Obj) -> Option<&'a Obj> {
    coordinator(repo, s(t, "coordinator"))
}

/// The oldest coordinator, if there is one.
pub fn primary(repo: &Repo) -> Option<&Obj> {
    coordinators(repo).into_iter().next()
}

/// The coordinators who see to a task: its owner while the owner's pane is running,
/// else every running coordinator, else none (the task needs the person).
pub fn sees_to<'a>(repo: &'a Repo, t: &Obj) -> Res<Vec<&'a Obj>> {
    let own = owner(repo, t);
    if alive(own)? {
        return Ok(own.into_iter().collect());
    }
    let mut out = Vec::new();
    for c in coordinators(repo) {
        if alive(Some(c))? {
            out.push(c);
        }
    }
    Ok(out)
}

/// Where a task's panes live: with the coordinator who sees to it, else (none running)
/// with its owner, else the primary coordinator.
pub fn home_of(repo: &Repo, t: &Obj) -> Res<Obj> {
    let sees = sees_to(repo, t)?;
    let c = sees.first().copied().or_else(|| owner(repo, t)).or_else(|| primary(repo));
    Ok(c.cloned().unwrap_or_default())
}

/// The coordinator whose own pane ran this command, or None. Rex gives each pane its
/// block ID as REX_BLOCK. Unsure (no REX_BLOCK, or a stored block that is out of date)
/// counts as none.
pub fn calling_coordinator(repo: &Repo) -> Option<&Obj> {
    coordinator_at(repo, &std::env::var("REX_BLOCK").unwrap_or_default(), false)
}
