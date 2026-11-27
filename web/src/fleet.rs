//! The snapshot the page's sidebar draws (`/api/fleet`, the `fleet` event): `bullpen
//! repos --json`'s shape (bullpen_core::fleet), with each task's and coordinator's group
//! (`needs`, `active`, `done`) and the tasks closed in the last two days. A thread reads
//! it again 100 ms after the state folder changes (watch.rs), and once a second in case
//! a change was missed, through a kept Reader that reads a file again only when it
//! changed. Whether a coordinator's pane is alive is Rex's to say: `bullpen repos --json`
//! asks it, every 5 seconds, in a thread of its own.

use bullpen_core::fleet::{Coordinator, Fleet, Reader, RepoState};
use bullpen_core::json::s;
use bullpen_core::task::{awaits_accept, is_closed, needs_you};
use bullpen_core::timeutil::{now, parse, Stamp};
use bullpen_core::words::{board_sentence, mark};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// How long the closed tasks stay in Done.
const DONE_FOR: i64 = 2 * 24 * 3600;
/// How long after a change the snapshot is read: a burst of writes is read once.
const SETTLE: Duration = Duration::from_millis(100);
/// How often it's read with no change heard.
const EVERY: Duration = Duration::from_secs(1);
/// How often coordinators' liveness is asked.
const ALIVE_EVERY: Duration = Duration::from_secs(5);

/// The snapshot as sent: its JSON, and its number (the `fleet` event's id). A new one
/// is made only when the JSON changed.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub n: u64,
    pub json: String,
}

pub type Latest = watch::Receiver<Arc<Snapshot>>;

/// What the reading thread hears.
pub enum Heard {
    /// something under the state folder changed
    Changed,
    /// coordinators' liveness: (repo's state folder, coordinator's ID) -> alive
    Alive(HashMap<(String, String), bool>),
}

/// Start reading the state folder: the first snapshot is read before this returns.
pub fn start(state: &Path, bullpen: &Path) -> Latest {
    let (tx, rx) = channel();
    let mut reader = Reader::default();
    let alive = HashMap::new();
    let first = Snapshot {
        n: 1,
        json: read(&mut reader, state, &alive),
    };
    let (send, latest) = watch::channel(Arc::new(first));
    crate::watch::state_folder(state, tx.clone());
    ask_alive(state.to_path_buf(), bullpen.to_path_buf(), tx);
    let state = state.to_path_buf();
    std::thread::spawn(move || keep_reading(&state, reader, alive, rx, send));
    latest
}

