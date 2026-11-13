//! One task line, as the board says it, for `ls` and `show` (bullpen/task_line.py,
//! with what it needs of agents.py and start.py).

use crate::core::{is_closed, own_model, pr_short, stamp, state_dir, Fail, Repo, Res, safe};
use crate::json::{self, get, obj, py_str, s, truthy, Obj};
use crate::text::{fit, len, upper_first, width, wrap};
use crate::timeutil::{self, clock, seconds_since, Stamp};
use serde_json::Value;
use std::collections::HashMap;

/// Every task of the repo by ID, for what a task waits on.
pub type Every = HashMap<String, Obj>;

pub fn every(repo: &Repo) -> Res<Every> {
    Ok(repo.tasks()?.into_iter().map(|t| (s(&t, "id").to_string(), t)).collect())
}

fn mark(status: &str) -> &'static str {
    match status {
        "needs" => "●",
        "stopped" => "■",
        "triage" => "◎",
        "ready" => "✔",
        "finished" => "◆",
        "done" => "·",
        "review" => "◐",
        "working" => "○",
        "todo" => "◌",
        "dropped" => "–",
        _ => " ",
    }
}

/// The board's groups, in order: what you do next first.
fn board_group(status: &str) -> u8 {
    match status {
        "needs" | "stopped" => 0,
        "triage" => 1,
        "ready" => 2,
        "finished" => 3,
        "review" => 4,
        "working" => 5,
        "todo" => 6,
        "done" | "dropped" => 7,
        _ => 8,
    }
}

pub fn board_order(mut tasks: Vec<Obj>) -> Vec<Obj> {
    tasks.sort_by_key(|t| (board_group(s(t, "status")), crate::core::task_num(t)));
    tasks
}

/// The SGR colour of a task's mark: a quiet agent's in the colour of one that needs you.
fn mark_color(t: &Obj) -> &'static str {
    if is_stuck(t) {
        return "1;33";
    }
    match s(t, "status") {
        "needs" => "1;33",
        "stopped" => "1;31",
        "ready" | "finished" => "1;32",
        "review" => "1;34",
        _ => "2",
    }
}

/// "3m", "2h", "1d", "now", or "" with no time.
pub fn ago(t: &Obj, at: Option<Stamp>) -> String {
    let st = stamp(t);
    if st.is_empty() {
        return String::new();
    }
    let secs = (seconds_since(st, at) as i64).max(0);
    for (unit, n) in [("d", 86400), ("h", 3600), ("m", 60)] {
        if secs >= n {
            return format!("{}{unit}", secs / n);
        }
    }
    "now".into()
}

fn stale(t: &Obj, at: Option<Stamp>) -> bool {
    s(t, "status") == "needs" && !stamp(t).is_empty() && seconds_since(stamp(t), at) >= 300.0
}

/// The status a task has while it waits on this agent ("worker" or "reviewer").
fn waits_on(t: &Obj, key: &str) -> bool {
    let who = s(obj(t, key), "who");
    s(t, "status") == if who == "reviewer" { "review" } else { "working" }
}

/// Has the agent the task waits on (its worker, or its reviewer in review) shown no progress for too long?
pub fn is_stuck(t: &Obj) -> bool {
    truthy(t.get("stuck")) && waits_on(t, "stuck")
}

/// agents.is_limited: is the agent the task waits on stopped at the usage limit?
pub fn is_limited(t: &Obj) -> bool {
    truthy(t.get("limit")) && waits_on(t, "limit")
}

/// "no progress for 25 min", or "" for a task that isn't stuck.
pub fn stuck_phrase(t: &Obj, at: Option<Stamp>) -> String {
    if !is_stuck(t) {
        return String::new();
    }
    let m = (seconds_since(s(obj(t, "stuck"), "since"), at) / 60.0).floor() as i64;
    if m < 120 {
        format!("no progress for {m} min")
    } else {
        format!("no progress for {} h", m.div_euclid(60))
    }
}

fn sent_back_by_github(t: &Obj) -> bool {
    truthy(obj(t, "gh").get("sent"))
}

/// The PR's latest state in a few words, or "".
pub fn pr_phrase(t: &Obj) -> String {
    let g = obj(t, "gh");
    let state = s(g, "state");
    if state == "merged" || state == "closed" {
        return format!("{state} on GitHub");
    }
    if s(t, "status") == "working" && !sent_back_by_github(t) {
        return String::new();
    }
    let mut parts = Vec::new();
    if let Some(c) = get(g, "checks").filter(|v| truthy(Some(v))) {
        parts.push(format!("checks {}", py_str(c)));
    }
    if let Some(n) = get(g, "new").filter(|v| truthy(Some(v))) {
        let one = n.as_f64() == Some(1.0);
        parts.push(format!("{} new comment{}", py_str(n), if one { "" } else { "s" }));
    }
    parts.join(", ")
}

