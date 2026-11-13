//! A reviewer's `verdict`: approve the task, or send its
//! findings back to the worker. With auto-accept (`config --auto-accept on`), an
//! approve verdict runs the normal accept itself (`_auto_accept`): the person said in
//! advance that an approval is enough. A task's own "auto_accept" (`add`/`start
//! --no-auto-accept`, or `--auto-accept`) wins over the config's. In remote mode it
//! waits for the PR's checks ("auto_wait"), which the PR follow-up (prs.rs) looks at.

use crate::accept::accept;
use crate::agents::pane;
use crate::args::{pos, Arg, Args, Nargs};
use crate::jobs::{background, Job};
use crate::news::ready_news;
use crate::ready::{summary_text, SUMMARY, SUMMARY_FILE};
use crate::review::{review_kind, rounds};
use crate::rex::close_last;
use crate::saves::save;
use crate::start::launch::hand_back;
use crate::stops::stop;
use bullpen_core::json::{obj, py_str, s, truthy, write_obj, Obj};
use bullpen_core::paths::self_bin;
use bullpen_core::proc::run_in;
use bullpen_core::repo::{repo_root, unindex_block};
use bullpen_core::settings::WAITING;
use bullpen_core::task::pr_short;
use bullpen_core::task::{auto_accepts, clip, commits, set_status, where_};
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};

const VERDICT: &[Arg] = &[
    pos("id", Nargs::One, "the task, like BP-3"),
    pos(
        "verdict",
        Nargs::One,
        "approve it, or send it back with changes to make",
    )
    .choices(&["approve", "changes"]),
    SUMMARY,
    SUMMARY_FILE,
];

/// `verdict` (reviewer): approve the task, or send findings back to its worker.
pub fn cmd_verdict(argv: &[String]) -> Res<i32> {
    let a = Args::parse("verdict", argv, VERDICT);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    let id = s(&t, "id").to_string();
    let r = obj(&t, "review").clone();
    let status = s(&t, "status");
    if !(status == "review" || WAITING.contains(&status)) || !truthy(r.get("block")) {
        return fail(format!("{id} isn't in review"));
    }
    let approve = a.arg("verdict") == Some("approve");
    let text = summary_text(&a, "findings", true)?;
    let (tour, rebase) = (s(&r, "kind") == "tour", s(&r, "kind") == "rebase");
    let round = py_str(r.get("round").unwrap_or(&Value::Null));
    let name = if tour {
        format!(
            "{id}.tour-{}.review.findings.md",
            py_str(r.get("tour").unwrap_or(&Value::Null))
        )
    } else if rebase {
        format!("{id}.review-{round}.rebase.findings.md")
    } else {
        format!("{id}.review-{round}.findings.md")
    };
    let findings = repo.dir.join("briefs").join(name);
    std::fs::write(&findings, format!("{text}\n"))?;
    let findings = findings.display().to_string();
    let rounds = rounds(&repo);
    let reviewer = pane(&t, "reviewer");
    unindex_block(s(&r, "block"))?;
    if let Some(Value::Object(r)) = t.get_mut("review") {
        r.insert("verdict".into(), json!(if approve { "approved" } else { "changes" }));
        r.insert("findings".into(), json!(findings));
        r.insert("block".into(), Value::Null);
    }
    let r = obj(&t, "review").clone();
    let pr = where_(&t);
    t.shift_remove("auto_wait"); // a new verdict decides again
    if tour {
        t.shift_remove("tour_sent"); // reviewed: the next review follows up on this one
    }
    let n = r.get("round").and_then(Value::as_i64).unwrap_or(0);
    if approve {
        set_status(&mut t, "ready", None, None);
        save(&repo, &mut t)?;
        if auto_accepts(&repo, &t) {
            auto_accept(&repo, &id);
        } else {
            let why = match r.get("why").filter(|w| truthy(Some(w))) {
                Some(w) => format!(": {}", py_str(w)), // last, so not in brackets in brackets
                None => String::new(),
            };
            let mut plain = r.clone();
            plain.insert("why".into(), Value::Null);
            let said = if rebase {
                "a rebase check approved the conflict resolution".to_string()
            } else {
                format!("approved by {}{why}", review_kind(&plain))
            };
            let done = t.get("commits").and_then(Value::as_i64).unwrap_or(0);
            ready_news(&repo, &mut t, &[commits(done), pr, said], "", false)?;
        }
    } else if tour || rebase || n < rounds {
        let said = if tour {
            "The reviewer says the person's comments aren't all addressed yet".to_string()
        } else if rebase {
            "A rebase check says the conflict resolution isn't right".to_string()
        } else {
            format!("The reviewer asked for changes (review round {n} of {rounds})")
        };
        let msg = format!(
            "{said}. Read {findings}, fix what you agree with, commit, and run your ready command again. If you \
             disagree with a finding, say why in your ready summary."
        );
        if let Some(Value::Object(gh)) = t.get_mut("gh").filter(|v| truthy(Some(v))) {
            gh.shift_remove("sent"); // it's the review it is fixing now, not GitHub
        }
        let failed = match hand_back(&repo, &mut t, &msg) {
            Ok(failed) => failed,
            Err(e) => Some(clip(&e.0, 160)),
        };
        if let Some(failed) = failed {
            // nobody is on it: the coordinator passes the findings on
            let line = format!(
                "{id}'s reviewer asked for changes, but the message didn't reach its worker ({failed}). The \
                 findings are in {findings}: `bullpen tell {id}` it to read them"
            );
            let why = "Review findings didn't reach its worker";
            stop(&repo, &mut t, "worker", "not_sent", why, &[line], "not-sent")?;
        }
    } else {
        let why = format!("Review still asks for changes after {rounds} rounds");
        set_status(&mut t, "ready", Some(&why), None);
        save(&repo, &mut t)?;
        let then = format!(", but review still asks for changes after {rounds} rounds: findings in {findings}");
        ready_news(&repo, &mut t, &[pr], &then, false)?;
    }
    println!("verdict recorded: {}", a.arg("verdict").unwrap_or(""));
    close_last(&reviewer.0, &reviewer.1)?; // last: this closes the pane we may be running in
    Ok(0)
}

