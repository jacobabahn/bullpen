//! The slow commands the board runs in the background, the quick ones it waits for,
//! and reading a worker's screen for a peek or an answer.

use super::{App, Mode, Msg, Note};
use crate::model::{Action, Prompt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How often a peek reads the worker's screen again.
const PEEK_EVERY: Duration = Duration::from_secs(1);

/// A slow command, run in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Accept,
    Close,
    Drop,
    DropForce,
    Start,
    Resume,
    /// a backlog item made a task and started (`later L7 --start`)
    StartItem,
}

impl Job {
    pub(super) fn args(self, id: &str) -> Vec<String> {
        let a: &[&str] = match self {
            Job::Accept => &["accept", id],
            Job::Close => &["accept", id], // accept closes a finished task
            Job::Drop => &["drop", id],
            Job::DropForce => &["drop", id, "--force"],
            Job::Start => &["start", id],
            Job::Resume => &["resume", id],
            Job::StartItem => &["later", id, "--start"],
        };
        a.iter().map(|s| s.to_string()).collect()
    }

    /// While it runs: "Accepting".
    pub fn doing(self) -> &'static str {
        match self {
            Job::Accept => "Accepting",
            Job::Close => "Closing",
            Job::Drop | Job::DropForce => "Dropping",
            Job::Start | Job::StartItem => "Starting",
            Job::Resume => "Resuming",
        }
    }

    /// Once it worked: "Accepted".
    fn done(self) -> &'static str {
        match self {
            Job::Accept => "Accepted",
            Job::Close => "Closed",
            Job::Drop | Job::DropForce => "Dropped",
            Job::Start | Job::StartItem => "Started",
            Job::Resume => "Resumed",
        }
    }
}

/// Run `bullpen <args>`: did it work, and what it said (stdout, else stderr).
pub(super) fn run(bullpen: &Path, root: &Path, args: &[String]) -> (bool, String) {
    let out = Command::new(bullpen)
        .args(args)
        .current_dir(root)
        // never let it read our terminal: it would wait there for keys meant for the board
        .stdin(Stdio::null())
        .output();
    match out {
        Ok(o) => {
            let text = if o.status.success() { o.stdout } else { o.stderr };
            (o.status.success(), String::from_utf8_lossy(&text).into_owned())
        }
        Err(e) => (false, format!("can't run {}: {e}", bullpen.display())),
    }
}

/// An accept that changed bullpen's keys.lua ends by saying so: that, not "Accepted",
/// is the line to show, since Rex runs the old keys until its config is reloaded.
fn keys_changed(job: Job, out: &str) -> bool {
    job == Job::Accept && out.starts_with("Keys changed")
}

/// What a command failed with, as a sentence: "BP-3's agent isn't running."
pub(super) fn said(line: &str) -> String {
    match line.chars().last() {
        Some('.' | '!' | '?') | None => line.to_string(),
        _ => format!("{line}."),
    }
}

/// The last line a command said, without its "bullpen: ".
pub(super) fn last_line(text: &str) -> String {
    let last = text.trim().lines().last().unwrap_or("");
    last.strip_prefix("bullpen: ").unwrap_or(last).to_string()
}

impl App {
    /// What's running for a task, if anything.
    pub fn job(&self, id: &str) -> Option<Job> {
        self.running.iter().find(|(x, _)| x == id).map(|(_, j)| *j)
    }

    /// Run `bullpen <args>` and wait: for quick commands.
    pub(super) fn bp(&self, args: &[&str]) -> (bool, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (ok, out) = run(&self.bullpen, &self.root, &args);
        (ok, last_line(&out))
    }

    pub(super) fn still(&mut self, id: &str) -> bool {
        match self.job(id) {
            Some(j) => {
                self.msg = Note::info(format!("{id} is still {}.", j.doing().to_lowercase()));
                true
            }
            None => false,
        }
    }

