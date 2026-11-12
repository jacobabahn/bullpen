//! What the board shows, apart from how it draws it: tasks and config as bullpen
//! stores them, the groups, and the sentences and keys each task gets.

use chrono::{DateTime, FixedOffset, Local};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use std::path::Path;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// A field the board can do without: a value of a shape it doesn't expect reads
/// as missing, so the task still shows.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).ok())
}

/// What GitHub last said about the task's PR.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Gh {
    #[serde(default, deserialize_with = "lenient")]
    pub state: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub checks: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub new: Option<u64>,
    /// its worker is fixing what GitHub sent it back for (cleared by its next `ready`)
    #[serde(default, deserialize_with = "lenient")]
    pub sent: Option<bool>,
}

/// An agent that has said it's working, and nothing else, for a while.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Stuck {
    /// "worker" or "reviewer"
    pub who: String,
    pub since: String,
}

/// An agent stopped at the usage limit.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Limit {
    /// "worker" or "reviewer"
    pub who: String,
    /// when the limit resets, if its message said
    #[serde(default, deserialize_with = "lenient")]
    pub resets: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Review {
    pub round: Option<u32>,
    pub verdict: Option<String>,
    /// "full", "light" or "followup"
    #[serde(default, deserialize_with = "lenient")]
    pub kind: Option<String>,
}

impl Review {
    /// "light review", "follow-up review", or plain "review" from before kinds.
    pub fn kind_words(&self) -> &'static str {
        match self.kind.as_deref() {
            Some("full") => "full review",
            Some("light") => "light review",
            Some("followup") => "follow-up review",
            _ => "review",
        }
    }
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
    /// no PR: its branch is merged locally
    #[serde(default, deserialize_with = "lenient")]
    pub local: Option<bool>,
    #[serde(default, deserialize_with = "lenient")]
    pub branch: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub base: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub merged: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub worktree: Option<String>,
    /// the size of its change: "3 files, +120 −14"
    #[serde(default, deserialize_with = "lenient")]
    pub diffstat: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub gh: Option<Gh>,
    #[serde(default, deserialize_with = "lenient")]
    pub stuck: Option<Stuck>,
    #[serde(default, deserialize_with = "lenient")]
    pub limit: Option<Limit>,
    /// the base it was rebased onto after another task's accept, until its next ready
    #[serde(default, deserialize_with = "lenient")]
    pub rebased: Option<String>,
    /// the tasks it waits on (`bullpen add --after`, `bullpen after`)
    #[serde(default, deserialize_with = "lenient")]
    pub after: Option<Vec<String>>,
    /// its own say over config's auto_accept (`--auto-accept`, `--no-auto-accept`)
    #[serde(default, deserialize_with = "lenient")]
    pub auto_accept: Option<bool>,
    /// approved, and auto-accepts once its PR's checks pass
    #[serde(default, deserialize_with = "lenient")]
    pub auto_wait: Option<bool>,
    /// the ID of the coordinator that owns it: its news goes there
    #[serde(default, deserialize_with = "lenient")]
    pub coordinator: Option<String>,
    /// its own worker model, over the config's (`add --model`, `start --model`, `model`)
    #[serde(default, deserialize_with = "lenient")]
    pub worker_model: Option<String>,
    /// what its worker said it did (`ready`, `finish`)
    #[serde(default, deserialize_with = "lenient")]
    pub summary: Option<String>,
    /// done by `close`: it finished with nothing to merge
    #[serde(default, deserialize_with = "lenient")]
    pub closed: Option<bool>,
    /// those not done yet, worked out from the other tasks: "BP-8", "BP-9 (dropped)"
    #[serde(skip)]
    pub waits: Vec<String>,
}

impl Task {
    /// The number after the prefix: tasks sort by it.
    pub fn num(&self) -> u64 {
        self.id.rsplit('-').next().and_then(|n| n.parse().ok()).unwrap_or(0)
    }

    pub fn needs_you(&self) -> bool {
        matches!(self.status.as_str(), "needs" | "stopped")
    }

    /// Stopped on something its coordinator sees to: not yours (yet).
    pub fn triage(&self) -> bool {
        self.status == "triage"
    }

    /// Is it stopped on its reviewer (a prompt, or no verdict), so ⏎ opens the reviewer?
    pub fn on_reviewer(&self) -> bool {
        matches!(self.status.as_str(), "needs" | "triage") && self.why.as_deref().unwrap_or("").starts_with("Reviewer")
    }

    pub fn closed(&self) -> bool {
        matches!(self.status.as_str(), "done" | "dropped")
    }

    pub fn is_local(&self) -> bool {
        self.local == Some(true)
    }

    /// The status a task has while it waits on this agent ("worker" or "reviewer").
    fn waits_on(&self, who: &str) -> bool {
        self.status == if who == "reviewer" { "review" } else { "working" }
    }

    /// Has the agent the task is waiting on (its worker, or its reviewer in review)
    /// shown no progress for too long?
    pub fn is_stuck(&self) -> bool {
        self.stuck.as_ref().is_some_and(|s| self.waits_on(&s.who))
    }

    /// Is the agent the task is waiting on stopped at the usage limit?
    pub fn is_limited(&self) -> bool {
        self.limit.as_ref().is_some_and(|l| self.waits_on(&l.who))
    }

    /// Does it get the amber (or red) bar: it needs you, or its agent has gone quiet.
    pub fn flagged(&self) -> bool {
        self.needs_you() || self.is_stuck()
    }

    /// Is the worker fixing failed checks or comments from its PR?
    pub fn sent_back_by_github(&self) -> bool {
        self.gh.as_ref().and_then(|g| g.sent) == Some(true)
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
    /// approved tasks accept themselves (`bullpen config --auto-accept on`)
    pub auto_accept: bool,
    /// the IDs of the repo's coordinators
    pub coordinators: Vec<String>,
    /// the coordinators whose boards show the other coordinators' tasks too
    /// (their entry's `board_others`, set by the board's `o`)
    pub board_others: Vec<String>,
    /// the worker model for all tasks (`bullpen config --worker-model`)
    pub worker_model: Option<String>,
    /// rows the board leaves empty at its bottom, for Rex's minimized-pane capsules
    /// (`bullpen config --board-bottom-gap`; 0: none)
    pub bottom_gap: usize,
}

impl Config {
    /// Is the task another coordinator's? Only when there are several: a task with
    /// no owner, or one whose owner is gone, is everyone's.
    pub fn theirs(&self, t: &Task, me: Option<&str>) -> bool {
        match (me, t.coordinator.as_deref()) {
            (Some(me), Some(o)) => self.coordinators.len() > 1 && o != me && self.coordinators.iter().any(|c| c == o),
            _ => false,
        }
    }

