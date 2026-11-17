//! Overlap: two open tasks whose branches change the same file. Only news, once per
//! pair of tasks (again when new files join): nothing waits on it, no worker is told,
//! and no order is forced. It's checked when a worker ends a turn and on `ready`, for
//! the task that did (`check`); `show` lists a task's overlaps (`overlaps_of`).
//!
//! What was said is kept in `overlaps.json` in the repo's state folder, by pair
//! ("BP-3 BP-5": the files said so far). A pair with a task that's no longer open
//! drops out at the next check, and `show` leaves it out at once.
//!
//! Room for later: a trial merge of the pair (rebase.rs's `merge-tree`) to tell a real
//! conflict from files that merge cleanly, telling the workers, ordering accepts.

use crate::coordinators::sees_to;
use crate::modes::task_base_ref;
use crate::news::{news, Opts};
use bullpen_core::json::{py_str, s, truthy, Obj};
use bullpen_core::task::is_closed;
use bullpen_core::{Repo, Res};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Files that every task may change without it meaning anything (lock files, a
/// changelog): never an overlap. By name, in any folder.
pub const IGNORED: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "poetry.lock",
    "uv.lock",
    "CHANGELOG.md",
];

type Pairs = BTreeMap<String, BTreeSet<String>>;

fn ignored(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    IGNORED.contains(&name)
}

/// Whether a task is open with a branch of its own to compare: started, not closed or
/// finished (a finished task leaves nothing to merge), its worktree there.
pub fn is_open(t: &Obj) -> bool {
    let status = s(t, "status");
    !is_closed(status)
        && !matches!(status, "finished" | "todo")
        && truthy(t.get("worktree"))
        && Path::new(s(t, "worktree")).is_dir()
}

/// The files the task's branch changed, against where it left its base (committed work).
fn changed(repo: &Repo, t: &Obj) -> Res<BTreeSet<String>> {
    let base_ref = task_base_ref(repo, t)?;
    let range = format!("{base_ref}...HEAD");
    let Some(files) = crate::git_names::names(s(t, "worktree"), &["diff", "--name-only", "--no-renames", &range])?
    else {
        return Ok(BTreeSet::new()); // no base to compare with (not fetched yet): nothing to say
    };
    Ok(files.into_iter().filter(|f| !ignored(f)).collect())
}

/// Task IDs in their order: BP-9 before BP-10.
fn by_number(a: &str, b: &str) -> std::cmp::Ordering {
    (a.len(), a).cmp(&(b.len(), b))
}

/// The key of a pair: its two IDs in order, with a space.
fn pair_key(a: &str, b: &str) -> String {
    let (x, y) = if by_number(a, b).is_le() { (a, b) } else { (b, a) };
    format!("{x} {y}")
}

fn store(repo: &Repo) -> PathBuf {
    repo.dir.join("overlaps.json")
}

fn load(repo: &Repo) -> Pairs {
    let Ok(Some(Value::Object(o))) = bullpen_core::json::read(&store(repo)) else {
        return Pairs::new();
    };
    o.into_iter()
        .map(|(k, v)| {
            let files = v.as_array().into_iter().flatten().map(py_str).collect();
            (k, files)
        })
        .collect()
}

fn write(repo: &Repo, pairs: &Pairs) -> Res<()> {
    let o: Map<String, Value> = pairs.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    std::fs::create_dir_all(&repo.dir)?;
    bullpen_core::json::write(&store(repo), &Value::Object(o))?;
    Ok(())
}

/// The news line for a pair: "BP-3 and BP-5 both changed a.rs (and 2 more)", with
/// "too" when it adds files to what was said before.
pub fn overlap_line(key: &str, new: &[String], again: bool) -> String {
    let (a, b) = key.split_once(' ').unwrap_or((key, ""));
    let first = new.first().map_or(String::new(), |f| crate::git_names::shown(f));
    let too = if again { " too" } else { "" };
    let more = match new.len() {
        0 | 1 => String::new(),
        n => format!(" (and {} more)", n - 1),
    };
    format!("{a} and {b} both changed {first}{too}{more}")
}

/// Check the task t against the other open tasks on its base, and say each new overlap.
/// Best effort: it never stops what called it (a failure is only left unsaid).
pub fn check(repo: &Repo, t: &Obj) {
    let _ = check_overlaps(repo, t);
}

fn check_overlaps(repo: &Repo, t: &Obj) -> Res<()> {
    let tasks = repo.tasks()?;
    let open: BTreeMap<String, &Obj> = tasks
        .iter()
        .filter(|o| is_open(o))
        .map(|o| (s(o, "id").to_string(), o))
        .collect();
    let _lock = bullpen_core::lock::lock(&repo.dir.join("overlaps.lock"))?;
    let mut pairs = load(repo);
    let before = pairs.len();
    pairs.retain(|k, _| k.split(' ').all(|id| open.contains_key(id)));
    let mut dirty = pairs.len() != before;
    let id = s(t, "id");
    let Some(me) = open.get(id) else {
        return if dirty { write(repo, &pairs) } else { Ok(()) };
    };
    let mine = changed(repo, me)?;
    let mut said = Vec::new();
    if !mine.is_empty() {
        for (oid, o) in &open {
            if oid == id || o.get("base") != me.get("base") {
                continue;
            }
            let both: BTreeSet<String> = changed(repo, o)?.intersection(&mine).cloned().collect();
            let key = pair_key(id, oid);
            let known = pairs.get(&key);
            let new: Vec<String> = both
                .iter()
                .filter(|f| !known.is_some_and(|k| k.contains(*f)))
                .cloned()
                .collect();
            if new.is_empty() {
                continue;
            }
            said.push((overlap_line(&key, &new, known.is_some()), *o));
            pairs.entry(key).or_default().extend(new);
            dirty = true;
        }
    }
    if dirty {
        write(repo, &pairs)?;
    }
    for (line, o) in said {
        news(
            repo,
            me,
            "overlap",
            &line,
            Opts {
                to: whom(repo, me, o)?,
                ..Opts::default()
            },
        )?;
    }
    Ok(())
}

/// The coordinators that see to either task (None: whoever news() picks for the first).
fn whom(repo: &Repo, a: &Obj, b: &Obj) -> Res<Option<Vec<String>>> {
    let mut to: Vec<String> = Vec::new();
    for t in [a, b] {
        for c in sees_to(repo, t)? {
            let cid = s(c, "id").to_string();
            if !to.contains(&cid) {
                to.push(cid);
            }
        }
    }
    Ok((!to.is_empty()).then_some(to))
}

/// The open tasks the task overlaps with, and the files said, for `show`.
pub fn overlaps_of(repo: &Repo, tid: &str, tasks: &[Obj]) -> Vec<(String, Vec<String>)> {
    let open: BTreeSet<&str> = tasks.iter().filter(|o| is_open(o)).map(|o| s(o, "id")).collect();
    if !open.contains(tid) {
        return Vec::new();
    }
    let mut out: Vec<(String, Vec<String>)> = load(repo)
        .into_iter()
        .filter_map(|(k, files)| {
            let (a, b) = k.split_once(' ')?;
            let other = if a == tid {
                b
            } else if b == tid {
                a
            } else {
                return None;
            };
            open.contains(other)
                .then(|| (other.to_string(), files.into_iter().collect()))
        })
        .collect();
    out.sort_by(|x, y| by_number(&x.0, &y.0));
    out
}

#[cfg(test)]
#[path = "test_overlap.rs"]
mod tests;