/// start.open_deps: the tasks t waits on that aren't done, with their status (None: no such task).
pub fn open_deps<'a>(t: &'a Obj, every: &'a Every) -> Vec<(&'a str, Option<&'a str>)> {
    let after = t.get("after").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    after
        .iter()
        .filter_map(Value::as_str)
        .map(|d| (d, every.get(d).and_then(|o| get(o, "status")).and_then(Value::as_str)))
        .filter(|(_, st)| *st != Some("done"))
        .collect()
}

/// "Waits on BP-8, BP-9 (dropped)" for a todo task with open dependencies, else "".
pub fn waits_phrase(t: &Obj, every: &Every) -> String {
    if s(t, "status") != "todo" {
        return String::new();
    }
    let deps = open_deps(t, every);
    if deps.is_empty() {
        return String::new();
    }
    let words: Vec<String> = deps
        .iter()
        .map(|(d, st)| match st {
            Some("dropped") => format!("{d} (dropped)"),
            None => format!("{d} (missing)"),
            _ => d.to_string(),
        })
        .collect();
    format!("Waits on {}", words.join(", "))
}

/// What the agent the task waits on last said it's doing, on one line, or "".
fn doing_now(t: &Obj, at: Option<Stamp>) -> Res<String> {
    let block = match s(t, "status") {
        "review" => get(obj(t, "review"), "block"),
        "working" => get(t, "block"),
        _ => None,
    };
    let Some(block) = block.filter(|b| truthy(Some(b))) else { return Ok(String::new()) };
    if is_stuck(t) || is_limited(t) || truthy(t.get("note")) {
        return Ok(String::new());
    }
    let path = state_dir().join("doing").join(format!("{}.json", safe(&py_str(block))));
    let d = match json::read(&path) {
        Ok(Some(Value::Object(o))) => o,
        Ok(_) => return Ok(String::new()),
        Err(_) => return Err(Fail::Python), // Python fails on it its own way
    };
    let when = match d.get("at") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::Bool(b)) => *b as u8 as f64,
        _ => return Ok(String::new()),
    };
    if s(&d, "state") != "working" {
        return Ok(String::new());
    }
    let now = at.unwrap_or_else(timeutil::now);
    let since = timeutil::parse(stamp(t)).map_or(f64::NEG_INFINITY, |t| t.timestamp_micros() as f64 / 1e6 - 30.0);
    let now = now.timestamp_micros() as f64 / 1e6;
    if when < since || now - when > 30.0 * 60.0 {
        return Ok(String::new());
    }
    let text = get(&d, "text").filter(|v| truthy(Some(v))).map(py_str).unwrap_or_default();
    Ok(crate::text::squash(&text))
}

fn board_kind(kind: Option<&Value>) -> Option<&'static str> {
    Some(match kind?.as_str()? {
        "full" => "full review",
        "light" => "light review",
        "followup" => "follow-up review",
        "tour" => "review of your comments",
        "rebase" => "rebase check",
        _ => return None,
    })
}

/// "Recorded the demo." from "# Recorded the demo", or "" when there's none.
fn first_sentence(summary: &str) -> String {
    for line in crate::text::splitlines(summary) {
        let line = crate::text::strip(crate::text::strip(line).trim_start_matches('#'));
        if !line.is_empty() {
            return if line.ends_with(['.', '!', '?']) { line.into() } else { format!("{line}.") };
        }
    }
    String::new()
}

fn limit_detail(lim: &Obj, at: Option<Stamp>) -> String {
    let reviewer = s(lim, "who") == "reviewer";
    let who = if reviewer { "Reviewer stopped at the usage limit" } else { "Usage limit" };
    if let Some(resets) = get(lim, "resets").filter(|v| truthy(Some(v))) {
        let when = timeutil::parse(&py_str(resets)).unwrap_or_else(timeutil::now);
        return format!("{who}, resets {}. Continues by itself then.", clock(when, at));
    }
    format!("{who}, reset time unknown. {}", if reviewer { "⏎ opens it." } else { "t tells it to continue." })
}

