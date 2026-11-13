//! `ls` and `show` (bullpen/show.py).

use crate::args::Args;
use crate::core::{self, home, known_repos, listed, own_model, repo_root, Repo, Res};
use crate::json::{self, dumps, get, obj, py_str, s, truthy, Obj};
use crate::task_line::{
    ago, board_detail, board_order, colored, every, is_stuck, pr_phrase, stuck_phrase, task_line, task_rows,
    term_width, waits_phrase, Every,
};
use crate::text::{self, len, rjust, width, wrap};
use crate::{coordinators, rex};
use serde_json::{json, Value};

fn print_all(lines: &[String]) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    for l in lines {
        let _ = writeln!(out, "{l}");
    }
}

pub fn cmd_ls(a: &Args) -> Res<()> {
    if a.flag("--all-repos") {
        return ls_repos(a);
    }
    let repo = Repo::new(&repo_root()?)?;
    let tasks = listed(&repo, a.flag("--all"))?;
    if a.flag("--json") {
        println!("{}", dumps(&Value::Array(tasks.into_iter().map(Value::Object).collect())));
        return Ok(());
    }
    let mut out = Vec::new();
    if tasks.is_empty() {
        out.push(format!("no open tasks{}", if a.flag("--all") { "" } else { " (--all shows finished ones)" }));
    }
    out.extend(ls_lines(&repo, tasks, term_width(), colored())?);
    print_all(&out);
    Ok(())
}

/// Columns a cut line must leave for the board's sentence, or ls takes two rows.
const SENTENCE_ROOM: i64 = 24;

/// `ls`'s lines, in the board's order: whole when piped (cols None), cut to the width
/// on a terminal, or the board's two rows each when some line would leave too little
/// room for its sentence.
fn ls_lines(repo: &Repo, tasks: Vec<Obj>, cols: Option<i64>, color: bool) -> Res<Vec<String>> {
    let every = every(repo)?;
    let idw = tasks.iter().map(|t| len(s(t, "id"))).max().unwrap_or(0);
    let tasks = board_order(tasks);
    let Some(w) = cols else {
        return tasks.iter().map(|t| task_line(repo, t, &every, idw, None, color)).collect();
    };
    let mut fits = true;
    for t in &tasks {
        let when = ago(t, None);
        let room = w - 1 - (2 + idw as i64 + 2) - width(s(t, "title")) as i64 - if when.is_empty() { 0 } else { len(&when) as i64 + 2 } - 2;
        if room < SENTENCE_ROOM && width(&task_line(repo, t, &every, idw, None, false)?) as i64 >= w {
            fits = false;
            break;
        }
    }
    if fits {
        return tasks.iter().map(|t| task_line(repo, t, &every, idw, Some(w - 1), color)).collect();
    }
    let mut out = Vec::new();
    for t in &tasks {
        out.extend(task_rows(repo, t, &every, idw, w, color, None)?);
    }
    Ok(out)
}

/// `ls --all-repos`: the open tasks of every repository bullpen knows, by repository.
fn ls_repos(a: &Args) -> Res<()> {
    let mut groups = Vec::new();
    for r in known_repos()? {
        let ts = listed(&r, a.flag("--all"))?;
        if !ts.is_empty() {
            groups.push((r, ts));
        }
    }
    if a.flag("--json") {
        let v: Vec<Value> = groups
            .into_iter()
            .map(|(r, ts)| json!({"repo": r.root, "tasks": ts.into_iter().map(Value::Object).collect::<Vec<_>>()}))
            .collect();
        println!("{}", dumps(&Value::Array(v)));
        return Ok(());
    }
    let mut out = Vec::new();
    if groups.is_empty() {
        out.push(format!("no open tasks in any repository{}", if a.flag("--all") { "" } else { " (--all shows finished ones)" }));
    }
    let cols = term_width();
    for (i, (r, ts)) in groups.into_iter().enumerate() {
        if i > 0 {
            out.push(String::new());
        }
        out.push(format!("{}  ({})", r.name(), r.root));
        for line in ls_lines(&r, ts, cols.map(|w| w - 2), colored())? {
            out.push(format!("  {line}"));
        }
    }
    print_all(&out);
    Ok(())
}

