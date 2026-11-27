//! An agent's Claude Code transcript as a chat, for `bullpen web`: finding the file
//! (`find`: the session ID bullpen recorded, else the newest transcript in the agent's
//! folder whose title is the agent's `-n` name) and reading it on from a byte offset
//! to its last whole line (`Transcript::read`), its records made into chat items
//! (items.rs) as docs/prd/web.md's "Transcripts into chat" says.

pub mod items;
pub mod records;

pub use items::{Chat, Item, Kind};

use crate::json::{obj, s};
use crate::Obj;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Whose transcript: a task's worker or reviewer (by the task's ID), or a repo's
/// coordinator (by the repo's name). Each is started with its own `-n` name.
#[derive(Clone, Debug, PartialEq)]
pub enum Agent {
    Worker(String),
    Reviewer(String),
    Coordinator(String),
}

impl Agent {
    /// Whether a transcript's title is this agent's `-n` name: "BP-3: <its title>"
    /// (the task's title may have changed since), "BP-3 review", "bullpen: <repo>".
    pub fn named(&self, title: &str) -> bool {
        match self {
            Agent::Worker(id) => title.starts_with(&format!("{id}: ")),
            Agent::Reviewer(id) => title == format!("{id} review"),
            Agent::Coordinator(repo) => title == format!("bullpen: {repo}"),
        }
    }
}

/// How a transcript was found: by the session ID bullpen recorded, or matched by title.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Recorded,
    Title,
}

impl Source {
    pub fn word(self) -> &'static str {
        match self {
            Source::Recorded => "recorded",
            Source::Title => "title",
        }
    }
}

/// Claude Code's transcripts: `$CLAUDE_CONFIG_DIR/projects`, else `~/.claude/projects`.
pub fn projects() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(d) if !d.is_empty() => PathBuf::from(d).join("projects"),
        _ => PathBuf::from(crate::paths::user_home()).join(".claude/projects"),
    }
}

/// The session ID recorded for a task's agent: the task's `chat` (its worker's) or its
/// review's `chat` (its reviewer's); a coordinator's is in its entry, `chat` too.
pub fn recorded(task: &Obj, reviewer: bool) -> &str {
    if reviewer {
        s(obj(task, "review"), "chat")
    } else {
        s(task, "chat")
    }
}

/// The folder Claude Code keeps a working folder's transcripts in: its path with each
/// character that isn't a letter or digit as `-`.
pub fn folder(projects: &Path, cwd: &Path) -> PathBuf {
    let name: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    projects.join(name)
}

/// An agent's transcript: the recorded session's (`<projects>/*/<id>.jsonl`) when there
/// is one, else the newest in the folder of `cwd` (where the agent runs) whose title
/// names the agent. None when neither is there.
pub fn find(projects: &Path, chat: &str, cwd: &Path, agent: &Agent) -> Option<(PathBuf, Source)> {
    if let Some(path) = by_session(projects, chat) {
        return Some((path, Source::Recorded));
    }
    by_title(&folder(projects, cwd), agent).map(|p| (p, Source::Title))
}

fn by_session(projects: &Path, id: &str) -> Option<PathBuf> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return None; // a session ID is a UUID: nothing else is looked for
    }
    let file = format!("{id}.jsonl");
    let dirs = std::fs::read_dir(projects).ok()?;
    dirs.flatten().map(|d| d.path().join(&file)).find(|p| p.is_file())
}

fn by_title(folder: &Path, agent: &Agent) -> Option<PathBuf> {
    let files = std::fs::read_dir(folder).ok()?.flatten().map(|e| e.path());
    let jsonl = files.filter(|p| p.extension().is_some_and(|e| e == "jsonl"));
    let named = jsonl.filter(|p| title(p).is_some_and(|t| agent.named(&t)));
    named.max_by_key(|p| modified(p))
}

fn modified(p: &Path) -> SystemTime {
    p.metadata()
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// How far into a transcript its title is looked for: Claude Code writes it first.
const TITLE_BYTES: u64 = 64 * 1024;

/// A transcript's title: its first `custom-title` record's, in its first lines.
pub fn title(path: &Path) -> Option<String> {
    let mut head = vec![];
    File::open(path).ok()?.take(TITLE_BYTES).read_to_end(&mut head).ok()?;
    let head = String::from_utf8_lossy(&head);
    head.lines().filter(|l| l.contains("custom-title")).find_map(|l| {
        records::parse(l).into_iter().find_map(|r| match r {
            records::Record::Title(t) => Some(t),
            _ => None,
        })
    })
}

/// The whole lines from `offset` to the last line break, and the offset after it. A
/// line still being written waits for the next read. A file shorter than `offset`
/// (written anew) is read from its start.
pub fn read_from(path: &Path, offset: u64) -> io::Result<(Vec<String>, u64)> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let from = if len < offset { 0 } else { offset };
    f.seek(SeekFrom::Start(from))?;
    let mut bytes = vec![];
    f.take(len - from).read_to_end(&mut bytes)?;
    let Some(end) = bytes.iter().rposition(|&b| b == b'\n') else {
        return Ok((vec![], from));
    };
    let text = String::from_utf8_lossy(&bytes[..end]);
    let lines = text.split('\n').map(String::from).collect();
    Ok((lines, from + end as u64 + 1))
}

/// A transcript being read: its file, how it was found, how far it is read, and its chat.
pub struct Transcript {
    pub path: PathBuf,
    pub source: Source,
    pub offset: u64,
    pub chat: Chat,
}

impl Transcript {
    pub fn new(path: PathBuf, source: Source) -> Transcript {
        Transcript {
            path,
            source,
            offset: 0,
            chat: Chat::new(),
        }
    }

    /// Read on from the offset: the items made or changed by the new lines. A file
    /// that was written anew starts its chat again.
    pub fn read(&mut self) -> io::Result<Vec<Item>> {
        let (lines, offset) = read_from(&self.path, self.offset)?;
        if offset < self.offset {
            self.chat = Chat::new();
        }
        self.offset = offset;
        for line in &lines {
            self.chat.line(line);
        }
        Ok(self.chat.take_changed())
    }
}

#[cfg(test)]
#[path = "tests/test_chat.rs"]
mod tests;
