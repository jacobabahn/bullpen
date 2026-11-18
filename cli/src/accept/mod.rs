//! Accepting a ready task: merging it, closing it, then what follows
//! the merge, each step on its own (`run_steps`: a failed one is said, and the rest
//! still run).
//! Rebasing the other tasks onto the moved base is rebase.rs.
//!
//! An accept in a checkout of bullpen that runs from its own build (this copy's, or one
//! another copy merged into: docs/rebuilds.md) may replace the code under it: the merge (local
//! mode) or the rebuild after it puts new source and, later, a new build in place. This
//! process is one compiled image, so it runs its own code to the end; the next call runs
//! the new build (docs/rebuilds.md).

pub mod left_in_review;
pub mod rebase;

use crate::args::{pos, Arg, Args, Nargs};
use crate::build::rebuild_merged_into;
use crate::close::{close, close_task};
use crate::guide::tell_guide_changes;
use crate::keys::{keys_hash, KEYS_CHANGED};
use crate::modes::{after_merge, merge, task_base_ref};
use crate::news::kind::Kind;
use crate::news::{news, tell_coordinator, Opts};
use crate::saves::run_later;
use crate::watcher::refresh_watcher;
use bullpen_core::json::{obj, py_str, s, truthy, Obj};
use bullpen_core::repo::repo_root;
use bullpen_core::task::{is_status, keeps_branch, pr_short};
use bullpen_core::text::squash;
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

const ACCEPT: &[Arg] = &[pos("id", Nargs::One, "the task, like BP-3")];

/// `accept` (the person's call): merge a ready task, or close a finished one.
pub fn cmd_accept(argv: &[String]) -> Res<i32> {
    let a = Args::parse("accept", argv, ACCEPT);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    accept(&repo, &mut t, false)?;
    Ok(0)
}

/// Merge a ready task (its PR, or its branch locally) and close it; a finished one
/// (nothing to merge) just closes. auto: review approved it and auto-accept is on.
/// Fails, leaving the task as it was, when it can't merge.
pub fn accept(repo: &Repo, t: &mut Obj, auto: bool) -> Res<()> {
    let merged_outside = s(obj(t, "gh"), "state") == "merged";
    let status = s(t, "status");
    if keeps_branch(t) && !merged_outside {
        return crate::keep::keep_branch(repo, t, None); // the person's yes on a branch to be kept
    }
    if status == "finished" && !merged_outside {
        return close_task(repo, t, false, ""); // the person's yes on work with nothing to merge
    }
    if status != "ready" && !merged_outside {
        return fail(format!("{}, not ready to accept or finished", is_status(t)));
    }
    let keys = keys_hash();
    let touched = changed_files(repo, t)?; // before the merge: what the others are checked against
    merge(repo, t)?; // fails, leaving the task as it was, if it can't
    let then = |t: &Obj| {
        accepted(repo, t, auto, merged_outside, touched, keys);
        Ok(())
    };
    close(repo, t, "done", then, false, true, &[("auto_accepted", json!(auto))])
}

/// One step after the merge, by what it does in words.
pub type Step<'a> = (&'static str, Box<dyn FnOnce() -> Res<()> + 'a>);

