//! Records into chat items (`Chat`): the brief, your messages, the coordinator's, news
//! lines, what the agent said, its tool calls grouped into one line a run, questions
//! (AskUserQuestion), dividers and errors. Fed a transcript's lines in order, it keeps
//! what it made and which items are new or changed since it was last asked
//! (`take_changed`), so a file read on from its offset sends only those. An item keeps
//! its `id` as it changes (a tool line whose call finished, a question answered, a
//! queued message taken in).

use super::records::{self, str_at, Call, Outcome, Record};
use crate::timeutil;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};

/// A chat item: its ID (the record it started from, or its first tool call's), the
/// time it started, and what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub at: String,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// The first message, bullpen's "You are the …", and the brief it names, if any.
    Brief {
        text: String,
        file: Option<String>,
    },
    /// What the person typed.
    You {
        text: String,
    },
    /// What a coordinator typed (`Coordinator: ` taken off).
    Coordinator {
        text: String,
    },
    /// A news line (`bullpen · ` taken off), and the task it names first.
    News {
        text: String,
        task: Option<String>,
    },
    /// The agent's words, as Markdown.
    Said {
        text: String,
    },
    /// A run of tool calls between two things said, and when its last result came.
    Tools {
        calls: Vec<ToolCall>,
        until: String,
    },
    /// An AskUserQuestion: its questions (the tool's input) and, once it has its
    /// result, the answers chosen or the error.
    Question {
        questions: Value,
        answers: Option<Value>,
        error: Option<String>,
    },
    /// A turn ended, after this long.
    Worked {
        ms: u64,
    },
    Compacted,
    /// The person pressed esc: Claude Code's "[Request interrupted by user]".
    Interrupted,
    Error {
        text: String,
    },
    /// Typed while the agent worked, waiting its turn; `gone` once taken in or taken back.
    Queued {
        text: String,
        gone: bool,
    },
}

/// One call in a tool line: its tool, what it did in a few words, and how it went.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub what: String,
    /// What its line says while it runs: "Running python3 tests/run.py".
    pub doing: String,
    pub state: State,
    /// A failed call's first lines, and the end of its output.
    pub error: Option<String>,
    pub tail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Running,
    Done,
    Failed,
}

/// A transcript's chat so far, and its header's title and permission mode.
#[derive(Default)]
pub struct Chat {
    items: Vec<Item>,
    /// A call's ID to its item, and its place in a tool line (None: a question).
    calls: HashMap<String, (usize, Option<usize>)>,
    /// The tool line still open: calls join it until something else is said.
    run: Option<usize>,
    /// The last item said, and its API message: its next text block joins it.
    said: Option<(usize, String)>,
    seen: HashSet<String>,
    /// Queued items not yet gone, oldest first.
    queued: Vec<usize>,
    changed: BTreeSet<usize>,
    started: bool,
    pub title: Option<String>,
    pub mode: Option<String>,
}

const NEWS: &str = "bullpen · ";
const COORDINATOR: &str = "Coordinator: ";
/// bullpen's first message to every agent it starts begins so.
const OPENING: &str = "You are the ";
/// What Claude Code writes as a user's message when the person presses esc ("… for
/// tool use" when it stopped a call).
const INTERRUPTED: &str = "[Request interrupted by user";
/// Typed text that is Claude Code's, not anyone's message: slash commands and their
/// output, `!` commands, a subagent's report, reminders, notifications (a notice is
/// read for its news; one queued waits unseen).
const HIDDEN_TAGS: [&str; 7] = [
    "<agent-message",
    "<command-",
    "<local-command-",
    "<bash-",
    "<system-reminder>",
    "<user-prompt-submit-hook>",
    "<task-notification>",
];
/// How much of a failed call's output an item keeps, and of what a call did.
const ERROR_LINES: usize = 3;
const TAIL_LINES: usize = 10;
const WHAT_CHARS: usize = 200;

impl Chat {
    pub fn new() -> Chat {
        Chat::default()
    }

    /// Every item, oldest first; queued ones that are gone are left out.
    pub fn items(&self) -> Vec<&Item> {
        self.items
            .iter()
            .filter(|i| !matches!(i.kind, Kind::Queued { gone: true, .. }))
            .collect()
    }

    /// The items made or changed since the last call, oldest first.
    pub fn take_changed(&mut self) -> Vec<Item> {
        let changed = std::mem::take(&mut self.changed);
        changed.into_iter().map(|i| self.items[i].clone()).collect()
    }

    /// Feed a transcript's line.
    pub fn line(&mut self, line: &str) {
        for r in records::parse(line) {
            self.record(r);
        }
    }

