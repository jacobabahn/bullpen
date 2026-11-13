//! A repo's coordinators (bullpen/coordinators.py).

use crate::core::Repo;
use crate::json::{s, Obj};

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
