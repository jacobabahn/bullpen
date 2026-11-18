//! Every setting bullpen has (core.SETTINGS), in the order `config` lists them, and
//! the constants that go with them. watch.lua's copies of some defaults are held to
//! these by tests/test_config.py.

use crate::json::{py_str, Obj};
use serde_json::Value;

/// What an agent runs when its config doesn't say.
pub const AGENT: [&str; 3] = ["claude", "--permission-mode", "auto"];
/// The notify levels.
pub const NOTIFY: [&str; 3] = ["all", "needs", "off"];
/// Where a worker's or reviewer's pane can live (panes.py).
pub const PLACES: [&str; 4] = ["float", "tab", "tabs", "background"];

/// Rex's block type for a terminal pane.
pub const TERMINAL: &str = "com.superlogical.terminal.shell";
/// A floating pane's place: the bottom right of the window, as x, y, w, h.
pub const FLOAT_BOUNDS: [f64; 4] = [0.4, 0.35, 0.58, 0.62];

/// The statuses of a task that is over.
pub const CLOSED: [&str; 2] = ["done", "dropped"];
/// `issues_start`'s choices: which judged issues start by themselves.
pub const ISSUES_START: [&str; 3] = ["off", "small", "medium"];
pub const REVIEW_MODES: [&str; 3] = ["full", "light", "auto"];
/// A task stopped on something: "triage" waits on its coordinator, "needs" on the person.
pub const WAITING: [&str; 2] = ["needs", "triage"];
/// A task with a stop: waiting, or "stopped" (its worker's pane is gone).
pub const STOPPED: [&str; 3] = ["needs", "triage", "stopped"];

/// What bullpen notes for itself in a repo's config.json.
pub const INTERNAL: [&str; 4] = ["root", "triage_migrated", "stops_migrated", "layered"];

/// What a setting is, which gives `config` its flag and the flag's check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// No flag: set in config.json by hand.
    File,
    Choice,
    OnOff,
    /// With a least value, and what to say when it is less.
    Number,
    /// A "" clears it, when the setting clears; with a pattern, it must match it.
    Text,
    /// claude's --model.
    Model,
}

impl Kind {
    /// The kind's name in core.py: "file", "choice", "on/off", "number", "text", "model".
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Choice => "choice",
            Kind::OnOff => "on/off",
            Kind::Number => "number",
            Kind::Text => "text",
            Kind::Model => "model",
        }
    }
}

/// A setting's default, as plain data (a serde_json Value can't be a constant).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Int(i64),
    Str(&'static str),
    List(&'static [&'static str]),
}

impl Val {
    pub fn to_value(self) -> Value {
        match self {
            Val::Null => Value::Null,
            Val::Bool(b) => Value::Bool(b),
            Val::Int(n) => Value::from(n),
            Val::Str(s) => Value::from(s),
            Val::List(l) => Value::Array(l.iter().map(|s| Value::from(*s)).collect()),
        }
    }
}

/// One setting (core.Setting): its default, its group in `config`'s list (None: not
/// listed), its kind, its help, whether only a repository can hold it, and what its
/// flag checks: choices, a least number and what to say below it (says), a pattern
/// a text must match (says, when it doesn't), the flag's metavar, whether "" clears
/// it, and what to call no value (none).
#[derive(Debug, Clone, Copy)]
pub struct Setting {
    pub name: &'static str,
    pub default: Val,
    pub group: Option<&'static str>,
    pub kind: Kind,
    pub help: &'static str,
    /// It belongs to one repository: the global config.json can't hold it.
    pub repo_only: bool,
    pub choices: Option<&'static [&'static str]>,
    pub least: Option<i64>,
    pub says: Option<&'static str>,
    pub metavar: Option<&'static str>,
    pub clears: bool,
    pub none: Option<&'static str>,
    pub pattern: Option<&'static str>,
}

/// A setting with nothing but its name, default, group and kind.
const fn base(name: &'static str, default: Val, group: Option<&'static str>, kind: Kind) -> Setting {
    Setting {
        name,
        default,
        group,
        kind,
        help: "",
        repo_only: false,
        choices: None,
        least: None,
        says: None,
        metavar: None,
        clears: false,
        none: None,
        pattern: None,
    }
}