    pub fn record(&mut self, r: Record) {
        match r {
            Record::Title(t) => self.title = Some(t),
            Record::Mode(m) => self.mode = Some(m),
            Record::Typed { uuid, at, text } => self.typed(uuid, at, text),
            Record::Notice { uuid, at, text } => self.notice(uuid, at, &text),
            Record::Result(o) => self.result(o),
            Record::Said {
                uuid,
                at,
                message,
                text,
            } => self.said(uuid, at, message, text),
            Record::Call(c) => self.call(c),
            Record::Worked { uuid, at, ms } => _ = self.push(uuid, at, Kind::Worked { ms }),
            Record::Compacted { uuid, at } => _ = self.push(uuid, at, Kind::Compacted),
            Record::Error { uuid, at, text } => _ = self.push(uuid, at, Kind::Error { text }),
            Record::Queued { at, text } => self.queue(at, text),
            Record::Unqueued { text } => self.unqueue(&text),
        }
    }

    /// Add an item; it closes the tool line before it. A record seen already (by its
    /// ID) adds nothing.
    fn push(&mut self, id: String, at: String, kind: Kind) -> Option<usize> {
        if !id.is_empty() && !self.seen.insert(id.clone()) {
            return None;
        }
        self.run = None;
        self.said = None;
        self.items.push(Item { id, at, kind });
        let i = self.items.len() - 1;
        self.changed.insert(i);
        Some(i)
    }

    fn typed(&mut self, uuid: String, at: String, text: String) {
        self.unqueue(&text);
        if let Some(kind) = typed_kind(&text, !self.started) {
            self.started = true;
            self.push(uuid, at, kind);
        }
    }

    /// A notice says nothing but for the news lines in it (news by monitor, or handed
    /// to a coordinator by its Stop hook): an item each.
    fn notice(&mut self, uuid: String, at: String, text: &str) {
        self.unqueue(text);
        for (n, line) in news_in(text).into_iter().enumerate() {
            let id = if n == 0 { uuid.clone() } else { format!("{uuid}.{n}") };
            self.push(id, at.clone(), news(&line));
        }
    }

    /// The agent's words: an item a message, its text blocks joined.
    fn said(&mut self, uuid: String, at: String, message: String, text: String) {
        if let Some((i, m)) = &self.said {
            if *m == message && !message.is_empty() {
                let i = *i;
                if let Kind::Said { text: was } = &mut self.items[i].kind {
                    was.push_str("\n\n");
                    was.push_str(&text);
                }
                self.changed.insert(i);
                return;
            }
        }
        if let Some(i) = self.push(uuid, at, Kind::Said { text }) {
            self.said = Some((i, message));
        }
    }

    /// A tool call joins the open tool line, or starts one; an AskUserQuestion is an
    /// item of its own.
    fn call(&mut self, c: Call) {
        if c.id.is_empty() || self.calls.contains_key(&c.id) {
            return;
        }
        if c.name == "AskUserQuestion" {
            let questions = c.input.get("questions").cloned().unwrap_or(Value::Null);
            let kind = Kind::Question {
                questions,
                answers: None,
                error: None,
            };
            if let Some(i) = self.push(c.id.clone(), c.at, kind) {
                self.calls.insert(c.id, (i, None));
            }
            return;
        }
        let what = what(&c);
        let call = ToolCall {
            id: c.id.clone(),
            name: c.name.clone(),
            doing: doing(&c, &what),
            what,
            state: State::Running,
            error: None,
            tail: None,
        };
        let i = match self.run {
            Some(i) => i,
            None => {
                let kind = Kind::Tools {
                    calls: vec![],
                    until: c.at.clone(),
                };
                let Some(i) = self.push(c.id.clone(), c.at, kind) else {
                    return;
                };
                self.run = Some(i);
                i
            }
        };
        if let Kind::Tools { calls, .. } = &mut self.items[i].kind {
            calls.push(call);
            self.calls.insert(c.id, (i, Some(calls.len() - 1)));
        }
        self.changed.insert(i);
    }

    /// A result completes its call; one whose call isn't in this chat is dropped.
    fn result(&mut self, o: Outcome) {
        let Some(&(i, place)) = self.calls.get(&o.id) else {
            return;
        };
        let error = o.error.then(|| first_lines(&untagged(&o.text), ERROR_LINES));
        match (&mut self.items[i].kind, place) {
            (Kind::Question { answers, error: e, .. }, None) => {
                *answers = o.detail.get("answers").cloned();
                *e = error;
            }
            (Kind::Tools { calls, until }, Some(k)) => {
                let call = &mut calls[k];
                call.state = if o.error { State::Failed } else { State::Done };
                call.tail = if o.error { tail(&o) } else { None };
                call.error = error;
                *until = o.at;
            }
            _ => return,
        }
        self.changed.insert(i);
    }

