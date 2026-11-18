//! A repo's coordinators as its settings list them, and who sees to a task: the rule
//! the command line (cli/src/coordinators.rs, asking Rex whether each pane is alive) and
//! the board (which takes a listed coordinator as running) both go by.
//!
//! cfg["coordinators"] lists them, oldest (the primary) first. A task belongs to the one
//! that added it (t["coordinator"]).

use crate::json::{s, Obj};
use crate::Res;
use serde_json::Value;

/// The repo's coordinators, oldest (the primary) first (an entry that isn't an object
/// is left out). cfg: the repo's settings.
pub fn list(cfg: &Obj) -> Vec<&Obj> {
    let list = cfg.get("coordinators").and_then(Value::as_array);
    list.into_iter().flatten().filter_map(Value::as_object).collect()
}

/// The coordinators who see to a task, of these: its owner while the owner's pane is
/// running (alive, asked of a coordinator), else every running coordinator, else none
/// (the task needs the person).
pub fn sees_to<'a>(cs: Vec<&'a Obj>, t: &Obj, alive: impl Fn(&Obj) -> Res<bool>) -> Res<Vec<&'a Obj>> {
    let id = s(t, "coordinator");
    let own = cs.iter().copied().find(|c| !id.is_empty() && s(c, "id") == id);
    if let Some(own) = own {
        if alive(own)? {
            return Ok(vec![own]);
        }
    }
    let mut out = Vec::new();
    for c in cs {
        if alive(c)? {
            out.push(c);
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "tests/test_coordinators.rs"]
mod tests;
