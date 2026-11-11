//! What the board shows, apart from how it draws it: tasks and config as bullpen
//! stores them, the groups, and the sentences and keys each task gets.

use chrono::{DateTime, FixedOffset, Local};
use serde::Deserialize;
use std::path::Path;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Review {
    pub round: Option<u32>,
    pub verdict: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(default)]
    pub title: String,
    pub status: String,
    pub note: Option<String>,
    pub why: Option<String>,
    pub pr: Option<String>,
    pub review: Option<Review>,
    pub since: Option<String>,
    pub updated: Option<String>,
    pub block: Option<String>,
}

impl Task {
    /// The number after the prefix: tasks sort by it.
    pub fn num(&self) -> u64 {
        self.id.rsplit('-').next().and_then(|n| n.parse().ok()).unwrap_or(0)
    }

    pub fn needs_you(&self) -> bool {
        matches!(self.status.as_str(), "needs" | "stopped")
    }

    pub fn closed(&self) -> bool {
        matches!(self.status.as_str(), "done" | "dropped")
    }

    /// When it entered its current status.
    pub fn stamp(&self) -> Option<&str> {
        self.since.as_deref().or(self.updated.as_deref())
    }
}

/// The settings the board reads from the repo's config.json.
#[derive(Debug, Clone)]
pub struct Config {
    pub layout: Layout,
    pub review_rounds: u32,
    pub merge: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Grouped,
    Compact,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::Grouped => "grouped",
            Layout::Compact => "compact",
        }
    }

    pub fn other(self) -> Layout {
        match self {
            Layout::Grouped => Layout::Compact,
            Layout::Compact => Layout::Grouped,
        }
    }
}

