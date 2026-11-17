//! `ls` and `show`.

use crate::args::{flag, pos, Arg, Args, Nargs};
use crate::coordinators::{coordinators, owner};
use crate::rex;
use crate::task_line::{colored, task_line, task_rows, term_width};
use bullpen_core::fleet;
use bullpen_core::json::{dumps, get, obj, py_str, s, truthy, Obj};
use bullpen_core::repo::repo_root;
use bullpen_core::task::{keeps_branch, listed, own_model};
use bullpen_core::text::{len, lstrip, rjust, splitlines, strip, width, wrap};
use bullpen_core::words::{
    ago, board_detail, board_order, every, is_limited, is_stuck, pr_phrase, stuck_phrase, waits_phrase, Every,
};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::io::Write;

const LS: &[Arg] = &[
    flag("--all", "closed tasks too (done and dropped)"),
    flag(
        "--all-repos",
        "every repository bullpen knows, by repository (those with nothing open left out)",
    ),
    flag(
        "--json",
        "the task records as JSON, with their raw status (with --all-repos: a list of {repo, tasks})",
    ),
];

const SHOW: &[Arg] = &[
    pos("id", Nargs::One, "the task, like BP-3"),
    flag("--json", "its task record as JSON"),
];

/// Print lines to stdout. A closed pipe ends the program quietly (SIGPIPE), as Python's
/// print does; another failed write is a Fail.
pub fn print_all(lines: &[String]) -> Res<()> {
    let mut out = std::io::stdout().lock();
    for l in lines {
        writeln!(out, "{l}").map_err(|e| bullpen_core::Fail(e.to_string()))?;
    }
    Ok(())
}

fn tasks_json(tasks: Vec<Obj>) -> Value {
    Value::Array(tasks.into_iter().map(Value::Object).collect())
}

/// `ls`: the repo's open tasks (all with --all) in the board's order, as the board says them.
pub fn cmd_ls(argv: &[String]) -> Res<i32> {
    let a = Args::parse("ls", argv, LS);
    let all = a.flag("--all");
    if a.flag("--all-repos") {
        return ls_repos(all, a.flag("--json"));
    }
    let repo = Repo::new(repo_root()?)?;
    let every_task = repo.tasks()?; // read once: reading them is most of ls's own time
    let tasks = listed(every_task.clone(), all);
    if a.flag("--json") {
        print_all(&[dumps(&tasks_json(tasks))])?;
        return Ok(0);
    }
    let mut out = Vec::new();
    if tasks.is_empty() {
        out.push(format!(
            "no open tasks{}",
            if all { "" } else { " (--all shows finished ones)" }
        ));
    }
    out.extend(ls_lines(&repo, tasks, term_width(), colored(), &every(&every_task))?);
    print_all(&out)?;
    Ok(0)
}

/// Columns a cut line must leave for the board's sentence, or ls takes two rows.
const SENTENCE_ROOM: i64 = 24;

/// `ls`'s lines, in the board's order: one task_line each, whole when piped (cols None),
/// cut to the width on a terminal. When a line would leave under SENTENCE_ROOM columns
/// for some task's sentence (a narrow terminal, a long title), the board's two rows
/// each instead.
pub fn ls_lines(repo: &Repo, tasks: Vec<Obj>, cols: Option<i64>, color: bool, every: &Every) -> Res<Vec<String>> {
    let idw = tasks.iter().map(|t| len(s(t, "id"))).max().unwrap_or(0);
    let tasks = board_order(tasks);
    let Some(w) = cols else {
        return tasks
            .iter()
            .map(|t| task_line(repo, t, every, idw, None, color))
            .collect();
    };
    let mut fits = true;
    for t in &tasks {
        let when = ago(t, None);
        let age_room = if when.is_empty() { 0 } else { len(&when) as i64 + 2 };
        let room = w - 1 - (2 + idw as i64 + 2) - width(s(t, "title")) as i64 - age_room - 2;
        if room < SENTENCE_ROOM && width(&task_line(repo, t, every, idw, None, false)?) as i64 >= w {
            fits = false;
            break;
        }
    }
    if fits {
        return tasks
            .iter()
            .map(|t| task_line(repo, t, every, idw, Some(w - 1), color))
            .collect();
    }
    let mut out = Vec::new();
    for t in &tasks {
        out.extend(task_rows(repo, t, every, idw, w, color, None)?);
    }
    Ok(out)
}

