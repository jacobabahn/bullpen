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

/// The coordinator cid, to change (the caller saves the repo).
pub fn coordinator_mut<'a>(repo: &'a mut Repo, cid: &str) -> Option<&'a mut Obj> {
    let list = repo.cfg.get_mut("coordinators").and_then(Value::as_array_mut)?;
    list.iter_mut()
        .filter_map(Value::as_object_mut)
        .find(|c| !cid.is_empty() && s(c, "id") == cid)
}

/// Does coordinator c's board show the task? Only one c sees to, or one nobody sees
/// to, while c's board_others is off. With one coordinator (or c None), every task.
pub fn on_board(repo: &Repo, c: Option<&Obj>, t: &Obj) -> Res<bool> {
    let Some(c) = c.filter(|c| !c.is_empty()) else {
        return Ok(true);
    };
    if bullpen_core::json::truthy(c.get("board_others")) || coordinators(repo).len() < 2 {
        return Ok(true);
    }
    let sees = sees_to(repo, t)?;
    Ok(sees.is_empty() || sees.iter().any(|o| o.get("id") == c.get("id")))
}

/// The Rex session to work in: given (--session), else the primary coordinator's while
/// it exists (a Rex restart gives sessions new IDs), else the one we're running in.
pub fn session_for(repo: &Repo, given: Option<&str>) -> Res<String> {
    if let Some(s) = given.filter(|s| !s.is_empty()) {
        return Ok(s.into());
    }
    let c = primary(repo).map_or("", |c| s(c, "session"));
    let here = std::env::var("REX_SESSION").unwrap_or_default();
    if !c.is_empty() && (c == here || crate::rex::rex_view(c)?.is_some()) {
        return Ok(c.into());
    }
    Ok(if here.is_empty() { c.into() } else { here })
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
    let c = sees
        .first()
        .copied()
        .or_else(|| owner(repo, t))
        .or_else(|| primary(repo));
    Ok(c.cloned().unwrap_or_default())
}

/// The coordinator whose own pane ran this command, or None. Rex gives each pane its
/// block ID as REX_BLOCK. Unsure (no REX_BLOCK, or a stored block that is out of date)
/// counts as none.
pub fn calling_coordinator(repo: &Repo) -> Option<&Obj> {
    coordinator_at(repo, &std::env::var("REX_BLOCK").unwrap_or_default(), false)
}