/// An approved task with auto-accept: `_auto_accept` accepts it now, or once
/// its PR's checks pass. It runs here, as a job (its output in jobs.log, a failure it
/// doesn't say itself in the news), and the verdict waits for it: the verdict closes
/// the reviewer's pane only after, and the accept closes the worker's, not this one.
fn auto_accept(repo: &Repo, id: &str) {
    let log = repo.dir.join("jobs.log");
    let job = Job {
        argv: vec![self_bin().display().to_string(), "_auto_accept".into(), id.into()],
        cwd: &repo.root,
        log: &log,
        what: format!("{id}'s auto-accept"),
        task: Some(id),
        pass_fds: vec![],
    };
    background(&job, true);
}

// ---------- auto-accept ----------

const AUTO_ACCEPT: &[Arg] = &[pos("id", Nargs::One, "the task, like BP-3")];

/// `_auto_accept` (bullpen): the auto-accept of an approval, away from the worker's and
/// the reviewer's panes.
pub fn cmd_auto_accept(argv: &[String]) -> Res<i32> {
    let a = Args::parse("_auto_accept", argv, AUTO_ACCEPT);
    let repo = Repo::new(repo_root()?)?;
    let mut t = repo.task(a.arg("id").unwrap_or(""))?;
    if s(&t, "status") == "ready" && s(obj(&t, "review"), "verdict") == "approved" {
        accept_approved(&repo, &mut t)?;
    }
    Ok(0)
}

/// An approved task: accept it now, or, with a PR whose checks are still running, once
/// they pass. Anything that stops it leaves the task ready, with news.
fn accept_approved(repo: &Repo, t: &mut Obj) -> Res<()> {
    if truthy(t.get("pr")) && !truthy(t.get("local")) {
        let facts = [pr_short(&py_str(&t["pr"])), "approved by review".to_string()];
        match pr_checks_now(repo, t)? {
            "failing" => {
                let then = ", but not auto-accepted: its PR's checks are failing";
                return ready_news(repo, t, &facts, then, false);
            }
            "running" => {
                t.insert("auto_wait".into(), json!(true));
                write_obj(&repo.task_path(s(t, "id")), t)?; // not a save: the status didn't change
                return ready_news(repo, t, &facts, "; it auto-accepts once its PR's checks pass", false);
            }
            _ => {}
        }
    }
    run_auto_accept(repo, t)
}