/// `ls --all-repos`: the open tasks of every repository bullpen knows, by repository.
/// One with nothing open, or whose checkout is gone, is left out.
fn ls_repos(all: bool, as_json: bool) -> Res<i32> {
    let mut groups = Vec::new();
    for r in fleet::read()?.repos {
        if let Some(why) = r.broken.first() {
            return Err(bullpen_core::Fail(why.clone()));
        }
        let ts = listed(r.all, all);
        if !ts.is_empty() {
            groups.push((r.repo, ts, r.every));
        }
    }
    if as_json {
        let v: Vec<Value> = groups
            .into_iter()
            .map(|(r, ts, _)| json!({"repo": r.root_str(), "tasks": tasks_json(ts)}))
            .collect();
        print_all(&[dumps(&Value::Array(v))])?;
        return Ok(0);
    }
    let mut out = Vec::new();
    if groups.is_empty() {
        let more = if all { "" } else { " (--all shows finished ones)" };
        out.push(format!("no open tasks in any repository{more}"));
    }
    let cols = term_width();
    for (i, (r, ts, every)) in groups.into_iter().enumerate() {
        if i > 0 {
            out.push(String::new());
        }
        out.push(format!("{}  ({})", r.name(), r.root_str()));
        // Python's `width and width - 2`: a width of 0 is no width
        let w = cols.filter(|w| *w != 0).map(|w| w - 2);
        for line in ls_lines(&r, ts, w, colored(), &every)? {
            out.push(format!("  {line}"));
        }
    }
    print_all(&out)?;
    Ok(0)
}

/// "opus (its own, over the config's sonnet)", and what runs now if that's another.
fn model_line(repo: &Repo, t: &Obj) -> Res<String> {
    let own = own_model(&repo.cfg, t).unwrap_or("");
    let cfg = repo.cfg.get("worker_model").filter(|v| truthy(Some(v))).map(py_str);
    let mut line = format!(
        "{own} (its own, over the config's{})",
        cfg.map_or(String::new(), |c| format!(" {c}"))
    );
    let alive = rex::alive(s(t, "session"), s(t, "block"))?;
    if let Some(now_on) = t.get("model").filter(|v| alive && truthy(Some(v))) {
        let now_on = py_str(now_on);
        if now_on != own {
            line += &format!("; running {now_on} until its next start or resume");
        }
    }
    Ok(line)
}

/// review.review_kind: "full review", "light review (docs only)", "follow-up review".
fn review_kind(r: &Obj) -> String {
    if !truthy(r.get("kind")) {
        return "review".into();
    }
    let kind = py_str(&r["kind"]);
    let word = match kind.as_str() {
        "full" => "full review",
        "light" => "light review",
        "followup" => "follow-up review",
        "tour" => "review of the person's comments",
        "rebase" => "rebase check",
        "rebased" => "review after a rebase",
        other => other,
    };
    match get(r, "why").filter(|v| truthy(Some(v))) {
        Some(why) => format!("{word} ({})", py_str(why)),
        None => word.into(),
    }
}