    /// Start a slow command in the background; its result comes back as Msg::Done.
    pub(super) fn spawn(&mut self, id: &str, job: Job) {
        if self.still(id) {
            return;
        }
        self.running.push((id.to_string(), job));
        let (bullpen, root, tx, id) = (self.bullpen.clone(), self.root.clone(), self.tx.clone(), id.to_string());
        std::thread::spawn(move || {
            let (ok, out) = run(&bullpen, &root, &job.args(&id));
            let _ = tx.send(Msg::Done { id, job, ok, out: last_line(&out) });
        });
    }

    pub(super) fn done(&mut self, id: String, job: Job, ok: bool, out: String) {
        self.running.retain(|(x, _)| *x != id);
        // accepted or closed: on to the next task that needs you, to run down a queue
        // (unless the selection already moved off it)
        if ok && matches!(job, Job::Accept | Job::Close) && self.sel.as_deref() == Some(id.as_str()) {
            let i = self.order().iter().position(|t| t.id == id).unwrap_or(0);
            if let Some((next, _)) = self.next_up(i, Some(&id)) {
                self.sel = Some(next);
            }
        }
        if job == Job::StartItem {
            self.item_started(&id, ok, out);
        } else if job == Job::Drop && !ok && out.contains("uncommitted") && self.pending.is_none() {
            // ask again on its row, about losing the changes this time
            self.sel = Some(id.clone());
            self.pending = Some((Action::DropForce, id));
            self.msg.clear();
        } else {
            self.msg = if keys_changed(job, &out) { Note::done(out) } else { Note::of(ok, format!("{} {id}.", job.done()), out) };
        }
    }

    /// Read a task's screen again in the background, for a peek or an answer that's open.
    pub(crate) fn peek(&mut self) {
        let (id, answer) = match &self.mode {
            Mode::Overlay { peek: Some(id), .. } => (id.clone(), false),
            Mode::Answer { id, .. } => (id.clone(), true),
            _ => return,
        };
        if self.peeking || Instant::now() < self.peek_due {
            return;
        }
        self.peeking = true;
        let (bullpen, root, tx) = (self.bullpen.clone(), self.root.clone(), self.tx.clone());
        if answer {
            let lines = self.height.saturating_sub(6).max(1).to_string();
            let args = vec!["prompt".into(), id.clone(), "--lines".into(), lines];
            std::thread::spawn(move || {
                let (ok, out) = run(&bullpen, &root, &args);
                let prompt = if ok { Prompt::parse(&out) } else { None };
                let _ = tx.send(Msg::Prompted { id, prompt, out: last_line(&out) });
            });
            return;
        }
        let lines = self.height.saturating_sub(4).max(1).to_string();
        let args = vec!["peek".into(), id.clone(), "--lines".into(), lines, "--vt".into()];
        std::thread::spawn(move || {
            let (ok, text) = run(&bullpen, &root, &args);
            // a screen that can't be read: why, in the board's words
            let text = if ok { text } else { said(&last_line(&text)) };
            let _ = tx.send(Msg::Peeked { id, text, ok });
        });
    }

    pub(super) fn peeked(&mut self, id: String, text: String, ok: bool) {
        self.peeking = false;
        self.peek_due = Instant::now() + PEEK_EVERY;
        if let Mode::Overlay { peek: Some(p), lines, .. } = &mut self.mode {
            if *p == id {
                *lines = text.lines().map(String::from).collect();
                self.peek_failed = !ok;
            }
        }
    }

    /// A prompt read for the answer that's open. One that can't be read (its agent
    /// went on, its pane is gone) closes it, saying why.
    pub(super) fn prompted(&mut self, id: String, prompt: Option<Prompt>, out: String) {
        self.peeking = false;
        self.peek_due = Instant::now() + PEEK_EVERY;
        let Mode::Answer { id: open, prompt: shown, .. } = &mut self.mode else { return };
        if *open != id {
            return;
        }
        match prompt {
            Some(p) => *shown = Some(p),
            None => {
                self.mode = Mode::Normal;
                self.msg = Note::failed(out);
            }
        }
    }
}

#[cfg(test)]
#[path = "test_jobs.rs"]
mod tests;
