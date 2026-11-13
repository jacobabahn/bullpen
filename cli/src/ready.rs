//! A worker's `ready` (bullpen/ready.py): publish the branch (pushed with a PR, or a
//! local branch), then review it or mark it ready; and the summary a worker or reviewer
//! gives. Python keeps ready.py's `summary_text` (a twin) for `finish` until T8.

use crate::args::{pos, value, Arg, Args, Nargs};
use crate::modes::{is_local, publish, task_base_ref};
use crate::news::ready_news;
use crate::review::{patch_id, review_at_ready, shortstat};
use crate::saves::save;
use bullpen_core::json::{obj, read_obj, s, truthy, Obj};
use bullpen_core::proc::{git, head};
use bullpen_core::repo::repo_root;
use bullpen_core::task::{commits, set_status, where_};
use bullpen_core::text::strip;
use bullpen_core::{fail, Fail, Repo, Res};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const SUMMARY: Arg = value("--summary", "SUMMARY", "what you did, as text");
pub const SUMMARY_FILE: Arg = value("--summary-file", "FILE", "the same, read from a file");

const READY: &[Arg] = &[pos("id", Nargs::One, "the task, like BP-3"), SUMMARY, SUMMARY_FILE];

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
    let local = is_local(repo, t)?;
    t.insert("local".into(), json!(local)); // its mode, fixed now: every later step goes by it
    let review = truthy(repo.cfg.get("review"));
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
        set_status(t, "ready", Some("Nothing to merge (no commits)"), None);
        let none = if summary.is_empty() {
            Value::Null
        } else {
            json!(summary)
        };
        t.insert("summary".into(), none);
        t.insert("diffstat".into(), Value::Null);
        save(repo, t)?;
        ready_news(repo, t, &["nothing to merge".into()], "", false)?;
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
    set_status(t, "ready", None, None);
    save(repo, t)?;
    ready_news(repo, t, &[commits(ahead), where_(t)], "", false)?;
    println!("{published}");
    Ok(())
}

/// The task's tour (tours.py's tour_path).
fn tour_path(repo: &Repo, t: &Obj) -> PathBuf {
    repo.dir.join("briefs").join(format!("{}.tour.json", s(t, "id")))
}

/// Why the tour no longer shows the task's code, or None while it does: the same
/// commit, or the same patch (after a clean rebase, as approvals carry over).
fn tour_stale(repo: &Repo, t: &Obj, tour: &Obj) -> Res<Option<String>> {
    let wt = s(t, "worktree");
    if wt.is_empty() || !Path::new(wt).is_dir() {
        return Ok(None); // nothing to compare with: the tour is all there is
    }
    let commit = s(tour, "commit");
    if head(wt)? == commit {
        return Ok(None);
    }
    let patch = patch_id(wt, &task_base_ref(repo, t)?)?;
    if patch.map_or(Value::Null, Value::from) == tour.get("patch").cloned().unwrap_or(Value::Null) {
        return Ok(None);
    }
    let short: String = commit.chars().take(12).collect();
    Ok(Some(format!(
        "the code changed since the tour was written (on {short})"
    )))
}
