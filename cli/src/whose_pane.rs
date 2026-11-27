//! Whose pane `prompt`, `answer` and `peek` work on: a task's (its ID), or with
//! `--coordinator cN` a coordinator's own, in this repository or the one `--repo` names.

use crate::args::{usage_error, value, Arg, Args};
use crate::go::repo_or_here;
use crate::host::Agent;
use bullpen_core::json::Obj;
use bullpen_core::repo::repo_root;
use bullpen_core::{Repo, Res};

pub const COORDINATOR: Arg = value(
    "--coordinator",
    "ID",
    "a coordinator's own pane, like c1, in place of a task's",
);
pub const REPO: Arg = value(
    "--repo",
    "REPO",
    "the repository (default: the pane's, else this folder's)",
);

/// What the arguments name.
pub enum Pane {
    /// a task, by its ID
    Task(Obj),
    /// a coordinator, running: its ID and its agent
    Coordinator(String, Agent),
}

/// The pane cmd's arguments name (spec is its arguments, for a usage error): the task
/// its "id" names, or the coordinator --coordinator names (never both), and its repo.
pub fn named(cmd: &str, spec: &[Arg], a: &Args) -> Res<(Repo, Pane)> {
    let (id, cid, path) = (a.arg("id"), a.opt("--coordinator"), a.opt("--repo"));
    let block = std::env::var("REX_BLOCK").unwrap_or_default();
    match (id, cid) {
        (Some(_), Some(_)) => usage_error(cmd, spec, "give a task's ID or --coordinator, not both"),
        (None, None) => usage_error(cmd, spec, "the following arguments are required: id"),
        (Some(id), None) => {
            let repo = match path {
                Some(p) => repo_or_here(p, &block)?,
                None => Repo::new(repo_root()?)?,
            };
            let t = repo.task(id)?;
            Ok((repo, Pane::Task(t)))
        }
        (None, Some(cid)) => {
            let repo = repo_or_here(path.unwrap_or(""), &block)?;
            let cid = crate::say::which(&repo, cid)?;
            let (repo, c) = crate::say::running(repo, &cid)?;
            Ok((repo, Pane::Coordinator(cid, Agent::of(&c))))
        }
    }
}
