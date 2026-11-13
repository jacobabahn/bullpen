//! Local mode and remote mode (bullpen/modes.py), as far as what a task's branch is
//! compared with.

use crate::core::{git, Repo, Res};
use crate::json::{get, py_str, s, truthy, Obj};

/// Is the task in remote mode? Its record from when it was published, else the repo's.
fn remote(repo: &Repo, t: &Obj) -> Res<bool> {
    if let Some(local) = get(t, "local") {
        return Ok(!truthy(Some(local)));
    }
    if truthy(t.get("pr")) {
        return Ok(true); // published before the record was kept
    }
    repo.has_origin()
}

/// What the task's branch is compared with: origin's copy of its base branch in remote
/// mode (when origin has it), else the local base branch.
pub fn task_base_ref(repo: &Repo, t: &Obj) -> Res<String> {
    let base = t.get("base").map_or("None".into(), py_str);
    if remote(repo, t)? && truthy(t.get("worktree")) {
        let on_origin = format!("origin/{base}");
        if git(s(t, "worktree"), &["rev-parse", "-q", "--verify", &on_origin], false)?.code == 0 {
            return Ok(on_origin);
        }
    }
    Ok(base)
}