    /// Does the board beside coordinator me show the other coordinators' tasks?
    pub fn shows_others(&self, me: Option<&str>) -> bool {
        me.is_some_and(|me| self.board_others.iter().any(|c| c == me))
    }

    /// Is the task left off this board? Another coordinator's, while the board shows
    /// only its own.
    pub fn hides(&self, t: &Task, me: Option<&str>) -> bool {
        !self.shows_others(me) && self.theirs(t, me)
    }
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
            auto_accept: v["auto_accept"].as_bool().unwrap_or(false),
            coordinators: v["coordinators"]
                .as_array()
                .map(|cs| cs.iter().filter_map(|c| c["id"].as_str().map(String::from)).collect())
                .unwrap_or_default(),
            board_others: v["coordinators"]
                .as_array()
                .map(|cs| {
                    cs.iter()
                        .filter(|c| c["board_others"].as_bool() == Some(true))
                        .filter_map(|c| c["id"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            worker_model: v["worker_model"].as_str().map(String::from),
            bottom_gap: v["board_bottom_gap"].as_u64().unwrap_or(1) as usize,
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
    link_waits(&mut out);
    out
}

/// Fill in what each task waits on: the tasks in its `after` that aren't done.
pub fn link_waits(tasks: &mut [Task]) {
    let status: std::collections::HashMap<String, String> =
        tasks.iter().map(|t| (t.id.clone(), t.status.clone())).collect();
    for t in tasks.iter_mut() {
        t.waits = t
            .after
            .iter()
            .flatten()
            .filter_map(|d| match status.get(d).map(String::as_str) {
                Some("done") => None,
                Some("dropped") => Some(format!("{d} (dropped)")),
                Some(_) => Some(d.clone()),
                None => Some(format!("{d} (missing)")),
            })
            .collect();
    }
}

// The board groups tasks by what the person does next. Colour only marks status;
// the one loud thing is the amber bar beside a task that needs you. A task its
// coordinator sees to (triage) sits dimmed below, with no bar: it isn't yours yet.

pub const GROUPS: &[(&str, &[&str])] = &[
    ("Needs you", &["needs", "stopped"]),
    ("Coordinator on it", &["triage"]),
    ("Ready to accept", &["ready"]),
    ("Finished", &["finished"]),
    ("In review", &["review"]),
    ("Working", &["working"]),
    ("Queued", &["todo"]),
    ("Closed", &["done", "dropped"]),
];

/// Seconds a task can need you before its age turns red.
pub const STALE: i64 = 300;

pub fn mark(status: &str) -> &'static str {
    match status {
        "needs" => "●",
        "stopped" => "■",
        "triage" => "◎",
        "ready" => "✔",
        "finished" => "◆",
        "done" => "✓",
        "review" => "◐",
        "working" => "○",
        "todo" => "◌", // a dotted circle: not started yet
        "dropped" => "–",
        _ => " ",
    }
}

/// The groups that have tasks, each sorted by ID; the closed ones only with `closed`.
pub fn board_groups(tasks: &[Task], closed: bool) -> Vec<(&'static str, Vec<&Task>)> {
    GROUPS
        .iter()
        .filter(|(heading, _)| closed || *heading != "Closed")
        .filter_map(|(heading, statuses)| {
            let mut ts: Vec<&Task> = tasks.iter().filter(|t| statuses.contains(&t.status.as_str())).collect();
            ts.sort_by_key(|t| t.num());
            (!ts.is_empty()).then_some((*heading, ts))
        })
        .collect()
}

/// The groups this board shows: board_groups without the tasks it hides.
pub fn shown_groups<'a>(tasks: &'a [Task], cfg: &Config, me: Option<&str>, closed: bool) -> Vec<(&'static str, Vec<&'a Task>)> {
    board_groups(tasks, closed)
        .into_iter()
        .filter_map(|(heading, ts)| {
            let ts: Vec<&Task> = ts.into_iter().filter(|t| !cfg.hides(t, me)).collect();
            (!ts.is_empty()).then_some((heading, ts))
        })
        .collect()
}

/// How many tasks the board hides, by coordinator in the config's order: those it
/// would show with the others' tasks on (closed ones only with `closed`).
pub fn hidden(tasks: &[Task], cfg: &Config, me: Option<&str>, closed: bool) -> Vec<(String, usize)> {
    let hid: Vec<&Task> = board_groups(tasks, closed).into_iter().flat_map(|(_, ts)| ts).filter(|t| cfg.hides(t, me)).collect();
    cfg.coordinators
        .iter()
        .map(|c| (c.clone(), hid.iter().filter(|t| t.coordinator.as_deref() == Some(c)).count()))
        .filter(|(_, n)| *n > 0)
        .collect()
}

/// The header's note of what's hidden: "+4 in c2 · o shows", "" when nothing is.
pub fn hidden_hint(hidden: &[(String, usize)]) -> String {
    if hidden.is_empty() {
        return String::new();
    }
    let n: usize = hidden.iter().map(|(_, n)| n).sum();
    let whose: Vec<&str> = hidden.iter().map(|(c, _)| c.as_str()).collect();
    format!("+{n} in {} · o shows", whose.join(", "))
}

/// A task's title as the board shows it: another coordinator's starts with its ID.
pub fn shown_title(t: &Task, cfg: &Config, me: Option<&str>) -> String {
    match t.coordinator.as_deref() {
        Some(c) if cfg.theirs(t, me) => format!("{c} · {}", t.title),
        _ => t.title.clone(),
    }
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

/// "no progress for 25 min", or "" for a task that isn't stuck.
pub fn stuck_phrase(t: &Task, at: DateTime<FixedOffset>) -> String {
    let Some(s) = t.stuck.as_ref().filter(|_| t.is_stuck()) else { return String::new() };
    let m = parse(&s.since).map_or(0, |since| (at - since).num_seconds() / 60);
    if m < 120 {
        format!("no progress for {m} min")
    } else {
        format!("no progress for {} h", m / 60)
    }
}

/// "3:00 PM", with the day when it isn't today: "Mon 12:00 AM", "Oct 9 3:00 PM".
pub fn clock(when: DateTime<FixedOffset>, at: DateTime<FixedOffset>) -> String {
    let (when, at) = (when.with_timezone(&Local), at.with_timezone(&Local));
    let text = when.format("%-I:%M %p").to_string();
    match (when.date_naive() - at.date_naive()).num_days() {
        0 => text,
        1..=6 => format!("{} {text}", when.format("%a")),
        _ => format!("{} {text}", when.format("%b %-d")),
    }
}

/// The board's line for a task stopped at the usage limit.
fn limit_detail(lim: &Limit, at: DateTime<FixedOffset>) -> String {
    let reviewer = lim.who == "reviewer";
    let who = if reviewer { "Reviewer stopped at the usage limit" } else { "Usage limit" };
    if let Some(resets) = lim.resets.as_deref().and_then(parse) {
        return format!("{who}, resets {}. Continues by itself then.", clock(resets, at));
    }
    let next = if reviewer { "⏎ opens it." } else { "t tells it to continue." };
    format!("{who}, reset time unknown. {next}")
}

/// The PR's latest state in a few words, or "". A working task only shows what
/// GitHub sent it back for: anything else is about the old push.
pub fn pr_phrase(t: &Task) -> String {
    let Some(g) = &t.gh else { return String::new() };
    if let Some(state) = g.state.as_deref().filter(|s| matches!(*s, "merged" | "closed")) {
        return format!("{state} on GitHub");
    }
    if t.status == "working" && !t.sent_back_by_github() {
        return String::new();
    }
    let mut parts = Vec::new();
    if let Some(c) = g.checks.as_deref().filter(|c| !c.is_empty()) {
        parts.push(format!("checks {c}"));
    }
    if let Some(n) = g.new.filter(|n| *n > 0) {
        parts.push(format!("{n} new comment{}", if n == 1 { "" } else { "s" }));
    }
    parts.join(", ")
}

/// What the task needs or where it stands, as a plain sentence, then what GitHub
/// last said about its PR.
pub fn task_detail(t: &Task, rounds: u32, at: DateTime<FixedOffset>) -> String {
    let text = status_detail(t, rounds, at);
    let gh = if t.closed() { String::new() } else { pr_phrase(t) };
    if gh.is_empty() || t.note.as_deref().is_some_and(|n| !n.is_empty()) {
        return text;
    }
    format!("{text} {}.", capitalize(&gh)).trim().to_string()
}

/// What auto-accept does with the task, when it isn't what the config says for all,
/// or it's waiting: "" otherwise. `on` is the config's auto_accept.
pub fn auto_phrase(t: &Task, on: bool) -> &'static str {
    if t.closed() {
        ""
    } else if t.auto_wait == Some(true) && t.status == "ready" {
        "Auto-accepts once its checks pass."
    } else if on && t.auto_accept == Some(false) {
        "Kept for you: no auto-accept."
    } else if !on && t.auto_accept == Some(true) {
        "Auto-accepts once approved."
    } else {
        ""
    }
}

/// The worker model the task chose, when it isn't the config's: "Model: opus." or "".
pub fn model_phrase(t: &Task, cfg: &Config) -> String {
    match t.worker_model.as_deref() {
        Some(m) if !m.is_empty() && !t.closed() && Some(m) != cfg.worker_model.as_deref() => format!("Model: {m}."),
        _ => String::new(),
    }
}

/// task_detail, then what auto-accept does with it and the model it chose.
pub fn detail_with_auto(t: &Task, cfg: &Config, at: DateTime<FixedOffset>) -> String {
    let text = task_detail(t, cfg.review_rounds, at);
    let model = model_phrase(t, cfg);
    let parts = [text.as_str(), auto_phrase(t, cfg.auto_accept), model.as_str()];
    parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" ").trim().to_string()
}

fn capitalize(text: &str) -> String {
    let mut c = text.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn ready_detail(pr: &str, r: &Review) -> String {
    match r.verdict.as_deref() {
        Some("approved") => format!("{pr}, approved by {}.", r.kind_words()),
        Some("changes") => format!("{pr}. Review still asks for changes."),
        _ if !pr.is_empty() => format!("{pr}."),
        _ => "Done.".into(),
    }
}

fn status_detail(t: &Task, rounds: u32, at: DateTime<FixedOffset>) -> String {
    let r = t.review.clone().unwrap_or_default();
    let round = r.round.unwrap_or(1);
    let verdict = r.verdict.as_deref();
    let why = t.why.as_deref().unwrap_or("");
    let has_pr = t.pr.as_deref().is_some_and(|p| !p.is_empty());
    let mut pr = pr_short(t.pr.as_deref());
    if pr.is_empty() && t.is_local() {
        pr = format!("Local branch {}", t.branch.as_deref().unwrap_or(""));
    }
    if let Some(note) = t.note.as_deref().filter(|n| !n.is_empty()) {
        return note.into();
    }
    if t.is_stuck() {
        let quiet = stuck_phrase(t, at);
        if t.stuck.as_ref().is_some_and(|s| s.who == "reviewer") {
            return format!("Reviewer has shown {quiet}. ⏎ opens it.");
        }
        return format!("{}. p shows its screen.", capitalize(&quiet));
    }
    if let Some(lim) = t.limit.as_ref().filter(|_| t.is_limited()) {
        return limit_detail(lim, at);
    }
    match t.status.as_str() {
        "needs" if why == "Ended its turn without saying ready" => {
            "Stopped mid-task, probably with a question. p shows its screen.".into()
        }
        "needs" if why.is_empty() => "Waiting for you.".into(),
        "needs" => why.into(),
        "triage" if why == "Ended its turn without saying ready" => "Ended its turn without ready.".into(),
        "triage" if why.is_empty() => "Waiting for its coordinator.".into(),
        "triage" => why.into(),
        "stopped" => "Its agent exited. r resumes it.".into(),
        "review" if r.kind.is_some() => {
            format!("{} on round {round} of {rounds}.", capitalize(r.kind_words()))
        }
        "review" => format!("Reviewer on round {round} of {rounds}."),
        "ready" if why.starts_with("Nothing to merge") => "Done, with nothing to merge.".into(),
        "finished" => match first_line(t.summary.as_deref()).as_str() {
            "" => "Finished, nothing to merge. c closes it.".into(),
            line => format!("Finished, nothing to merge: {line} c closes it."),
        },
        "ready" => match &t.rebased {
            Some(base) => format!("{} Rebased onto {base}.", ready_detail(&pr, &r)),
            None => ready_detail(&pr, &r),
        },
        "working" if t.sent_back_by_github() => "Fixing what GitHub said.".into(),
        "working" if verdict == Some("changes") => format!("Fixing what review round {round} found."),
        "todo" if !t.waits.is_empty() => format!("Waits on {}.", t.waits.join(", ")),
        "todo" => "Not started. s starts it.".into(),
        "done" if t.is_local() && !has_pr => match (&t.merged, &t.base) {
            (Some(_), Some(base)) => format!("Accepted. Merged into {base}."),
            _ => "Accepted.".into(),
        },
        "done" if t.closed == Some(true) => "Closed, nothing to merge.".into(),
        "done" if !pr.is_empty() => format!("Accepted. {pr} merged."),
        "done" => "Accepted.".into(),
        "dropped" => "Dropped.".into(),
        _ => String::new(),
    }
}

/// The first line of a summary as a sentence: "Recorded the demo." from "# Recorded
/// the demo", or "" when there's none.
fn first_line(summary: Option<&str>) -> String {
    let line = summary.unwrap_or("").lines().map(|l| l.trim().trim_start_matches('#').trim()).find(|l| !l.is_empty());
    match line {
        Some(l) if l.ends_with(['.', '!', '?']) => l.to_string(),
        Some(l) => format!("{l}."),
        None => String::new(),
    }
}

/// The size of a task's change, for one in review or ready: "3 files, +120 −14".
pub fn task_stat(t: &Task) -> &str {
    match t.status.as_str() {
        "review" | "ready" => t.diffstat.as_deref().unwrap_or(""),
        _ => "",
    }
}

/// The keys that make sense for this task, for the footer: (key, what it does).
pub fn task_keys(t: Option<&Task>) -> Vec<(&'static str, &'static str)> {
    let Some(t) = t else { return Vec::new() };
    let mut keys: Vec<(&str, &str)> = match t.status.as_str() {
        "needs" => vec![("⏎", "answer"), ("t", "tell"), ("p", "peek")],
        "triage" => vec![("⏎", "open"), ("p", "peek"), ("t", "tell")],
        "stopped" => vec![("r", "resume"), ("p", "peek"), ("d", "drop")],
        "review" => vec![("⏎", "reviewer"), ("p", "peek")],
        "ready" => vec![("a", "accept"), ("d", "drop"), ("⏎", "open")],
        "finished" => vec![("c", "close"), ("d", "drop"), ("⏎", "open")],
        "working" => vec![("⏎", "open"), ("t", "tell"), ("p", "peek")],
        "todo" => vec![("s", "start"), ("d", "drop")],
        _ => Vec::new(),
    };
    if t.worktree.as_deref().is_some_and(|w| !w.is_empty()) && !t.closed() {
        keys.push(("v", "diff"));
    }
    keys
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Accept,
    Close,
    Drop,
    DropForce,
}

/// The question a task's own row turns into while the board waits for y or n.
pub fn confirm_text(action: Action, t: &Task, merge: &str) -> String {
    let pr = pr_short(t.pr.as_deref());
    let verb = match merge {
        "rebase" => "rebase-merge",
        "squash" => "squash-merge",
        other => other,
    };
    let nothing = t.why.as_deref().unwrap_or("").starts_with("Nothing to merge");
    match action {
        Action::Accept if pr.is_empty() && t.is_local() && !nothing => {
            format!("Accept and {verb} into {}?", t.base.as_deref().unwrap_or("its base"))
        }
        Action::Accept if pr.is_empty() => "Accept and clean up?".into(),
        Action::Accept => format!("Accept and {verb}?"),
        Action::Close => "Close it and remove its worktree?".into(),
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
pub(crate) mod tests {
    use super::*;

    impl Config {
        pub(crate) fn default_for_test() -> Config {
            Config {
                layout: Layout::Grouped,
                review_rounds: 2,
                merge: "rebase".into(),
                auto_accept: false,
                coordinators: Vec::new(),
                board_others: Vec::new(),
                worker_model: None,
                bottom_gap: 1,
            }
        }
    }

    fn task(id: &str, status: &str) -> Task {
        Task { id: id.into(), status: status.into(), ..Default::default() }
    }

    #[test]
    fn another_coordinators_task_is_theirs_only_while_it_is_listed() {
        let mut cfg = Config::default_for_test();
        let owned = |o: Option<&str>| Task { coordinator: o.map(String::from), ..task("BP-1", "working") };
        cfg.coordinators = vec!["c1".into()];
        assert!(!cfg.theirs(&owned(Some("c2")), Some("c1"))); // one coordinator: all are its
        cfg.coordinators = vec!["c1".into(), "c2".into()];
        assert!(cfg.theirs(&owned(Some("c2")), Some("c1")));
        assert!(!cfg.theirs(&owned(Some("c1")), Some("c1")));
        assert!(!cfg.theirs(&owned(Some("c3")), Some("c1"))); // its owner is gone: everyone's
        assert!(!cfg.theirs(&owned(None), Some("c1")));
        assert!(!cfg.theirs(&owned(Some("c2")), None)); // a board that isn't anyone's
    }

    /// Two coordinators: c1's and c2's tasks, one with no owner, one whose owner is gone.
    fn two_boards() -> (Config, Vec<Task>) {
        let mut cfg = Config::default_for_test();
        cfg.coordinators = vec!["c1".into(), "c2".into()];
        let owned = |id: &str, status: &str, o: Option<&str>| Task { coordinator: o.map(String::from), ..task(id, status) };
        let tasks = vec![
            owned("BP-1", "working", Some("c1")),
            owned("BP-2", "needs", Some("c2")),
            owned("BP-3", "ready", Some("c2")),
            owned("BP-4", "done", Some("c2")),
            owned("BP-5", "todo", None),
            owned("BP-6", "review", Some("c9")),
        ];
        (cfg, tasks)
    }

    fn shown_ids<'a>(tasks: &'a [Task], cfg: &Config, me: Option<&str>, closed: bool) -> Vec<&'a str> {
        let mut ids: Vec<&str> = shown_groups(tasks, cfg, me, closed).into_iter().flat_map(|(_, ts)| ts).map(|t| t.id.as_str()).collect();
        ids.sort_by_key(|id| id[3..].parse::<u32>().unwrap());
        ids
    }

    #[test]
    fn a_board_shows_only_its_own_coordinators_tasks() {
        let (cfg, tasks) = two_boards();
        // c2's are left off c1's board; its own, the ownerless and the gone owner's stay
        assert_eq!(shown_ids(&tasks, &cfg, Some("c1"), true), vec!["BP-1", "BP-5", "BP-6"]);
        assert_eq!(shown_ids(&tasks, &cfg, Some("c2"), false), vec!["BP-2", "BP-3", "BP-5", "BP-6"]);
        // a board that isn't anyone's shows everything
        assert_eq!(shown_ids(&tasks, &cfg, None, false), vec!["BP-1", "BP-2", "BP-3", "BP-5", "BP-6"]);
    }

    #[test]
    fn ownerless_and_gone_owners_tasks_stay_on_every_board() {
        let (cfg, tasks) = two_boards();
        for me in ["c1", "c2"] {
            let ids = shown_ids(&tasks, &cfg, Some(me), false);
            assert!(ids.contains(&"BP-5") && ids.contains(&"BP-6"), "{me}: {ids:?}");
        }
        assert_eq!(shown_title(&tasks[4], &cfg, Some("c1")), "");
        assert_eq!(shown_title(&tasks[5], &cfg, Some("c1")), "");
    }

    #[test]
    fn showing_the_others_brings_their_tasks_back_labelled() {
        let (mut cfg, mut tasks) = two_boards();
        cfg.board_others = vec!["c1".into()];
        assert!(cfg.shows_others(Some("c1")) && !cfg.shows_others(Some("c2")));
        // c1's board: everything, closed ones only with f
        assert_eq!(shown_ids(&tasks, &cfg, Some("c1"), false), vec!["BP-1", "BP-2", "BP-3", "BP-5", "BP-6"]);
        assert_eq!(shown_ids(&tasks, &cfg, Some("c1"), true), vec!["BP-1", "BP-2", "BP-3", "BP-4", "BP-5", "BP-6"]);
        // in their usual groups
        let groups = shown_groups(&tasks, &cfg, Some("c1"), false);
        let group = |id: &str| groups.iter().find(|(_, ts)| ts.iter().any(|t| t.id == id)).unwrap().0;
        assert_eq!((group("BP-2"), group("BP-3")), ("Needs you", "Ready to accept"));
        // labelled with their coordinator; its own aren't
        tasks[1].title = "Fix it".into();
        assert_eq!(shown_title(&tasks[1], &cfg, Some("c1")), "c2 · Fix it");
        tasks[0].title = "Mine".into();
        assert_eq!(shown_title(&tasks[0], &cfg, Some("c1")), "Mine");
        // c2's board still shows only its own
        assert_eq!(shown_ids(&tasks, &cfg, Some("c2"), false), vec!["BP-2", "BP-3", "BP-5", "BP-6"]);
        assert!(hidden(&tasks, &cfg, Some("c1"), true).is_empty());
    }

    #[test]
    fn the_header_counts_what_is_hidden() {
        let (mut cfg, tasks) = two_boards();
        // BP-2 and BP-3 open; BP-4 is closed, so it counts only with f
        assert_eq!(hidden(&tasks, &cfg, Some("c1"), false), vec![("c2".to_string(), 2)]);
        assert_eq!(hidden(&tasks, &cfg, Some("c1"), true), vec![("c2".to_string(), 3)]);
        assert_eq!(hidden_hint(&hidden(&tasks, &cfg, Some("c1"), false)), "+2 in c2 · o shows");
        // c2 hides c1's one task
        assert_eq!(hidden_hint(&hidden(&tasks, &cfg, Some("c2"), false)), "+1 in c1 · o shows");
        // several others
        cfg.coordinators.push("c3".into());
        let mut more = tasks.clone();
        more.push(Task { coordinator: Some("c3".into()), ..task("BP-7", "working") });
        assert_eq!(hidden_hint(&hidden(&more, &cfg, Some("c1"), false)), "+3 in c2, c3 · o shows");
        // nothing hidden: nothing said
        let mine: Vec<Task> = tasks.iter().filter(|t| t.coordinator.as_deref() != Some("c2")).cloned().collect();
        assert_eq!(hidden_hint(&hidden(&mine, &cfg, Some("c1"), false)), "");
    }

    #[test]
    fn one_coordinator_shows_everything_as_before() {
        let (mut cfg, tasks) = two_boards();
        cfg.coordinators = vec!["c1".into()];
        // c2 isn't listed: its tasks are everyone's
        assert_eq!(shown_ids(&tasks, &cfg, Some("c1"), true), vec!["BP-1", "BP-2", "BP-3", "BP-4", "BP-5", "BP-6"]);
        assert!(hidden(&tasks, &cfg, Some("c1"), true).is_empty());
        assert_eq!(shown_title(&tasks[1], &cfg, Some("c1")), "");
    }

    #[test]
    fn groups_follow_what_you_do_next() {
        let tasks: Vec<Task> = ["todo", "working", "ready", "needs", "stopped", "review", "done", "triage"]
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
                ("Coordinator on it", vec!["BP-8"]),
                ("Ready to accept", vec!["BP-3"]),
                ("In review", vec!["BP-6"]),
                ("Working", vec!["BP-2"]),
                ("Queued", vec!["BP-1"]),
            ]
        );
        assert_eq!(board_groups(&tasks, true).last().unwrap().0, "Closed");
    }