/// What follows a merge, once the task is closed: the news, the merge's follow-up in
/// the task's mode, the other tasks rebased, and whatever the merge changed in bullpen
/// itself. Each step runs whatever another did: one that fails is said, and the rest go on.
fn accepted(
    repo: &Repo,
    t: &Obj,
    auto: bool,
    merged_outside: bool,
    touched: Option<BTreeSet<String>>,
    keys: Option<String>,
) {
    let id = s(t, "id");
    let merge_how = py_str(repo.cfg.get("merge").unwrap_or(&Value::Null));
    let how = if truthy(t.get("pr")) {
        let pr = py_str(t.get("pr").unwrap_or(&Value::Null));
        println!("{id} done (merged {pr})");
        if merged_outside {
            format!("{} was merged outside bullpen", pr_short(&pr))
        } else {
            format!("merged {}, {merge_how}", pr_short(&pr))
        }
    } else if truthy(t.get("merged")) {
        let base = py_str(t.get("base").unwrap_or(&Value::Null));
        let merged: String = s(t, "merged").chars().take(12).collect();
        println!("{id} done (merged into {base}: {merged})");
        format!("merged into {base}, {merge_how}")
    } else {
        println!("{id} done");
        "nothing to merge".to_string()
    };
    let merged = how != "nothing to merge";
    let steps: Vec<Step> = vec![
        (
            "the accepted news",
            Box::new(|| {
                if auto {
                    let notes = crate::notes::with_notes(t);
                    let line = format!("{id} approved by review{notes} and auto-accepted ({how})");
                    news(repo, t, Kind::AutoAccepted, &line, Opts::default())
                } else {
                    tell_coordinator(repo, t, Kind::Accepted, &format!("{id} accepted ({how})"))
                }
            }),
        ),
        (
            "the merge's follow-up",
            Box::new(|| {
                // the branch deleted on origin, or the push remote
                if let Some(said) = after_merge(repo, t)? {
                    println!("{said}");
                }
                Ok(())
            }),
        ),
        // before the rest: a command the merge moved runs in neither the old build nor the
        // new Python until it's built (a call in that window waits for it: the catch-up)
        ("rebuilding bullpen and the board", Box::new(|| rebuild(repo))),
        (
            "rebasing the other tasks",
            Box::new(|| match merged {
                true => rebase::rebase_others(repo, t, touched.as_ref()),
                false => Ok(()),
            }),
        ),
        (
            "the run queue",
            Box::new(|| {
                run_later(); // starts the next task now, before the rest
                Ok(())
            }),
        ),
        (
            "telling the coordinators about guide changes",
            Box::new(|| {
                tell_guide_changes(&mut Repo::new(&repo.root)?);
                Ok(())
            }),
        ),
        (
            "refreshing the watcher", // the merge may have changed it
            Box::new(|| {
                refresh_watcher(repo);
                Ok(())
            }),
        ),
        (
            "the keys-changed news",
            Box::new(|| {
                if keys_hash() != keys {
                    // last: the line the board shows
                    let everyone = Opts {
                        everyone: true,
                        ..Opts::default()
                    };
                    // the board's line, and for a coordinator, whose it is to do
                    let line =
                        "Keys changed: tell the person to run `rex config reload` (the cmd+; keys run the old ones \
                                until then)";
                    news(repo, t, Kind::Keys, line, everyone)?;
                    println!("{KEYS_CHANGED}");
                }
                Ok(())
            }),
        ),
    ];
    run_steps(id, steps, &mut |line| {
        let line = format!("{line}. The accept still counts: tell the person");
        news(repo, t, Kind::AcceptFailed, &line, Opts::default())
    });
}

/// Run each step after an accept of task id. One that fails is said (stderr, and the
/// line to tell, the news to the coordinator) rather than lose the rest.
pub fn run_steps(id: &str, steps: Vec<Step>, tell: &mut dyn FnMut(&str) -> Res<()>) {
    for (what, step) in steps {
        let Err(e) = step() else { continue };
        let why = match squash(&e.0) {
            why if why.is_empty() => "Fail".to_string(),
            why => why,
        };
        let line = format!("{id} was accepted, but {what} failed: {why}");
        eprintln!("bullpen: {line}");
        if let Err(e2) = tell(&line) {
            eprintln!("bullpen: and the news of it failed too: {e2}");
        }
    }
}

/// A checkout of bullpen that runs from its own build (this copy's, or another's): its
/// programs too, when their source changed.
fn rebuild(repo: &Repo) -> Res<()> {
    if let Some(said) = rebuild_merged_into(repo) {
        println!("{said}");
    }
    Ok(())
}

/// The files a task's branch changes against its base (both paths of a rename), or None
/// when that can't be told (no worktree, or a base that already has the branch).
fn changed_files(repo: &Repo, t: &Obj) -> Res<Option<BTreeSet<String>>> {
    let wt = s(t, "worktree");
    if wt.is_empty() || !Path::new(wt).is_dir() {
        return Ok(None);
    }
    let range = format!("{}...HEAD", task_base_ref(repo, t)?);
    let Some(files) = crate::git_names::names(wt, &["diff", "--name-only", "--no-renames", &range])? else {
        return Ok(None);
    };
    let files: BTreeSet<String> = files.into_iter().collect();
    Ok((!files.is_empty()).then_some(files))
}

#[cfg(test)]
#[path = "test_accept.rs"]
mod tests;
