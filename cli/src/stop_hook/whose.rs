//! Whose turn ended, for the worker's Stop hook: the reviewer starts in the same
//! worktree, so it runs the hook too. A session ID that is the task's `chat` is the
//! worker's, one that is its review's `chat` the reviewer's; an unknown one (a `/clear`
//! starts a new conversation, and an agent started before bullpen recorded its `chat`
//! has none) is told by its transcript's `custom-title`, the `-n` name it started with
//! (`BP-3: <title>` the worker, `BP-3 review` the reviewer), and that agent's `chat`
//! follows it (chat_ids.rs). One titled for neither is nobody's here. One with no title
//! to read is untold: its turn record is written, as every Stop's was before the
//! reviewer was told apart, and no `chat` moves on a guess.

use bullpen_core::json::{obj, read_obj, s, write_obj, Obj};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Whose turn it was.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Agent {
    Worker,
    Reviewer,
    /// an unknown session with no title to read
    Untold,
}

impl Agent {
    /// Does the task's turn record take this turn? The worker's, and an untold one.
    pub fn writes_record(self) -> bool {
        self != Agent::Reviewer
    }
}

/// The most of a transcript read for its title: it is among the first records.
const TITLE_WITHIN: u64 = 64 << 10;

/// Whose turn of task id (t) the Stop hook's input is, if anyone's here. title: the
/// transcript's `custom-title`, read only when the session ID doesn't tell.
pub fn whose(t: &Obj, id: &str, session: &str, title: impl FnOnce() -> Option<String>) -> Option<Agent> {
    if !session.is_empty() && session == s(t, "chat") {
        return Some(Agent::Worker);
    }
    if !session.is_empty() && session == s(obj(t, "review"), "chat") {
        return Some(Agent::Reviewer);
    }
    match title() {
        None => Some(Agent::Untold),
        Some(name) if name == format!("{id} review") => Some(Agent::Reviewer),
        Some(name) if name.starts_with(&format!("{id}: ")) => Some(Agent::Worker),
        Some(_) => None,
    }
}

/// The `custom-title` a transcript starts with, if it has one to read.
pub fn title(transcript: &Path) -> Option<String> {
    let mut head = String::new();
    std::fs::File::open(transcript)
        .ok()?
        .take(TITLE_WITHIN)
        .read_to_string(&mut head)
        .ok()?;
    head.lines()
        .filter(|l| l.contains("\"custom-title\""))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|r| r.get("type").and_then(Value::as_str) == Some("custom-title"))
        .and_then(|r| r.get("customTitle").and_then(Value::as_str).map(str::to_string))
}

/// Move the agent's `chat` to session, if it is somewhere else. True if the task changed.
pub fn follow(t: &mut Obj, who: Agent, session: &str) -> bool {
    if session.is_empty() {
        return false;
    }
    let holder = match who {
        Agent::Worker => Some(t),
        Agent::Reviewer => t.get_mut("review").and_then(Value::as_object_mut),
        Agent::Untold => None,
    };
    match holder {
        Some(h) if s(h, "chat") != session => {
            h.insert("chat".into(), json!(session));
            true
        }
        _ => false,
    }
}

/// The task file beside the turn record (`<state>/turns/BP-3.json`), and its ID.
fn task_file(record: &Path) -> Option<(PathBuf, String)> {
    let id = record.file_stem()?.to_str()?.to_string();
    let dir = record.parent()?.parent()?;
    Some((dir.join("tasks").join(format!("{id}.json")), id))
}

/// For the hook: whose turn the input is, moving that agent's `chat` in the task's file
/// when its session moved. A task bullpen can't read leaves it untold.
pub fn settle(record: &Path, input: &Obj) -> Option<Agent> {
    let Some((path, id)) = task_file(record) else {
        return Some(Agent::Untold);
    };
    let Ok(Some(mut t)) = read_obj(&path) else {
        return Some(Agent::Untold);
    };
    let session = s(input, "session_id");
    let transcript = s(input, "transcript_path");
    let who = whose(&t, &id, session, || match transcript {
        "" => None,
        p => title(Path::new(p)),
    })?;
    if follow(&mut t, who, session) {
        // as it is now, read just above: only `chat` changes (by rename)
        let _ = write_obj(&path, &t);
    }
    Some(who)
}

#[cfg(test)]
#[path = "tests/test_whose.rs"]
mod tests;