    #[test]
    fn auto_accept_says_when_a_task_differs_or_waits() {
        let cfg = |on| Config {
            layout: Layout::Grouped,
            review_rounds: 2,
            merge: "rebase".into(),
            auto_accept: on,
            coordinators: Vec::new(),
            board_others: Vec::new(),
            worker_model: None,
            bottom_gap: 1,
        };
        let approved = Some(Review { round: Some(1), verdict: Some("approved".into()), kind: None });
        let t = Task { pr: Some("https://github.com/me/repo/pull/4".into()), review: approved, ..task("BP-1", "ready") };
        assert_eq!(detail_with_auto(&t, &cfg(true), now()), "PR #4, approved by review.");
        let held = Task { auto_accept: Some(false), ..t.clone() };
        assert_eq!(detail_with_auto(&held, &cfg(true), now()), "PR #4, approved by review. Kept for you: no auto-accept.");
        assert_eq!(detail_with_auto(&held, &cfg(false), now()), "PR #4, approved by review.");
        let own = Task { auto_accept: Some(true), ..task("BP-1", "todo") };
        assert_eq!(detail_with_auto(&own, &cfg(false), now()), "Not started. s starts it. Auto-accepts once approved.");
        let waiting = Task { auto_wait: Some(true), ..t.clone() };
        assert_eq!(detail_with_auto(&waiting, &cfg(true), now()), "PR #4, approved by review. Auto-accepts once its checks pass.");
        assert_eq!(auto_phrase(&Task { status: "done".into(), ..held }, true), "");
    }

