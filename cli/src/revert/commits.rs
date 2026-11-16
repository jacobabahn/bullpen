//! Which commits an accepted task left on its base, from its record alone (never from
//! commit messages). An accept records `merged` (the base's head after it), and since
//! `revert` came, `merged_how` (rebase, squash or merge) and in local mode `merged_from`
//! (the base's head before it); `ready` records `commits` (how many the branch had).
//!
//!   rebase   the commits from `merged_from` to `merged` (without it, a PR's: the last
//!            `commits` ones up to `merged`), each reverted
//!   squash   `merged`, the one commit
//!   merge    `merged`, the merge commit, reverted against its first parent (the base)
//!   unknown  (accepted before `merged_how` was kept, or a PR merged outside bullpen):
//!            a commit with two parents is a merge commit, and a branch of one commit
//!            left one commit either way; anything else can't be told, and is refused

use bullpen_core::json::{py_str, s, truthy, Obj};
use bullpen_core::proc::{git, run_in};
use bullpen_core::text::{split, splitlines, strip};
use bullpen_core::{fail, Repo, Res};
use serde_json::Value;

/// What to revert: the commits, newest first (the order to revert them in), and whether
/// it is a merge commit (reverted with `-m 1`).
#[derive(Debug, PartialEq)]
pub struct Taken {
    pub commits: Vec<String>,
    pub mainline: bool,
}

/// What the record and the merged commit's shape say the commits are.
#[derive(Debug, PartialEq)]
pub enum Shape {
    /// `merged` alone: a squash's commit, or a branch of one commit
    One,
    /// `merged`, a merge commit
    Merge,
    /// the commits after this one, up to `merged`
    Since(String),
    /// the last n commits up to `merged`
    Last(i64),
}

/// The shape of what task t's merge left, given how many parents its `merged` commit
/// has; or why it can't be told.
pub fn shape(t: &Obj, parents: usize) -> Result<Shape, String> {
    let id = s(t, "id");
    let n = t.get("commits").and_then(Value::as_i64).unwrap_or(0);
    let odd = |how: &str, want: &str| {
        Err(format!(
            "{id}'s record says it was merged by {how}, but the commit it names has {parents} parent{}, not {want}: \
             its commits can't be told from its record",
            if parents == 1 { "" } else { "s" }
        ))
    };
    match s(t, "merged_how") {
        "merge" if parents == 2 => Ok(Shape::Merge),
        "merge" => odd("a merge commit", "2"),
        "squash" if parents == 1 => Ok(Shape::One),
        "squash" => odd("squash", "1"),
        "rebase" if truthy(t.get("merged_from")) => Ok(Shape::Since(s(t, "merged_from").to_string())),
        "rebase" if n >= 1 => Ok(Shape::Last(n)),
        "rebase" => Err(format!(
            "{id}'s record doesn't say how many commits it had: its commits can't be told from it"
        )),
        _ if parents == 2 => Ok(Shape::Merge),
        _ if parents == 1 && n == 1 => Ok(Shape::One),
        _ => Err(format!(
            "{id}'s record doesn't say how it was merged (it was accepted before bullpen kept that, or its PR was \
             merged outside bullpen), and its branch had {}: a rebase and a squash leave different commits, so \
             which are its can't be told. Revert it by hand",
            if n >= 1 {
                bullpen_core::task::commits(n)
            } else {
                "an unknown number of commits".into()
            }
        )),
    }
}

/// The commit a merged PR left on its base, as gh says it; None when the task has no
/// PR, or gh doesn't say (offline, not merged).
pub fn pr_merge_commit(repo: &Repo, t: &Obj) -> Option<String> {
    if !truthy(t.get("pr")) {
        return None;
    }
    let pr = py_str(t.get("pr").unwrap_or(&Value::Null));
    let argv = [
        repo.gh(),
        "pr".into(),
        "view".into(),
        pr,
        "--json".into(),
        "mergeCommit".into(),
    ];
    let p = run_in(&argv, &repo.root, false).ok().filter(|p| p.ok())?;
    let said: Value = serde_json::from_str(&p.stdout).ok()?;
    let oid = said.get("mergeCommit")?.get("oid")?.as_str()?;
    (!oid.is_empty()).then(|| oid.to_string())
}

/// The base as it is now: origin's copy, fetched, when the repo has an origin; else the
/// local branch.
fn base_now(repo: &Repo, base: &str) -> Res<String> {
    if repo.has_origin()? {
        crate::modes::quiet_git(repo, &["fetch", "-q", "origin", base], 60)?;
        let on_origin = format!("origin/{base}");
        if git(&repo.root, &["rev-parse", "-q", "--verify", &on_origin], false)?.ok() {
            return Ok(on_origin);
        }
    }
    Ok(base.to_string())
}

fn short(sha: &str) -> String {
    sha.chars().take(12).collect()
}

/// The commits accepted task t left on its base, to revert; or a refusal that says why
/// they can't be told, or that they're no longer in the base's history.
pub fn commits_of(repo: &Repo, t: &Obj) -> Res<Taken> {
    let (id, root) = (s(t, "id"), &repo.root);
    let base = py_str(t.get("base").unwrap_or(&Value::Null));
    let merged = match s(t, "merged") {
        "" => pr_merge_commit(repo, t),
        sha => Some(sha.to_string()),
    };
    let Some(merged) = merged else {
        return fail(format!(
            "{id}'s record doesn't name the commit its merge left on {base}{}: its commits can't be told from it. \
             Revert it by hand",
            if truthy(t.get("pr")) {
                ", and gh doesn't say it now"
            } else {
                ""
            }
        ));
    };
    let base_ref = base_now(repo, &base)?;
    let gone = || {
        fail(format!(
            "{id}'s commits are no longer in {base}'s history ({} isn't in {base_ref}): there is nothing to revert",
            short(&merged)
        ))
    };
    let commit = format!("{merged}^{{commit}}");
    if !git(root, &["cat-file", "-e", &commit], false)?.ok()
        || !git(root, &["merge-base", "--is-ancestor", &merged, &base_ref], false)?.ok()
    {
        return gone();
    }
    let line = git(root, &["rev-list", "--parents", "-n", "1", &merged], true)?.stdout;
    let parents = split(&line).len().saturating_sub(1);
    let list = |args: &[&str]| -> Res<Vec<String>> {
        let p = git(root, args, false)?;
        match p.ok() {
            true => Ok(splitlines(strip(&p.stdout)).iter().map(|l| l.to_string()).collect()),
            false => Ok(Vec::new()),
        }
    };
    let (commits, mainline) = match shape(t, parents).or_else(fail)? {
        Shape::One => (vec![merged.clone()], false),
        Shape::Merge => (vec![merged.clone()], true),
        Shape::Since(from) => (list(&["rev-list", &format!("{from}..{merged}")])?, false),
        Shape::Last(n) => {
            let found = list(&["rev-list", "--first-parent", "-n", &n.to_string(), &merged])?;
            // fewer than it had: the base's history is shorter than the record says
            (if found.len() as i64 == n { found } else { Vec::new() }, false)
        }
    };
    if commits.is_empty() {
        return fail(format!(
            "{id}'s record doesn't lead to its commits on {base} (from {}): revert it by hand",
            short(&merged)
        ));
    }
    Ok(Taken { commits, mainline })
}

#[cfg(test)]
#[path = "test_commits.rs"]
mod tests;
