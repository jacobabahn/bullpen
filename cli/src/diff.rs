//! `diff ID --stat` (bullpen/show.py cmd_diff, diff_stat). The diff itself goes through
//! a pager or a pane, so without --stat (or with --pane) it stays with Python.

use crate::args::Args;
use crate::core::{fail, git, is_status, repo_root, Fail, Repo, Res};
use crate::json::{s, truthy};
use crate::modes::task_base_ref;
use crate::text::{rstrip, splitlines};

const UNCOMMITTED: &str = "Uncommitted (not in any commit yet):";

pub fn cmd_diff(a: &Args) -> Res<()> {
    if !a.flag("--stat") || a.flag("--pane") {
        return Err(Fail::Python);
    }
    let repo = Repo::new(&repo_root()?)?;
    let t = repo.task(a.arg(0).unwrap_or(""))?;
    let wt = s(&t, "worktree");
    if !truthy(t.get("worktree")) || !std::path::Path::new(wt).is_dir() {
        return fail(format!("{}, with no worktree, so there's nothing to diff", is_status(&t)));
    }
    println!("{}", diff_stat(wt, &task_base_ref(&repo, &t)?, s(&t, "id"))?);
    Ok(())
}

/// The files the branch changed since its base, then any not committed yet.
fn diff_stat(wt: &str, base_ref: &str, tid: &str) -> Res<String> {
    let committed = git(wt, &["diff", "--stat", &format!("{base_ref}...HEAD")], true)?.stdout;
    let mut parts = vec![match rstrip(&committed) {
        "" => format!("{tid} has no commits against {base_ref}."),
        c => c.to_string(),
    }];
    let loose = git(wt, &["diff", "--stat", "HEAD"], true)?.stdout;
    let loose = rstrip(&loose);
    let untracked = git(wt, &["ls-files", "--others", "--exclude-standard"], true)?.stdout;
    let untracked = splitlines(&untracked);
    if !loose.is_empty() || !untracked.is_empty() {
        let mut lines = vec![UNCOMMITTED.to_string()];
        if !loose.is_empty() {
            lines.push(loose.to_string());
        }
        lines.extend(untracked.iter().map(|f| format!(" untracked: {f}")));
        parts.push(lines.join("\n"));
    }
    Ok(parts.join("\n\n"))
}
