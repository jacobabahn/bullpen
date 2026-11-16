//! Which copy of bullpen a repository's coordinator uses, and what another copy leaves
//! alone there.
//!
//! There can be several checkouts of bullpen on a machine (the installed one, a worktree
//! of it on another branch), each with its own build, watch.lua and prompts, all sharing
//! one state folder. `up` records in each coordinator the copy it was started from
//! (c["bullpen"]: that copy's folder; the coordinator's agent was told that copy's
//! `bin/bullpen`). That copy is the repository's: the watcher runs from it (watcher.rs),
//! and guide changes are told from it (guide.rs). A command run there by another copy
//! does its own work, notes that it ran (<repo's state>/other-copy.json), and doctor
//! says so while the two differ in source (`copy`), with which is older.

use crate::build::CLI;
use crate::coordinators::{coordinators, primary};
use crate::watcher::{watcher_copy, watcher_record};
use bullpen_core::json::{read_obj, s, write_obj, Obj};
use bullpen_core::paths::home;
use bullpen_core::proc::{file_hash, git};
use bullpen_core::text::strip;
use bullpen_core::timeutil::iso_now;
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The copy of bullpen coordinator c was started from, or None for one started before
/// `up` recorded it.
pub fn copy_of(c: &Obj) -> Option<PathBuf> {
    Some(s(c, "bullpen")).filter(|p| !p.is_empty()).map(PathBuf::from)
}

/// The repository's copy: its primary coordinator's. None with no coordinator, or one
/// whose copy isn't recorded.
pub fn coordinator_copy(repo: &Repo) -> Option<PathBuf> {
    primary(repo).and_then(copy_of)
}

/// Is copy the one some coordinator of the repo was started from (the first or a later one)?
pub fn a_coordinators(repo: &Repo, copy: &Path) -> bool {
    coordinators(repo)
        .into_iter()
        .filter_map(copy_of)
        .any(|c| same(&c, copy))
}

/// The later coordinators recorded with another copy than the first's: "; c2 uses ~/wt/bullpen".
fn others(repo: &Repo, first: &Path) -> String {
    let named = coordinators(repo).into_iter().skip(1).filter_map(|c| {
        let copy = copy_of(c).filter(|copy| !same(copy, first))?;
        Some(format!("; {} uses {}", s(c, "id"), copy.display()))
    });
    named.collect()
}

/// Are these the same folder?
pub fn same(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// Is the copy still there: its entry and its watcher?
pub fn there(copy: &Path) -> bool {
    copy.join("bin/bullpen").is_file() && copy.join("lua/watch.lua").is_file()
}

/// Is coordinator c this copy's? One whose copy isn't recorded counts as ours.
pub fn ours(c: &Obj) -> bool {
    copy_of(c).is_none_or(|copy| same(&copy, &home()))
}

/// Coordinator c uses this copy (`up` started it, or ran in its pane with --here).
pub fn record(c: &mut Obj) {
    c.insert("bullpen".into(), json!(home().display().to_string()));
}

/// `up`: a coordinator from before copies were recorded is taken to use the copy that
/// runs `up`.
pub fn record_unrecorded(repo: &mut Repo) {
    let list = repo.cfg.get_mut("coordinators").and_then(Value::as_array_mut);
    for c in list.into_iter().flatten().filter_map(Value::as_object_mut) {
        if copy_of(c).is_none() {
            record(c);
        }
    }
}

fn note_path(repo: &Repo) -> PathBuf {
    repo.dir.join("other-copy.json")
}

/// A command that saved a task in repo: when this copy isn't one a coordinator of the
/// repo was started from, note it for doctor (the last such command; once a command).
pub fn note_command(repo: &Repo) {
    static NOTED: AtomicBool = AtomicBool::new(false);
    if NOTED.swap(true, Ordering::Relaxed) {
        return;
    }
    note_copy(repo, &home(), &std::env::args().nth(1).unwrap_or_default());
}

/// Note that copy ran command in repo, unless it is one a coordinator of the repo was
/// started from. A copy from before the note leaves none itself: what it left undone
/// tells (risk_late.rs).
pub fn note_copy(repo: &Repo, copy: &Path, command: &str) {
    if coordinator_copy(repo).is_none() || a_coordinators(repo, copy) {
        return;
    }
    let note = json!({"copy": copy.display().to_string(), "command": command, "at": iso_now()});
    let _ = write_obj(&note_path(repo), note.as_object().expect("an object"));
}

/// `up` from the coordinator's copy: what another copy ran before is seen to.
pub fn clear_note(repo: &Repo) {
    if coordinator_copy(repo).is_some_and(|copy| same(&copy, &home())) {
        let _ = std::fs::remove_file(note_path(repo));
    }
}

/// What a copy runs, as one hash: the command line's source and watch.lua.
fn source(copy: &Path) -> Res<String> {
    let watch = file_hash(copy.join("lua/watch.lua")).unwrap_or_default();
    Ok(format!("{} {watch}", CLI.source_hash_in(copy)?))
}

fn git_line(dir: &Path, args: &[&str]) -> Option<String> {
    let p = git(dir, args, false).ok()?;
    Some(strip(&p.stdout).to_string()).filter(|out| p.ok() && !out.is_empty())
}

fn commits(n: u64) -> String {
    format!("{n} commit{}", if n == 1 { "" } else { "s" })
}

/// How copy b's source stands to copy a's, by their commits: ("older than", "3 commits
/// behind"), ("newer than", "1 commit ahead"), or for branches that parted, the one whose
/// last commit is earlier is older ("it lacks 10 of the other's commits, and has 1 of its
/// own"). Without git, or with the same commit checked out, only that it differs.
pub fn age(a: &Path, b: &Path) -> (&'static str, String) {
    let (Some(ha), Some(hb)) = (git_line(a, &["rev-parse", "HEAD"]), git_line(b, &["rev-parse", "HEAD"])) else {
        return ("different from", "which is older can't be told without git".into());
    };
    if ha == hb {
        return ("different from", "the same commit, with uncommitted changes".into());
    }
    let when = |dir: &Path| git_line(dir, &["log", "-1", "--format=%ct", "HEAD"]).and_then(|t| t.parse::<i64>().ok());
    let by_date = match (when(a), when(b)) {
        (Some(ta), Some(tb)) if tb < ta => "older than",
        (Some(ta), Some(tb)) if tb > ta => "newer than",
        _ => "different from",
    };
    // the commits each has that the other lacks, when one checkout knows both
    let range = format!("{ha}...{hb}");
    let counts = [a, b]
        .iter()
        .find_map(|dir| git_line(dir, &["rev-list", "--left-right", "--count", &range]));
    let counts = counts.and_then(|out| {
        let mut n = out.split_whitespace().filter_map(|w| w.parse::<u64>().ok());
        Some((n.next()?, n.next()?))
    });
    match counts {
        Some((behind, 0)) if behind > 0 => ("older than", format!("{} behind", commits(behind))),
        Some((0, ahead)) if ahead > 0 => ("newer than", format!("{} ahead", commits(ahead))),
        Some((behind, ahead)) => (
            by_date,
            format!("it lacks {behind} of the other's commits, and has {ahead} of its own"),
        ),
        None => (by_date, "by its last commit".into()),
    }
}

/// What copy `ours` relies on that `theirs` has no code for, as a sentence to end a
/// line with, or "". Told by a file of the source: cheap, and enough for a hint.
fn lacks(ours: &Path, theirs: &Path) -> &'static str {
    let risk = "cli/src/risk.rs";
    if ours.join(risk).is_file() && !theirs.join(risk).is_file() {
        return ". That copy has no risk label: a task it makes ready gets its label late, from this one's `check --fix` \
                (the watcher runs it)";
    }
    ""
}