use Kind::*;
use Val::{Bool, Int, List, Null, Str};

/// Every setting, in the order `config` lists them.
pub static SETTINGS: &[Setting] = &[
    Setting { help: "the letters new task IDs start with (2 to 5 capitals)", repo_only: true, says: Some("2 to 5 capital letters"), pattern: Some("[A-Z]{2,5}"), ..base("prefix", Str("BP"), Some("Tasks"), Text) },
    Setting { repo_only: true, ..base("next", Int(1), None, File) },
    Setting { help: "the branch tasks start from and open PRs against", repo_only: true, none: Some("the repo's default branch"), ..base("base", Null, Some("Tasks"), Text) },
    Setting { help: "tasks working at once (review doesn't count); the queue starts more as they hand off", least: Some(1), says: Some("a number of tasks, 1 or more"), metavar: Some("N"), ..base("max_running", Int(3), Some("Tasks"), Number) },
    Setting { help: "how accept merges a PR", choices: Some(&["rebase", "squash", "merge"]), ..base("merge", Str("rebase"), Some("Tasks"), Choice) },
    Setting { help: "accept rebases the other open tasks onto the new base", ..base("rebase_others", Bool(true), Some("Tasks"), OnOff) },
    Setting { help: "local mode: push the base branch to this remote (a backup, not origin) after each accept, and when `up` finds it behind (\"\" turns it off)", repo_only: true, metavar: Some("REMOTE"), none: Some("none"), clears: true, ..base("push_remote", Null, Some("Tasks"), Text) },
    Setting { help: "accept (merge) a task by itself once review approves it", ..base("auto_accept", Bool(false), Some("Tasks"), OnOff) },
    Setting { help: "a command run in each new worktree before its worker starts, over .bullpen/setup.sh (\"\" clears it)", metavar: Some("CMD"), none: Some("none"), clears: true, ..base("setup", Null, Some("Tasks"), Text) },
    Setting { help: "a command run in a worktree before accept, close or drop removes it, over .bullpen/teardown.sh (\"\" clears it)", metavar: Some("CMD"), none: Some("none"), clears: true, ..base("teardown", Null, Some("Tasks"), Text) },
    Setting { help: "the gh program, if not the one on PATH; BULLPEN_GH wins (\"\" clears it)", metavar: Some("PATH"), none: Some("gh on PATH"), clears: true, ..base("gh", Null, Some("Tasks"), Text) },
    base("worker", List(&AGENT), Some("Agents"), File),
    base("coordinator_cmd", List(&AGENT), Some("Agents"), File),
    base("reviewer", List(&AGENT), Some("Agents"), File),
    base("tour", List(&["claude"]), Some("Agents"), File),
    Setting { help: "new workers as capsules in the coordinator's tab (float), as capsules in one workers tab (tab), a tab each (tabs), or in the background: in no tab until `go` shows one", choices: Some(&PLACES), ..base("workers", Str("float"), Some("Agents"), Choice) },
    Setting { help: "new reviewers the same way (float, tab, tabs, background), or where workers go (same)", choices: Some(&["same", "float", "tab", "tabs", "background"]), ..base("reviewers", Str("background"), Some("Agents"), Choice) },
    Setting { help: "claude's --model for workers (\"default\": none, so Claude Code picks)", none: Some("the worker command's own"), ..base("worker_model", Null, Some("Agents"), Model) },
    Setting { help: "the same, for reviewers (a full first review)", none: Some("the reviewer command's own"), ..base("reviewer_model", Null, Some("Agents"), Model) },
    Setting { help: "the same, for later review rounds and light reviews", ..base("reviewer_followup_model", Str("sonnet"), Some("Agents"), Model) },
    Setting { help: "claude's --model for the agent that writes guided review tours", ..base("tour_model", Str("sonnet"), Some("Agents"), Model) },
    Setting { help: "a reviewer agent checks each task before it's ready", ..base("review", Bool(true), Some("Review"), OnOff) },
    Setting { help: "reviews a task gets before it is ready anyway, findings listed", least: Some(1), says: Some("a number of rounds, 1 or more"), metavar: Some("N"), ..base("review_rounds", Int(2), Some("Review"), Number) },
    Setting { help: "a change of at most this many lines in at most 2 files gets a light review", least: Some(0), says: Some("a number of lines, or 0 for docs-only changes alone"), metavar: Some("LINES"), ..base("light_review_lines", Int(40), Some("Review"), Number) },
    Setting { help: "more changed lines than this in a medium risk path make a ready task's risk high", least: Some(1), says: Some("a number of lines, 1 or more"), metavar: Some("LINES"), ..base("risk_large_lines", Int(400), Some("Review"), Number) },
    Setting { help: "more changed lines of code than this make a ready task's risk high whatever it touches (0: never)", least: Some(0), says: Some("a number of lines, or 0 for never"), metavar: Some("LINES"), ..base("risk_huge_lines", Int(1200), Some("Review"), Number) },
    Setting { help: "the paths whose change makes a ready task's risk high or medium, each with why: set with `config --risk-path` (docs/review.md)", none: Some("none"), ..base("risk_paths", Null, Some("Review"), File) },
    Setting { help: "how the board lists tasks", choices: Some(&["grouped", "compact"]), ..base("board_layout", Str("grouped"), Some("Board"), Choice) },
    Setting { help: "how the board draws itself: as text, or as images (kitty graphics; text when the terminal can't)", choices: Some(&["text", "image"]), ..base("board_look", Str("text"), Some("Board"), Choice) },
    Setting { help: "the image look's type size: small (a row a line, as the text), medium or large", choices: Some(&["small", "medium", "large"]), ..base("board_text_size", Str("medium"), Some("Board"), Choice) },
    Setting { help: "a font file (TTF or OTF) the image look draws with (\"\" clears it)", metavar: Some("PATH"), none: Some("Inter, built in"), clears: true, ..base("board_font", Null, Some("Board"), Text) },
    Setting { help: "empty rows at the board's bottom, under Rex's minimized-pane capsules (0: none)", least: Some(0), says: Some("rows, or 0 for none"), metavar: Some("ROWS"), none: Some("1 while workers or reviewers float, else 0"), ..base("board_bottom_gap", Null, Some("Board"), Number) },
    Setting { help: "the board's top line counts the tasks that need you or are ready in other repositories", ..base("board_repos", Bool(true), Some("Board"), OnOff) },
    Setting { help: "how news reaches the coordinator: typed into its pane (type), by a Claude Code hook that wakes it, written to the repo's .claude/settings.local.json (wake), or read by its Monitor tool from `bullpen news --follow` (monitor)", repo_only: true, choices: Some(&["type", "wake", "monitor"]), ..base("news", Str("type"), Some("News"), Choice) },
    Setting { help: "native notifications: when a task needs you (needs), also when one is ready, finished or auto-accepted (all), or none (off)", choices: Some(&NOTIFY), ..base("notify", Str("needs"), Some("News"), Choice) },
    Setting { help: "news when a working agent shows no progress this long (0: never)", least: Some(0), says: Some("minutes, or 0 to turn it off"), metavar: Some("MINUTES"), ..base("stuck_after", Int(20), Some("News"), Number) },
    Setting { help: "how long a task can wait on its coordinator before it needs the person", least: Some(0), says: Some("minutes"), metavar: Some("MINUTES"), ..base("triage_minutes", Int(10), Some("News"), Number) },
    Setting { help: "start backlog items by themselves when there's room (docs/backlog.md)", ..base("pickup", Bool(false), Some("Pickup"), OnOff) },
    Setting { help: "self-started tasks working or in review at once", least: Some(1), says: Some("a number of tasks, 1 or more"), metavar: Some("N"), ..base("pickup_running", Int(1), Some("Pickup"), Number) },
    Setting { help: "pickup pauses while this many self-started tasks wait for you", least: Some(1), says: Some("a number of tasks, 1 or more"), metavar: Some("N"), ..base("pickup_waiting", Int(2), Some("Pickup"), Number) },
    Setting { help: "self-started tasks a day, a spending guard", least: Some(1), says: Some("a number of tasks, 1 or more"), metavar: Some("N"), ..base("pickup_per_day", Int(4), Some("Pickup"), Number) },
    Setting { help: "claude's --model for self-started tasks", none: Some("the worker model"), ..base("pickup_model", Null, Some("Pickup"), Model) },
    Setting { help: "self-started tasks accept by themselves once review approves them", ..base("pickup_auto_accept", Bool(false), Some("Pickup"), OnOff) },
    Setting { help: "read this repository's GitHub issues into the backlog, for the coordinator to judge (docs/issues.md)", ..base("issues", Bool(false), Some("Issues"), OnOff) },
    Setting { help: "the GitHub repository issues are read from, as OWNER/REPO (\"\" clears it)", repo_only: true, metavar: Some("OWNER/REPO"), none: Some("origin's, or in local mode the push remote's"), clears: true, ..base("issues_repo", Null, Some("Issues"), Text) },
    Setting { help: "minutes between the watcher's reads of the GitHub issues", least: Some(1), says: Some("minutes, 1 or more"), metavar: Some("MINUTES"), ..base("issues_every", Int(30), Some("Issues"), Number) },
    Setting { help: "which judged issues start by themselves, through pickup (so only with pickup on): none (off), a trusted author's clear, small, low-risk ones the coordinator scored worth doing (small), or medium-sized ones too (medium)", choices: Some(&ISSUES_START), ..base("issues_start", Str("small"), Some("Issues"), Choice) },
    Setting { help: "what bullpen writes back to an issue: the labels `bullpen: started` and `bullpen: declined` (labels), those and a comment at start and decline (comments), or nothing (off); unless off, an accepted issue's task closes it", choices: Some(&["labels", "comments", "off"]), ..base("issues_write", Str("labels"), Some("Issues"), Choice) },
    Setting { help: "GitHub logins whose issues are trusted besides the gh account's own, separated by commas (\"\" clears it)", metavar: Some("LOGINS"), none: Some("nobody else"), clears: true, ..base("issues_trusted", Null, Some("Issues"), Text) },
    Setting { help: "a label that makes an issue trusted when the gh account's own login added it", metavar: Some("LABEL"), ..base("issues_label", Str("bullpen"), Some("Issues"), Text) },
    Setting { repo_only: true, ..base("coordinators", List(&[]), None, File) },
    Setting { repo_only: true, ..base("next_coordinator", Int(1), None, File) },
];