/// "opus (its own, over the config's sonnet)", and what runs now if that's another.
fn model_line(repo: &Repo, t: &Obj) -> Res<String> {
    let own = own_model(repo, t).unwrap_or("");
    let cfg = get(&repo.cfg, "worker_model").filter(|v| truthy(Some(v))).map(py_str);
    let mut line = format!("{own} (its own, over the config's{})", cfg.map_or(String::new(), |c| format!(" {c}")));
    if truthy(t.get("model")) && rex::alive(s(t, "session"), s(t, "block"))? {
        let now_on = py_str(&t["model"]);
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
        other => other,
    };
    match get(r, "why").filter(|v| truthy(Some(v))) {
        Some(why) => format!("{word} ({})", py_str(why)),
        None => word.into(),
    }
}

pub fn cmd_show(a: &Args) -> Res<()> {
    let repo = Repo::new(&repo_root()?)?;
    let t = repo.task(a.arg(0).unwrap_or(""))?;
    if a.flag("--json") {
        println!("{}", dumps(&Value::Object(t)));
        return Ok(());
    }
    let every = every(&repo)?;
    let detail = board_detail(&repo, &t, &every, None)?;
    let cols = term_width();
    let mut out = Vec::new();
    let line = task_line(&repo, &t, &every, 0, None, false)?;
    if cols.is_none_or(|w| (width(&line) as i64) < w) {
        out.push(task_line(&repo, &t, &every, 0, None, colored())?);
    } else {
        out.push(task_rows(&repo, &t, &every, 0, cols.unwrap(), colored(), Some(99))?.join("\n"));
    }
    for k in ["why", "base", "branch", "worktree", "block", "model", "pr", "diffstat", "summary"] {
        if k == "model" && own_model(&repo, &t).is_some() {
            out.push(format!("{}: {}", rjust(k, 9), model_line(&repo, &t)?));
        } else if k == "why" && truthy(t.get(k)) && detail.contains(&py_str(&t[k])) {
            continue; // the first line says it
        } else if truthy(t.get(k)) {
            let body = py_str(&t[k]);
            let lines: Vec<String> = text::splitlines(text::strip(&body))
                .iter()
                .map(|l| if l.is_empty() { String::new() } else { format!("{}{l}", " ".repeat(11)) })
                .collect();
            out.push(format!("{}: {}", rjust(k, 9), text::lstrip(&lines.join("\n"))));
        } else if k == "pr" && truthy(t.get("local")) {
            let base = t.get("base").map_or("None".into(), py_str);
            out.push(format!("{}: none; local branch, merged into {base} by accept", rjust(k, 9)));
        }
    }
    if truthy(t.get("after")) {
        let after: Vec<String> = t["after"].as_array().into_iter().flatten().map(py_str).collect();
        out.push(format!("    after: {}", after.join(", ")));
    }
    let owner = coordinators::owner(&repo, &t);
    if repo.coordinators().len() > 1 || (truthy(t.get("coordinator")) && owner.is_none()) {
        out.push(match owner {
            Some(c) => format!("    coord: {} ({})", py_str(c.get("id").unwrap_or(&Value::Null)), py_str(c.get("block").unwrap_or(&Value::Null))),
            None => {
                let who = get(&t, "coordinator").filter(|v| truthy(Some(v))).map_or("none".into(), py_str);
                format!("    coord: {who}, gone: every running coordinator sees to it")
            }
        });
    }
    if let Some(on) = get(&t, "auto_accept") {
        let says = if truthy(Some(on)) { "accepts by itself once approved" } else { "kept for you (--no-auto-accept)" };
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
        out.push(format!("    quiet: its {} has shown {}", s(obj(&t, "stuck"), "who"), stuck_phrase(&t, None)));
    }
    if truthy(t.get("review_mode")) {
        out.push(format!("     mode: {} first review (its own)", py_str(&t["review_mode"])));
    }
    review_line(&t, &mut out);
    for name in ["setup", "teardown"] {
        let log = repo.dir.join("briefs").join(format!("{}.{name}.log", s(&t, "id")));
        if log.exists() {
            let failed = if name == "setup" && truthy(t.get("needs_setup")) { " (failed: the worker didn't start)" } else { "" };
            out.push(format!("{}: {}{failed}", rjust(name, 9), log.display()));
        }
    }
    out.push(format!("    brief: {}", repo.dir.join("briefs").join(format!("{}.md", s(&t, "id"))).display()));
    let nxt = next_steps(&t, &every);
    let lines = match cols {
        Some(w) => wrap(&nxt, w - 12, 99),
        None => vec![nxt],
    };
    out.push(format!("     next: {}", lines.join(&format!("\n{}", " ".repeat(11)))));
    print_all(&out);
    Ok(())
}

