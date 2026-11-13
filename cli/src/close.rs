//! Closing a task (bullpen/close.py): the one close step that close, drop, finish's
//! auto-accept (and, from T9, accept) share, and the `close` and `drop` commands.
//! Python keeps close.py's `close` and `close_task` (a twin) for its accept and
//! auto-accept until T9.

use crate::agents::pane;
use crate::args::{flag, pos, Arg, Args, Nargs};
use crate::hooks::remove_worktree;
use crate::modes::withdraw;
use crate::news::{news, tell_coordinator, Opts};
use crate::review::close_reviewer;
use crate::rex::close_last;
use crate::saves::save;
use bullpen_core::json::{s, truthy, Obj};
use bullpen_core::proc::git;
use bullpen_core::repo::{repo_root, unindex_block};
use bullpen_core::task::{is_status, set_status};
use bullpen_core::text::strip;
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};
use std::path::Path;

const ID: Arg = pos("id", Nargs::One, "the task, like BP-3");
const CLOSE: &[Arg] = &[ID];
const DROP: &[Arg] = &[
    ID,
    flag("--force", "drop it even with uncommitted changes in its worktree"),
];

/// The one close step, in one order: the reviewer, the PR (withdrawn, unless it was
/// merged), the worktree and branch, the status and the save (which moves the run
/// queue), then what the caller says and does (then), and the worker's pane last,
/// since this may run in it. fields go on the task with the status.
pub fn close(
    repo: &Repo,
    t: &mut Obj,
    status: &str,
    then: impl FnOnce(&Obj) -> Res<()>,
    withdraw_pr: bool,
    force: bool,
    fields: &[(&str, Value)],
) -> Res<()> {
    std::env::set_current_dir(&repo.root)?; // we may be running in the worktree this removes
    let worker = pane(t, "worker");
    unindex_block(s(t, "block"))?;
    close_reviewer(t)?;
    if withdraw_pr {
        withdraw(repo, t, status == "done")?;
    }
    remove_worktree(repo, t, force)?;
    let why = t.get("why").and_then(Value::as_str).map(str::to_string);
    set_status(t, status, why.as_deref(), None);
    t.insert("block".into(), Value::Null);
    t.insert("worktree".into(), Value::Null);
    for (k, v) in fields {
        t.insert((*k).into(), v.clone());
    }
    save(repo, t)?;
    then(t)?;
    close_last(&worker.0, &worker.1)
}

/// `close` (the person's call): close a finished task: its worktree, branch and pane
/// go. `accept` does the same on a finished task; this stays as another name for it.
pub fn cmd_close(argv: &[String]) -> Res<i32> {
    let a = Args::parse("close", argv, CLOSE);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    if s(&t, "status") != "finished" {
        let hint = if s(&t, "status") == "ready" {
            format!(": `accept {}` merges it", s(&t, "id"))
        } else {
            String::new()
        };
        return fail(format!("{}, not finished{hint}", is_status(&t)));
    }
    close_task(&repo, &mut t, false, "")?;
    Ok(0)
}

/// A finished task (nothing to merge): close its PR if it has one, and mark it done.
/// auto: auto-accept closes it, and first is the first line of its summary.
pub fn close_task(repo: &Repo, t: &mut Obj, auto: bool, first: &str) -> Res<()> {
    let said = |t: &Obj| -> Res<()> {
        let id = s(t, "id");
        if auto {
            let line = format!("{id} finished (nothing to merge) and was closed by auto-accept: {first}");
            news(repo, t, "closed", &line, Opts::default())
        } else {
            println!("{id} closed");
            tell_coordinator(repo, t, "closed", &format!("{id} closed (finished, nothing to merge)"))
        }
    };
    let fields = [("closed", json!(true)), ("auto_accepted", json!(auto))];
    close(repo, t, "done", said, true, true, &fields)
}

// ---------- drop ----------

/// `drop`: stop a task, close its PR, remove its worktree.
pub fn cmd_drop(argv: &[String]) -> Res<i32> {
    let a = Args::parse("drop", argv, DROP);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let force = a.flag("--force");
    let wt = s(&t, "worktree");
    let dirty = |wt: &str| -> Res<bool> { Ok(!strip(&git(wt, &["status", "--porcelain"], true)?.stdout).is_empty()) };
    if !wt.is_empty() && Path::new(wt).exists() && !force && dirty(wt)? {
        return fail(format!(
            "{} has uncommitted changes in {wt}; pass --force to discard them",
            s(&t, "id")
        ));
    }
    let said = |t: &Obj| -> Res<()> {
        let id = s(t, "id");
        println!("{id} dropped");
        tell_coordinator(&repo, t, "dropped", &format!("{id} dropped"))?;
        dropped_dep(&repo, t)
    };
    close(&repo, &mut t, "dropped", said, true, force, &[])?;
    Ok(0)
}

/// t was dropped: the tasks waiting on it stay todo, with one news line.
fn dropped_dep(repo: &Repo, t: &Obj) -> Res<()> {
    let id = s(t, "id");
    let waiting: Vec<String> = repo
        .tasks()?
        .iter()
        .filter(|o| s(o, "status") == "todo" && waits_on(o, id))
        .map(|o| s(o, "id").to_string())
        .collect();
    if waiting.is_empty() {
        return Ok(());
    }
    news(
        repo,
        t,
        "dropped-dependency",
        &dropped_line(&waiting, id),
        Opts {
            everyone: true,
            ..Opts::default()
        },
    )
}

/// Does task o wait on id (its `after`)?
fn waits_on(o: &Obj, id: &str) -> bool {
    match o.get("after") {
        Some(Value::Array(after)) if truthy(o.get("after")) => after.iter().any(|v| v.as_str() == Some(id)),
        _ => false,
    }
}

/// The news that waiting tasks wait on id, which was dropped.
pub fn dropped_line(waiting: &[String], id: &str) -> String {
    let one = waiting.len() == 1;
    let names = waiting.join(", ");
    let first = &waiting[0];
    let (verb, stays, them) = if one {
        ("waits", "it stays", "it")
    } else {
        ("wait", "they stay", "them")
    };
    format!(
        "{names} {verb} on {id}, which was dropped, so {stays} queued. Ask the person what to do: \
         `bullpen after {first} --none` (or other tasks) to go ahead without it, then \
         `bullpen start {first}`; or drop {them}"
    )
}

#[cfg(test)]
#[path = "test_close.rs"]
mod tests;