impl Config {
    pub fn load(dir: &Path) -> Config {
        let v: serde_json::Value = std::fs::read_to_string(dir.join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Config {
            layout: if v["board_layout"].as_str() == Some("compact") { Layout::Compact } else { Layout::Grouped },
            review_rounds: v["review_rounds"].as_u64().unwrap_or(2) as u32,
            merge: v["merge"].as_str().unwrap_or("rebase").to_string(),
        }
    }
}

/// The repo's tasks, by ID. A file caught half-written is skipped until next time.
pub fn load_tasks(dir: &Path) -> Vec<Task> {
    let mut out: Vec<Task> = std::fs::read_dir(dir.join("tasks"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok())
        .collect();
    out.sort_by_key(Task::num);
    out
}

// The board groups tasks by what the person does next. Colour only marks status;
// the one loud thing is the amber bar beside a task that needs you.

pub const GROUPS: &[(&str, &[&str])] = &[
    ("Needs you", &["needs", "stopped"]),
    ("Ready to accept", &["ready"]),
    ("In review", &["review"]),
    ("Working", &["working"]),
    ("Queued", &["todo"]),
    ("Finished", &["done", "dropped"]),
];

/// Seconds a task can need you before its age turns red.
pub const STALE: i64 = 300;

pub fn mark(status: &str) -> &'static str {
    match status {
        "needs" => "●",
        "stopped" => "■",
        "ready" | "done" => "✓",
        "review" => "◐",
        "working" => "○",
        "todo" => "·",
        "dropped" => "–",
        _ => " ",
    }
}

/// The groups that have tasks, each sorted by ID.
pub fn board_groups(tasks: &[Task], finished: bool) -> Vec<(&'static str, Vec<&Task>)> {
    GROUPS
        .iter()
        .filter(|(heading, _)| finished || *heading != "Finished")
        .filter_map(|(heading, statuses)| {
            let mut ts: Vec<&Task> = tasks.iter().filter(|t| statuses.contains(&t.status.as_str())).collect();
            ts.sort_by_key(|t| t.num());
            (!ts.is_empty()).then_some((*heading, ts))
        })
        .collect()
}

pub fn pr_short(url: Option<&str>) -> String {
    let url = url.unwrap_or("");
    if let Some(i) = url.find("/pull/") {
        let n: String = url[i + 6..].chars().take_while(char::is_ascii_digit).collect();
        if !n.is_empty() {
            return format!("PR #{n}");
        }
    }
    url.to_string()
}

fn parse(stamp: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(stamp).ok()
}

pub fn seconds_in_status(t: &Task, at: DateTime<FixedOffset>) -> i64 {
    t.stamp().and_then(parse).map_or(0, |s| (at - s).num_seconds())
}

pub fn ago(stamp: Option<&str>, at: DateTime<FixedOffset>) -> String {
    let Some(s) = stamp.and_then(parse) else { return String::new() };
    let s = (at - s).num_seconds().max(0);
    for (unit, n) in [("d", 86400), ("h", 3600), ("m", 60)] {
        if s >= n {
            return format!("{}{unit}", s / n);
        }
    }
    "now".into()
}

pub fn now() -> DateTime<FixedOffset> {
    Local::now().fixed_offset()
}

/// What the task needs or where it stands, as a plain sentence.
pub fn task_detail(t: &Task, rounds: u32) -> String {
    let r = t.review.clone().unwrap_or_default();
    let round = r.round.unwrap_or(1);
    let verdict = r.verdict.as_deref();
    let why = t.why.as_deref().unwrap_or("");
    let pr = pr_short(t.pr.as_deref());
    if let Some(note) = t.note.as_deref().filter(|n| !n.is_empty()) {
        return note.into();
    }
    match t.status.as_str() {
        "needs" if why == "Ended its turn without saying ready" => {
            "Stopped mid-task, probably with a question. p shows its screen.".into()
        }
        "needs" if why.is_empty() => "Waiting for you.".into(),
        "needs" => why.into(),
        "stopped" => "Its agent exited. r resumes it.".into(),
        "review" => format!("Reviewer on round {round} of {rounds}."),
        "ready" if why.starts_with("Nothing to merge") => "Done, with nothing to merge.".into(),
        "ready" if verdict == Some("approved") => format!("{pr}, approved by review."),
        "ready" if verdict == Some("changes") => format!("{pr}. Review still asks for changes."),
        "ready" if !pr.is_empty() => format!("{pr}."),
        "ready" => "Done.".into(),
        "working" if verdict == Some("changes") => format!("Fixing what review round {round} found."),
        "todo" => "Not started. s starts it.".into(),
        "done" if !pr.is_empty() => format!("Accepted. {pr} merged."),
        "done" => "Accepted.".into(),
        "dropped" => "Dropped.".into(),
        _ => String::new(),
    }
}

/// The keys that make sense for this task, for the footer.
pub fn task_keys(t: Option<&Task>) -> &'static str {
    match t.map(|t| t.status.as_str()) {
        Some("needs") => "⏎ answer  t tell  p peek",
        Some("stopped") => "r resume  p peek  d drop",
        Some("review") => "⏎ reviewer  p peek",
        Some("ready") => "a accept  d drop  ⏎ open",
        Some("working") => "⏎ open  t tell  p peek",
        Some("todo") => "s start  d drop",
        _ => "",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Accept,
    Drop,
    DropForce,
}

/// The question a task's own row turns into while the board waits for y or n.
pub fn confirm_text(action: Action, t: &Task, merge: &str) -> String {
    let pr = pr_short(t.pr.as_deref());
    match action {
        Action::Accept if pr.is_empty() => "Accept and clean up?".into(),
        Action::Accept => {
            let verb = match merge {
                "rebase" => "rebase-merge",
                "squash" => "squash-merge",
                other => other,
            };
            format!("Accept and {verb}?")
        }
        Action::Drop if pr.is_empty() => "Drop it?".into(),
        Action::Drop => format!("Drop it and close {pr}?"),
        Action::DropForce => "Discard uncommitted changes?".into(),
    }
}

/// Columns the text takes on screen.
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Cut text to width, with an ellipsis when it doesn't fit.
pub fn fit(text: &str, w: usize) -> String {
    if w == 0 {
        return String::new();
    }
    if width(text) <= w {
        return text.into();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw > w - 1 {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    out
}

/// Pad text with spaces to width columns.
pub fn pad(text: &str, w: usize) -> String {
    format!("{text}{}", " ".repeat(w.saturating_sub(width(text))))
}

/// Wrap text into at most `lines` lines of width columns, ending in " …" when cut.
pub fn wrap(text: &str, w: usize, lines: usize) -> Vec<String> {
    let w = w.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        // a word longer than a line is broken across lines
        while width(&word) > w {
            let room = if cur.is_empty() { w } else { w.saturating_sub(width(&cur) + 1) };
            if room == 0 {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            let (head, tail) = split_at_width(&word, room);
            if cur.is_empty() {
                cur = head;
            } else {
                cur = format!("{cur} {head}");
            }
            out.push(std::mem::take(&mut cur));
            word = tail;
        }
        if cur.is_empty() {
            cur = word;
        } else if width(&cur) + 1 + width(&word) <= w {
            cur = format!("{cur} {word}");
        } else {
            out.push(std::mem::replace(&mut cur, word));
        }
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    if out.len() > lines.max(1) {
        out.truncate(lines.max(1));
        let last = out.last_mut().unwrap();
        *last = if width(last) + 2 > w { fit(&format!("{last} …"), w) } else { format!("{last} …") };
    }
    out
}

fn split_at_width(s: &str, w: usize) -> (String, String) {
    let mut used = 0;
    for (i, c) in s.char_indices() {
        let cw = c.width().unwrap_or(0);
        if used + cw > w {
            return (s[..i].into(), s[i..].into());
        }
        used += cw;
    }
    (s.into(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, status: &str) -> Task {
        Task { id: id.into(), status: status.into(), ..Default::default() }
    }

    #[test]
    fn groups_follow_what_you_do_next() {
        let tasks: Vec<Task> = ["todo", "working", "ready", "needs", "stopped", "review", "done"]
            .iter()
            .enumerate()
            .map(|(i, st)| task(&format!("BP-{}", i + 1), st))
            .collect();
        let groups: Vec<(&str, Vec<&str>)> = board_groups(&tasks, false)
            .into_iter()
            .map(|(h, ts)| (h, ts.iter().map(|t| t.id.as_str()).collect()))
            .collect();
        assert_eq!(
            groups,
            vec![
                ("Needs you", vec!["BP-4", "BP-5"]),
                ("Ready to accept", vec!["BP-3"]),
                ("In review", vec!["BP-6"]),
                ("Working", vec!["BP-2"]),
                ("Queued", vec!["BP-1"]),
            ]
        );
        assert_eq!(board_groups(&tasks, true).last().unwrap().0, "Finished");
    }

    #[test]
    fn groups_sort_by_number_not_text() {
        let tasks = vec![task("BP-10", "todo"), task("BP-9", "todo")];
        let ids: Vec<&str> = board_groups(&tasks, false)[0].1.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["BP-9", "BP-10"]);
    }

    #[test]
    fn detail_says_what_a_task_needs() {
        let ended = Some("Ended its turn without saying ready".to_string());
        let t = Task { why: ended.clone(), note: Some("Asks: python3 or python?".into()), ..task("BP-1", "needs") };
        assert_eq!(task_detail(&t, 2), "Asks: python3 or python?");
        let t = Task { why: ended, ..task("BP-1", "needs") };
        assert!(task_detail(&t, 2).contains("probably with a question"));
        let t = Task { review: Some(Review { round: Some(2), verdict: None }), ..task("BP-1", "review") };
        assert_eq!(task_detail(&t, 2), "Reviewer on round 2 of 2.");
        let t = Task {
            pr: Some("https://x/pull/4".into()),
            review: Some(Review { round: None, verdict: Some("approved".into()) }),
            ..task("BP-1", "ready")
        };
        assert_eq!(task_detail(&t, 2), "PR #4, approved by review.");
        let t = Task { review: Some(Review { round: Some(1), verdict: Some("changes".into()) }), ..task("BP-1", "working") };
        assert_eq!(task_detail(&t, 2), "Fixing what review round 1 found.");
    }

    #[test]
    fn keys_fit_the_task() {
        assert!(task_keys(Some(&task("BP-1", "ready"))).starts_with("a accept"));
        assert!(task_keys(Some(&task("BP-1", "needs"))).starts_with("⏎ answer"));
        assert_eq!(task_keys(None), "");
    }

    #[test]
    fn fit_and_age() {
        assert_eq!(fit("Add a done command", 10), "Add a don…");
        assert_eq!(fit("short", 10), "short");
        let at = DateTime::parse_from_rfc3339("2026-01-01T12:00:00+00:00").unwrap();
        assert_eq!(ago(Some("2026-01-01T11:55:00+00:00"), at), "5m");
        assert_eq!(ago(Some("2026-01-01T09:55:00+00:00"), at), "2h");
        assert_eq!(ago(Some("2026-01-01T11:59:30.123456+00:00"), at), "now");
        assert_eq!(ago(None, at), "");
    }

    #[test]
    fn wrap_cuts_to_the_lines_given() {
        assert_eq!(wrap("one two three", 7, 2), vec!["one two", "three"]);
        assert_eq!(wrap("one two three four", 7, 1), vec!["one tw…"]);
        assert_eq!(wrap("", 7, 2), vec![""]);
        assert_eq!(wrap("abcdefghij", 4, 3), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn confirm_names_the_merge() {
        let t = Task { pr: Some("https://x/pull/7".into()), ..task("BP-1", "ready") };
        assert_eq!(confirm_text(Action::Accept, &t, "squash"), "Accept and squash-merge?");
        assert_eq!(confirm_text(Action::Drop, &t, "rebase"), "Drop it and close PR #7?");
        assert_eq!(confirm_text(Action::Accept, &task("BP-1", "ready"), "rebase"), "Accept and clean up?");
    }
}
