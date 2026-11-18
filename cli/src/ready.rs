//! A worker's `ready`: publish the branch (pushed with a PR, or a
//! local branch), then review it or mark it ready; its `finish`, for a task with
//! nothing to merge (or, with --keep-branch, a branch to keep unmerged); and the
//! summary a worker or reviewer gives (`summary_text`).

use crate::args::{flag, pos, value, Arg, Args, Nargs};
use crate::become_ready::{make_ready, How};
use crate::close::close_task;
use crate::modes::{is_local, publish, task_base_ref};
use crate::news::kind::Kind;
use crate::news::{news, Opts};
use crate::review::{close_reviewer, review_at_ready, shortstat};
use crate::saves::save;
use crate::tours::{tour_path, tour_stale};
use bullpen_core::json::{obj, read_obj, s, truthy, Obj};
use bullpen_core::proc::git;
use bullpen_core::repo::repo_root;
use bullpen_core::settings::CLOSED;
use bullpen_core::task::{auto_accepts, is_status, set_status};
use bullpen_core::text::strip;
use bullpen_core::{fail, Fail, Repo, Res};
use serde_json::{json, Value};

pub const SUMMARY: Arg = value("--summary", "SUMMARY", "what you did, as text");
pub const SUMMARY_FILE: Arg = value("--summary-file", "FILE", "the same, read from a file");

const ID: Arg = pos("id", Nargs::One, "the task, like BP-3");
const READY: &[Arg] = &[ID, SUMMARY, SUMMARY_FILE];
const FINISH: &[Arg] = &[
    ID,
    SUMMARY,
    SUMMARY_FILE,
    flag(
        "--keep-branch",
        "the branch has commits to keep, not merge: the person's close keeps it",
    ),
];

/// The text of --summary-file (or --summary). An empty file is refused: it is a write
/// that failed (a full disk, /tmp out of inodes), not "nothing to say".
pub fn summary_text(a: &Args, what: &str, required: bool) -> Res<String> {
    let Some(file) = a.opt("--summary-file").filter(|f| !f.is_empty()) else {
        let text = strip(a.opt("--summary").unwrap_or("")).to_string();
        if required && text.is_empty() {
            return fail(format!("give your {what} with --summary-file (or --summary)"));
        }
        return Ok(text);
    };
    let text = match std::fs::read(file) {
        Ok(bytes) => String::from_utf8_lossy(&bytes)
            .replace("\r\n", "\n")
            .replace('\r', "\n"),
        Err(e) => return fail(format!("can't read {file}: {}", strerror(&e))),
    };
    let text = strip(&text).to_string();
    if text.is_empty() {
        return fail(format!(
            "{file} is empty, so writing your {what} to it probably failed (a full disk, or /tmp out of \
             inodes). Write it again, somewhere else if need be, and rerun this command"
        ));
    }
    Ok(text)
}

/// An OS error's words, as Python's strerror ("No such file or directory").
fn strerror(e: &std::io::Error) -> String {
    let said = e.to_string();
    match e.raw_os_error() {
        Some(n) => said.trim_end_matches(&format!(" (os error {n})")).to_string(),
        None => said,
    }
}

/// `ready` (worker): push the branch, open the PR (none without a remote), and start
/// its review or tell the coordinator.
pub fn cmd_ready(argv: &[String]) -> Res<i32> {
    let a = Args::parse("ready", argv, READY);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    ready(&repo, &mut t, &a)?;
    crate::overlap::check(&repo, &t);
    Ok(0)
}

fn ready(repo: &Repo, t: &mut Obj, a: &Args) -> Res<()> {
    let id = s(t, "id").to_string();
    if !truthy(t.get("worktree")) {
        return fail(format!("{id} has no worktree"));
    }
    let wt = s(t, "worktree").to_string();
    if !strip(&git(&wt, &["status", "--porcelain"], true)?.stdout).is_empty() {
        return fail("uncommitted changes: commit (or discard) them, then run ready again");
    }
    let summary = summary_text(a, "summary", false)?;
    if let Some(Value::Object(gh)) = t.get_mut("gh").filter(|v| truthy(Some(v))) {
        gh.shift_remove("sent"); // whatever GitHub sent it back for is answered now
    }
    t.shift_remove("rebased"); // what the board says of a rebase after an accept

    // Not through tours::update_task: ready changes its copy of the task and saves it
    // all after the push, as every command that changes a task's status does, so a tour
    // write made meanwhile is lost. That is safe: a ready means the code changed, and a
    // tour is only written while the code isn't changing. One written meanwhile anyway
    // is still on disk for the stepper; at worst the task says it failed (its writer
    // gone while "writing"), and g writes it again.
    if s(obj(t, "tour"), "state") == "written" {
        let tour = read_obj(&tour_path(repo, t))?;
        if let Some(tour) = tour.filter(|o| !o.is_empty()) {
            if tour_stale(repo, t, &tour)?.is_some() {
                // new code: not toured yet, as the board says
                if let Some(Value::Object(o)) = t.get_mut("tour") {
                    o.shift_remove("opened");
                    o.insert("stale".into(), json!(true));
                }
            }
        }
    }
    t.shift_remove("from_ready");
    t.shift_remove("keep_branch"); // an earlier `finish --keep-branch`: it is to merge now
    t.shift_remove("nothing_to_merge"); // an earlier ready's with no commits: this one says again
    let local = is_local(repo, t)?;
    t.insert("local".into(), json!(local)); // its mode, fixed now: every later step goes by it

    // a task made from an issue is reviewed whatever the settings say
    let review = truthy(repo.cfg.get("review")) || s(t, "origin") == "issue";
    if !review {
        t.shift_remove("tour_sent"); // no reviewer checks the person's comments with review off
    }
    let base_ref = task_base_ref(repo, t)?;
    let count = git(&wt, &["rev-list", "--count", &format!("{base_ref}..HEAD")], true)?.stdout;
    let ahead: i64 = strip(&count)
        .parse()
        .map_err(|_| Fail(format!("invalid literal for int() with base 10: '{}'", strip(&count))))?;
    let summary_or = |t: &Obj| -> Value {
        if summary.is_empty() {
            t.get("summary").cloned().unwrap_or(Value::Null)
        } else {
            json!(summary)
        }
    };
    if ahead == 0 {
        let none = if summary.is_empty() {
            Value::Null
        } else {
            json!(summary)
        };
        t.insert("summary".into(), none);
        t.insert("diffstat".into(), Value::Null);
        make_ready(repo, t, &base_ref, How::Nothing)?;
        println!("no commits: marked ready with nothing to merge");
        return Ok(());
    }
    t.insert("diffstat".into(), json!(shortstat(&wt, &base_ref)?)); // for the board, worked out once per ready
    let published = publish(repo, t, ahead, &base_ref, &summary)?; // pushed and a PR, or nothing
    t.insert("why".into(), Value::Null);
    let kept = summary_or(t);
    t.insert("summary".into(), kept);
    t.insert("commits".into(), json!(ahead));
    if review {
        let said = review_at_ready(repo, t, &base_ref)?;
        println!("{published}. {said}");
        return Ok(());
    }
    make_ready(repo, t, &base_ref, How::Plain)?;
    println!("{published}");
    Ok(())
}

