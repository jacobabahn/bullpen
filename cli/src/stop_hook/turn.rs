//! The worker's turn record: `_worker_stop` writes it from the Stop hook's input, and the
//! worker's end of turn (events/) reads it.
//!
//! Claude Code 2.1.326 gives a Stop hook, on its stdin, JSON with `session_id`,
//! `last_assistant_message` (the text of the turn's last reply) and `background_tasks`:
//! each `{"id", "type", "status", "description", ...}`. A `run_in_background` Bash job
//! and a Monitor are `"type": "shell"` with a `command` (both seen in a scratch session;
//! the footer says "1 shell", "1 monitor"). A background subagent is `"type":
//! "subagent"`, and Claude Code lists kinds of its own there too: only shells count, the
//! ones the worker started and the footer counts (screen/). It also gives
//! `transcript_path`, the conversation's file. The record keeps
//! `{"at", "session", "transcript", "jobs": [{"command", "description"}], "said"}`, for
//! the worker's turns only (whose.rs).

use bullpen_core::json::{obj, s, write_obj, Obj};
use bullpen_core::task::clip;
use bullpen_core::timeutil::{iso_now, iso_now_micro, seconds_since};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;

/// The most of the worker's last reply the record keeps: its end.
const SAID_MAX: usize = 2000;
/// The most of its input the hook reads.
const INPUT_MAX: u64 = 4 << 20;
/// How long the question in a triage line may be.
const QUESTION_MAX: usize = 160;

/// `_worker_stop RECORD` (the worker's Stop hook): write the turn record from the hook's
/// input, when the turn was the worker's. Fast, starts nothing, and exits 0 whatever happens: it must never block or fail
/// the worker's turn.
pub fn cmd_worker_stop(argv: &[String]) -> Res<i32> {
    let _ = std::panic::catch_unwind(|| {
        if let Some(path) = argv.first() {
            let mut input = String::new();
            let _ = std::io::stdin().take(INPUT_MAX).read_to_string(&mut input);
            if let Ok(Value::Object(said)) = serde_json::from_str::<Value>(&input) {
                // the reviewer runs this hook too: its turn moves only its review's `chat`
                if super::whose::settle(Path::new(path), &said).is_some_and(|who| who.writes_record()) {
                    let _ = write_obj(Path::new(path), &record(&said)); // by rename
                }
            }
        }
    });
    Ok(0)
}