/// "   review: round 2 (full review on opus), approved, kept 2 times as ...".
fn review_line(t: &Obj, out: &mut Vec<String>) {
    let r = obj(t, "review");
    if r.is_empty() {
        return;
    }
    let verdict = get(r, "verdict").filter(|v| truthy(Some(v)));
    let mut state = verdict.map_or("in progress".into(), py_str);
    if s(r, "verdict") == "approved" && truthy(r.get("kept")) {
        let kept = &r["kept"];
        let one = kept.as_f64() == Some(1.0);
        state += &format!(", kept {} time{} as the code didn't change", py_str(kept), if one { "" } else { "s" });
    }
    let model = get(r, "model").filter(|v| truthy(Some(v))).map_or(String::new(), |m| format!(" on {}", py_str(m)));
    let findings = get(r, "findings").filter(|v| truthy(Some(v))).map_or(String::new(), |f| format!("; findings: {}", py_str(f)));
    let round = r.get("round").map_or("None".into(), py_str);
    out.push(format!("   review: round {round} ({}{model}), {state}{findings}", review_kind(r)));
}

/// What you can do with the task now: the commands, with the board's keys.
fn next_steps(t: &Obj, every: &Every) -> String {
    let tid = s(t, "id");
    let st = s(t, "status");
    let limited_unknown = crate::task_line::is_limited(t) && !truthy(obj(t, "limit").get("resets"));
    match st {
        "needs" if truthy(t.get("ask")) => format!("`bullpen reply {tid} <answer>`, or i on the board"),
        "needs" if s(obj(t, "stop"), "kind") == "prompt" => {
            format!("`bullpen answer {tid} <option>` (`bullpen peek {tid}` shows the prompt), or i on the board")
        }
        "needs" => format!("`bullpen go {tid}` opens it (⏎ on the board), or `bullpen tell {tid} <message>` (t)"),
        "stopped" => format!("`bullpen resume {tid}` (r on the board), or `bullpen drop {tid}` (d)"),
        "triage" => format!("nothing: its coordinator is on it. `bullpen go {tid}` opens it (⏎ on the board)"),
        "ready" => format!("`bullpen accept {tid}` (a on the board); `bullpen diff {tid}` shows the change (v)"),
        "finished" => format!("`bullpen close {tid}` (a on the board), or `bullpen drop {tid}` (d)"),
        "review" => format!("nothing until its review is done; `bullpen go {tid} --review` opens the reviewer (⏎ on the board)"),
        "working" if is_stuck(t) || limited_unknown => {
            format!("`bullpen peek {tid}` shows its screen (p on the board), `bullpen tell {tid} <message>` (t)")
        }
        "working" => format!("nothing until it's ready; `bullpen tell {tid} <message>` steers it (t on the board)"),
        "todo" if !waits_phrase(t, every).is_empty() => {
            format!("nothing: it starts by itself once what it waits on is accepted; `bullpen start {tid} --force` starts it now")
        }
        "todo" => format!("`bullpen start {tid}` (s on the board), or `bullpen drop {tid}` (d)"),
        _ => "nothing: it's closed".into(),
    }
}
