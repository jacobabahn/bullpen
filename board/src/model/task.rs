//! Tasks as bullpen-core reads them, with the board's typed view of each, what a
//! task is waiting on, and loading a repo's tasks. A `Task` is the object core read
//! (`obj()`, which core's sentences read) and the board's typed view of the fields it
//! reads itself; `load_tasks` (core's `task_files`); what a queued task waits on and
//! what its agent is doing.

use bullpen_core::json::{self, Obj};
use bullpen_core::{config, paths, task as core, words};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use std::path::Path;

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
    pub checks: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Review {
    pub verdict: Option<String>,
    /// the reviewer's pane
    #[serde(default, deserialize_with = "lenient")]
    pub block: Option<String>,
}

/// Its guided review tour (`bullpen tour`): being written, written or failed.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TourRun {
    /// "writing", "written" or "failed"
    #[serde(default)]
    pub state: String,
    /// the commit it was written on
    #[serde(default, deserialize_with = "lenient")]
    pub commit: Option<String>,
    /// the commit of the tour the person last opened: it's toured when that's this one
    #[serde(default, deserialize_with = "lenient")]
    pub opened: Option<String>,
    /// the worker's ready since changed the code: it's of the old code
    #[serde(default, deserialize_with = "lenient")]
    pub stale: Option<bool>,
}

/// What `bullpen tour --show --json` says of a task's tour.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TourShow {
    /// "writing", "failed", "current", "stale" or "none"
    #[serde(default)]
    pub state: String,
}

impl TourShow {
    pub fn parse(out: &str) -> Option<TourShow> {
        serde_json::from_str(out.trim()).ok()
    }
}

/// Why a task waits on someone: {"by": "worker" | "reviewer", "kind": "prompt" | ...}.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Stop {
    /// read only by the tests: who it waits on is core's rule, which reads the task's
    /// object (once main.rs's `App` was in a private module, the compiler saw that)
    #[serde(default)]
    #[allow(dead_code)]
    pub by: String,
    #[serde(default)]
    pub kind: String,
}

/// What `i` answers on a task that needs you.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    /// its worker or reviewer is at a prompt: press one of its options
    Prompt,
    /// its coordinator asked: the answer goes to the coordinator
    Ask,
}

/// The prompt a task's agent is at, as `bullpen prompt` reads it off its screen.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Prompt {
    /// "worker" or "reviewer"
    #[serde(default)]
    pub who: String,
    /// None: its screen shows no prompt now
    #[serde(default)]
    pub question: Option<String>,
    /// (key, label): "1", "Yes"
    #[serde(default)]
    pub options: Vec<(String, String)>,
    /// which prompt it is: `bullpen answer --prompt` refuses another
    #[serde(default)]
    pub id: Option<String>,
    /// the end of its screen, in colour
    #[serde(default)]
    pub screen: Vec<String>,
}

impl Prompt {
    /// What `bullpen prompt` printed, or None when it said something else (an error).
    pub fn parse(out: &str) -> Option<Prompt> {
        serde_json::from_str(out.trim()).ok()
    }

    /// The label of the option this key picks: digits as they are, y and n either case.
    pub fn option(&self, c: char) -> Option<(&str, &str)> {
        let key = c.to_ascii_lowercase().to_string();
        self.options.iter().find(|(k, _)| *k == key).map(|(k, l)| (k.as_str(), l.as_str()))
    }
}

/// A task: the object bullpen-core read, which bullpen-core's sentences read, and the
/// board's typed view of it (the fields below, read from it once).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Task {
    /// the task as its file has it
    #[serde(skip)]
    obj: Obj,
    pub id: String,
    #[serde(default)]
    pub title: String,
    pub status: String,
    pub note: Option<String>,
    pub why: Option<String>,
    pub pr: Option<String>,
    pub review: Option<Review>,
    pub block: Option<String>,
    /// no PR: its branch is merged locally
    #[serde(default, deserialize_with = "lenient")]
    pub local: Option<bool>,
    #[serde(default, deserialize_with = "lenient")]
    pub base: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub worktree: Option<String>,
    /// the size of its change: "3 files, +120 −14"
    #[serde(default, deserialize_with = "lenient")]
    pub diffstat: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub gh: Option<Gh>,
    /// the ID of the coordinator that owns it: its news goes there
    #[serde(default, deserialize_with = "lenient")]
    pub coordinator: Option<String>,
    /// what its worker said it did (`ready`, `finish`)
    #[serde(default, deserialize_with = "lenient")]
    pub summary: Option<String>,
    /// its coordinator asked the person about it (`bullpen ask`): the question is its why
    #[serde(default, deserialize_with = "lenient")]
    pub ask: Option<bool>,
    /// why it waits on someone: its worker's or reviewer's stop
    #[serde(default, deserialize_with = "lenient")]
    pub stop: Option<Stop>,
    /// its guided review tour, once one was asked for
    #[serde(default, deserialize_with = "lenient")]
    pub tour: Option<TourRun>,
    /// what it waits on, from the other tasks: "Waits on BP-8, BP-9 (dropped)", or ""
    #[serde(skip)]
    pub waits: String,
    /// what the agent it waits on (worker, or reviewer in review) last said it's doing:
    /// <state>/doing/<block>.json, as the watcher writes it
    #[serde(skip)]
    pub doing: Option<Obj>,
}

