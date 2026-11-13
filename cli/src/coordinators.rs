//! A repo's coordinators (bullpen/coordinators.py).

use crate::core::{Repo, Res};
use crate::json::{obj, s, Obj};

pub fn coordinator<'a>(repo: &'a Repo, cid: &str) -> Option<&'a Obj> {
    if cid.is_empty() {
        return None;
    }
    repo.coordinators().into_iter().find(|c| s(c, "id") == cid)
}

/// The task's coordinator, if it is still one (an older task may have none).
pub fn owner<'a>(repo: &'a Repo, t: &Obj) -> Option<&'a Obj> {
    coordinator(repo, s(t, "coordinator"))
}

/// The coordinator whose pane is block (or whose board is, with boards).
pub fn coordinator_at<'a>(repo: &'a Repo, block: &str, boards: bool) -> Option<&'a Obj> {
    if block.is_empty() {
        return None;
    }
    repo.coordinators()
        .into_iter()
        .find(|c| s(c, "block") == block || (boards && s(obj(c, "board"), "block") == block))
}

/// The coordinator whose own pane ran this command (Rex gives each pane REX_BLOCK), or None.
pub fn calling_coordinator(repo: &Repo) -> Option<&Obj> {
    coordinator_at(repo, &std::env::var("REX_BLOCK").unwrap_or_default(), false)
}

/// The oldest coordinator, if there is one.
pub fn primary(repo: &Repo) -> Option<&Obj> {
    repo.coordinators().into_iter().next()
}

/// Is the coordinator's pane running?
pub fn alive(c: Option<&Obj>) -> Res<bool> {
    match c {
        Some(c) if !c.is_empty() => crate::rex::alive(s(c, "session"), s(c, "block")),
        _ => Ok(false),
    }
}

/// Does any coordinator see to the task (sees_to isn't empty): its owner, else any, running?
pub fn anyone_sees_to(repo: &Repo, t: &Obj) -> Res<bool> {
    if alive(owner(repo, t))? {
        return Ok(true);
    }
    for c in repo.coordinators() {
        if alive(Some(c))? {
            return Ok(true);
        }
    }
    Ok(false)
}