impl Setting {
    /// Its flag: --max-running for max_running.
    pub fn flag(&self) -> String {
        format!("--{}", self.name.replace('_', "-"))
    }

    pub fn default_value(&self) -> Value {
        self.default.to_value()
    }

    /// The default, as config's help states it: "on", "off", the value, or what no
    /// value means (none).
    pub fn default_phrase(&self) -> Option<String> {
        match self.default {
            Val::Null => self.none.map(String::from),
            Val::Bool(b) => Some(if b { "on" } else { "off" }.into()),
            d => Some(py_str(&d.to_value())),
        }
    }

    /// Its help, with the default: "how accept merges a PR (default rebase)".
    pub fn full_help(&self) -> String {
        match self.default_phrase() {
            Some(phrase) if !phrase.is_empty() => format!("{} (default {phrase})", self.help),
            _ => self.help.into(),
        }
    }

    /// Does text match the setting's pattern (all of it)? True when it has none.
    pub fn matches(&self, text: &str) -> bool {
        self.pattern.is_none_or(|p| pattern_matches(p, text))
    }
}

/// The setting called name (core.SETTING[name]).
pub fn setting(name: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.name == name)
}

/// DEFAULT_CONFIG: every setting's default, in the table's order.
pub fn defaults() -> Obj {
    SETTINGS
        .iter()
        .map(|s| (s.name.to_string(), s.default_value()))
        .collect()
}

/// Does the setting belong to one repository (its repo_only), so the global config.json
/// can't hold it?
pub fn is_repo_only(key: &str) -> bool {
    setting(key).is_some_and(|s| s.repo_only)
}

pub fn is_internal(key: &str) -> bool {
    INTERNAL.contains(&key)
}

/// re.fullmatch(pattern, text), for the patterns the table has: a class of capitals
/// with a count, "[A-Z]{2,5}". Any other pattern matches nothing, so a new one needs
/// a matcher here (a test checks the table has no other).
pub fn pattern_matches(pattern: &str, text: &str) -> bool {
    match pattern {
        "[A-Z]{2,5}" => (2..=5).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_uppercase()),
        _ => false,
    }
}

#[cfg(test)]
#[path = "tests/test_settings.rs"]
mod tests;