impl Task {
    /// The task in this object, or None when it has no id or status (or one of the
    /// wrong shape): the board can't show it.
    pub fn from_obj(obj: Obj) -> Option<Task> {
        let mut t: Task = serde_json::from_value(serde_json::Value::Object(obj.clone())).ok()?;
        t.obj = obj;
        Some(t)
    }

    /// The task as its file has it, for bullpen-core.
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// The task with these keys set too (for tests).
    #[cfg(test)]
    pub fn with(self, more: serde_json::Value) -> Task {
        let mut obj = self.obj.clone();
        obj.extend(more.as_object().cloned().unwrap_or_default());
        Task { waits: self.waits, doing: self.doing, ..Task::from_obj(obj).expect("a task") }
    }

    /// The number after the prefix: tasks sort by it.
    pub fn num(&self) -> i64 {
        core::task_num(&self.obj)
    }

    pub fn needs_you(&self) -> bool {
        matches!(self.status.as_str(), "needs" | "stopped")
    }

    /// Stopped on something its coordinator sees to: not yours (yet).
    pub fn triage(&self) -> bool {
        self.status == "triage"
    }

    /// Is it stopped on its reviewer (a prompt, or no verdict)? Core's rule, by its stop:
    /// one stopped at the usage limit is seen to as its worker's.
    pub fn on_reviewer(&self) -> bool {
        core::on_reviewer(&self.obj)
    }

    /// Is it its reviewer's turn (in review, or stopped on its reviewer), so ⏎ and peek
    /// open the reviewer? Core's `whose_turn`, as the command line has it.
    pub fn reviewers_turn(&self) -> bool {
        core::whose_turn(&self.obj) == Some("reviewer")
    }

    pub fn closed(&self) -> bool {
        core::is_closed(&self.status)
    }

    /// The question the person can answer from the board, if any: its coordinator's
    /// (an ask) before its agent's prompt.
    pub fn question(&self) -> Option<Question> {
        if self.status != "needs" {
            None
        } else if self.ask == Some(true) {
            Some(Question::Ask)
        } else if self.stop.as_ref().is_some_and(|s| s.kind == "prompt") {
            Some(Question::Prompt)
        } else {
            None
        }
    }

    pub fn is_local(&self) -> bool {
        self.local == Some(true)
    }

    /// Has the agent the task is waiting on (its worker, or its reviewer in review)
    /// shown no progress for too long?
    pub fn is_stuck(&self) -> bool {
        words::is_stuck(&self.obj)
    }

    /// Is the agent the task is waiting on stopped at the usage limit?
    pub fn is_limited(&self) -> bool {
        core::is_limited(&self.obj)
    }

    /// Does it get the amber (or red) bar: it needs you, or its agent has gone quiet.
    pub fn flagged(&self) -> bool {
        self.needs_you() || self.is_stuck()
    }

    /// The pane of the agent it waits on now: the reviewer's in review, else the worker's.
    pub fn agent_block(&self) -> Option<String> {
        words::agent_block(&self.obj)
    }

    /// Can it have a tour: ready or in review, so its code isn't changing.
    pub fn tourable(&self) -> bool {
        matches!(self.status.as_str(), "ready" | "review")
    }

    /// Is its tour being written now?
    pub fn tour_writing(&self) -> bool {
        self.tourable() && self.tour.as_ref().is_some_and(|r| r.state == "writing")
    }

    /// Has the person opened the tour of its code as it is?
    pub fn toured(&self) -> bool {
        self.tour.as_ref().is_some_and(|r| {
            r.state == "written" && r.stale != Some(true) && r.opened.is_some() && r.opened == r.commit
        })
    }

    /// Did it come from the backlog? A dim ↻ follows its mark.
    pub fn is_from_backlog(&self) -> bool {
        words::from_backlog(&self.obj)
    }

    /// When it entered its current status.
    pub fn stamp(&self) -> Option<&str> {
        core::stamp(&self.obj)
    }
}

/// The repo's tasks, by ID. A file caught half-written is skipped until next time.
pub fn load_tasks(dir: &Path) -> Vec<Task> {
    let mut out: Vec<Task> = config::task_files(dir)
        .iter()
        .filter_map(|p| json::read_obj(p).ok().flatten())
        .filter_map(Task::from_obj)
        .collect();
    out.sort_by_key(Task::num);
    link_waits(&mut out);
    // the repo's folder is <state>/repos/<repo>: doing/ is beside repos/
    if let Some(state) = dir.parent().and_then(Path::parent) {
        link_doing(&mut out, &state.join("doing"));
    }
    out
}

/// Fill in what the agent each working or in-review task waits on last said.
pub fn link_doing(tasks: &mut [Task], dir: &Path) {
    for t in tasks.iter_mut() {
        let note = t.agent_block().map(|b| dir.join(format!("{}.json", paths::safe(&b))));
        t.doing = note.and_then(|p| json::read_obj(&p).ok().flatten());
    }
}

/// Fill in what each task waits on: the tasks in its `after` that aren't done.
pub fn link_waits(tasks: &mut [Task]) {
    let every = words::every(&tasks.iter().map(|t| t.obj.clone()).collect::<Vec<_>>());
    for t in tasks.iter_mut() {
        t.waits = words::waits_phrase(&t.obj, &every);
    }
}

#[cfg(test)]
#[path = "test_task.rs"]
mod tests;