/// The record for a Stop hook's input.
pub fn record(input: &Obj) -> Obj {
    let jobs: Vec<Value> = input
        .get("background_tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter(|j| j.get("status").and_then(Value::as_str).is_none_or(|st| st == "running"))
        .filter(|j| j.get("type").and_then(Value::as_str) == Some("shell"))
        .map(|j| json!({"command": s(j, "command"), "description": s(j, "description")}))
        .collect();
    let said = s(input, "last_assistant_message");
    let n = said.chars().count();
    let said: String = said.chars().skip(n.saturating_sub(SAID_MAX)).collect();
    let mut out = Obj::new();
    out.insert("at".into(), json!(iso_now_micro()));
    out.insert("session".into(), json!(s(input, "session_id")));
    out.insert("transcript".into(), json!(s(input, "transcript_path")));
    out.insert("jobs".into(), json!(jobs));
    out.insert("said".into(), json!(said));
    out
}

/// The task's turn record, if the hook wrote it in the turn that just ended: not before
/// the turn began (`turn_at`, set when its first working event came).
pub fn this_turn(repo: &Repo, t: &Obj) -> Option<Obj> {
    let began = t.get("turn_at").and_then(Value::as_str)?;
    let rec = bullpen_core::json::read_obj(&super::record_path(repo, s(t, "id"))).ok()??;
    let at = rec.get("at").and_then(Value::as_str)?;
    (seconds_since(at, None) <= seconds_since(began, None)).then_some(rec)
}

/// The record's running jobs, in words: "`cargo test`", "`cargo test` and 1 more". None
/// with none.
pub fn jobs_phrase(rec: &Obj) -> Option<String> {
    let jobs = rec.get("jobs").and_then(Value::as_array)?;
    let first = jobs.first().and_then(Value::as_object)?;
    let name = [s(first, "command"), s(first, "description")]
        .into_iter()
        .map(one_line)
        .find(|n| !n.is_empty())
        .unwrap_or_else(|| "a background job".into());
    let more = match jobs.len() - 1 {
        0 => String::new(),
        n => format!(" and {n} more"),
    };
    Some(format!("{}{more}", clip(&name, 60)))
}

/// Start waiting on its job: the task's `job`, from the record (else unnamed, when only
/// the screen says a shell runs). A wait already begun keeps its start.
pub fn wait_on_job(t: &mut Obj, rec: Option<&Obj>) {
    let since = match obj(t, "job").get("since") {
        Some(v) => v.clone(),
        None => json!(iso_now()),
    };
    let what = rec.and_then(jobs_phrase);
    t.insert("job".into(), json!({"what": what, "since": since}));
}

/// What the task waits on, in words, if it waits on its job.
pub fn job_what(t: &Obj) -> Option<String> {
    let job = t.get("job").and_then(Value::as_object)?;
    Some(job.get("what").and_then(Value::as_str).unwrap_or("").to_string())
}

/// Text a worker wrote, fit for one news line: control characters gone, on one line,
/// backticks as quotes (a command in a line is bullpen's, never the worker's), and double
/// quotes single (the line quotes it).
pub fn one_line(text: &str) -> String {
    let plain: String = text
        .chars()
        .map(|c| match c {
            c if c.is_control() => ' ',
            '`' | '"' | '“' | '”' => '\'',
            c => c,
        })
        .collect();
    bullpen_core::text::squash(&plain)
}

/// The question the worker ended its reply with, if it asked one: the last sentence
/// ending in `?` among its reply's last lines, clipped to one line.
pub fn question(said: &str) -> Option<String> {
    let lines: Vec<&str> = said.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let line = lines.iter().rev().take(8).find(|l| l.contains('?'))?;
    let line = line.trim_start_matches(['#', '>', '*', '-', ' ']);
    let end = line.rfind('?')?;
    let upto = &line[..=end];
    // back to the sentence's start: after the last `. `, `! ` or `? ` before it
    let start = [". ", "! ", "? "]
        .iter()
        .filter_map(|p| upto[..end].rfind(p).map(|i| i + p.len()))
        .max()
        .unwrap_or(0);
    let q = one_line(upto[start..].trim_start_matches(['*', '_', ' ']));
    let q = q.trim_end_matches(['*', '_']).to_string();
    (q.chars().count() > 1).then(|| clip(&q, QUESTION_MAX))
}

/// The triage line for a worker that ended its turn without `ready`. rec: its turn's
/// record; shell_min: `check` found it idle this long with a background shell running;
/// job_gone: it waited on its job, the job ended, and it hasn't taken a turn since.
pub fn ended_line(id: &str, t: &Obj, rec: Option<&Obj>, shell_min: Option<i64>, job_gone: bool) -> String {
    let peek = format!("`bullpen peek {id}`");
    let job = job_what(t).filter(|w| !w.is_empty());
    if let Some(m) = shell_min {
        return match job {
            Some(w) => format!(
                "{id} ended its turn without `ready` {m} min ago, and still waits on its job ({w}): read it with {peek}"
            ),
            None => format!(
                "{id} ended its turn without `ready` {m} min ago, and a background shell of its own still runs \
                 (a server, a watch?): read it with {peek}"
            ),
        };
    }
    if job_gone {
        let named = job.map_or(String::new(), |w| format!(" ({w})"));
        return format!(
            "{id} waited on its job{named}, which has ended, but hasn't taken a turn since: read it with {peek}"
        );
    }
    if let Some(q) = rec.and_then(|r| question(s(r, "said"))) {
        return format!("{id} ended its turn with a question: \"{q}\": {peek}");
    }
    format!("{id} ended its turn without `ready`: read it with {peek}")
}

#[cfg(test)]
#[path = "tests/test_turn.rs"]
mod tests;