/// The board's sentence for where the task stands.
fn status_detail(repo: &Repo, t: &Obj, every: &Every, at: Option<Stamp>) -> String {
    let r = obj(t, "review");
    let rnd = get(r, "round").filter(|v| truthy(Some(v))).map_or("1".into(), py_str);
    let why = if truthy(t.get("why")) { py_str(&t["why"]) } else { String::new() };
    let st = s(t, "status");
    let mut pr = pr_short(s(t, "pr"));
    if pr.is_empty() && truthy(t.get("local")) {
        pr = format!("Local branch {}", if truthy(t.get("branch")) { py_str(&t["branch"]) } else { String::new() });
    }
    if truthy(t.get("note")) {
        return py_str(&t["note"]);
    }
    if is_stuck(t) {
        let quiet = stuck_phrase(t, at);
        if s(obj(t, "stuck"), "who") == "reviewer" {
            return format!("Reviewer has shown {quiet}. ⏎ opens it.");
        }
        return format!("{}. p shows its screen.", upper_first(&quiet));
    }
    if is_limited(t) {
        return limit_detail(obj(t, "limit"), at);
    }
    let ended = why == "Ended its turn without saying ready";
    match st {
        "needs" if ended => "Stopped mid-task, probably with a question. p shows its screen.".into(),
        "triage" if ended => "Ended its turn without ready.".into(),
        "needs" if why.is_empty() => "Waiting for you.".into(),
        "triage" if why.is_empty() => "Waiting for its coordinator.".into(),
        "needs" | "triage" => why,
        "stopped" => "Its agent exited. r resumes it.".into(),
        "review" => {
            let kind = board_kind(r.get("kind")).unwrap_or(if truthy(r.get("kind")) { "review" } else { "reviewer" });
            format!("{} on round {rnd} of {}.", upper_first(kind), py_str(repo.cfg.get("review_rounds").unwrap_or(&Value::Null)))
        }
        "ready" if why.starts_with("Nothing to merge") => "Done, with nothing to merge.".into(),
        "finished" => match first_sentence(s(t, "summary")).as_str() {
            "" => "Finished, nothing to merge. a closes it.".into(),
            line => format!("Finished, nothing to merge: {line} a closes it."),
        },
        "ready" => {
            let text = match s(r, "verdict") {
                "approved" => format!("{pr}, approved by {}.", board_kind(r.get("kind")).unwrap_or("review")),
                "changes" => format!("{pr}. Review still asks for changes."),
                _ if !pr.is_empty() => format!("{pr}."),
                _ => "Done.".into(),
            };
            match get(t, "rebased").filter(|v| truthy(Some(v))) {
                Some(b) => format!("{text} Rebased onto {}.", py_str(b)),
                None => text,
            }
        }
        "working" if sent_back_by_github(t) => "Fixing what GitHub said.".into(),
        "working" if s(r, "verdict") == "changes" => format!("Fixing what review round {rnd} found."),
        "todo" => match waits_phrase(t, every).as_str() {
            "" => "Not started. s starts it.".into(),
            w => format!("{w}."),
        },
        "done" if truthy(t.get("local")) && !truthy(t.get("pr")) => {
            if truthy(t.get("merged")) && truthy(t.get("base")) {
                format!("Accepted. Merged into {}.", py_str(&t["base"]))
            } else {
                "Accepted.".into()
            }
        }
        "done" if truthy(t.get("closed")) => "Closed, nothing to merge.".into(),
        "done" if !pr.is_empty() => format!("Accepted. {pr} merged."),
        "done" => "Accepted.".into(),
        "dropped" => "Dropped.".into(),
        _ => String::new(),
    }
}

/// What auto-accept does with it, when that isn't what the config says for all.
fn auto_detail(repo: &Repo, t: &Obj) -> &'static str {
    let on = truthy(repo.cfg.get("auto_accept"));
    if is_closed(s(t, "status")) {
        ""
    } else if truthy(t.get("auto_wait")) && s(t, "status") == "ready" {
        "Auto-accepts once its checks pass."
    } else if on && t.get("auto_accept") == Some(&Value::Bool(false)) {
        "Kept for you: no auto-accept."
    } else if !on && t.get("auto_accept") == Some(&Value::Bool(true)) {
        "Auto-accepts once approved."
    } else {
        ""
    }
}

