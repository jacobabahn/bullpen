//! Every repo bullpen knows, in one snapshot: its root, settings and coordinators, its
//! open tasks with the board's words for each, and its counts (what needs you, what's
//! ready, what's running). `bullpen repos`, `ls --all-repos` and the board's top line
//! read it, and so will mission control.
//!
//! Only files are read, and only these: <state>/repos/*/config.json, each repo's tasks,
//! and doing/ for the panes they name. A `Reader` kept between snapshots (a long-running
//! program's) reads a file again only when its time or size changed. Whether a
//! coordinator's pane is alive is Rex's to say, so it's asked apart (`Fleet::ask_alive`).

use crate::config::{load_config, read_own, task_files};
use crate::json::{self, s, Obj};
use crate::paths::{safe, state};
use crate::task::{is_closed, task_num};
use crate::words::{agent_block, board_order, board_sentence, doing_said, every, mark, Every};
use crate::{Repo, Res};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Every known repo whose checkout is still there, by state folder.
#[derive(Debug, Clone, Default)]
pub struct Fleet {
    pub repos: Vec<RepoState>,
}

/// One repo as it is now.
#[derive(Debug, Clone)]
pub struct RepoState {
    /// its root, state folder and settings (read, never migrated or saved)
    pub repo: Repo,
    pub coordinators: Vec<Coordinator>,
    /// its open tasks, in the board's order
    pub open: Vec<TaskState>,
    /// every task, closed ones too, by number
    pub all: Vec<Obj>,
    pub every: Every,
    pub counts: Counts,
    /// task files that don't parse, each with why: left out of the rest
    pub broken: Vec<String>,
}

/// What you act on and what's under way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// needs or stopped
    pub needs: usize,
    /// ready or finished: waits on your accept or close
    pub ready: usize,
    /// working or in review: an agent on it
    pub running: usize,
}

/// A coordinator: its pane, and what the watcher last noted it doing (doing/<block>.json).
#[derive(Debug, Clone, Default)]
pub struct Coordinator {
    pub id: String,
    pub block: String,
    pub session: String,
    /// Rex's state for its pane ("working", "idle", "done", "blocked"); "" when none noted
    pub state: String,
    pub text: String,
    /// when it was noted (seconds since the epoch)
    pub at: Option<f64>,
    /// is its pane running: None until asked (Fleet::ask_alive)
    pub alive: Option<bool>,
}

impl Coordinator {
    /// Its state in a word: "gone", "working", "blocked" (on a question in its pane) or "idle".
    pub fn word(&self) -> &'static str {
        match (self.alive, self.state.as_str()) {
            (Some(false), _) => "gone",
            (_, "working") => "working",
            (_, "blocked") => "blocked",
            _ => "idle",
        }
    }
}

/// An open task with the board's words for it.
#[derive(Debug, Clone)]
pub struct TaskState {
    pub task: Obj,
    pub mark: &'static str,
    /// the board's sentence (what its agent is doing, when it said, else where it stands)
    pub words: String,
    /// what its agent is doing now, as the board shows it; "" when it hasn't said
    pub doing: String,
}

/// A file read once, kept with the time and size it had.
#[derive(Debug, Clone)]
struct Seen {
    changed: SystemTime,
    len: u64,
    read: Result<Option<Obj>, String>,
}

/// A change in the same second as a read may share its time: read again until this old.
const SETTLED: Duration = Duration::from_secs(2);

/// Reads the files a snapshot needs; kept between snapshots, it reads again only the
/// files whose time or size changed (or that changed in the last two seconds).
#[derive(Debug, Default)]
pub struct Reader {
    files: HashMap<PathBuf, Seen>,
    /// the files the snapshot being read used: the others are forgotten after it
    used: BTreeSet<PathBuf>,
}

impl Reader {
    /// The snapshot of every repo under the state folder.
    pub fn read(&mut self, state: &Path) -> Res<Fleet> {
        self.used.clear();
        let mut repos = Vec::new();
        for dir in repo_dirs(state) {
            // a config.json caught half-written, or broken: that repo's own commands say so
            let Ok(own) = read_own(&dir) else { continue };
            let root = s(&own, "root");
            if root.is_empty() || !Path::new(root).is_dir() {
                continue; // its checkout is gone
            }
            let cfg = load_config(&dir)?;
            let repo = Repo {
                root: PathBuf::from(root),
                dir,
                own: own.keys().cloned().collect(),
                cfg,
            };
            repos.push(self.repo_state(state, repo));
        }
        self.files.retain(|p, _| self.used.contains(p));
        Ok(Fleet { repos })
    }

    fn repo_state(&mut self, state: &Path, repo: Repo) -> RepoState {
        let mut all = Vec::new();
        let mut broken = Vec::new();
        for p in task_files(&repo.dir) {
            match self.obj(&p) {
                Ok(Some(t)) => all.push(t),
                Ok(None) => {}
                Err(e) => broken.push(e),
            }
        }
        all.sort_by_key(task_num);
        let every = every(&all);
        let open = board_order(all.iter().filter(|t| !is_closed(s(t, "status"))).cloned().collect());
        let open: Vec<TaskState> = open
            .into_iter()
            .map(|t| self.task_state(state, &repo, &every, t))
            .collect();
        let coordinators = coordinator_list(&repo)
            .into_iter()
            .map(|c| self.coordinator(state, c))
            .collect();
        let counts = counts(open.iter().map(|t| &t.task));
        RepoState {
            repo,
            coordinators,
            open,
            all,
            every,
            counts,
            broken,
        }
    }