    #[test]
    fn a_task_says_its_model_when_it_isnt_the_configs() {
        let mut cfg = Config::default_for_test();
        let own = Task { worker_model: Some("haiku".into()), ..task("BP-1", "todo") };
        assert_eq!(detail_with_auto(&own, &cfg, now()), "Not started. s starts it. Model: haiku.");
        assert_eq!(detail_with_auto(&task("BP-1", "todo"), &cfg, now()), "Not started. s starts it.");
        cfg.worker_model = Some("haiku".into());
        assert_eq!(detail_with_auto(&own, &cfg, now()), "Not started. s starts it.");
        assert_eq!(model_phrase(&Task { status: "done".into(), ..own }, &Config::default_for_test()), "");
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
        assert_eq!(task_detail(&t, 2, now()), "Asks: python3 or python?");
        let t = Task { why: ended, ..task("BP-1", "needs") };
        assert!(task_detail(&t, 2, now()).contains("probably with a question"));
        let t = Task { review: Some(Review { round: Some(2), verdict: None, kind: None }), ..task("BP-1", "review") };
        assert_eq!(task_detail(&t, 2, now()), "Reviewer on round 2 of 2.");
        let t = Task {
            pr: Some("https://x/pull/4".into()),
            review: Some(Review { round: None, verdict: Some("approved".into()), kind: None }),
            ..task("BP-1", "ready")
        };
        assert_eq!(task_detail(&t, 2, now()), "PR #4, approved by review.");
        let t = Task { review: Some(Review { round: Some(1), verdict: Some("changes".into()), kind: None }), ..task("BP-1", "working") };
        assert_eq!(task_detail(&t, 2, now()), "Fixing what review round 1 found.");
    }

