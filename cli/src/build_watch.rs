//! The watcher's periodic check (`check --fix --all-repos`) keeps its own copy of bullpen
//! built: when a program's source is newer than its build (a `git pull`, a merge made by
//! a copy from before BP-165), it starts the background build an accept starts
//! (build::start_rebuild: the same lock, log, stamps, news and watcher restart). Only
//! this copy, the one the watcher runs, and only a settled tree: not while a build
//! runs, a merge or rebase is in progress, or the source has uncommitted changes. Why it
//! was skipped is one news line while the reason stands (target/watch-build.json).

use crate::build::{build_log, building, holder, no_build, programs, runs_itself, say, start_rebuild, target, CLI};
use crate::guide::tell_guide_changes_everywhere;
use bullpen_core::json::{read_obj, write_obj, Obj};
use bullpen_core::paths::home;
use bullpen_core::proc::git;
use bullpen_core::text::strip;
use bullpen_core::{Repo, Res};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The folders whose uncommitted changes keep a build back: the programs' source and the watcher's.
const SOURCE: [&str; 4] = ["core", "cli", "board", "lua"];

/// What this check last did: `skipped` (the reason it said in news), `seen` (a build it
/// found running, not yet said) and `started` (it started a build: guide news is due).
fn note_path() -> PathBuf {
    target().join("watch-build.json")
}

/// A reason not to build now. key: the same while the reason stands. patient: said only
/// when it still stands at the next check (a build that's running is usually done by then).
struct Unsettled {
    key: String,
    text: String,
    patient: bool,
}

/// Build this copy when its source is newer than its build. Never fails the check.
pub fn catch_up(repos: &[Repo]) {
    if let Err(e) = caught_up(repos) {
        eprintln!("bullpen: couldn't check bullpen's build: {e}");
    }
}

fn caught_up(repos: &[Repo]) -> Res<()> {
    let h = home();
    if no_build() || !runs_itself(&h) {
        return Ok(());
    }
    // news goes to bullpen's own repository when it is one bullpen knows, else the first
    let Some(repo) = repos
        .iter()
        .find(|r| crate::copies::same(&r.root, &h))
        .or(repos.first())
    else {
        return Ok(());
    };
    let before = read_obj(&note_path())?.unwrap_or_default();
    let mut note = before.clone();
    let mut stale = false;
    for p in programs(true) {
        stale = stale || p.stale()?;
    }
    if !stale {
        // current, or its build failed already (that was news, and it isn't tried again)
        if note.contains_key("started") {
            tell_guide_changes_everywhere();
        }
        note.clear();
    } else if let Some(u) = unsettled(&h)? {
        let key = Value::String(u.key);
        if u.patient && note.get("seen") != Some(&key) {
            note.insert("seen".into(), key);
        } else if note.get("skipped") != Some(&key) {
            say(repo, &CLI, &u.text)?;
            note.insert("skipped".into(), key);
        }
    } else {
        note.retain(|k, _| k == "started");
        if start_rebuild(repo, &h)?.is_some() {
            note.insert("started".into(), Value::Bool(true));
        }
    }
    save(&note, &before)
}

fn save(note: &Obj, before: &Obj) -> Res<()> {
    if note.is_empty() {
        let _ = std::fs::remove_file(note_path());
    } else if note != before {
        write_obj(&note_path(), note)?;
    }
    Ok(())
}

/// Why the copy at h can't be built now, if it can't.
fn unsettled(h: &Path) -> Res<Option<Unsettled>> {
    const KEEPS: &str = "Commands and boards keep running the old build";
    let at = h.display();
    if building()? {
        // the lock's pid isn't running when it's held by what that pid started (cargo, after
        // the build was killed): name none, never `kill 0`
        let (key, who, stop) = match holder() {
            Some(pid) => (
                format!("building {pid}"),
                format!(" (pid {pid})"),
                format!("stop it (`kill {pid}`)"),
            ),
            None => ("building".into(), String::new(), "stop it".into()),
        };
        return Ok(Some(Unsettled {
            key,
            text: format!(
                "bullpen's source in {at} is newer than its build, and a build{who} has been running since the \
                 check before this one. {KEEPS}. Tell the person: if it's stuck, {stop} and the watcher builds \
                 bullpen at its next check (log: {}).",
                build_log().display()
            ),
            patient: true,
        }));
    }
    if let Some(what) = in_progress(h)? {
        return Ok(Some(Unsettled {
            key: what.into(),
            text: format!(
                "bullpen wasn't rebuilt: a {what} is in progress in {at}, and its source is newer than its build. \
                 {KEEPS}. Tell the person: once the {what} is finished or aborted, the watcher builds bullpen at \
                 its next check."
            ),
            patient: false,
        }));
    }
    let mut args = vec!["status", "--porcelain", "--"];
    args.extend(SOURCE);
    let p = git(h, &args, false)?;
    if !p.ok() || strip(&p.stdout).is_empty() {
        return Ok(None);
    }
    Ok(Some(Unsettled {
        key: "uncommitted".into(),
        text: format!(
            "bullpen wasn't rebuilt: {at} has uncommitted changes under {}/, and its source is newer than its \
             build. {KEEPS}. Tell the person: once they are committed or dropped, the watcher builds bullpen at \
             its next check (or `bullpen build`, as they are).",
            SOURCE.join("/, ")
        ),
        patient: false,
    }))
}

/// "merge" or "rebase" when one is in progress in the checkout at h.
fn in_progress(h: &Path) -> Res<Option<&'static str>> {
    let p = git(h, &["rev-parse", "--absolute-git-dir"], false)?;
    if !p.ok() {
        return Ok(None);
    }
    let dir = PathBuf::from(strip(&p.stdout));
    Ok(if dir.join("MERGE_HEAD").exists() {
        Some("merge")
    } else if dir.join("rebase-merge").exists() || dir.join("rebase-apply").exists() {
        Some("rebase")
    } else {
        None
    })
}