/// Read the snapshot again after each change (once things settle) and every second;
/// send it on when it changed.
fn keep_reading(
    state: &Path,
    mut reader: Reader,
    mut alive: HashMap<(String, String), bool>,
    rx: Receiver<Heard>,
    send: watch::Sender<Arc<Snapshot>>,
) {
    let mut read_at = Instant::now();
    loop {
        let wait = EVERY.saturating_sub(read_at.elapsed());
        match rx.recv_timeout(wait) {
            Ok(Heard::Changed) => settle(&rx, &mut alive),
            Ok(Heard::Alive(a)) => alive = a,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let json = read(&mut reader, state, &alive);
        read_at = Instant::now();
        let last = send.borrow().clone();
        if json != last.json {
            send.send_replace(Arc::new(Snapshot { n: last.n + 1, json }));
        }
    }
}

/// Wait until nothing has changed for SETTLE, keeping any liveness heard meanwhile.
fn settle(rx: &Receiver<Heard>, alive: &mut HashMap<(String, String), bool>) {
    while let Ok(h) = rx.recv_timeout(SETTLE) {
        if let Heard::Alive(a) = h {
            *alive = a;
        }
    }
}

/// The snapshot now, as JSON; with what went wrong when the state can't be read.
fn read(reader: &mut Reader, state: &Path, alive: &HashMap<(String, String), bool>) -> String {
    let value = match reader.read(state) {
        Ok(mut fleet) => {
            for r in fleet.repos.iter_mut() {
                let dir = r.repo.dir.display().to_string();
                for c in r.coordinators.iter_mut() {
                    c.alive = alive.get(&(dir.clone(), c.id.clone())).copied();
                }
            }
            snapshot(&fleet, now())
        }
        Err(e) => json!({"repos": [], "error": e.0}),
    };
    bullpen_core::json::dumps_line(&value)
}

/// The fleet as the page has it: each repo as `repos --json` has it, its tasks and
/// coordinators each with their group, and its tasks closed since two days before now
/// after the open ones.
pub fn snapshot(fleet: &Fleet, at: Stamp) -> Value {
    let repos: Vec<Value> = fleet.repos.iter().map(|r| repo(r, at)).collect();
    json!({ "repos": repos })
}

fn repo(r: &RepoState, at: Stamp) -> Value {
    let mut v = r.to_json();
    if let Some(tasks) = v.get_mut("tasks").and_then(Value::as_array_mut) {
        for t in tasks.iter_mut() {
            let st = t
                .get("task")
                .and_then(|t| t.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("");
            t["group"] = json!(task_group(st));
        }
        tasks.extend(done(r, at));
    }
    if let Some(cs) = v.get_mut("coordinators").and_then(Value::as_array_mut) {
        for (c, coordinator) in cs.iter_mut().zip(&r.coordinators) {
            c["group"] = json!(coordinator_group(coordinator));
        }
    }
    v
}

/// A task's group in the sidebar: what needs you, stopped, or waits on your accept;
/// what's closed; the rest.
pub fn task_group(status: &str) -> &'static str {
    if needs_you(status) || awaits_accept(status) {
        "needs"
    } else if is_closed(status) {
        "done"
    } else {
        "active"
    }
}

/// A coordinator blocked at a prompt needs you; any other is active.
pub fn coordinator_group(c: &Coordinator) -> &'static str {
    if c.word() == "blocked" {
        "needs"
    } else {
        "active"
    }
}

/// The repo's tasks closed in the two days before at, newest first, as `repos --json`
/// has an open task.
fn done(r: &RepoState, at: Stamp) -> Vec<Value> {
    let mut closed: Vec<(Stamp, &bullpen_core::json::Obj)> = r
        .all
        .iter()
        .filter(|t| is_closed(s(t, "status")))
        .filter_map(|t| Some((bullpen_core::task::stamp(t).and_then(parse)?, t)))
        .filter(|(when, _)| (at - *when).num_seconds() <= DONE_FOR)
        .collect();
    closed.sort_by_key(|c| std::cmp::Reverse(c.0));
    closed
        .into_iter()
        .map(|(_, t)| {
            let words = board_sentence(&r.repo.cfg, t, &r.every, None, String::new());
            json!({"mark": mark(s(t, "status")), "words": words, "doing": "", "task": t, "group": "done"})
        })
        .collect()
}

/// Ask `bullpen repos --json` which coordinators are alive, now and every 5 seconds.
fn ask_alive(state: PathBuf, bullpen: PathBuf, tx: Sender<Heard>) {
    std::thread::spawn(move || loop {
        if let Some(alive) = alive(&state, &bullpen) {
            if tx.send(Heard::Alive(alive)).is_err() {
                return;
            }
        }
        std::thread::sleep(ALIVE_EVERY);
    });
}

/// Each coordinator's liveness from `bullpen repos --json`; None when it couldn't say.
fn alive(state: &Path, bullpen: &Path) -> Option<HashMap<(String, String), bool>> {
    let out = std::process::Command::new(bullpen)
        .args(["repos", "--json"])
        .env("BULLPEN_STATE", state)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let mut map = HashMap::new();
    for r in v.get("repos")?.as_array()? {
        let dir = r.get("dir").and_then(Value::as_str).unwrap_or("");
        for c in r.get("coordinators").and_then(Value::as_array).into_iter().flatten() {
            if let (Some(id), Some(a)) = (
                c.get("id").and_then(Value::as_str),
                c.get("alive").and_then(Value::as_bool),
            ) {
                map.insert((dir.to_string(), id.to_string()), a);
            }
        }
    }
    Some(map)
}

#[cfg(test)]
#[path = "tests/test_fleet.rs"]
mod tests;