// ---------- finish: a task with nothing to merge ----------
//
// Some tasks leave nothing in the repo (a video, research notes). Their worker runs
// `finish` instead of `ready`: no PR and no review. The person closes the task, or
// auto-accept does it at once.

/// `finish` (worker): the task is done and leaves nothing to merge.
pub fn cmd_finish(argv: &[String]) -> Res<i32> {
    let a = Args::parse("finish", argv, FINISH);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    finish(&repo, &mut t, &a)?;
    Ok(0)
}

fn finish(repo: &Repo, t: &mut Obj, a: &Args) -> Res<()> {
    let id = s(t, "id").to_string();
    if CLOSED.contains(&s(t, "status")) {
        return fail(is_status(t));
    }
    if !truthy(t.get("worktree")) {
        return fail(format!("{id} has no worktree"));
    }
    let wt = s(t, "worktree").to_string();
    let keep = a.flag("--keep-branch");
    if !strip(&git(&wt, &["status", "--porcelain"], true)?.stdout).is_empty() {
        if keep {
            return fail(format!(
                "uncommitted changes: commit them, then run `finish {id} --keep-branch` again"
            ));
        }
        return fail("uncommitted changes: commit them and run `ready`, or discard them and run finish again");
    }
    let base_ref = task_base_ref(repo, t)?;
    if keep {
        let ahead = git(&wt, &["rev-list", "--count", &format!("{base_ref}..HEAD")], true)?.stdout;
        if strip(&ahead) == "0" {
            return fail(format!(
                "your branch has no commits on {base_ref}, so there's no branch to keep: run `finish {id}` without \
                 --keep-branch"
            ));
        }
    } else if !git(&wt, &["diff", "--quiet", &format!("{base_ref}...HEAD")], false)?.ok() {
        let changes = shortstat(&wt, &base_ref)?.unwrap_or_else(|| "files".into());
        return fail(format!(
            "your branch changes {changes} against {base_ref}, so there is something to merge: run `ready` instead, \
             or `finish {id} --keep-branch` if your brief asked for the branch to be kept and not merged"
        ));
    }
    let summary = summary_text(a, "summary", true)?;
    close_reviewer(t)?;
    if let Some(Value::Object(gh)) = t.get_mut("gh").filter(|v| truthy(Some(v))) {
        gh.shift_remove("sent");
    }
    t.shift_remove("rebased");
    set_status(t, "finished", None, None);
    t.insert("summary".into(), json!(summary));
    t.insert("diffstat".into(), Value::Null);
    if keep {
        t.insert("keep_branch".into(), json!(true));
    } else {
        t.shift_remove("keep_branch");
    }
    save(repo, t)?;
    let first = first_line(&summary);
    if keep {
        // never auto-accept's: the close pushes the branch, and that is the person's call
        let line = format!("{id} finished (branch to be kept, not merged): {first}");
        news(repo, t, Kind::Finished, &line, Opts::default())?;
        println!("{id} finished, its branch to be kept. The person closes it: end your turn.");
        return Ok(());
    }
    if auto_accepts(repo, t) {
        println!("{id} finished, with nothing to merge; auto-accept closes it now. End your turn.");
        return close_task(repo, t, true, &first); // last: this closes the pane we run in
    }
    news(
        repo,
        t,
        Kind::Finished,
        &format!("{id} finished (nothing to merge): {first}"),
        Opts::default(),
    )?;
    println!("{id} finished, with nothing to merge. The person closes it: end your turn.");
    Ok(())
}

/// A summary's first line, a Markdown title's marks dropped.
pub fn first_line(summary: &str) -> String {
    let line = bullpen_core::text::splitlines(summary).first().copied().unwrap_or("");
    strip(strip(line).trim_start_matches('#')).to_string()
}

#[cfg(test)]
#[path = "tests/test_ready.rs"]
mod tests;