    #[test]
    fn detail_says_which_kind_of_review() {
        let kind = |k: &str, verdict: Option<&str>| Review {
            round: Some(1), verdict: verdict.map(Into::into), kind: Some(k.into()),
        };
        let t = Task { review: Some(kind("light", None)), ..task("BP-1", "review") };
        assert_eq!(task_detail(&t, 2, now()), "Light review on round 1 of 2.");
        let t = Task { review: Some(kind("followup", None)), ..task("BP-1", "review") };
        assert_eq!(task_detail(&t, 2, now()), "Follow-up review on round 1 of 2.");
        let t = Task { pr: Some("https://x/pull/4".into()), review: Some(kind("full", Some("approved"))), ..task("BP-1", "ready") };
        assert_eq!(task_detail(&t, 2, now()), "PR #4, approved by full review.");
        let t = Task { pr: Some("https://x/pull/4".into()), review: Some(kind("light", Some("approved"))), ..task("BP-1", "ready") };
        assert_eq!(task_detail(&t, 2, now()), "PR #4, approved by light review.");
    }

    #[test]
    fn detail_ends_with_what_github_said() {
        let pr = Some("https://x/pull/4".to_string());
        let gh = |state: Option<&str>, checks: Option<&str>, new| {
            Some(Gh { state: state.map(String::from), checks: checks.map(String::from), new, sent: None })
        };
        let t = Task { pr: pr.clone(), gh: gh(None, Some("failing"), Some(1)), ..task("BP-1", "ready") };
        assert_eq!(task_detail(&t, 2, now()), "PR #4. Checks failing, 1 new comment.");
        let sent = Some(Gh { checks: Some("failing".into()), sent: Some(true), ..Default::default() });
        let t = Task { gh: sent, ..task("BP-1", "working") };
        assert_eq!(task_detail(&t, 2, now()), "Fixing what GitHub said. Checks failing.");
        let t = Task { pr, gh: gh(Some("merged"), None, None), ..task("BP-1", "ready") };
        assert_eq!(task_detail(&t, 2, now()), "PR #4. Merged on GitHub.");
    }