    /// Something typed while the agent works: a faint bubble until it is taken in.
    /// Notifications and news wait unseen.
    fn queue(&mut self, at: String, text: String) {
        if !matches!(
            typed_kind(&text, false),
            Some(Kind::You { .. } | Kind::Coordinator { .. })
        ) {
            return;
        }
        let id = format!("queued-{}", self.items.len());
        let (run, said) = (self.run, self.said.clone());
        if let Some(i) = self.push(id, at, Kind::Queued { text, gone: false }) {
            self.queued.push(i);
        }
        (self.run, self.said) = (run, said); // the agent's line goes on past it
    }

    /// The queued item with this text is gone: taken in, or taken back.
    fn unqueue(&mut self, text: &str) {
        let at = self.queued.iter().position(|&i| match &self.items[i].kind {
            Kind::Queued { text: t, .. } => t.trim() == text.trim(),
            _ => false,
        });
        if let Some(n) = at {
            let i = self.queued.remove(n);
            if let Kind::Queued { gone, .. } = &mut self.items[i].kind {
                *gone = true;
            }
            self.changed.insert(i);
        }
    }
}

/// What typed text is in the chat: None for Claude Code's own.
fn typed_kind(text: &str, first: bool) -> Option<Kind> {
    let t = text.trim_start();
    if HIDDEN_TAGS.iter().any(|tag| t.starts_with(tag)) {
        return None;
    }
    if t.starts_with(INTERRUPTED) {
        return Some(Kind::Interrupted);
    }
    if t.starts_with(NEWS) {
        return Some(news(t));
    }
    if let Some(rest) = t.strip_prefix(COORDINATOR) {
        return Some(Kind::Coordinator { text: rest.into() });
    }
    if first && t.starts_with(OPENING) {
        return Some(Kind::Brief {
            text: text.into(),
            file: brief_file(text),
        });
    }
    Some(Kind::You { text: text.into() })
}

fn news(line: &str) -> Kind {
    let text = line.trim().strip_prefix(NEWS).unwrap_or(line.trim()).to_string();
    Kind::News {
        task: task_named(text.lines().next().unwrap_or("")),
        text,
    }
}

/// The news in a notice: a monitor's `<event>…</event>`, or what a Stop hook hands a
/// coordinator after "Stop hook blocking error from command …: ". In either, a news
/// line starts where `bullpen · ` opens a line, and the lines after it, up to the
/// next, are its own: once a diff starts (a guide change's), every line up to the
/// block's end is the diff's, whatever it starts with.
fn news_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    // in a block; its news line is the last one out; that line's diff has started
    let (mut open, mut mine, mut diff) = (false, false, false);
    for raw in text.lines() {
        let mut line = raw;
        if let Some(rest) = block_start(line) {
            (open, mine, diff, line) = (true, false, false, rest);
        } else if !open {
            continue;
        }
        let (line, closes) = match BLOCK_ENDS.iter().find_map(|end| line.find(end)) {
            Some(p) => (&line[..p], true),
            None => (line, false),
        };
        if !diff && line.starts_with(NEWS) {
            out.push(line.to_string());
            mine = true;
        } else if let Some(last) = out.last_mut().filter(|_| mine) {
            diff |= ["--- ", "+++ ", "@@ "].iter().any(|d| line.starts_with(d));
            last.push('\n');
            last.push_str(line);
        }
        open &= !closes;
    }
    out.iter().map(|n| unescape(n.trim_end())).collect()
}

/// Where a block of news starts on this line: what follows `<event>`, or the Stop
/// hook's "…from command "Stop": ".
fn block_start(line: &str) -> Option<&str> {
    if let Some(p) = line.find("<event>") {
        return Some(&line[p + "<event>".len()..]);
    }
    let p = line.find(STOP_HOOK)?;
    let after = &line[p + STOP_HOOK.len()..];
    after.find(": ").map(|q| &after[q + 2..])
}

const STOP_HOOK: &str = "Stop hook blocking error from command ";
const BLOCK_ENDS: [&str; 3] = ["</event>", "</system-reminder>", "</task-notification>"];

/// A notification's text with its XML escapes undone.
fn unescape(s: &str) -> String {
    let s = s
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    s.replace("&apos;", "'").replace("&amp;", "&")
}

/// The first task ID a news line names (`BP-12`): capitals, a hyphen, digits.
pub fn task_named(text: &str) -> Option<String> {
    let words = text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'));
    words
        .map(|w| w.trim_matches('-'))
        .find(|w| is_task_id(w))
        .map(String::from)
}