/// "failing", "running", or "passing" (no checks counts as passing). Unsure (gh failed)
/// counts as running: the PR follow-up looks again.
fn pr_checks_now(repo: &Repo, t: &Obj) -> Res<&'static str> {
    let Some(pr) = pr_view(repo, t)? else {
        return Ok("running");
    };
    Ok(checks(&pr).0.unwrap_or("passing"))
}

/// The task's PR as `gh pr view` gives it, or None when gh failed or said no JSON.
pub fn pr_view(repo: &Repo, t: &Obj) -> Res<Option<Obj>> {
    let pr = py_str(t.get("pr").unwrap_or(&Value::Null));
    let argv = [
        repo.gh(),
        "pr".into(),
        "view".into(),
        pr,
        "--json".into(),
        PR_FIELDS.into(),
    ];
    let p = run_in(&argv, &repo.root, false)?;
    if !p.ok() {
        return Ok(None);
    }
    Ok(match serde_json::from_str::<Value>(&p.stdout) {
        Ok(Value::Object(o)) => Some(o),
        Ok(_) => Some(Obj::new()),
        Err(_) => None,
    })
}

/// The normal accept, as if the person ran it. A refused merge leaves the task ready.
pub fn run_auto_accept(repo: &Repo, t: &mut Obj) -> Res<()> {
    std::env::set_current_dir(&repo.root)?; // we may be running in the worktree accept removes
    let Err(e) = accept(repo, t, true) else {
        return Ok(());
    };
    let mut t = repo.task(s(t, "id"))?;
    t.shift_remove("auto_wait");
    write_obj(&repo.task_path(s(&t, "id")), &t)?;
    let facts = [where_(&t), "approved by review".to_string()];
    let then = format!(", but the auto-accept was refused: {e}. Send it back to rebase");
    ready_news(repo, &mut t, &facts, &then, false)
}

const FAILED_CHECK: [&str; 6] = [
    "FAILURE",
    "ERROR",
    "TIMED_OUT",
    "CANCELLED",
    "ACTION_REQUIRED",
    "STARTUP_FAILURE",
];
pub const PR_FIELDS: &str = "state,headRefOid,statusCheckRollup,reviews,comments";

/// The first of these keys with a truthy value, as text, else or.
pub fn first_of(o: &Obj, keys: &[&str], or: &str) -> String {
    keys.iter()
        .find_map(|k| o.get(*k).filter(|v| truthy(Some(v))))
        .map_or(or.to_string(), py_str)
}

/// (summary, names of failed checks) from a PR's statusCheckRollup.
pub fn checks(pr: &Obj) -> (Option<&'static str>, Vec<String>) {
    let (mut failed, mut running, mut total) = (Vec::new(), false, 0);
    let (none, empty) = (Vec::new(), Obj::new());
    for c in pr.get("statusCheckRollup").and_then(Value::as_array).unwrap_or(&none) {
        total += 1;
        let c = c.as_object().unwrap_or(&empty);
        let name = first_of(c, &["name", "context"], "check");
        let result = first_of(c, &["conclusion", "state"], "");
        let pending = matches!(result.as_str(), "PENDING" | "EXPECTED");
        if FAILED_CHECK.contains(&result.as_str()) {
            failed.push(name);
        } else if first_of(c, &["status"], "COMPLETED") != "COMPLETED" || pending {
            running = true;
        }
    }
    let summary = if !failed.is_empty() {
        Some("failing")
    } else if running {
        Some("running")
    } else if total > 0 {
        Some("passing")
    } else {
        None
    };
    (summary, failed)
}

#[cfg(test)]
#[path = "test_auto_accept.rs"]
mod tests;
