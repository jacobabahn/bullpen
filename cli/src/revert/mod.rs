//! `bullpen revert BP-N`: take an accepted task back. It queues the revert as a task of
//! its own (`Revert BP-N: <title>`, with `reverts: BP-N` on it), whose brief names the
//! commits to revert (commits.rs), and starts it within max_running, so the revert gets
//! a branch, a review and an accept like any other change. Nothing is reverted here.

pub mod commits;

use crate::args::{pos, value, Arg, Args, Nargs};
use crate::news::tell_coordinator;
use crate::start::launch::py_format;
use bullpen_core::json::{py_str, s, truthy, Obj};
use bullpen_core::paths::prompts;
use bullpen_core::proc::git;
use bullpen_core::repo::repo_root;
use bullpen_core::task::{is_closed, is_status, pr_short, status_words};
use bullpen_core::text::{shell_join, squash, strip};
use bullpen_core::{fail, Repo, Res};
use commits::Taken;
use serde_json::{json, Value};

const REVERT: &[Arg] = &[
    pos("id", Nargs::One, "the accepted task to take back, like BP-3"),
    value(
        "--why",
        "WHY",
        "why it's taken back: it goes into the revert task's brief",
    ),
];

/// Was the task accepted: done, by a merge (not closed with nothing to merge, or with
/// its branch kept)?
fn accepted(t: &Obj) -> bool {
    s(t, "status") == "done" && !truthy(t.get("closed")) && !truthy(t.get("kept"))
}

/// Why task t can't be reverted: only an accepted one, with something merged, can.
fn not_accepted(t: &Obj) -> Option<String> {
    let id = s(t, "id");
    let merged = truthy(t.get("merged")) || truthy(t.get("pr"));
    Some(match s(t, "status") {
        "dropped" => format!("{id} was dropped: nothing of it was merged, so there is nothing to revert"),
        "done" if truthy(t.get("kept")) => {
            format!("{id} was closed with its branch kept, not merged: there is nothing to revert")
        }
        "done" if truthy(t.get("closed")) || !merged => {
            format!("{id} finished with nothing to merge: there is nothing to revert")
        }
        "done" => return None,
        _ => format!("{}: only an accepted task can be reverted", is_status(t)),
    })
}

/// What reverts a task, if anything does.
#[derive(Debug, PartialEq)]
pub enum Reverted {
    /// a revert task that isn't closed yet: its ID
    Open(String),
    /// an accepted revert task: its ID, and the task that reverted that one in turn
    By(String, Option<String>),
}

/// What reverts task id among tasks: an open revert task first, else the last accepted
/// one. A revert task that was dropped or closed unmerged reverts nothing.
pub fn reverted(tasks: &[Obj], id: &str) -> Option<Reverted> {
    let of: Vec<&Obj> = tasks.iter().filter(|o| s(o, "reverts") == id).collect();
    if let Some(open) = of.iter().find(|o| !is_closed(s(o, "status"))) {
        return Some(Reverted::Open(s(open, "id").to_string()));
    }
    let by = of.iter().rfind(|o| accepted(o))?;
    let by_id = s(by, "id");
    // a revert has a later ID than what it reverts, so this ends
    let undone = match reverted(tasks, by_id) {
        Some(Reverted::By(again, None)) => Some(again),
        _ => None,
    };
    Some(Reverted::By(by_id.to_string(), undone))
}

/// For `show`: what the task reverts, and what reverts it.
pub fn show_lines(t: &Obj, tasks: &[Obj]) -> Vec<String> {
    let mut out = Vec::new();
    if truthy(t.get("reverts")) {
        out.push(format!("  reverts: {}", py_str(&t["reverts"])));
    }
    match reverted(tasks, s(t, "id")) {
        Some(Reverted::Open(m)) => out.push(format!(" reverted: a revert is open: {m}")),
        Some(Reverted::By(m, None)) => out.push(format!(" reverted: reverted by {m}")),
        Some(Reverted::By(m, Some(k))) => out.push(format!(" reverted: reverted by {m} (which {k} reverted)")),
        None => {}
    }
    out
}

/// Why id can't be reverted now: a revert of it is open, or one was accepted (and not
/// taken back itself).
fn already(tasks: &[Obj], id: &str) -> Option<String> {
    match reverted(tasks, id)? {
        Reverted::Open(m) => {
            let status = tasks.iter().find(|o| s(o, "id") == m).map_or("", |o| s(o, "status"));
            Some(format!(
                "{id} already has an open revert task: {m} ({})",
                status_words(status)
            ))
        }
        Reverted::By(m, None) => Some(format!(
            "{id} was reverted by {m} already: `bullpen revert {m}` brings it back"
        )),
        Reverted::By(_, Some(_)) => None,
    }
}

/// "merged into main, rebase", or "#7 merged, squash": where and how t was accepted.
fn where_merged(t: &Obj) -> String {
    let at = if truthy(t.get("pr")) {
        format!("{} merged", pr_short(&py_str(&t["pr"])))
    } else {
        format!("merged into {}", py_str(t.get("base").unwrap_or(&Value::Null)))
    };
    match s(t, "merged_how") {
        "" => at,
        "merge" => format!("{at}, by a merge commit"),
        how => format!("{at}, {how}"),
    }
}

