//! The commands that only write a task's files, from bullpen/start.py: `add`, `after`,
//! `model` and `review-mode`. None of them types to a pane, notifies or moves the queue.

use crate::args::Args;
use crate::core::{fail, home, is_closed, is_status, repo_root, Fail, Repo, Res};
use crate::json::{self, py_str, s, truthy, Obj};
use crate::task_line::{every, Every};
use crate::text::{rstrip, split};
use crate::{coordinators, rex};
use serde_json::{json, Value};
use std::collections::HashSet;

/// core.REVIEW_MODES: argparse's choices for --review and review-mode's mode.
const REVIEW_MODES: [&str; 3] = ["full", "light", "auto"];

/// An argparse choices= error is Python's to say.
fn check_mode(mode: Option<&str>) -> Res<()> {
    match mode {
        Some(m) if !REVIEW_MODES.contains(&m) => Err(Fail::Python),
        _ => Ok(()),
    }
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

/// [start, ..., goal] if start waits on goal, directly or through others, else None.
fn dep_path(tasks: &Every, start: &str, goal: &str, seen: &mut HashSet<String>) -> Option<Vec<String>> {
    if start == goal {
        return Some(vec![goal.into()]);
    }
    if !seen.insert(start.into()) {
        return None;
    }
    let after = tasks.get(start).and_then(|t| t.get("after")).and_then(Value::as_array);
    for d in after.into_iter().flatten().filter_map(Value::as_str) {
        if let Some(rest) = dep_path(tasks, d, goal, seen) {
            return Some([vec![start.to_string()], rest].concat());
        }
    }
    None
}

/// after, if the task tid can wait on those tasks: each exists, isn't tid itself or
/// dropped, and none of them waits on tid already (a cycle).
fn check_after(repo: &Repo, tid: &str, after: Vec<String>) -> Res<Vec<String>> {
    let tasks = every(repo)?;
    for d in &after {
        if d == tid {
            return fail(format!("{tid} can't wait on itself"));
        }
        let Some(t) = tasks.get(d) else {
            return fail(format!("no task {d} in {}: `bullpen ls --all` lists them", home(&repo.root)));
        };
        if s(t, "status") == "dropped" {
            return fail(format!("{d} was dropped: {tid} would wait on it forever"));
        }
        if let Some(path) = dep_path(&tasks, d, tid, &mut HashSet::new()) {
            return fail(format!("{d} already waits on {tid} ({}): that would be a cycle", path.join(" → ")));
        }
    }
    Ok(after)
}

/// Path.read_text(): the file as UTF-8 with universal newlines. One it can't read goes
/// to Python, whose traceback says why; so does anything but a plain file (a pipe,
/// /dev/stdin), which Python couldn't read again once the port had.
fn read_text(path: &str) -> Res<String> {
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return Err(Fail::Python);
    }
    let bytes = std::fs::read(path).map_err(|_| Fail::Python)?;
    let text = String::from_utf8(bytes).map_err(|_| Fail::Python)?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// The id of the coordinator who hears the task's news: the one whose pane runs this,
/// else the first. One without an id is Python's to fail on.
fn news_coordinator(repo: &Repo) -> Res<Option<Value>> {
    match coordinators::calling_coordinator(repo).or_else(|| coordinators::primary(repo)) {
        Some(c) if !c.is_empty() => c.get("id").cloned().map(Some).ok_or(Fail::Python),
        _ => Ok(None),
    }
}

pub fn cmd_add(a: &Args) -> Res<()> {
    check_mode(a.opt("--review"))?;
    let mut repo = Repo::new(&repo_root()?)?;
    let brief = match a.opt("--brief-file").filter(|f| !f.is_empty()) {
        Some(f) => read_text(f)?,
        None => a.opt("--brief").unwrap_or("").to_string(),
    };
    if brief.is_empty() {
        return fail("give the task's brief with --brief or --brief-file");
    }
    let next = repo.cfg.get("next").and_then(Value::as_i64).ok_or(Fail::Python)?;
    let tid = format!("{}-{next}", py_str(repo.cfg.get("prefix").unwrap_or(&Value::Null)));
    let after = check_after(&repo, &tid, dep_ids(&repo, &a.all("--after")))?;
    let coordinator = news_coordinator(&repo)?;
    repo.cfg.insert("next".into(), json!(next + 1));
    repo.save()?;
    let base = match a.opt("--base").filter(|b| !b.is_empty()) {
        Some(b) => b.to_string(),
        None => repo.base()?,
    };
    let mut t: Obj = Obj::new();
    t.insert("id".into(), json!(tid));
    t.insert("title".into(), json!(a.arg(0).unwrap_or("")));
    t.insert("status".into(), json!("todo"));
    t.insert("created".into(), json!(crate::timeutil::iso_now()));
    t.insert("base".into(), json!(base));
    for k in ["branch", "worktree", "block", "session", "pr", "why", "last"] {
        t.insert(k.into(), Value::Null);
    }
    if let Some(c) = coordinator {
        t.insert("coordinator".into(), c);
    }
    if !after.is_empty() {
        t.insert("after".into(), json!(after));
    }
    if let Some(m) = a.opt("--model").filter(|m| !m.is_empty()) {
        t.insert("worker_model".into(), json!(m)); // its own, over the config's, from its first start
    }
    if let Some(mode) = a.opt("--review").filter(|m| *m != "auto") {
        t.insert("review_mode".into(), json!(mode));
    }
    if let Some(on) = a.last(&["--auto-accept", "--no-auto-accept"]) {
        t.insert("auto_accept".into(), json!(on == "--auto-accept"));
    }
    let briefs = repo.dir.join("briefs");
    std::fs::create_dir_all(&briefs)?;
    std::fs::write(briefs.join(format!("{tid}.md")), format!("{}\n", rstrip(&brief)))?;
    repo.save_task(&mut t)?;
    println!("{tid}");
    Ok(())
}

/// `after`, `model` and `review-mode` change only a task that isn't closed.
fn not_closed(t: &Obj) -> Res<()> {
    if is_closed(s(t, "status")) {
        return fail(is_status(t));
    }
    Ok(())
}

/// Write the task as it is: not save_task, as its status didn't change.
fn write_task(repo: &Repo, t: &Obj) -> Res<()> {
    json::write(&repo.task_path(s(t, "id")), &Value::Object(t.clone()))?;
    Ok(())
}

/// Set what a task waits on, or clear it with --none.
pub fn cmd_after(a: &Args) -> Res<()> {
    let repo = Repo::new(&repo_root()?)?;
    let mut t = repo.task(a.arg(0).unwrap_or(""))?;
    let deps: Vec<&str> = a.pos[1..].iter().map(String::as_str).collect();
    if a.flag("--none") == !deps.is_empty() {
        return fail("give the tasks it waits on, or --none");
    }
    not_closed(&t)?;
    let tid = s(&t, "id").to_string();
    let after = if a.flag("--none") { Vec::new() } else { check_after(&repo, &tid, dep_ids(&repo, &deps))? };
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
    Ok(())
}

/// Set, change or clear the worker model a task starts and resumes with.
pub fn cmd_model(a: &Args) -> Res<()> {
    let repo = Repo::new(&repo_root()?)?;
    let mut t = repo.task(a.arg(0).unwrap_or(""))?;
    let model = a.arg(1).unwrap_or("");
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
    if rex::alive(s(&t, "session"), s(&t, "block"))? {
        let on = t.get("model").filter(|v| truthy(Some(v))).map_or("its model".into(), py_str);
        said += &format!("; its worker keeps running on {on}: this takes effect at its next start or resume");
    }
    println!("{said}");
    Ok(())
}

/// Set how a task's first review goes: full, light, or auto (by the size of the change).
pub fn cmd_review_mode(a: &Args) -> Res<()> {
    let mode = a.arg(1).unwrap_or("");
    check_mode(Some(mode))?;
    let repo = Repo::new(&repo_root()?)?;
    let mut t = repo.task(a.arg(0).unwrap_or(""))?;
    not_closed(&t)?;
    if mode == "auto" {
        t.shift_remove("review_mode");
    } else {
        t.insert("review_mode".into(), json!(mode));
    }
    write_task(&repo, &t)?;
    let reviewed = truthy(json::obj(&t, "review").get("round"));
    let later = if reviewed { " (it has had a review already: later rounds follow up on it, whatever the mode)" } else { "" };
    println!("{}'s first review: {mode}{later}", s(&t, "id"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> Repo {
        let mut cfg = crate::core::defaults();
        cfg.insert("prefix".into(), json!("BP"));
        Repo { root: "/r".into(), dir: "/nowhere".into(), cfg, own: vec![] }
    }

    #[test]
    fn dep_ids_split_and_dedupe() {
        let ids = dep_ids(&repo(), &["8, bp-9", " 8 BP-10,,", ""]);
        assert_eq!(ids, vec!["BP-8", "BP-9", "BP-10"]);
    }

    #[test]
    fn dep_path_finds_a_cycle() {
        let mut tasks = Every::new();
        for (id, after) in [("BP-1", json!(["BP-2"])), ("BP-2", json!(["BP-3", "BP-1"])), ("BP-3", json!([]))] {
            let t = json!({"id": id, "after": after});
            tasks.insert(id.into(), t.as_object().unwrap().clone());
        }
        let path = dep_path(&tasks, "BP-1", "BP-3", &mut HashSet::new());
        assert_eq!(path, Some(vec!["BP-1".into(), "BP-2".into(), "BP-3".into()]));
        assert_eq!(dep_path(&tasks, "BP-3", "BP-1", &mut HashSet::new()), None);
    }
}