fn is_task_id(w: &str) -> bool {
    let Some((pre, num)) = w.split_once('-') else {
        return false;
    };
    !pre.is_empty()
        && pre.chars().all(|c| c.is_ascii_uppercase())
        && !num.is_empty()
        && num.chars().all(|c| c.is_ascii_digit())
}

/// The brief a first message names: the last part of its first `.md` path.
fn brief_file(text: &str) -> Option<String> {
    let words = text
        .split_whitespace()
        .map(|w| w.trim_end_matches(|c: char| ",.;:)`'\"".contains(c)));
    let path = words.into_iter().find(|w| w.ends_with(".md"))?;
    Some(path.rsplit('/').next().unwrap_or(path).to_string())
}

/// A call in a few words: Bash's description, else its command's first line; a file's
/// path from the folder it ran in; an agent's description; a search's pattern.
fn what(c: &Call) -> String {
    let i = &c.input;
    let w = match c.name.as_str() {
        "Bash" => match str_at(i, "description") {
            "" => str_at(i, "command").lines().next().unwrap_or("").to_string(),
            d => d.to_string(),
        },
        "Read" | "Write" | "Edit" | "MultiEdit" => relative(str_at(i, "file_path"), &c.cwd),
        "NotebookEdit" => relative(str_at(i, "notebook_path"), &c.cwd),
        "Agent" | "Task" => str_at(i, "description").to_string(),
        "Grep" | "Glob" => str_at(i, "pattern").to_string(),
        "WebFetch" => str_at(i, "url").to_string(),
        "WebSearch" | "ToolSearch" => str_at(i, "query").to_string(),
        "Skill" => str_at(i, "skill").to_string(),
        _ => String::new(),
    };
    cut(w.trim(), WHAT_CHARS)
}

fn relative(path: &str, cwd: &str) -> String {
    let base = format!("{}/", cwd.trim_end_matches('/'));
    match path.strip_prefix(&base) {
        Some(rest) if !cwd.is_empty() => rest.to_string(),
        _ => path.to_string(),
    }
}

fn cut(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s.to_string(),
    }
}

/// An error without the tags Claude Code wraps some in.
fn untagged(text: &str) -> String {
    text.replace("<tool_use_error>", "").replace("</tool_use_error>", "")
}

fn first_lines(text: &str, n: usize) -> String {
    text.trim().lines().take(n).collect::<Vec<_>>().join("\n")
}

