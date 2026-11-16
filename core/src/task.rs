//! A task: its status (changed only by set_status), its stop, its own settings, and
//! the task in words.
//!
//! A stopped task (STOPPED) has a stop, {"by": "worker" or "reviewer", "kind": ...},
//! saying who stopped and how:
//!   prompt     at a prompt: it needs the person
//!   failed     its turn failed
//!   ended      it ended its turn without ready (a reviewer: without a verdict)
//!   exited     its pane closed before ready (a reviewer: before a verdict)
//!   pane_gone  its pane wasn't found again after a Rex restart (a worker is stopped)
//!   not_sent   review findings or PR feedback didn't reach the worker
//!   limit      it couldn't be continued after the usage limit
//!   setup      its worktree's setup failed, so its worker never started
//!   other      a stop from before stops were recorded, of no known kind
//! why is the stop in words, for people and the board: nothing goes by its wording.
//! ask and ok are said of the status they were set in, and end with it.

use crate::json::{get, obj, s, truthy, Obj};
use crate::repo::Repo;
use crate::settings::{CLOSED, STOPPED, WAITING};
use serde_json::{json, Value};
use std::path::Path;

// ---------- the lifecycle ----------

/// The one place a task's status changes. A new status ends ask and ok. stop: why it
/// stopped now; a task that stays stopped without a new one (an ask, an escalation)
/// keeps the one it had.
pub fn set_status(t: &mut Obj, status: &str, why: Option<&str>, stop: Option<Value>) {
    if t.get("status").and_then(Value::as_str) != Some(status) {
        for k in ["ok", "ask", "asked_from"] {
            t.shift_remove(k);
        }
    }
    t.insert("status".into(), json!(status));
    t.insert("why".into(), why.map_or(Value::Null, |w| json!(w)));
    match stop.filter(|v| truthy(Some(v))) {
        Some(stop) => {
            t.insert("stop".into(), stop);
        }
        None if !is_stopped(status) => {
            t.shift_remove("stop");
        }
        None => {}
    }
}

/// The task needs the person, for question. A stopped task keeps its stop: whether
/// its worker is mid-turn still goes by it. One that wasn't stopped (working, in
/// review, ready...) remembers its status and why (asked_from): the answer puts them
/// back.
pub fn ask(t: &mut Obj, question: &str) {
    let was = s(t, "status").to_string();
    let why = t.get("why").cloned().unwrap_or(Value::Null);
    let asked_from = get(t, "asked_from").cloned();
    set_status(t, "needs", Some(question), None);
    t.insert("ask".into(), json!(true));
    if was == "needs" {
        if let Some(from) = asked_from.filter(|v| truthy(Some(v))) {
            t.insert("asked_from".into(), from); // asked again: still from where it was first
        }
    } else if !is_stopped(&was) {
        t.insert("asked_from".into(), json!({"status": was, "why": why}));
    }
}

/// Its coordinator saw to the task's stop: back to working, or to review for its
/// reviewer's. ok: the idle agent isn't a stop again until its next turn.
pub fn ok(t: &mut Obj) {
    let status = if reviewers_stop(t) { "review" } else { "working" };
    set_status(t, status, None, None);
    t.insert("ok".into(), json!(true));
}

/// The task's worker is at it again, and has to run ready again. Its approval stays on
/// record: that ready keeps it if the code didn't change (approval_kept).
pub fn back_to_work(t: &mut Obj) {
    t.shift_remove("from_ready");
    set_status(t, "working", None, None);
}

/// Is the task's stop its reviewer's? (One that couldn't be continued after the usage
/// limit is seen to as its worker's.)
pub fn reviewers_stop(t: &Obj) -> bool {
    let stop = obj(t, "stop");
    s(stop, "by") == "reviewer" && s(stop, "kind") != "limit"
}

/// Is the task's worker stopped at a prompt?
pub fn at_prompt(t: &Obj) -> bool {
    t.get("stop") == Some(&json!({"by": "worker", "kind": "prompt"}))
}