    #[test]
    fn local_tasks_show_their_branch() {
        let t = Task {
            local: Some(true),
            branch: Some("bullpen/bp-2-x".into()),
            base: Some("main".into()),
            review: Some(Review { round: None, verdict: Some("approved".into()), kind: None }),
            ..task("BP-2", "ready")
        };
        assert_eq!(task_detail(&t, 2, now()), "Local branch bullpen/bp-2-x, approved by review.");
        assert_eq!(task_detail(&Task { review: None, ..t.clone() }, 2, now()), "Local branch bullpen/bp-2-x.");
        let rebased = Task { rebased: Some("main".into()), ..t.clone() };
        assert_eq!(task_detail(&rebased, 2, now()), "Local branch bullpen/bp-2-x, approved by review. Rebased onto main.");
        let done = Task { status: "done".into(), merged: Some("abc".into()), ..t.clone() };
        assert_eq!(task_detail(&done, 2, now()), "Accepted. Merged into main.");
        assert_eq!(confirm_text(Action::Accept, &t, "squash"), "Accept and squash-merge into main?");
        assert_eq!(confirm_text(Action::Drop, &t, "rebase"), "Drop it?");
    }

    #[test]
    fn stat_shows_for_review_and_ready() {
        let t = Task { diffstat: Some("3 files, +120 −14".into()), ..task("BP-1", "ready") };
        assert_eq!(task_stat(&t), "3 files, +120 −14");
        assert_eq!(task_stat(&Task { status: "review".into(), ..t.clone() }), "3 files, +120 −14");
        assert_eq!(task_stat(&Task { status: "working".into(), ..t }), "");
        assert_eq!(task_stat(&task("BP-1", "ready")), "");
    }

