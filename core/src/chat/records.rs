//! One line of a Claude Code transcript into what it says for the chat (`Record`s), or
//! nothing. A whitelist: bookkeeping records (attachments, API requests, data flow,
//! file history, cost, the per-launch owner records), sidechains, meta notes, thinking
//! and any record type this doesn't know say nothing, and a line that isn't JSON is
//! skipped, never an error. docs/prd/web.md's "Transcripts into chat" has the table.

use serde_json::{Map, Value};

/// What a record says, in the order its content has it. `uuid` and `at` (the record's
/// timestamp) come from the record each part is in.
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    /// The `-n` name (`custom-title`), for the header and for finding a transcript.
    Title(String),
    /// The permission mode (`permission-mode`), for the header.
    Mode(String),
    /// Text a user record carries: what was typed into the pane (by the person, a
    /// coordinator's `tell`, bullpen's news), or a message absorbed mid-turn.
    Typed { uuid: String, at: String, text: String },
    /// A `<task-notification>`: what a background job, a monitor or a Stop hook said.
    Notice { uuid: String, at: String, text: String },
    /// A tool's result: it completes its call.
    Result(Outcome),
    /// The agent's words (an `assistant` text block), with its API message's ID.
    Said {
        uuid: String,
        at: String,
        message: String,
        text: String,
    },
    /// A tool call (an `assistant` tool_use block).
    Call(Call),
    /// A turn ended (`turn_duration`).
    Worked { uuid: String, at: String, ms: u64 },
    /// `compact_boundary`: what came before was summarised for the model.
    Compacted { uuid: String, at: String },
    /// A system record at level `error`.
    Error { uuid: String, at: String, text: String },
    /// `queue-operation` enqueue: something typed while the agent works waits its turn.
    Queued { at: String, text: String },
    /// `queue-operation` remove: what waited is taken back, or taken in mid-turn (a
    /// dequeue says nothing: the user record it leads to has the text).
    Unqueued { text: String },
}

/// A tool call: its ID, the tool's name, its input, and the folder it ran in.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub uuid: String,
    pub at: String,
    pub message: String,
    pub id: String,
    pub name: String,
    pub input: Value,
    pub cwd: String,
}

/// A tool's result: its call's ID, whether it failed, its text, and `toolUseResult`
/// (the tool's own account: stdout and stderr, or an AskUserQuestion's answers).
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub at: String,
    pub id: String,
    pub error: bool,
    pub text: String,
    pub detail: Value,
}

/// One line into its records: none for a line that says nothing to the chat.
pub fn parse(line: &str) -> Vec<Record> {
    match serde_json::from_str::<Value>(line) {
        Ok(Value::Object(r)) => records(&r),
        _ => vec![],
    }
}

fn records(r: &Map<String, Value>) -> Vec<Record> {
    if flag(r, "isSidechain") {
        return vec![]; // a subagent's: they write files of their own
    }
    match str_of(r, "type") {
        "custom-title" => one(Record::Title(str_of(r, "customTitle").into())),
        "permission-mode" => one(Record::Mode(str_of(r, "permissionMode").into())),
        "user" => user(r),
        "assistant" => assistant(r),
        "system" => system(r),
        "queue-operation" => queue(r),
        "attachment" => absorbed(r),
        _ => vec![],
    }
}

fn one(r: Record) -> Vec<Record> {
    vec![r]
}

