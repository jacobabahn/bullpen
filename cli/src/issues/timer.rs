//! The watcher's timer for GitHub issues (docs/issues.md). `<state>/issues` exists
//! while some repo has `issues` on, and holds when the next read is due (seconds since
//! 1970; empty: now), so the watcher's tick costs one small file read and calls
//! `_issues` only then. `_issues` starts a read, as a job (jobs.rs: gh can be slow, and
//! the watcher mustn't wait for it), in each repo with `issues` on whose last one is
//! `issues_every` minutes old, and writes when the next is due.
//!
//! The job (`_issues --read`) is the read `bullpen issues --now` does. New issues are
//! one news line for the lot, which wakes a coordinator: each asks the person. A read
//! that fails (gh signed out, no network) is one news line the first time and silence
//! while it keeps failing; it never stops the watcher. `issues-timer.json` in the
//! repo's state folder holds when the last read started and what it failed with.

use super::text;
use crate::args::{flag as arg_flag, Arg, Args};
use crate::jobs::{background, Job};
use crate::news::kind::Kind;
use crate::news::{news, Opts};
use bullpen_core::json::{self, s, truthy, Obj};
use bullpen_core::paths::{self_bin, state};
use bullpen_core::repo::{known_repos, repo_root};
use bullpen_core::task::clip;
use bullpen_core::timeutil::{iso_now, seconds_since};
use bullpen_core::{Fail, Repo, Res};
use serde_json::{json, Value};
use std::path::PathBuf;

const TIMER: &[Arg] = &[arg_flag("--read", "")];

/// <state>/issues: there while some repo has `issues` on; in it, when the next read is due.
pub fn flag() -> PathBuf {
    state().join("issues")
}

/// Have the watcher look at its next tick (`issues` was turned on, or `issues_every`
/// changed): only a repo whose read is due is read then.
pub fn soon() -> Res<()> {
    std::fs::create_dir_all(state())?;
    Ok(std::fs::write(flag(), "")?)
}

/// `up`, in a repo with `issues` on: the watcher's file is there. It is missing where
/// `issues` was turned on before there was a timer, or after an `_issues` that couldn't
/// read this repo's config just then and so found no repo with issues on.
pub fn ensure(repo: &Repo) -> Res<()> {
    if on(repo) && !flag().exists() {
        soon()?;
    }
    Ok(())
}

fn on(repo: &Repo) -> bool {
    truthy(repo.cfg.get("issues"))
}

/// Seconds between a repo's reads (`issues_every`, in minutes).
fn every(repo: &Repo) -> f64 {
    let d = bullpen_core::settings::setting("issues_every").map(|s| s.default_value());
    let minutes = repo.cfg.get("issues_every").and_then(Value::as_i64);
    let minutes = minutes.or_else(|| d.and_then(|d| d.as_i64())).unwrap_or(30);
    minutes.max(1) as f64 * 60.0
}

/// Change `issues-timer.json` (`at`: when the last read started; `failed`: what the
/// last one failed with), under a lock: `_issues` and a read's job both write it.
fn update<T>(repo: &Repo, f: impl FnOnce(&mut Obj) -> T) -> Res<T> {
    std::fs::create_dir_all(&repo.dir)?;
    let _held = bullpen_core::lock::lock(&repo.dir.join("issues-timer.lock"))?;
    let path = repo.dir.join("issues-timer.json");
    let mut st = json::read_obj(&path)?.unwrap_or_default();
    let out = f(&mut st);
    json::write_obj(&path, &st)?;
    Ok(out)
}

/// Seconds until the repo's next read is due, starting one now if it is.
fn tick(repo: &Repo) -> Res<f64> {
    let every = every(repo);
    let wait = update(repo, |st| {
        let wait = match s(st, "at") {
            "" => 0.0,
            at => every - seconds_since(at, None),
        };
        if wait <= 0.0 {
            st.insert("at".into(), json!(iso_now()));
        }
        wait
    })?;
    if wait > 0.0 {
        return Ok(wait);
    }
    start(repo);
    Ok(every)
}

/// Start the repo's read, as a job: its output in jobs.log. The tests wait for it
/// (BULLPEN_ISSUES_WAIT).
fn start(repo: &Repo) {
    let log = repo.dir.join("jobs.log");
    let job = Job {
        argv: vec![self_bin().display().to_string(), "_issues".into(), "--read".into()],
        cwd: &repo.root,
        log: &log,
        what: "Reading the GitHub issues".into(),
        task: None,
        pass_fds: vec![],
    };
    background(&job, std::env::var("BULLPEN_ISSUES_WAIT").is_ok_and(|v| !v.is_empty()));
}

/// `_issues`: the watcher's timer. A read in every repo that is due one; then the flag
/// says when the next is due, or goes when no repo has `issues` on.
pub fn cmd_timer(argv: &[String]) -> Res<i32> {
    let a = Args::parse("_issues", argv, TIMER);
    if a.flag("--read") {
        timed_read(&Repo::new(repo_root()?)?)?;
        return Ok(0);
    }
    let mut next: Option<f64> = None;
    for repo in known_repos()?.iter().filter(|r| on(r)) {
        // a repo that can't be looked at now is looked at again in a minute
        let wait = tick(repo).unwrap_or_else(|e| {
            eprintln!("bullpen: {}: {e}", repo.name());
            60.0
        });
        next = Some(next.map_or(wait, |n| n.min(wait)));
    }
    match next {
        Some(wait) => {
            let due = chrono::Local::now().timestamp() + wait.ceil() as i64;
            std::fs::write(flag(), format!("{due}\n"))?;
        }
        None => match std::fs::remove_file(flag()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        },
    }
    Ok(0)
}

/// News about the repo's issues: for every coordinator, about no one task.
fn say(repo: &Repo, text: &str) -> Res<()> {
    let about: Obj = json!({"id": "issues"}).as_object().cloned().unwrap_or_default();
    news(
        repo,
        &about,
        Kind::Issues,
        text,
        Opts {
            everyone: true,
            ..Opts::default()
        },
    )
}

/// The timer's read (`_issues --read`, the job): what changed goes to the job's log,
/// new issues are one news line, and a failure is news only when the read before it
/// worked.
fn timed_read(repo: &Repo) -> Res<()> {
    if !on(repo) {
        return Ok(()); // turned off since the job was started
    }
    match super::read(repo) {
        Ok(read) => {
            for c in &read.changes {
                println!("{}", c.line);
            }
            update(repo, |st| st.shift_remove("failed"))?;
            let new: Vec<(i64, String)> = read.changes.into_iter().filter_map(|c| c.new).collect();
            if !new.is_empty() {
                say(repo, &text::news_line(&new))?;
            }
        }
        Err(Fail(e)) => {
            println!("{e}");
            let before = update(repo, |st| st.insert("failed".into(), json!(e)))?;
            if before.is_none() {
                say(repo, &failed_line(repo, &e))?;
            }
        }
    }
    Ok(())
}

/// The news of a read that failed, said once.
fn failed_line(repo: &Repo, why: &str) -> String {
    let minutes = (every(repo) / 60.0) as i64;
    format!(
        "The GitHub issues weren't read: {}. Tell the person (`bullpen doctor` checks gh). bullpen tries again \
         every {minutes} min, and says nothing more while it keeps failing",
        clip(why, 300)
    )
}
