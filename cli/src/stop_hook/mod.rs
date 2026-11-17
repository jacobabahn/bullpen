//! The worker's Stop hook: what a worker that ended its turn is waiting on.
//!
//! When a Claude Code worker starts (`launch`), bullpen writes a Stop hook to its
//! worktree's `.claude/settings.local.json` (git-ignored through `.git/info/exclude`, as
//! the coordinator's news hook is; it goes with the worktree). On each Stop, Claude Code
//! runs `bullpen _worker_stop <record>`, which writes the task's turn record
//! (`turns/<ID>.json` in the repo's state folder, turn.rs): the background jobs the
//! session still runs and the last thing the worker said. The worker's end of turn
//! (events/) reads it: a worker waiting on its own job isn't triage, and one that ended
//! with a question has it in the triage line.
//!
//! The task keeps `stop_hook` (true once its hook was written: doctor checks those),
//! `turn_at` (when its turn began: an older record is a past turn's) and `job` (what it
//! waits on, while it does).

pub mod turn;

use crate::news::hook::{read, write};
use bullpen_core::json::{s, truthy, Obj};
use bullpen_core::paths::self_bin;
use bullpen_core::task::is_claude;
use bullpen_core::text::shell_join;
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The settings file, from the worktree's top.
pub const FILE: &str = ".claude/settings.local.json";
/// The internal command the hook runs.
pub const COMMAND: &str = "_worker_stop";
/// Seconds Claude Code gives the hook; it takes a few milliseconds.
const TIMEOUT: u64 = 10;

/// The task's turn record.
pub fn record_path(repo: &Repo, id: &str) -> PathBuf {
    repo.dir.join("turns").join(format!("{id}.json"))
}

/// Remove the task's turn record (its worktree is going: the task is closed).
pub fn forget(repo: &Repo, id: &str) {
    let _ = std::fs::remove_file(record_path(repo, id));
}

/// The settings file in the task's worktree.
pub fn path(t: &Obj) -> PathBuf {
    Path::new(s(t, "worktree")).join(FILE)
}

/// What the hook runs. `|| true`: whatever happens to bullpen, the hook exits 0, so it
/// never blocks the worker's turn (exit 2 would).
pub fn command(repo: &Repo, id: &str) -> String {
    let argv = [
        self_bin().display().to_string(),
        COMMAND.into(),
        record_path(repo, id).display().to_string(),
    ];
    format!("{} 2>/dev/null || true", shell_join(&argv))
}

fn handler(command: &str) -> Value {
    json!({"type": "command", "command": command, "timeout": TIMEOUT})
}

/// Is this handler bullpen's?
fn ours(h: &Value) -> bool {
    let command = h.get("command").and_then(Value::as_str).unwrap_or("");
    command.contains(&format!(" {COMMAND} "))
}

/// The Stop groups of the settings, made if missing. None: the file has another shape
/// there (not bullpen's to change).
fn stop_groups(settings: &mut Obj) -> Option<&mut Vec<Value>> {
    let hooks = settings.entry("hooks").or_insert_with(|| json!({})).as_object_mut()?;
    hooks.entry("Stop").or_insert_with(|| json!([])).as_array_mut()
}

/// Put bullpen's handler for command under Stop, once, leaving the rest as it is. True
/// if that changed the settings.
pub fn put(settings: &mut Obj, command: &str) -> bool {
    let want = handler(command);
    let Some(groups) = stop_groups(settings) else {
        return false;
    };
    let mut found = false;
    let mut changed = false;
    for g in groups.iter_mut() {
        let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        let before = hs.len();
        hs.retain_mut(|h| {
            if !ours(h) {
                return true;
            }
            if found {
                return false; // one is enough
            }
            found = true;
            if *h != want {
                *h = want.clone();
                changed = true;
            }
            true
        });
        changed |= hs.len() != before;
    }
    groups.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|hs| !hs.is_empty()));
    if !found {
        groups.push(json!({"hooks": [want]}));
        changed = true;
    }
    changed
}

/// Is bullpen's handler, for this command, in the settings?
pub fn has(settings: &Obj, command: &str) -> bool {
    let want = handler(command);
    let groups = settings
        .get("hooks")
        .and_then(|h| h.get("Stop"))
        .and_then(Value::as_array);
    groups
        .into_iter()
        .flatten()
        .filter_map(|g| g.get("hooks").and_then(Value::as_array))
        .flatten()
        .any(|h| *h == want)
}

/// Write the Stop hook to the task's worktree when its worker is Claude Code (cmd: the
/// command it starts with), and mark the task. A worker of another kind gets none. Never
/// fails the start: what went wrong is said on stderr, and doctor says it again.
pub fn install(repo: &Repo, t: &mut Obj, cmd: &[String]) {
    if !is_claude(cmd) || !truthy(t.get("worktree")) {
        return;
    }
    if let Err(e) = rewrite(repo, t) {
        eprintln!("{}: the worker's Stop hook wasn't written: {e}", s(t, "id"));
        return;
    }
    t.insert("stop_hook".into(), json!(true));
}

/// Write (or bring up to date) the hook in the task's worktree.
pub fn rewrite(repo: &Repo, t: &Obj) -> Res<()> {
    let path = path(t);
    let mut settings = read(&path)?.unwrap_or_default();
    crate::news::hook::ignore(repo)?; // the exclude is the repository's: it covers each worktree
    if put(&mut settings, &command(repo, s(t, "id"))) {
        write(&path, &settings)?;
    }
    Ok(())
}

/// Why the task's running worker has no working Stop hook, or None. A task started
/// before workers had one (no `stop_hook` mark) isn't asked.
pub fn missing(repo: &Repo, t: &Obj) -> Option<String> {
    if !truthy(t.get("stop_hook")) {
        return None;
    }
    let shown = path(t);
    match read(&shown) {
        Err(e) => Some(e.to_string()),
        Ok(None) => Some(format!("{} is gone", shown.display())),
        Ok(Some(st)) if !has(&st, &command(repo, s(t, "id"))) => {
            Some(format!("{} doesn't have bullpen's Stop hook", shown.display()))
        }
        Ok(Some(_)) => None,
    }
}

#[cfg(test)]
#[path = "test_stop_hook.rs"]
mod tests;