/// `show`: one task, as the board says it, then what's known about it and what you can do now.
pub fn cmd_show(argv: &[String]) -> Res<i32> {
    let a = Args::parse("show", argv, SHOW);
    let repo = Repo::new(repo_root()?)?;
    let t = repo.task(a.arg("id").unwrap_or(""))?;
    if a.flag("--json") {
        print_all(&[dumps(&Value::Object(t))])?;
        return Ok(0);
    }
    let all = repo.tasks()?;
    let every = every(&all);
    let detail = board_detail(&repo.cfg, &t, &every, None)?;
    let cols = term_width();
    let mut out = Vec::new();
    let line = task_line(&repo, &t, &every, 0, None, false)?;
    match cols {
        Some(w) if width(&line) as i64 >= w => {
            out.push(task_rows(&repo, &t, &every, 0, w, colored(), Some(99))?.join("\n"));
        }
        _ => out.push(task_line(&repo, &t, &every, 0, None, colored())?),
    }
    for k in [
        "why", "base", "branch", "worktree", "block", "model", "pr", "diffstat", "summary",
    ] {
        if k == "model" && own_model(&repo.cfg, &t).is_some() {
            out.push(format!("{}: {}", rjust(k, 9), model_line(&repo, &t)?));
        } else if k == "why" && truthy(t.get(k)) && detail.contains(&py_str(&t[k])) {
            continue; // the first line says it
        } else if truthy(t.get(k)) {
            let body = py_str(&t[k]);
            let lines: Vec<String> = splitlines(strip(&body))
                .iter()
                .map(|l| {
                    if l.is_empty() {
                        String::new()
                    } else {
                        format!("{}{l}", " ".repeat(11))
                    }
                })
                .collect();
            out.push(format!("{}: {}", rjust(k, 9), lstrip(&lines.join("\n"))));
        } else if k == "pr" && truthy(t.get("local")) {
            let base = t.get("base").map_or("None".into(), py_str);
            out.push(format!(
                "{}: none; local branch, merged into {base} by accept",
                rjust(k, 9)
            ));
        }
    }
    if truthy(t.get("after")) {
        let after: Vec<String> = t["after"].as_array().into_iter().flatten().map(py_str).collect();
        out.push(format!("    after: {}", after.join(", ")));
    }
    out.extend(crate::backlog::from_line(&t));
    out.extend(crate::revert::show_lines(&t, &all));
    let own = owner(&repo, &t);
    if coordinators(&repo).len() > 1 || (truthy(t.get("coordinator")) && own.is_none()) {
        out.push(match own {
            Some(c) => {
                let field = |k| c.get(k).map_or("None".into(), py_str);
                format!("    coord: {} ({})", field("id"), field("block"))
            }
            None => {
                let who = t
                    .get("coordinator")
                    .filter(|v| truthy(Some(v)))
                    .map_or("none".into(), py_str);
                format!("    coord: {who}, gone: every running coordinator sees to it")
            }
        });
    }
    if let Some(on) = get(&t, "auto_accept") {
        let says = if truthy(Some(on)) {
            "accepts by itself once approved"
        } else {
            "kept for you (--no-auto-accept)"
        };
        out.push(format!("     auto: {says}"));
    }
    let waits = waits_phrase(&t, &every);
    if !waits.is_empty() {
        out.push(format!("    waits: {waits}"));
    }
    let gh = pr_phrase(&t);
    if !gh.is_empty() && !detail.to_lowercase().contains(&gh.to_lowercase()) {
        out.push(format!("   github: {gh}"));
    }
    if is_stuck(&t) {
        out.push(format!(
            "    quiet: its {} has shown {}",
            s(obj(&t, "stuck"), "who"),
            stuck_phrase(&t, None)
        ));
    }
    if truthy(t.get("placement")) {
        out.push(format!(
            "     pane: {} (its own, over config --workers)",
            py_str(&t["placement"])
        ));
    }
    if truthy(t.get("review_mode")) {
        out.push(format!(
            "     mode: {} first review (its own)",
            py_str(&t["review_mode"])
        ));
    }
    for (other, files) in crate::overlap::overlaps_of(&repo, s(&t, "id"), &all) {
        out.push(format!(
            "  overlap: {other} changed {} too",
            crate::git_names::listed(&files)
        ));
    }
    if let Some(files) = crate::accept::rebase::stale_conflicts(&repo, &t)? {
        out.push(crate::accept::rebase::stale_line(&t, &files));
    }
    if let Some(line) = review_line(&t) {
        out.push(line);
    }
    if let Some(notes) = crate::notes::review_notes(&t) {
        out.push(notes_lines(&notes));
    }
    if let Some(line) = risk_line(&t) {
        out.push(line);
    }
    for name in ["setup", "teardown"] {
        let log = repo.dir.join("briefs").join(format!("{}.{name}.log", s(&t, "id")));
        if log.exists() {
            let failed = if name == "setup" && truthy(t.get("needs_setup")) {
                " (failed: the worker didn't start)"
            } else {
                ""
            };
            out.push(format!("{}: {}{failed}", rjust(name, 9), log.display()));
        }
    }
    let changed = match crate::brief::count(&t) {
        0 => String::new(),
        1 => " (changed once since it was added)".into(),
        n => format!(" (changed {n} times since it was added)"),
    };
    out.push(format!(
        "    brief: {}{changed}",
        repo.dir.join("briefs").join(format!("{}.md", s(&t, "id"))).display()
    ));
    let nxt = next_steps(&t, &every);
    let lines = match cols {
        Some(w) if w != 0 => wrap(&nxt, w - 12, 99),
        _ => vec![nxt],
    };
    out.push(format!("     next: {}", lines.join(&format!("\n{}", " ".repeat(11)))));
    print_all(&out)?;
    Ok(0)
}