    #[test]
    fn triage_is_the_coordinators_not_yours() {
        let t = Task { why: Some("Failed: API error".into()), ..task("BP-1", "triage") };
        assert!(t.triage());
        assert!(!t.needs_you());
        assert!(!t.flagged()); // no amber bar
        assert_eq!(task_detail(&t, 2, now()), "Failed: API error");
        let ended = Task { why: Some("Ended its turn without saying ready".into()), ..task("BP-1", "triage") };
        assert_eq!(task_detail(&ended, 2, now()), "Ended its turn without ready.");
        assert_eq!(task_keys(Some(&t)), vec![("⏎", "open"), ("p", "peek"), ("t", "tell")]);
        let reviewer = Task { why: Some("Reviewer stopped without a verdict".into()), ..task("BP-1", "triage") };
        assert!(reviewer.on_reviewer());
        assert!(!t.on_reviewer());
        let asked = Task { why: Some("Reviewer needs you (permission)".into()), ..task("BP-1", "needs") };
        assert!(asked.on_reviewer() && asked.flagged());
        // escalated: it needs you, with its reason
        let late = Task { why: Some("Failed (coordinator didn't resolve it)".into()), ..task("BP-1", "needs") };
        assert!(late.needs_you() && late.flagged());
        assert_eq!(task_detail(&late, 2, now()), "Failed (coordinator didn't resolve it)");
    }