    fn task_state(&mut self, state: &Path, repo: &Repo, every: &Every, t: Obj) -> TaskState {
        let doing = match agent_block(&t) {
            Some(block) => match self.doing(state, &block) {
                Some(d) => doing_said(&t, &d, None),
                None => String::new(),
            },
            None => String::new(),
        };
        let words = board_sentence(&repo.cfg, &t, every, None, doing.clone());
        TaskState {
            mark: mark(s(&t, "status")),
            words,
            doing,
            task: t,
        }
    }

    fn coordinator(&mut self, state: &Path, c: &Obj) -> Coordinator {
        let mut out = Coordinator {
            id: s(c, "id").into(),
            block: s(c, "block").into(),
            session: s(c, "session").into(),
            ..Default::default()
        };
        if let Some(d) = self.doing(state, &out.block) {
            out.state = s(&d, "state").into();
            out.text = s(&d, "text").into();
            out.at = d.get("at").and_then(Value::as_f64);
        }
        out
    }

    /// What the watcher noted pane block doing; None when nothing (or a file that doesn't parse).
    fn doing(&mut self, state: &Path, block: &str) -> Option<Obj> {
        if block.is_empty() {
            return None;
        }
        self.obj(&state.join("doing").join(format!("{}.json", safe(block))))
            .ok()
            .flatten()
    }

    /// A JSON object file: None when it's missing, Err with why when it doesn't parse.
    fn obj(&mut self, path: &Path) -> Result<Option<Obj>, String> {
        self.used.insert(path.to_path_buf());
        let Ok(meta) = std::fs::metadata(path) else {
            self.files.remove(path);
            return Ok(None);
        };
        let changed = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let settled = changed.elapsed().is_ok_and(|a| a > SETTLED);
        if let Some(seen) = self.files.get(path) {
            if seen.changed == changed && seen.len == meta.len() && settled {
                return seen.read.clone();
            }
        }
        let read = json::read_obj(path).map_err(|e| e.0);
        self.files.insert(
            path.to_path_buf(),
            Seen {
                changed,
                len: meta.len(),
                read: read.clone(),
            },
        );
        read
    }
}

/// The snapshot as it is now, read once.
pub fn read() -> Res<Fleet> {
    Reader::default().read(&state())
}

/// <state>/repos/*, sorted.
fn repo_dirs(state: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(state.join("repos")).into_iter().flatten().flatten();
    let mut dirs: Vec<PathBuf> = entries
        .map(|e| e.path())
        .filter(|d| d.join("config.json").is_file())
        .collect();
    dirs.sort();
    dirs
}

/// The repo's coordinators, oldest first (an entry that isn't an object is left out).
fn coordinator_list(repo: &Repo) -> Vec<&Obj> {
    let list = repo.cfg.get("coordinators").and_then(Value::as_array);
    list.into_iter().flatten().filter_map(Value::as_object).collect()
}

/// What needs you, what's ready and what's running among these tasks.
pub fn counts<'a>(tasks: impl Iterator<Item = &'a Obj>) -> Counts {
    let mut c = Counts::default();
    for t in tasks {
        match s(t, "status") {
            "needs" | "stopped" => c.needs += 1,
            "ready" | "finished" => c.ready += 1,
            "working" | "review" => c.running += 1,
            _ => {}
        }
    }
    c
}

impl Fleet {
    /// Ask whether each coordinator's pane is running (alive(session, block), Rex's
    /// answer), all at once: each question is a Rex call of its own.
    pub fn ask_alive(&mut self, alive: impl Fn(&str, &str) -> Res<bool> + Sync) {
        let alive = &alive;
        let cs: Vec<&mut Coordinator> = self.repos.iter_mut().flat_map(|r| r.coordinators.iter_mut()).collect();
        std::thread::scope(|scope| {
            for c in cs {
                scope.spawn(move || c.alive = Some(alive(&c.session, &c.block).unwrap_or(false)));
            }
        });
    }

    /// The whole snapshot as JSON: {"repos": [...]}, for `bullpen repos --json`.
    pub fn to_json(&self) -> Value {
        json!({"repos": self.repos.iter().map(RepoState::to_json).collect::<Vec<_>>()})
    }
}

impl RepoState {
    pub fn to_json(&self) -> Value {
        let tasks: Vec<Value> = self
            .open
            .iter()
            .map(|t| json!({"mark": t.mark, "words": t.words, "doing": t.doing, "task": t.task}))
            .collect();
        json!({
            "name": self.repo.name(),
            "root": self.repo.root_str(),
            "dir": self.repo.dir.display().to_string(),
            "coordinators": self.coordinators.iter().map(Coordinator::to_json).collect::<Vec<_>>(),
            "counts": {"needs": self.counts.needs, "ready": self.counts.ready, "running": self.counts.running},
            "tasks": tasks,
            "broken": self.broken,
            "settings": self.repo.cfg,
        })
    }
}

impl Coordinator {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "block": self.block,
            "session": self.session,
            "alive": self.alive,
            "word": self.word(),
            "state": self.state,
            "text": self.text,
            "at": self.at,
        })
    }
}

#[cfg(test)]
#[path = "test_fleet.rs"]
mod tests;