/// Is the worker between turns, so a message typed into it lands as a new turn?
/// Not when it's blocked on a prompt, or failed mid-turn.
pub fn worker_idle(t: &Obj) -> bool {
    if !is_waiting(s(t, "status")) {
        return true;
    }
    !(at_prompt(t) || matches!(s(obj(t, "stop"), "kind"), "failed" | "limit"))
}

pub fn is_closed(status: &str) -> bool {
    CLOSED.contains(&status)
}

/// Waiting on its coordinator (triage) or the person (needs).
pub fn is_waiting(status: &str) -> bool {
    WAITING.contains(&status)
}

/// Waiting, or stopped (its worker's pane is gone): a status with a stop.
pub fn is_stopped(status: &str) -> bool {
    STOPPED.contains(&status)
}

/// A task's number: 3 for BP-3 (0 when its ID has none).
pub fn task_num(t: &Obj) -> i64 {
    s(t, "id").rsplit('-').next().and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// When it entered its current status.
pub fn stamp(t: &Obj) -> Option<&str> {
    [t.get("since"), t.get("updated")]
        .into_iter()
        .flatten()
        .find(|v| truthy(Some(v)))
        .and_then(Value::as_str)
}

/// Of a repo's tasks, the ones `ls` lists: the open ones, or all with --all.
pub fn listed(tasks: Vec<Obj>, all: bool) -> Vec<Obj> {
    tasks
        .into_iter()
        .filter(|t| all || !is_closed(s(t, "status")))
        .collect()
}

// ---------- its own settings ----------

/// The worker model a task chose for itself, if it isn't the config's (cfg: the repo's settings).
pub fn own_model<'a>(cfg: &Obj, t: &'a Obj) -> Option<&'a str> {
    let m = t.get("worker_model").filter(|v| truthy(Some(v)))?;
    (cfg.get("worker_model") != Some(m)).then(|| m.as_str()).flatten()
}

/// The task's own auto-accept (on: true or false), over the config's; None leaves it.
pub fn set_auto_accept(t: &mut Obj, on: Option<bool>) {
    if let Some(on) = on {
        t.insert("auto_accept".into(), json!(on));
    }
}

/// Does the task get accepted by itself once review approves it?
pub fn auto_accepts(repo: &Repo, t: &Obj) -> bool {
    match get(t, "auto_accept") {
        None => truthy(repo.cfg.get("auto_accept")),
        own => truthy(own),
    }
}

/// Is the task finished with its branch to be kept (`finish --keep-branch`)? Its
/// close keeps the branch (`close --keep-branch`), and auto-accept leaves it.
pub fn keeps_branch(t: &Obj) -> bool {
    s(t, "status") == "finished" && truthy(t.get("keep_branch"))
}

// ---------- agents' commands ----------

/// Does the command run claude?
pub fn is_claude(cmd: &[String]) -> bool {
    cmd.first()
        .is_some_and(|c| Path::new(c).file_name().is_some_and(|n| n == "claude"))
}

/// The --model a command gives, if any.
pub fn model_in(cmd: &[String]) -> Option<String> {
    for (i, a) in cmd.iter().enumerate() {
        if a == "--model" && i + 1 < cmd.len() {
            return Some(cmd[i + 1].clone());
        }
        if let Some(m) = a.strip_prefix("--model=") {
            return Some(m.into());
        }
    }
    None
}

/// cmd with claude's --model set to model, replacing any it has. No model: cmd as
/// it is; "default": no --model, so Claude Code picks. Other agents are left alone.
pub fn with_model(cmd: &[String], model: Option<&str>) -> Vec<String> {
    let Some(model) = model.filter(|m| !m.is_empty() && is_claude(cmd)) else {
        return cmd.to_vec();
    };
    let mut out = Vec::new();
    let mut skip = false;
    for a in cmd {
        if skip {
            skip = false;
        } else if a == "--model" {
            skip = true;
        } else if !a.starts_with("--model=") {
            out.push(a.clone());
        }
    }
    if model != "default" {
        out.extend(["--model".to_string(), model.to_string()]);
    }
    out
}