/// A user record: typed text, a notice, or tool results. Meta notes (image sizes and
/// the like) and the summary after a compaction are hidden.
fn user(r: &Map<String, Value>) -> Vec<Record> {
    if flag(r, "isMeta") || flag(r, "isCompactSummary") || flag(r, "isVisibleInTranscriptOnly") {
        return vec![];
    }
    let (uuid, at) = (str_of(r, "uuid").to_string(), str_of(r, "timestamp").to_string());
    let content = r.get("message").and_then(|m| m.get("content"));
    let mut out = vec![];
    let mut words = vec![];
    match content {
        Some(Value::String(s)) => words.push(s.as_str()),
        Some(Value::Array(blocks)) => {
            for b in blocks {
                match str_at(b, "type") {
                    "text" => words.push(str_at(b, "text")),
                    "tool_result" => out.push(Record::Result(outcome(b, r, &at))),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    let text = words.join("\n");
    if !text.trim().is_empty() {
        out.insert(0, typed(uuid, at, text));
    }
    out
}

/// Typed text, or a notice when it is a `<task-notification>`.
fn typed(uuid: String, at: String, text: String) -> Record {
    if text.trim_start().starts_with("<task-notification>") {
        Record::Notice { uuid, at, text }
    } else {
        Record::Typed { uuid, at, text }
    }
}

fn outcome(b: &Value, r: &Map<String, Value>, at: &str) -> Outcome {
    Outcome {
        at: at.into(),
        id: str_at(b, "tool_use_id").into(),
        error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
        text: block_text(b.get("content")),
        detail: r.get("toolUseResult").cloned().unwrap_or(Value::Null),
    }
}

/// A tool result's content as text: a string, or its text blocks joined (an image
/// block says nothing).
fn block_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|b| str_at(b, "type") == "text")
            .map(|b| str_at(b, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// An assistant record: its text and tool calls. Thinking is hidden (its text is
/// withheld; only a signature is kept).
fn assistant(r: &Map<String, Value>) -> Vec<Record> {
    let (uuid, at) = (str_of(r, "uuid"), str_of(r, "timestamp"));
    let message = r.get("message");
    let id = message.map(|m| str_at(m, "id")).unwrap_or("");
    let blocks = message.and_then(|m| m.get("content")).and_then(Value::as_array);
    let mut out = vec![];
    for b in blocks.into_iter().flatten() {
        match str_at(b, "type") {
            "text" if !str_at(b, "text").trim().is_empty() => out.push(Record::Said {
                uuid: uuid.into(),
                at: at.into(),
                message: id.into(),
                text: str_at(b, "text").into(),
            }),
            "tool_use" => out.push(Record::Call(Call {
                uuid: uuid.into(),
                at: at.into(),
                message: id.into(),
                id: str_at(b, "id").into(),
                name: str_at(b, "name").into(),
                input: b.get("input").cloned().unwrap_or(Value::Null),
                cwd: str_of(r, "cwd").into(),
            })),
            _ => {}
        }
    }
    out
}

/// A system record: a turn's length, a compaction, or an error. The rest (the Stop
/// hook's summary, the away summary, notices, local commands) are hidden.
fn system(r: &Map<String, Value>) -> Vec<Record> {
    let (uuid, at) = (str_of(r, "uuid").to_string(), str_of(r, "timestamp").to_string());
    match str_of(r, "subtype") {
        "turn_duration" => {
            let ms = r.get("durationMs").and_then(Value::as_u64).unwrap_or(0);
            one(Record::Worked { uuid, at, ms })
        }
        "compact_boundary" => one(Record::Compacted { uuid, at }),
        _ if str_of(r, "level") == "error" => one(Record::Error {
            uuid,
            at,
            text: str_of(r, "content").into(),
        }),
        _ => vec![],
    }
}

fn queue(r: &Map<String, Value>) -> Vec<Record> {
    let text = r.get("content").and_then(Value::as_str).map(String::from);
    match (str_of(r, "operation"), text) {
        ("enqueue", Some(text)) => one(Record::Queued {
            at: str_of(r, "timestamp").into(),
            text,
        }),
        ("remove", Some(text)) => one(Record::Unqueued { text }),
        _ => vec![],
    }
}

/// An attachment says nothing, but one: a message typed while the agent worked and
/// taken in mid-turn (`queued_command`) reaches the model as an attachment, not as a
/// user record, and is what was typed all the same.
fn absorbed(r: &Map<String, Value>) -> Vec<Record> {
    let a = r.get("attachment");
    if a.map(|a| str_at(a, "type")) != Some("queued_command") {
        return vec![];
    }
    let text = a.map(|a| str_at(a, "prompt")).unwrap_or("").to_string();
    if text.trim().is_empty() {
        return vec![];
    }
    one(typed(str_of(r, "uuid").into(), str_of(r, "timestamp").into(), text))
}

fn flag(r: &Map<String, Value>, key: &str) -> bool {
    r.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn str_of<'a>(r: &'a Map<String, Value>, key: &str) -> &'a str {
    r.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The string at key in v, or "".
pub(crate) fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
#[path = "tests/test_records.rs"]
mod tests;
