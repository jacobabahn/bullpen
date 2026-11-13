//! Tasks as bullpen stores them: the JSON types, what a task is waiting on, and
//! loading a repo's tasks.

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
    /// "full", "light", "followup", "tour" (of the person's comments after a tour) or
    /// "rebase" (a check of a conflict resolution after a rebase)
    #[serde(default, deserialize_with = "lenient")]
    pub kind: Option<String>,
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
    #[serde(default)]
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

/// What an agent last said it's doing: the watcher writes it to
/// <state>/doing/<block>.json from the pane's "working: ..." status.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Doing {
    /// the pane's state when it was written: only "working" has news
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub text: String,
    /// when it was written, in seconds since 1970
    #[serde(default)]
    pub at: i64,
}

impl Review {
    /// "light review", "follow-up review", or plain "review" from before kinds.
    pub fn kind_words(&self) -> &'static str {
        match self.kind.as_deref() {
            Some("full") => "full review",
            Some("light") => "light review",
            Some("followup") => "follow-up review",
            Some("tour") => "review of your comments",
            Some("rebase") => "rebase check",
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
    /// its coordinator asked the person about it (`bullpen ask`): the question is its why
    #[serde(default, deserialize_with = "lenient")]
    pub ask: Option<bool>,
    /// why it waits on someone: its worker's or reviewer's stop
    #[serde(default, deserialize_with = "lenient")]
    pub stop: Option<Stop>,
    /// its guided review tour, once one was asked for
    #[serde(default, deserialize_with = "lenient")]
    pub tour: Option<TourRun>,
    /// those not done yet, worked out from the other tasks: "BP-8", "BP-9 (dropped)"
    #[serde(skip)]
    pub waits: Vec<String>,
    /// what the agent it waits on (worker, or reviewer in review) last said it's doing
    #[serde(skip)]
    pub doing: Option<Doing>,
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

    /// The pane of the agent it waits on now: the reviewer's in review, else the worker's.
    pub fn agent_block(&self) -> Option<&str> {
        match self.status.as_str() {
            "review" => self.review.as_ref().and_then(|r| r.block.as_deref()),
            "working" => self.block.as_deref(),
            _ => None,
        }
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

    /// When it entered its current status.
    pub fn stamp(&self) -> Option<&str> {
        self.since.as_deref().or(self.updated.as_deref())
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
    // the repo's folder is <state>/repos/<repo>: doing/ is beside repos/
    if let Some(state) = dir.parent().and_then(Path::parent) {
        link_doing(&mut out, &state.join("doing"));
    }
    out
}

/// A pane's name as a file name, as bullpen and the watcher write it.
fn safe(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect()
}

/// Fill in what the agent each working or in-review task waits on last said.
pub fn link_doing(tasks: &mut [Task], dir: &Path) {
    for t in tasks.iter_mut() {
        t.doing = t
            .agent_block()
            .and_then(|b| std::fs::read_to_string(dir.join(format!("{}.json", safe(b)))).ok())
            .and_then(|s| serde_json::from_str(&s).ok());
    }
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

#[cfg(test)]
#[path = "test_task.rs"]
mod tests;
