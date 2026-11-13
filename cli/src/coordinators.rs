//! A repo's coordinators (bullpen/coordinators.py): what the commands moved so far need.

use bullpen_core::json::{s, Obj};
use bullpen_core::Repo;
use serde_json::Value;

/// The repo's coordinators, oldest (the primary) first.
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

/// The task's coordinator, if it is still one (an older task may have none).
pub fn owner<'a>(repo: &'a Repo, t: &Obj) -> Option<&'a Obj> {
    coordinator(repo, s(t, "coordinator"))
}
