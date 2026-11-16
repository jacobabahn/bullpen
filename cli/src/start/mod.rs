//! Tasks and what they wait on: `add`, `after`, `model` and
//! `review-mode`, the dependencies, and how a save moves the run queue (slot_freed).
//! Starting, resuming and telling a worker are in launch.rs; the queue in queue.rs.
//!
//! A task can wait on others (`add --after`, `after`): its "after" is a list of IDs.
//! `start` refuses it until they're all done. When a task is accepted or dropped, or its
//! worker hands it off (to review, ready or finished), the queue starts each todo task
//! that has dependencies, all done, within max_running.

use crate::args::{flag, pos, value, Arg, Args, Nargs};
use crate::coordinators::{calling_coordinator, primary};
use bullpen_core::json::{self, obj, py_str, s, truthy, Obj};
use bullpen_core::paths::home_path;
use bullpen_core::repo::repo_root;
use bullpen_core::settings::{PLACES, REVIEW_MODES};
use bullpen_core::task::{is_closed, is_status, set_auto_accept};
use bullpen_core::text::{rstrip, split};
use bullpen_core::timeutil::iso_now;
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub mod launch;
pub mod peek;
pub mod queue;

const ID: Arg = pos("id", Nargs::One, "the task, like BP-3");
const PANE: &str = "where its worker's pane goes, over config --workers: float, tab, tabs or background";

const ADD: &[Arg] = &[
    pos("title", Nargs::One, "a short name for the task"),
    value("--brief", "BRIEF", "what the worker should do, as text"),
    value("--brief-file", "FILE", "the same, read from a file (over --brief)"),
    value("--base", "BASE", "branch to start from and open the PR against"),
    value(
        "--after",
        "ID",
        "a task this one waits on (repeat, or comma-separated): it starts once they're accepted",
    ),
    value(
        "--model",
        "MODEL",
        "claude's --model for this task's worker, over the config's, whenever it starts (by itself too) or resumes",
    ),
    value(
        "--review",
        "REVIEW",
        "its first review: full, light, or auto (light for a small or docs-only change; the default)",
    )
    .choices(&REVIEW_MODES),
    flag(
        "--auto-accept",
        "accept it by itself once review approves it, whatever config --auto-accept says",
    ),
    flag(
        "--no-auto-accept",
        "keep it for you to accept even with config --auto-accept on",
    ),
    value("--pane", "PANE", PANE).choices(&PLACES),
];

const AFTER: &[Arg] = &[
    ID,
    pos(
        "ID",
        Nargs::Any,
        "the tasks it waits on: it starts once they're accepted",
    ),
    flag("--none", "it waits on nothing"),
];

const MODEL: &[Arg] = &[
    ID,
    pos(
        "model",
        Nargs::Maybe,
        "an alias like opus, or a model ID; default: let Claude Code pick",
    ),
    flag("--clear", "use the config's worker model again"),
];

const REVIEW_MODE: &[Arg] = &[
    ID,
    pos(
        "mode",
        Nargs::One,
        "light: a quick look; auto: light for a small or docs-only change",
    )
    .choices(&REVIEW_MODES),
];

/// The tasks by ID.
pub type Tasks = HashMap<String, Obj>;

pub fn by_id(repo: &Repo) -> Res<Tasks> {
    Ok(repo
        .tasks()?
        .into_iter()
        .map(|t| (s(&t, "id").to_string(), t))
        .collect())
}