/// doctor's `copy`: the copy of bullpen this repo's first coordinator uses is there, the
/// watcher runs from it, and no copy that none of the repo's coordinators was started
/// from, with different source, runs commands here (this one now, or one that noted it did).
pub fn check_copy(repo: Option<&Repo>, why_not: Option<&str>) -> Res<(&'static str, String)> {
    if let (None, Some(why)) = (repo, why_not) {
        return Ok(("skip", format!("can't tell: {why}")));
    }
    let Some((repo, c)) = repo.and_then(|r| Some((r, primary(r)?))) else {
        return Ok(("skip", "no coordinator yet: run `bullpen up`".into()));
    };
    let (id, me) = (s(c, "id"), home());
    let Some(copy) = copy_of(c) else {
        return Ok((
            "skip",
            format!("{id} was started before bullpen recorded its copy: `bullpen up` from the copy it uses records it"),
        ));
    };
    let who = if coordinators(repo).len() > 1 {
        format!("this repo's first coordinator ({id})")
    } else {
        format!("this repo's coordinator ({id})")
    };
    let also = others(repo, &copy);
    if !there(&copy) {
        return Ok((
            "fail",
            format!(
                "{who} was started from {}, which is gone (no bin/bullpen and lua/watch.lua there): the watcher \
                 isn't started from a guess. Run `{} up --here` in its pane to have it use this copy",
                copy.display(),
                me.join("bin/bullpen").display()
            ),
        ));
    }
    let bin = copy.join("bin/bullpen");
    if !same(&copy, &me) {
        if a_coordinators(repo, &me) {
            // a later coordinator's own copy: its agent was given this bin/bullpen
            return Ok((
                "ok",
                format!(
                    "{who} uses {}, and the watcher runs from that copy{also}",
                    copy.display()
                ),
            ));
        }
        if source(&copy)? == source(&me)? {
            return Ok((
                "ok",
                format!(
                    "{who} uses {}; this copy ({}) has the same source",
                    copy.display(),
                    me.display()
                ),
            ));
        }
        let (how, detail) = age(&copy, &me);
        return Ok((
            "fail",
            format!(
                "this is {}'s bullpen, but {who} uses {}'s, and this one's source is {how} that one's \
                 ({detail}): run {} here",
                me.display(),
                copy.display(),
                bin.display()
            ),
        ));
    }
    let w = watcher_record()?;
    if let Some(theirs) = watcher_copy(&w).filter(|t| !same(t, &copy)) {
        if crate::rex::alive(s(&w, "session"), s(&w, "block"))? {
            return Ok((
                "fail",
                format!(
                    "the watcher runs from another copy of bullpen ({}), not the one {who} uses ({}): \
                     `bullpen up` here restarts it from this one",
                    theirs.display(),
                    copy.display()
                ),
            ));
        }
    }
    if let Some(note) = read_obj(&note_path(repo))? {
        let other = PathBuf::from(s(&note, "copy"));
        if there(&other) && !a_coordinators(repo, &other) && source(&other)? != source(&copy)? {
            let (how, detail) = age(&copy, &other);
            return Ok((
                "fail",
                format!(
                    "`{}` was run here by another copy of bullpen ({}, {}), whose source is {how} this one's \
                     ({detail}), the one {who} uses. Run this repo's commands with {} (a board opened by the \
                     other copy runs the other's: open it from here); then `bullpen up` clears this{}",
                    s(&note, "command"),
                    other.display(),
                    s(&note, "at"),
                    bin.display(),
                    lacks(&copy, &other)
                ),
            ));
        }
    }
    Ok(("ok", format!("{who} uses this copy, {}{also}", copy.display())))
}

#[cfg(test)]
#[path = "test_copies.rs"]
mod tests;