/// The board's sentence for a task: what its agent is doing now, else where it stands,
/// what GitHub said, what auto-accept does with it and its own model.
pub fn board_detail(repo: &Repo, t: &Obj, every: &Every, at: Option<Stamp>) -> Res<String> {
    let doing = doing_now(t, at)?;
    if !doing.is_empty() {
        return Ok(doing);
    }
    let mut text = status_detail(repo, t, every, at);
    let closed = is_closed(s(t, "status"));
    let gh = if closed || truthy(t.get("note")) { String::new() } else { pr_phrase(t) };
    if !gh.is_empty() {
        text = crate::text::strip(&format!("{text} {}.", upper_first(&gh))).to_string();
    }
    let model = if closed { None } else { own_model(repo, t) }.map(|m| format!("Model: {m}."));
    let parts = [text.as_str(), auto_detail(repo, t), model.as_deref().unwrap_or("")];
    Ok(parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" "))
}

fn sgr(code: &str, text: &str, color: bool) -> String {
    if color && !text.is_empty() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.into()
    }
}

fn title_dim(t: &Obj) -> &'static str {
    let st = s(t, "status");
    if is_closed(st) || st == "triage" {
        "2"
    } else {
        "0"
    }
}

/// One task as the board says it: mark, ID, title, age, then the board's sentence. Cut
/// to `cut` columns when given; in colour with color.
pub fn task_line(repo: &Repo, t: &Obj, every: &Every, idw: usize, cut: Option<i64>, color: bool) -> Res<String> {
    let st = s(t, "status");
    let tid = crate::text::ljust(s(t, "id"), idw);
    let when = ago(t, None);
    let mut text = board_detail(repo, t, every, None)?;
    let head = (2 + len(&tid) + 2) as i64;
    let age_room = if when.is_empty() { 0 } else { len(&when) as i64 + 2 };
    let title = match cut {
        None => s(t, "title").to_string(),
        Some(w) => fit(s(t, "title"), (w - head - age_room).max(1)),
    };
    if let Some(w) = cut {
        let room = w - head - width(&title) as i64 - age_room - 2;
        text = if room >= 4 { fit(&text, room) } else { String::new() };
    }
    let mut parts = vec![format!("{} {}  {}", sgr(mark_color(t), mark(st), color), sgr("2", &tid, color), sgr(title_dim(t), &title, color))];
    if !when.is_empty() {
        parts.push(sgr(if stale(t, None) { "1;31" } else { "2" }, &when, color));
    }
    if !text.is_empty() {
        parts.push(sgr("2", &text, color));
    }
    Ok(parts.join("  "))
}

/// The board's rows for a task, for a terminal too narrow for task_line: the title
/// row with the age at the right edge, then the sentence in `lines` lines (by default
/// two for one that needs you, else one).
pub fn task_rows(repo: &Repo, t: &Obj, every: &Every, idw: usize, cols: i64, color: bool, lines: Option<usize>) -> Res<Vec<String>> {
    let st = s(t, "status");
    let tid = crate::text::ljust(s(t, "id"), idw);
    let when = ago(t, None);
    let w = cols - 1;
    let head = (2 + len(&tid) + 2) as i64;
    let title = fit(s(t, "title"), (w - head - len(&when) as i64 - 2).max(1));
    let pad = if when.is_empty() { String::new() } else { " ".repeat((w - head - width(&title) as i64 - len(&when) as i64).max(2) as usize) };
    let mut rows = vec![format!(
        "{} {}  {}{pad}{}",
        sgr(mark_color(t), mark(st), color),
        sgr("2", &tid, color),
        sgr(title_dim(t), &title, color),
        sgr(if stale(t, None) { "1;31" } else { "2" }, &when, color)
    )];
    let text = board_detail(repo, t, every, None)?;
    let two = matches!(st, "needs" | "stopped" | "finished") || is_stuck(t) || is_limited(t) || !pr_phrase(t).is_empty();
    let n = lines.unwrap_or(if two { 2 } else { 1 });
    for line in wrap(&text, w - head, n) {
        if !line.is_empty() {
            rows.push(format!("{}{}", " ".repeat(head as usize), sgr("2", &line, color)));
        }
    }
    Ok(rows)
}

/// The terminal's width when stdout is one, else None: piped output is whole.
pub fn term_width() -> Option<i64> {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return None;
    }
    // shutil.get_terminal_size: $COLUMNS first, then the terminal, then 80
    if let Some(c) = std::env::var("COLUMNS").ok().and_then(|v| v.trim().parse::<i64>().ok()).filter(|c| *c > 0) {
        return Some(c);
    }
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } == 0;
    Some(if ok && ws.ws_col > 0 { ws.ws_col as i64 } else { 80 })
}

pub fn colored() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::env::var("NO_COLOR").map_or(true, |v| v.is_empty())
}