/// IDs from --after values, each one or several: "BP-8", "8,9", "BP-8 BP-9".
pub fn dep_ids(repo: &Repo, values: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in values {
        for part in split(&v.replace(',', " ")) {
            let id = repo.norm(part);
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// after, if the task tid can wait on those tasks: each exists, isn't tid itself or
/// dropped, and none of them waits on tid already (a cycle).
fn check_after(repo: &Repo, tid: &str, after: Vec<String>) -> Res<Vec<String>> {
    let tasks = by_id(repo)?;
    for d in &after {
        if d == tid {
            return fail(format!("{tid} can't wait on itself"));
        }
        let Some(t) = tasks.get(d) else {
            return fail(format!(
                "no task {d} in {}: `bullpen ls --all` lists them",
                home_path(&repo.root)
            ));
        };
        if s(t, "status") == "dropped" {
            return fail(format!("{d} was dropped: {tid} would wait on it forever"));
        }
        if let Some(path) = dep_path(&tasks, d, tid, &mut HashSet::new()) {
            return fail(format!(
                "{d} already waits on {tid} ({}): that would be a cycle",
                path.join(" → ")
            ));
        }
    }
    Ok(after)
}

/// [start, ..., goal] if start waits on goal, directly or through others, else None.
pub fn dep_path(tasks: &Tasks, start: &str, goal: &str, seen: &mut HashSet<String>) -> Option<Vec<String>> {
    if start == goal {
        return Some(vec![goal.into()]);
    }
    if !seen.insert(start.into()) {
        return None;
    }
    let after = tasks.get(start).map(|t| json::strs(t, "after")).unwrap_or_default();
    for d in after {
        if let Some(rest) = dep_path(tasks, &d, goal, seen) {
            return Some([vec![start.to_string()], rest].concat());
        }
    }
    None
}

/// The leaving-working statuses that free a slot.
const FREES: [&str; 3] = ["review", "ready", "finished"];

/// How a save moves the run queue: "review", "ready" or "finished" for a task that left
/// working for one of them, "accepted", "closed" or "dropped" for one that closed (its
/// dependents may start), else None. A stop (needs, triage, stopped) frees nothing: its
/// agent comes back to the task.
pub fn slot_freed(old: &Obj, t: &Obj) -> Option<&'static str> {
    let was = old.get("status").and_then(Value::as_str).unwrap_or("");
    let now = s(t, "status");
    if is_closed(now) && !is_closed(was) {
        return Some(if now == "dropped" {
            "dropped"
        } else if truthy(t.get("closed")) {
            "closed"
        } else {
            "accepted"
        });
    }
    if was == "working" {
        return FREES.into_iter().find(|f| *f == now);
    }
    None
}

/// Path.read_text(): the file as UTF-8, with universal newlines.
pub fn read_text(path: &str) -> Res<String> {
    let bytes = std::fs::read(path).map_err(|e| {
        let n = e.raw_os_error().unwrap_or(0);
        let said = e.to_string();
        bullpen_core::Fail(format!(
            "[Errno {n}] {}: '{path}'",
            said.trim_end_matches(&format!(" (os error {n})"))
        ))
    })?;
    let text = String::from_utf8(bytes).map_err(|e| bullpen_core::Fail(format!("{path}: {e}")))?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// `add`: add a task; prints its ID.
pub fn cmd_add(argv: &[String]) -> Res<i32> {
    println!("{}", add(argv, Obj::new())?);
    Ok(0)
}

/// Add the task `add` would with these arguments, with extra's fields on it too (the
/// backlog's `from` and `origin`); its ID.
pub fn add(argv: &[String], extra: Obj) -> Res<String> {
    add_in(Repo::new(repo_root()?)?, argv, extra)
}

/// add, in the repo given (pickup's, from the watcher, runs outside its checkout).
pub fn add_in(mut repo: Repo, argv: &[String], extra: Obj) -> Res<String> {
    let a = Args::parse("add", argv, ADD);
    let brief = match a.opt("--brief-file").filter(|f| !f.is_empty()) {
        Some(f) => read_text(f)?,
        None => a.opt("--brief").unwrap_or("").to_string(),
    };
    if brief.is_empty() {
        return fail("give the task's brief with --brief or --brief-file");
    }
    let next = repo.cfg.get("next").cloned().unwrap_or(Value::Null);
    let tid = format!(
        "{}-{}",
        py_str(repo.cfg.get("prefix").unwrap_or(&Value::Null)),
        py_str(&next)
    );
    let after = check_after(&repo, &tid, dep_ids(&repo, &a.all("--after")))?;
    repo.cfg.insert("next".into(), json!(next.as_i64().unwrap_or(0) + 1));
    repo.save()?;
    let base = match a.opt("--base").filter(|b| !b.is_empty()) {
        Some(b) => b.to_string(),
        None => repo.base()?,
    };
    let mut t: Obj = Obj::new();
    t.insert("id".into(), json!(tid));
    t.insert("title".into(), json!(a.arg("title").unwrap_or("")));
    t.insert("status".into(), json!("todo"));
    t.insert("created".into(), json!(iso_now()));
    t.insert("base".into(), json!(base));
    for k in ["branch", "worktree", "block", "session", "pr", "why", "last"] {
        t.insert(k.into(), Value::Null);
    }
    // who hears its news
    if let Some(c) = calling_coordinator(&repo)
        .or_else(|| primary(&repo))
        .filter(|c| !c.is_empty())
    {
        t.insert("coordinator".into(), c.get("id").cloned().unwrap_or(Value::Null));
    }
    if !after.is_empty() {
        t.insert("after".into(), json!(after));
    }
    if let Some(m) = a.opt("--model").filter(|m| !m.is_empty()) {
        t.insert("worker_model".into(), json!(m)); // its own, over the config's, from its first start
    }
    if let Some(mode) = a.opt("--review").filter(|m| !m.is_empty() && *m != "auto") {
        t.insert("review_mode".into(), json!(mode));
    }
    set_auto_accept(
        &mut t,
        a.last(&["--auto-accept", "--no-auto-accept"])
            .map(|f| f == "--auto-accept"),
    );
    if let Some(pane) = a.opt("--pane").filter(|p| !p.is_empty()) {
        t.insert("placement".into(), json!(pane)); // over the workers setting, from its next start
    }
    t.extend(extra);
    let briefs = repo.dir.join("briefs");
    std::fs::create_dir_all(&briefs)?;
    std::fs::write(briefs.join(format!("{tid}.md")), format!("{}\n", rstrip(&brief)))?;
    crate::saves::save(&repo, &mut t)?;
    Ok(tid)
}

/// `after`, `model` and `review-mode` change only a task that isn't closed.
fn not_closed(t: &Obj) -> Res<()> {
    if is_closed(s(t, "status")) {
        return fail(is_status(t));
    }
    Ok(())
}

/// Write the task as it is: not a save, as its status didn't change.
fn write_task(repo: &Repo, t: &Obj) -> Res<()> {
    json::write_obj(&repo.task_path(s(t, "id")), t)?;
    Ok(())
}

/// `after`: set what a task waits on, or clear it with --none.
pub fn cmd_after(argv: &[String]) -> Res<i32> {
    let a = Args::parse("after", argv, AFTER);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let deps = a.args("ID");
    let none = a.flag("--none");
    if none == !deps.is_empty() {
        return fail("give the tasks it waits on, or --none");
    }
    not_closed(&t)?;
    let tid = s(&t, "id").to_string();
    let after = if none {
        Vec::new()
    } else {
        check_after(&repo, &tid, dep_ids(&repo, &deps))?
    };
    if after.is_empty() {
        t.shift_remove("after");
    } else {
        t.insert("after".into(), json!(after));
    }
    write_task(&repo, &t)?;
    if after.is_empty() {
        println!("{tid} waits on nothing");
    } else {
        println!("{tid} waits on {}", after.join(", "));
    }
    Ok(0)
}

/// `model`: set, change or clear the worker model a task starts and resumes with.
pub fn cmd_model(argv: &[String]) -> Res<i32> {
    let a = Args::parse("model", argv, MODEL);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let model = a.arg("model").unwrap_or("");
    let clear = a.flag("--clear");
    if clear == !model.is_empty() {
        return fail("give the model (an alias like opus, or a model ID), or --clear");
    }
    not_closed(&t)?;
    if clear {
        t.shift_remove("worker_model");
    } else {
        t.insert("worker_model".into(), json!(model));
    }
    write_task(&repo, &t)?;
    let id = s(&t, "id");
    let cfg = repo.cfg.get("worker_model").filter(|v| truthy(Some(v)));
    let mut said = match (clear, cfg) {
        (true, Some(c)) => format!("{id} uses the config's worker model ({})", py_str(c)),
        (true, None) => format!("{id} uses the worker command's own model"),
        _ => format!("{id} uses {model}"),
    };
    if crate::rex::alive(s(&t, "session"), s(&t, "block"))? {
        let on = t
            .get("model")
            .filter(|v| truthy(Some(v)))
            .map_or("its model".into(), py_str);
        said += &format!("; its worker keeps running on {on}: this takes effect at its next start or resume");
    }
    println!("{said}");
    Ok(0)
}

/// `review-mode`: set how a task's first review goes: full, light, or auto (by the size
/// of the change).
pub fn cmd_review_mode(argv: &[String]) -> Res<i32> {
    let a = Args::parse("review-mode", argv, REVIEW_MODE);
    let mode = a.arg("mode").unwrap_or("");
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    not_closed(&t)?;
    if mode == "auto" {
        t.shift_remove("review_mode");
    } else {
        t.insert("review_mode".into(), json!(mode));
    }
    write_task(&repo, &t)?;
    let reviewed = truthy(obj(&t, "review").get("round"));
    let later = if reviewed {
        " (it has had a review already: later rounds follow up on it, whatever the mode)"
    } else {
        ""
    };
    println!("{}'s first review: {mode}{later}", s(&t, "id"));
    Ok(0)
}

#[cfg(test)]
#[path = "test_start.rs"]
mod tests;