/// The revert task's brief, from prompts/revert.md: the commits and what to do with them.
fn brief(repo: &Repo, t: &Obj, taken: &Taken, why: &str) -> Res<String> {
    let (id, title) = (s(t, "id"), s(t, "title"));
    let mut lines = Vec::new();
    for sha in &taken.commits {
        let subject = git(&repo.root, &["log", "-1", "--format=%s", sha], true)?.stdout;
        lines.push(format!("- `{sha}` {}", squash(&subject)));
    }
    let mut revert: Vec<&str> = vec!["git", "revert", "--no-commit"];
    if taken.mainline {
        revert.extend(["-m", "1"]);
    }
    revert.extend(taken.commits.iter().map(String::as_str));
    let message = format!("Revert {id}: {title}");
    let one = match (taken.mainline, taken.commits.len()) {
        (true, _) => {
            "It is a merge commit: `-m 1` reverts it against its first parent, the base as it was, which takes \
             the whole branch back."
        }
        (false, 1) => "One commit, so one revert.",
        _ => {
            "One commit for the lot, not one per commit: the revert is read, accepted and (if it comes to that) \
             taken back as one change, and a conflict with later work is settled once, not once per commit."
        }
    };
    let why = match strip(why) {
        "" => String::new(),
        why => format!("\nWhy it's taken back: {why}\n"),
    };
    let text = std::fs::read_to_string(prompts().join("revert.md"))?;
    let fields = [
        ("id", id.to_string()),
        ("title", title.to_string()),
        ("base", py_str(t.get("base").unwrap_or(&Value::Null))),
        ("why", why),
        ("where", where_merged(t)),
        ("count", bullpen_core::task::commits(taken.commits.len() as i64)),
        ("commits", lines.join("\n")),
        ("revert", shell_join(&revert)),
        ("commit", shell_join(&["git", "commit", "-m", &message])),
        ("one", one.to_string()),
    ];
    Ok(py_format(&text, &fields))
}

/// Why the new task isn't started now: as many tasks work as max_running allows.
fn no_room(repo: &Repo) -> Res<Option<String>> {
    let max = repo.cfg.get("max_running").and_then(Value::as_i64).unwrap_or(0);
    let working = repo.tasks()?.iter().filter(|o| s(o, "status") == "working").count() as i64;
    Ok((working >= max).then(|| {
        format!(
            "{} working, and max_running is {max}",
            if working == 1 {
                "1 task is".to_string()
            } else {
                format!("{working} tasks are")
            }
        )
    }))
}

/// `revert` (the person's call): queue a task that takes an accepted task back, and start it.
pub fn cmd_revert(argv: &[String]) -> Res<i32> {
    let a = Args::parse("revert", argv, REVERT);
    let repo = Repo::new(repo_root()?)?;
    let t = repo.task(a.arg("id").unwrap_or(""))?;
    let id = s(&t, "id");
    if let Some(why) = not_accepted(&t) {
        return fail(why);
    }
    if let Some(why) = already(&repo.tasks()?, id) {
        return fail(why);
    }
    let taken = commits::commits_of(&repo, &t)?;
    let text = brief(&repo, &t, &taken, a.opt("--why").unwrap_or(""))?;
    let base = py_str(t.get("base").unwrap_or(&Value::Null));
    let title = format!("Revert {id}: {}", s(&t, "title"));
    let mut extra = Obj::new();
    extra.insert("reverts".into(), json!(id));
    // `--` last: a title may start with a dash
    let add: Vec<String> = ["--brief", &text, "--base", &base, "--", &title]
        .map(String::from)
        .to_vec();
    let tid = crate::start::add_in(Repo::new(&repo.root)?, &add, extra)?;
    println!(
        "{tid} added: {title} ({} to revert)",
        bullpen_core::task::commits(taken.commits.len() as i64)
    );
    let stays = |why: &str| -> Res<()> {
        println!("{tid} stays todo ({why}): `bullpen start {tid}` starts it");
        let line = format!("{tid} was added to revert {id} but isn't started ({why}): `bullpen start {tid}` starts it");
        tell_coordinator(&repo, &repo.task(&tid)?, "queue", &line)
    };
    if let Some(why) = no_room(&repo)? {
        stays(&why)?;
        return Ok(0);
    }
    match crate::start::launch::start(std::slice::from_ref(&tid)) {
        Ok(new) => {
            println!(
                "{tid} started in {} (branch {}, pane {})",
                s(&new, "worktree"),
                s(&new, "branch"),
                s(&new, "block")
            );
            tell_coordinator(&repo, &new, "started", &format!("{tid} started (it reverts {id})"))?;
            Ok(0)
        }
        Err(e) => {
            stays(&format!("it didn't start: {}", squash(&e.0)))?;
            Ok(1)
        }
    }
}

#[cfg(test)]
#[path = "test_revert.rs"]
mod tests;