/// "   review: round 2 (full review on opus), approved, kept 2 times as ...", for a task with a review.
fn review_line(t: &Obj) -> Option<String> {
    let r = obj(t, "review");
    if r.is_empty() {
        return None;
    }
    let mut state = get(r, "verdict")
        .filter(|v| truthy(Some(v)))
        .map_or("in progress".into(), py_str);
    if truthy(r.get("void")) {
        state = "stopped: the brief changed".into();
    }
    if s(r, "verdict") == "approved" && truthy(r.get("kept")) {
        let kept = &r["kept"];
        let one = kept.as_f64() == Some(1.0) || kept == &Value::Bool(true);
        state += &format!(
            ", kept {} time{} as the code didn't change",
            py_str(kept),
            if one { "" } else { "s" }
        );
    }
    let model = get(r, "model")
        .filter(|v| truthy(Some(v)))
        .map_or(String::new(), |m| format!(" on {}", py_str(m)));
    let findings = get(r, "findings")
        .filter(|v| truthy(Some(v)))
        .map_or(String::new(), |f| format!("; findings: {}", py_str(f)));
    let round = r.get("round").map_or("None".into(), py_str);
    Some(format!(
        "   review: round {round} ({}{model}), {state}{findings}",
        review_kind(r)
    ))
}

/// "     risk: high · changes how accept merges (rules: high · ...; review: low · ...)",
/// for a task with a risk label: the label, then the parts it came from.
fn risk_line(t: &Obj) -> Option<String> {
    let (level, why) = bullpen_core::risk::stored(t)?;
    let r = obj(t, "risk");
    let part = |k: &str| {
        let p = obj(r, k);
        let label = bullpen_core::risk::label(s(p, "level"), s(p, "why"));
        (!s(p, "level").is_empty()).then(|| label.replacen("risk: ", "", 1))
    };
    let parts: Vec<String> = [("rules", "rules"), ("review", "review"), ("floor", "light review")]
        .into_iter()
        .filter_map(|(k, name)| part(k).map(|p| format!("{name}: {p}")))
        .collect();
    let label = bullpen_core::risk::label(level, why).replacen("risk: ", "", 1);
    Some(format!("     risk: {label} ({})", parts.join("; ")))
}

/// "    notes: - ..." with the reviewer's notes under it, indented to the column.
fn notes_lines(notes: &str) -> String {
    let lines: Vec<String> = notes
        .lines()
        .enumerate()
        .map(|(i, l)| match (i, l.trim().is_empty()) {
            (0, _) => format!("    notes: {l}"),
            (_, true) => String::new(),
            _ => format!("{}{l}", " ".repeat(11)),
        })
        .collect();
    lines.join("\n")
}

/// What you can do with the task now: the commands, with the board's keys.
pub fn next_steps(t: &Obj, every: &Every) -> String {
    let tid = s(t, "id");
    let limited_unknown = is_limited(t) && !truthy(obj(t, "limit").get("resets"));
    match s(t, "status") {
        "needs" if truthy(t.get("ask")) => format!("`bullpen reply {tid} <answer>`, or i on the board"),
        "needs" if s(obj(t, "stop"), "kind") == "prompt" => {
            format!("`bullpen answer {tid} <option>` (`bullpen peek {tid}` shows the prompt), or i on the board")
        }
        "needs" => format!("`bullpen go {tid}` opens it (⏎ on the board), or `bullpen tell {tid} <message>` (t)"),
        "stopped" => format!("`bullpen resume {tid}` (r on the board), or `bullpen drop {tid}` (d)"),
        "triage" => format!("nothing: its coordinator is on it. `bullpen go {tid}` opens it (⏎ on the board)"),
        "ready" => format!("`bullpen accept {tid}` (a on the board); `bullpen diff {tid}` shows the change (v)"),
        "finished" if keeps_branch(t) => format!(
            "`bullpen close {tid} --keep-branch` keeps its branch (`--as NAME` names it; a on the board), \
             or `bullpen drop {tid}` (d)"
        ),
        "finished" => format!("`bullpen close {tid}` (a on the board), or `bullpen drop {tid}` (d)"),
        "review" => {
            format!("nothing until its review is done; `bullpen go {tid} --review` opens the reviewer (⏎ on the board)")
        }
        "working" if is_stuck(t) || limited_unknown => {
            format!("`bullpen peek {tid}` shows its screen (p on the board), `bullpen tell {tid} <message>` (t)")
        }
        "working" => format!("nothing until it's ready; `bullpen tell {tid} <message>` steers it (t on the board)"),
        "todo" if !waits_phrase(t, every).is_empty() => format!(
            "nothing: it starts by itself once what it waits on is accepted; `bullpen start {tid} --force` starts it now"
        ),
        "todo" => format!("`bullpen start {tid}` (s on the board), or `bullpen drop {tid}` (d)"),
        _ => "nothing: it's closed".into(),
    }
}

#[cfg(test)]
#[path = "test_show.rs"]
mod tests;