    #[test]
    fn a_finished_task_shows_its_summary_and_closes_with_c() {
        let summary = Some("# Recorded the demo video\n\nSaved to ~/Movies/demo.mov".to_string());
        let t = Task { summary, worktree: Some("/w".into()), ..task("BP-3", "finished") };
        let tasks = vec![task("BP-1", "ready"), t.clone(), task("BP-2", "working")];
        let heads: Vec<&str> = board_groups(&tasks, false).iter().map(|(h, _)| *h).collect();
        assert_eq!(heads, vec!["Ready to accept", "Finished", "Working"]);
        assert_eq!(mark("finished"), "◆");
        assert!(!t.needs_you() && !t.flagged() && !t.closed());
        assert_eq!(task_detail(&t, 2, now()), "Finished, nothing to merge: Recorded the demo video. c closes it.");
        assert_eq!(task_detail(&task("BP-3", "finished"), 2, now()), "Finished, nothing to merge. c closes it.");
        assert_eq!(task_keys(Some(&t))[0], ("c", "close"));
        assert!(task_keys(Some(&t)).contains(&("v", "diff")));
        assert_eq!(confirm_text(Action::Close, &t, "rebase"), "Close it and remove its worktree?");
        assert_eq!(task_stat(&t), "");
        // once closed it is done, under Closed, and counts as done for what waits on it
        let closed = Task { status: "done".into(), closed: Some(true), worktree: None, ..t };
        assert_eq!(task_detail(&closed, 2, now()), "Closed, nothing to merge.");
        let mut ts = vec![closed, Task { after: Some(vec!["BP-3".into()]), ..task("BP-4", "todo") }];
        link_waits(&mut ts);
        assert!(ts[1].waits.is_empty());
        let t: Task = serde_json::from_str(r#"{"id":"BP-3","status":"finished","summary":"Notes in ~/n.md"}"#).unwrap();
        assert_eq!(task_detail(&t, 2, now()), "Finished, nothing to merge: Notes in ~/n.md. c closes it.");
    }

    #[test]
    fn a_field_of_the_wrong_shape_reads_as_missing() {
        let t: Task = serde_json::from_str(r#"{"id":"BP-1","status":"ready","merged":true,"gh":"?","diffstat":3}"#).unwrap();
        assert_eq!((t.merged, t.diffstat, t.gh.is_none()), (None, None, true));
    }

    #[test]
    fn keys_fit_the_task() {
        assert_eq!(task_keys(Some(&task("BP-1", "ready")))[0], ("a", "accept"));
        assert_eq!(task_keys(Some(&task("BP-1", "needs")))[0], ("⏎", "answer"));
        assert!(task_keys(None).is_empty());
        let wt = |st: &str| Task { worktree: Some("/w".into()), ..task("BP-1", st) };
        assert_eq!(task_keys(Some(&wt("working"))).last(), Some(&("v", "diff")));
        assert!(!task_keys(Some(&task("BP-1", "todo"))).contains(&("v", "diff")));
        assert!(!task_keys(Some(&wt("done"))).contains(&("v", "diff")));
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

    fn changes(round: u32) -> Option<Review> {
        Some(Review { round: Some(round), verdict: Some("changes".into()), kind: None })
    }

    #[test]
    fn a_working_task_says_who_sent_it_back() {
        let pr = Some("https://x/pull/4".to_string());
        let gh = Gh { state: Some("open".into()), checks: Some("passing".into()), new: Some(0), sent: None };
        // a reviewer's changes, after the watcher recorded the old push's checks
        let t = Task { pr: pr.clone(), review: changes(1), gh: Some(gh.clone()), ..task("BP-1", "working") };
        assert_eq!(task_detail(&t, 2, now()), "Fixing what review round 1 found.");
        // GitHub's, even when an earlier review round asked for changes
        let failing = Gh { checks: Some("failing".into()), new: Some(1), sent: Some(true), ..gh.clone() };
        let t = Task { pr: pr.clone(), review: changes(1), gh: Some(failing), ..task("BP-1", "working") };
        assert_eq!(task_detail(&t, 2, now()), "Fixing what GitHub said. Checks failing, 1 new comment.");
        // resumed or told by the person: nothing about the old push
        let t = Task { pr, gh: Some(gh.clone()), ..task("BP-1", "working") };
        assert_eq!(task_detail(&t, 2, now()), "");
        assert_eq!(pr_phrase(&t), "");
        assert_eq!(pr_phrase(&Task { status: "ready".into(), ..t }), "checks passing");
    }

    fn minutes_ago(m: i64) -> String {
        (now() - chrono::Duration::minutes(m)).to_rfc3339()
    }

    #[test]
    fn a_stuck_task_says_so() {
        let stuck = |who: &str, m| Some(Stuck { who: who.into(), since: minutes_ago(m) });
        let t = Task { stuck: stuck("worker", 25), ..task("BP-1", "working") };
        assert!(t.is_stuck() && t.flagged());
        assert_eq!(task_detail(&t, 2, now()), "No progress for 25 min. p shows its screen.");
        let noted = Task { note: Some("Hung on npm install".into()), ..t.clone() };
        assert_eq!(task_detail(&noted, 2, now()), "Hung on npm install");
        let review = Some(Review { round: Some(1), verdict: None, kind: None });
        let t = Task { review: review.clone(), stuck: stuck("reviewer", 190), ..task("BP-1", "review") };
        assert_eq!(task_detail(&t, 2, now()), "Reviewer has shown no progress for 3 h. ⏎ opens it.");
        // a worker's quiet spell says nothing about a task that has moved on
        let moved = Task { review, stuck: stuck("worker", 25), ..task("BP-1", "review") };
        assert!(!moved.is_stuck() && !moved.flagged());
        assert_eq!(task_detail(&moved, 2, now()), "Reviewer on round 1 of 2.");
        assert!(!task("BP-1", "working").is_stuck());
    }

    fn local(d: u32, h: u32, m: u32) -> DateTime<FixedOffset> {
        use chrono::TimeZone;
        Local.with_ymd_and_hms(2026, 11, d, h, m, 0).unwrap().fixed_offset()
    }

    #[test]
    fn a_limited_task_says_when_it_resets() {
        let at = local(11, 13, 0); // a Wednesday, 1 PM
        let lim = |who: &str, resets: Option<String>| Some(Limit { who: who.into(), resets });
        let three = Some(local(11, 15, 0).to_rfc3339());
        let t = Task { limit: lim("worker", three.clone()), ..task("BP-1", "working") };
        assert!(t.is_limited());
        assert_eq!(task_detail(&t, 2, at), "Usage limit, resets 3:00 PM. Continues by itself then.");
        let t = Task { limit: lim("worker", None), ..task("BP-1", "working") };
        assert_eq!(task_detail(&t, 2, at), "Usage limit, reset time unknown. t tells it to continue.");
        let t = Task { limit: lim("reviewer", three.clone()), ..task("BP-1", "review") };
        assert_eq!(task_detail(&t, 2, at),
                   "Reviewer stopped at the usage limit, resets 3:00 PM. Continues by itself then.");
        let t = Task { limit: lim("reviewer", None), ..task("BP-1", "review") };
        assert_eq!(task_detail(&t, 2, at), "Reviewer stopped at the usage limit, reset time unknown. ⏎ opens it.");
        // a worker's limit says nothing about a task that has moved on
        assert!(!Task { limit: lim("worker", three), ..task("BP-1", "ready") }.is_limited());
    }

    #[test]
    fn clock_says_the_day_when_it_isnt_today() {
        let at = local(11, 13, 0);
        assert_eq!(clock(local(11, 15, 0), at), "3:00 PM");
        assert_eq!(clock(local(16, 0, 0), at), "Mon 12:00 AM");
        assert_eq!(clock(local(30, 9, 5), at), "Nov 30 9:05 AM");
    }

    #[test]
    fn a_queued_task_says_what_it_waits_on() {
        let after = |ids: &[&str]| Some(ids.iter().map(|s| s.to_string()).collect());
        let mut tasks: Vec<Task> = vec![
            task("BP-1", "done"),
            task("BP-2", "working"),
            task("BP-3", "dropped"),
            Task { after: after(&["BP-1", "BP-2"]), ..task("BP-4", "todo") },
            Task { after: after(&["BP-1"]), ..task("BP-5", "todo") },
            Task { after: after(&["BP-3", "BP-9"]), ..task("BP-6", "todo") },
            Task { after: after(&["BP-2"]), ..task("BP-7", "working") },
        ];
        link_waits(&mut tasks);
        let detail = |i: usize| task_detail(&tasks[i], 2, now());
        assert_eq!(detail(3), "Waits on BP-2.");
        assert_eq!(detail(4), "Not started. s starts it.");
        assert_eq!(detail(5), "Waits on BP-3 (dropped), BP-9 (missing).");
        assert_eq!(detail(6), ""); // started anyway (--force): it waits on nothing now
        // read from the task's JSON, as bullpen writes it
        let t: Task = serde_json::from_str(r#"{"id":"BP-4","status":"todo","after":["BP-1"]}"#).unwrap();
        assert_eq!(t.after, after(&["BP-1"]));
    }

    #[test]
    fn stuck_and_limit_of_the_wrong_shape_read_as_missing() {
        let t: Task = serde_json::from_str(r#"{"id":"BP-1","status":"working","stuck":1,"limit":"x"}"#).unwrap();
        assert!(!t.is_stuck() && !t.is_limited());
    }
}
