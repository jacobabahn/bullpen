//! `bullpen later`'s lines: the list in order (`◇` kept, `?` asks for a yes), then the
//! items that became tasks, dim, while their task is open; and one item in full.

use crate::task_line::colored;
use bullpen_core::backlog::{self, is_open};
use bullpen_core::json::{dumps, py_str, s, truthy, Obj};
use bullpen_core::task::{is_closed, status_words};
use bullpen_core::{Repo, Res};
use serde_json::Value;
use std::collections::HashMap;

/// Print the list (every item with all), or its items as JSON.
pub fn print(repo: &Repo, all: bool, as_json: bool) -> Res<i32> {
    let b = backlog::load(repo)?;
    let statuses: HashMap<String, String> = repo
        .tasks()?
        .into_iter()
        .map(|t| (s(&t, "id").to_string(), s(&t, "status").to_string()))
        .collect();
    let shown: Vec<&Obj> = b.items.iter().filter(|i| all || listed(i, &statuses)).collect();
    if as_json {
        let items = shown.into_iter().cloned().map(Value::Object).collect();
        println!("{}", dumps(&Value::Array(items)));
        return Ok(0);
    }
    let out = lines(&shown, &statuses, colored());
    if out.is_empty() {
        println!("the backlog is empty: `bullpen later \"...\"` adds an idea to it");
    } else {
        println!("{}", out.join("\n"));
    }
    if let Some(line) = super::pickup::status_line(repo)? {
        println!("{line}");
    }
    Ok(0)
}

/// Whether the list shows the item: open, or started with its task not closed yet.
fn listed(item: &Obj, statuses: &HashMap<String, String>) -> bool {
    is_open(item) || (s(item, "state") == "started" && !is_closed(task_status(item, statuses)))
}

fn task_status<'a>(item: &Obj, statuses: &'a HashMap<String, String>) -> &'a str {
    statuses.get(s(item, "task")).map_or("", String::as_str)
}

/// The lines: the open items in order, then the rest (started, dropped), dim.
pub fn lines(items: &[&Obj], statuses: &HashMap<String, String>, color: bool) -> Vec<String> {
    let idw = items.iter().map(|i| s(i, "id").len()).max().unwrap_or(0);
    let (open, rest): (Vec<&&Obj>, Vec<&&Obj>) = items.iter().partition(|i| is_open(i));
    let mut out: Vec<String> = open.iter().map(|i| line(i, idw, statuses)).collect();
    for i in rest {
        let l = line(i, idw, statuses);
        out.push(if color { format!("\x1b[2m{l}\x1b[0m") } else { l });
    }
    out
}

/// One item's line: its mark, ID and title, and what's to know about it.
fn line(item: &Obj, idw: usize, statuses: &HashMap<String, String>) -> String {
    let id = s(item, "id");
    let mark = match s(item, "state") {
        "kept" => "◇",
        "asks" => "?",
        "started" => "↳",
        _ => "·",
    };
    let mut tail = Vec::new();
    match s(item, "state") {
        "asks" => tail.push(format!("asks: from {}", s(item, "by"))),
        "started" => {
            let status = task_status(item, statuses);
            let said = if status.is_empty() {
                "gone".into()
            } else {
                status_words(status)
            };
            tail.push(format!("{}, {said}", s(item, "task")));
        }
        "kept" | "" => {}
        state => tail.push(state.to_string()),
    }
    if is_open(item) && truthy(item.get("brief")) {
        tail.push("has a brief".into());
    }
    if is_open(item) && truthy(item.get("skipped")) {
        tail.push(format!("skipped: {}", s(item, "skipped")));
    }
    let tail = if tail.is_empty() {
        String::new()
    } else {
        format!(" · {}", tail.join(" · "))
    };
    format!("{mark} {id:<idw$}  {}{tail}", s(item, "title"))
}

/// One item in full, for `later L7`.
pub fn detail(repo: &Repo, item: &Obj) -> Res<String> {
    let id = s(item, "id");
    let by = match s(item, "by") {
        "person" => "you".to_string(),
        b => b.to_string(),
    };
    let mut out = vec![
        format!("{id}  {}", s(item, "title")),
        format!("    state: {}, added by {by} at {}", s(item, "state"), s(item, "added")),
    ];
    if truthy(item.get("brief")) {
        let path = repo.dir.join("briefs").join(s(item, "brief"));
        out.push(format!("    brief: {}", path.display()));
    }
    for (k, label) in [("note", "     note"), ("skipped", "  skipped"), ("task", "     task")] {
        if truthy(item.get(k)) {
            out.push(format!("{label}: {}", py_str(&item[k])));
        }
    }
    out.push(format!("     next: {}", next_steps(item)));
    Ok(out.join("\n"))
}

/// What can be done with the item now.
fn next_steps(item: &Obj) -> String {
    let id = s(item, "id");
    match s(item, "state") {
        "asks" => format!("`bullpen later {id} --keep` puts it on the list, `--drop` says no"),
        "kept" if truthy(item.get("brief")) => format!("`bullpen later {id} --start` makes it a task and starts it"),
        "kept" => format!("`bullpen later {id} --brief-file FILE` gives it a brief; then `--start`"),
        "started" => format!("see to its task, {}", s(item, "task")),
        _ => format!("`bullpen later {id} --keep` puts it back on the list"),
    }
}