/// What model an agent with this command and setting runs, for `config`.
pub fn model_phrase(cmd: &[String], model: Option<&str>) -> String {
    if !is_claude(cmd) {
        let what = cmd.first().map_or("empty", String::as_str);
        return format!("not set by bullpen: the command is {what}, not claude");
    }
    model_in(&with_model(cmd, model)).unwrap_or_else(|| "default (Claude Code picks)".into())
}

// ---------- a task in words ----------

/// "PR #12" for a pull request's URL; anything else as it is.
pub fn pr_short(url: &str) -> String {
    // the first /pull/ followed by digits, as re.search(r"/pull/(\d+)") finds it
    let mut from = 0;
    while let Some(i) = url[from..].find("/pull/") {
        let at = from + i + "/pull/".len();
        let n: String = url[at..].chars().take_while(char::is_ascii_digit).collect();
        if !n.is_empty() {
            return format!("PR #{n}");
        }
        from = from + i + 1;
    }
    url.into()
}

/// What to call the task's work in news: its PR, else "" (a local branch, in a repo
/// with no remote, goes without saying). core.where.
pub fn where_(t: &Obj) -> String {
    if truthy(t.get("pr")) && !truthy(t.get("local")) {
        pr_short(s(t, "pr"))
    } else {
        String::new()
    }
}

/// The board's group for each status, so `ls` uses the same words.
pub const BOARD_WORDS: [(&str, &str); 10] = [
    ("needs", "needs you"),
    ("stopped", "needs you"),
    ("triage", "coordinator on it"),
    ("ready", "ready to accept"),
    ("finished", "finished"),
    ("review", "in review"),
    ("working", "working"),
    ("todo", "queued"),
    ("done", "closed"),
    ("dropped", "closed"),
];

/// The board's words for a status, with the status itself when they don't say it:
/// "needs you", "needs you (stopped)", "coordinator on it (triage)".
pub fn status_words(status: &str) -> String {
    let words = BOARD_WORDS
        .iter()
        .find(|(st, _)| *st == status)
        .map_or(status, |(_, w)| *w);
    if words.contains(status) {
        words.into()
    } else {
        format!("{words} ({status})")
    }
}

/// "BP-3 is queued (todo)", "BP-3 needs you (stopped)": a task's status in the
/// board's words, for errors.
pub fn is_status(t: &Obj) -> String {
    let words = status_words(s(t, "status"));
    let id = s(t, "id");
    if words.starts_with("coordinator") {
        format!("{id}'s {}", words.replacen(" on it", " is on it", 1))
    } else if words.starts_with("needs") {
        format!("{id} {words}")
    } else {
        format!("{id} is {words}")
    }
}

/// Text on one line (runs of whitespace as one space), cut to n characters with "…".
pub fn clip(text: &str, n: usize) -> String {
    let text = crate::text::squash(text);
    if text.chars().count() <= n {
        return text;
    }
    let mut out: String = text.chars().take(n.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// "1 commit", "3 commits".
pub fn commits(n: i64) -> String {
    format!("{n} commit{}", if n != 1 { "s" } else { "" })
}

/// A title as a branch name: lower case, each run of other characters a "-", at most
/// n characters ("task" when nothing is left).
pub fn slug(title: &str, n: usize) -> String {
    let mut s = String::new();
    for c in title.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    let s = s.trim_matches('-');
    let cut: String = s.chars().take(n).collect();
    let cut = cut.trim_end_matches('-');
    if cut.is_empty() {
        "task".into()
    } else {
        cut.into()
    }
}

#[cfg(test)]
#[path = "test_task.rs"]
mod tests;