/// The end of a failed call's output: its stdout and stderr when the tool gave them,
/// else what its result says after the error's first lines. None when there's nothing more.
fn tail(o: &Outcome) -> Option<String> {
    let out = match (o.detail.get("stdout"), o.detail.get("stderr")) {
        (None, None) => o.text.trim().lines().skip(ERROR_LINES).collect::<Vec<_>>().join("\n"),
        _ => [str_at(&o.detail, "stdout").trim(), str_at(&o.detail, "stderr").trim()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("\n"),
    };
    let lines: Vec<&str> = out.lines().collect();
    let end = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
    (!end.trim().is_empty()).then_some(end)
}

// ---------- in words, and as JSON for the page ----------

/// A tool line in words: "Ran 5 commands, read 2 files".
pub fn summary(calls: &[ToolCall]) -> String {
    let mut parts: Vec<(&str, &str, &str, usize)> = vec![];
    for c in calls {
        let (verb, one, many) = tally(&c.name);
        match parts.iter_mut().find(|p| p.0 == verb && p.1 == one) {
            Some(p) => p.3 += 1,
            None => parts.push((verb, one, many, 1)),
        }
    }
    let words: Vec<String> = parts
        .iter()
        .map(|(v, one, many, n)| format!("{v} {n} {}", if *n == 1 { one } else { many }))
        .collect();
    crate::text::upper_first(&words.join(", "))
}

fn tally(tool: &str) -> (&'static str, &'static str, &'static str) {
    match tool {
        "Bash" => ("ran", "command", "commands"),
        "Read" => ("read", "file", "files"),
        "Write" => ("wrote", "file", "files"),
        "Edit" | "MultiEdit" | "NotebookEdit" => ("edited", "file", "files"),
        "Grep" | "Glob" => ("searched", "time", "times"),
        "Agent" | "Task" => ("ran", "agent", "agents"),
        "WebFetch" => ("fetched", "page", "pages"),
        "WebSearch" => ("searched the web", "time", "times"),
        _ => ("used", "tool", "tools"),
    }
}

/// What a call's line says while it runs: Bash's description as it is, else the
/// call's words after a verb: "Running python3 tests/run.py", "Reading cli/src/main.rs".
fn doing(c: &Call, what: &str) -> String {
    if c.name == "Bash" && !str_at(&c.input, "description").is_empty() {
        return what.to_string();
    }
    let verb = match c.name.as_str() {
        "Bash" | "Agent" | "Task" => "Running",
        "Read" => "Reading",
        "Write" => "Writing",
        "Edit" | "MultiEdit" | "NotebookEdit" => "Editing",
        "Grep" | "Glob" | "WebSearch" => "Searching",
        "WebFetch" => "Fetching",
        _ => "Using",
    };
    match what {
        "" => format!("{verb} {}", c.name),
        w => format!("{verb} {w}"),
    }
}

/// What a tool line says while a call in it runs (the latest one, when calls run side
/// by side), or None once they're all done.
pub fn running(calls: &[ToolCall]) -> Option<&str> {
    calls
        .iter()
        .rev()
        .find(|c| c.state == State::Running)
        .map(|c| c.doing.as_str())
}

/// A length of time in words: "26 s", "4 min 38 s", "1 h 29 min".
pub fn span(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    match (h, m) {
        (0, 0) => format!("{s} s"),
        (0, m) if m < 10 && s > 0 => format!("{m} min {s} s"),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// From one stamp to another, in ms (0 if either doesn't parse).
fn ms_between(from: &str, to: &str) -> u64 {
    match (timeutil::parse(from), timeutil::parse(to)) {
        (Some(a), Some(b)) => (b - a).num_milliseconds().max(0) as u64,
        _ => 0,
    }
}

/// Typed text in parts: the words, and each `<pasted_content>` in it folded to its
/// line count.
pub fn parts(text: &str) -> Vec<Value> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(p) = rest.find("<pasted_content") {
        push_words(&mut out, &rest[..p]);
        let after = &rest[p..];
        let body_at = after.find('>').map(|i| i + 1).unwrap_or(after.len());
        let (body, next) = match after.find("</pasted_content>") {
            Some(e) => (&after[body_at.min(e)..e], &after[e + "</pasted_content>".len()..]),
            None => (&after[body_at..], ""),
        };
        let body = body.trim_matches('\n');
        out.push(json!({"pasted": body, "lines": body.lines().count()}));
        rest = next;
    }
    push_words(&mut out, rest);
    out
}

fn push_words(out: &mut Vec<Value>, words: &str) {
    if !words.trim().is_empty() {
        out.push(json!({"text": words.trim()}));
    }
}

impl Item {
    /// The item as the page gets it: `id`, `at`, `kind`, and the kind's own fields.
    pub fn json(&self) -> Value {
        let mut v = match &self.kind {
            Kind::Brief { text, file } => json!({"kind": "brief", "text": text, "file": file}),
            Kind::You { text } => json!({"kind": "you", "text": text, "parts": parts(text)}),
            Kind::Coordinator { text } => json!({"kind": "coordinator", "text": text, "parts": parts(text)}),
            Kind::News { text, task } => json!({"kind": "news", "text": text, "task": task}),
            Kind::Said { text } => json!({"kind": "said", "text": text}),
            Kind::Tools { calls, until } => json!({
                "kind": "tools",
                "summary": summary(calls),
                "doing": running(calls),
                "ms": ms_between(&self.at, until),
                "calls": calls.iter().map(ToolCall::json).collect::<Vec<_>>(),
            }),
            Kind::Question {
                questions,
                answers,
                error,
            } => {
                json!({"kind": "question", "questions": questions, "answers": answers, "error": error})
            }
            Kind::Worked { ms } => json!({"kind": "worked", "ms": ms, "text": format!("Worked for {}", span(*ms))}),
            Kind::Compacted => json!({"kind": "compacted", "text": "Conversation compacted"}),
            Kind::Interrupted => json!({"kind": "interrupted", "text": "Interrupted"}),
            Kind::Error { text } => json!({"kind": "error", "text": text}),
            Kind::Queued { text, gone } => json!({"kind": "queued", "text": text, "gone": gone}),
        };
        v["id"] = json!(self.id);
        v["at"] = json!(self.at);
        v
    }
}

impl ToolCall {
    fn json(&self) -> Value {
        let state = match self.state {
            State::Running => "running",
            State::Done => "done",
            State::Failed => "failed",
        };
        json!({
            "id": self.id,
            "name": self.name,
            "what": self.what,
            "doing": self.doing,
            "state": state,
            "error": self.error,
            "tail": self.tail,
        })
    }
}

#[cfg(test)]
#[path = "tests/test_items.rs"]
mod tests;
