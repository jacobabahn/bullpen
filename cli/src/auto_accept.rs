//! A reviewer's `verdict`: approve the task, or send its
//! findings back to the worker. With auto-accept (`config --auto-accept on`), an
//! approve verdict runs the normal accept itself (`_auto_accept`): the person said in
//! advance that an approval is enough. A task's own "auto_accept" (`add`/`start
//! --no-auto-accept`, or `--auto-accept`) wins over the config's. In remote mode it
//! waits for the PR's checks ("auto_wait"), which the PR follow-up (prs.rs) looks at.
//!
//! An approval with auto-accept runs `_auto_accept` as a job and waits for it, before the
//! verdict closes the reviewer's pane; `_auto_accept` accepts, or waits for the PR's
//! checks (`checks`, `pr_view`). A merge refused for a conflict sends the task back to
//! rebase (`base_conflict`, `send_back_conflict` in accept/rebase.rs), at most
//! `CONFLICT_SENDS` times.

use crate::accept::accept;
use crate::accept::left_in_review;
use crate::accept::rebase::{base_conflict, names_of, send_back_conflict};
use crate::agents::pane;
use crate::args::{pos, Arg, Args, Nargs};
use crate::become_ready::{make_ready, How};
use crate::jobs::{background, Job};
use crate::news::kind::Kind;
use crate::news::{ready_news, say};
use crate::notes::with_notes;
use crate::ready::{summary_text, SUMMARY, SUMMARY_FILE};
use crate::review::{remove_scratch, review_kind, rounds};
use crate::rex::close_last;
use crate::start::launch::hand_back;
use crate::stops::stop;
use bullpen_core::json::{obj, py_str, s, truthy, write_obj, Obj};
use bullpen_core::paths::self_bin;
use bullpen_core::proc::run_in;
use bullpen_core::repo::{repo_root, unindex_block};
use bullpen_core::settings::WAITING;
use bullpen_core::task::pr_short;
use bullpen_core::task::{clip, where_};
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
    remove_scratch(&r)?; // the findings are copied: the reviewer's own files can go
    let rounds = rounds(&repo);
    let reviewer = pane(&t, "reviewer");
    unindex_block(s(&r, "block"))?;
    if let Some(Value::Object(r)) = t.get_mut("review") {
        r.insert("verdict".into(), json!(if approve { "approved" } else { "changes" }));
        r.insert("findings".into(), json!(findings));
        r.insert("block".into(), Value::Null);
    }
    let r = obj(&t, "review").clone();
    t.shift_remove("auto_wait"); // a new verdict decides again
    if tour {
        t.shift_remove("tour_sent"); // reviewed: the next review follows up on this one
    }
    let n = r.get("round").and_then(Value::as_i64).unwrap_or(0);
    let left_by = left_in_review::take(&mut t); // accepts while it was in review
    if approve {
        let why = match r.get("why").filter(|w| truthy(Some(w))) {
            Some(w) => format!(": {}", py_str(w)), // last, so not in brackets in brackets
            None => String::new(),
        };
        let mut plain = r.clone();
        plain.insert("why".into(), Value::Null);
        let notes = with_notes(&t);
        let said = if rebase {
            format!("a rebase check approved the conflict resolution{notes}")
        } else {
            match (why.is_empty(), notes.is_empty()) {
                (false, false) => format!("approved by {}{why},{notes}", review_kind(&plain)),
                _ => format!("approved by {}{notes}{why}", review_kind(&plain)),
            }
        };
        let base_ref = crate::modes::task_base_ref(&repo, &t)?;
        let how = How::Approved {
            text: &text,
            said,
            left_by: &left_by,
        };
        make_ready(&repo, &mut t, &base_ref, how)?;
    } else if tour || rebase || n < rounds {
        let said = if tour {
            "The reviewer says the person's comments aren't all addressed yet".to_string()
        } else if rebase {
            "A rebase check says the conflict resolution isn't right".to_string()
        } else {
            format!("The reviewer asked for changes (review round {n} of {rounds})")
        };
        let fix = match crate::notes::notes_of(&text) {
            Some(_) => "fix the findings you agree with (its Notes are for the coordinator, not you)",
            None => "fix what you agree with",
        };
        let msg = format!(
            "{said}. Read {findings}, {fix}, commit, and run your ready command again. If you disagree with a \
             finding, say why in your ready summary."
        );
        if let Some(Value::Object(gh)) = t.get_mut("gh").filter(|v| truthy(Some(v))) {
            gh.shift_remove("sent"); // it's the review it is fixing now, not GitHub
        }
        let failed = match hand_back(&repo, &mut t, &msg, if rebase { "rebase" } else { "review" }) {
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
            stop(&repo, &mut t, "worker", "not_sent", why, &[line], Kind::NotSent)?;
        }
    } else {
        let base_ref = crate::modes::task_base_ref(&repo, &t)?;
        let how = How::OutOfRounds {
            rounds,
            findings: &findings,
        };
        make_ready(&repo, &mut t, &base_ref, how)?;
    }
    println!("verdict recorded: {}", a.arg("verdict").unwrap_or(""));
    close_last(&reviewer.0, &reviewer.1)?; // last: this closes the pane we may be running in
    Ok(0)
}

/// An approved task with auto-accept: `_auto_accept` accepts it now, or once
/// its PR's checks pass. It runs as a job (its output in jobs.log, a failure it
/// doesn't say itself in the news). wait: the caller waits for it (the verdict, which
/// closes the reviewer's pane only after, while the accept closes the worker's); a
/// `ready` doesn't, as it runs in the worker's pane, which accepting closes.
pub fn start_auto_accept(repo: &Repo, id: &str, wait: bool) {
    let log = repo.dir.join("jobs.log");
    let job = Job {
        argv: vec![self_bin().display().to_string(), "_auto_accept".into(), id.into()],
        cwd: &repo.root,
        log: &log,
        what: format!("{id}'s auto-accept"),
        task: Some(id),
        pass_fds: vec![],
    };
    background(&job, wait);
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
        let facts = [
            pr_short(&py_str(&t["pr"])),
            format!("approved by review{}", with_notes(t)),
        ];
        match pr_checks_now(repo, t)? {
            "failing" => {
                refused(repo, t, "checks", "its PR's checks are failing");
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

/// The normal accept, as if the person ran it. A merge refused for a conflict with the
/// base sends the task back to its worker to rebase (up to CONFLICT_SENDS times); any
/// other refusal, or one past that, leaves the task ready, with news.
pub fn run_auto_accept(repo: &Repo, t: &mut Obj) -> Res<()> {
    std::env::set_current_dir(&repo.root)?; // we may be running in the worktree accept removes
    let Err(e) = accept(repo, t, true) else {
        return Ok(());
    };
    let mut t = repo.task(s(t, "id"))?;
    t.shift_remove("auto_wait");
    write_obj(&repo.task_path(s(&t, "id")), &t)?;
    let facts = [where_(&t), format!("approved by review{}", with_notes(&t))];
    let conflict = base_conflict(repo, &t)?;
    refused(repo, &t, if conflict.is_some() { "conflict" } else { "other" }, &e.0);
    let Some((base_ref, files, shown)) = conflict else {
        // not a conflict: the person sees to it
        return ready_news(
            repo,
            &mut t,
            &facts,
            &format!(", but the auto-accept was refused: {e}"),
            false,
        );
    };
    let (id, names) = (s(&t, "id").to_string(), names_of(&files));
    let base = py_str(t.get("base").unwrap_or(&Value::Null));
    let sent = t.get("conflict_sends").and_then(Value::as_i64).unwrap_or(0);
    if sent >= CONFLICT_SENDS {
        let then = format!(
            ", but the auto-accept was refused: it still conflicts with {base} in {names} after {sent} trips back to \
             rebase. Left for you"
        );
        return ready_news(repo, &mut t, &facts, &then, false);
    }
    t.insert("conflict_sends".into(), json!(sent + 1));
    let why = format!(
        "Review approved your work, but the auto-accept was refused: your branch doesn't rebase cleanly onto {base}"
    );
    let failed = match send_back_conflict(repo, &mut t, &why, &base_ref, &files, &shown) {
        Ok(failed) => failed,
        Err(e) => Some(clip(&e.0, 160)),
    };
    let line = format!(
        "{id} approved by review{}, but the auto-accept was refused: it conflicts with {base} in {names}",
        with_notes(&t)
    );
    match failed {
        None => {
            let line = format!(
                "{line}. Sent back to its worker to rebase (trip {} of {CONFLICT_SENDS})",
                sent + 1
            );
            say(repo, &t, Kind::RebaseSent, &line)
        }
        Some(failed) => {
            let line = format!(
                "{line}, and sending it back to rebase didn't reach its worker ({failed}): `bullpen resume {id}` \
                 or `bullpen tell {id}` it to rebase onto {base_ref} and fix them"
            );
            let why = "The request to rebase didn't reach its worker";
            stop(repo, &mut t, "worker", "not_sent", why, &[line], Kind::NotSent).map(|_| ())
        }
    }
}

/// An auto-accept that didn't merge the task, on its timeline: reason is "conflict",
/// "checks", "comments" (new on its PR while it waited for checks) or "other".
pub(crate) fn refused(repo: &Repo, t: &Obj, reason: &str, why: &str) {
    let fields = json!({"reason": reason, "why": clip(why, 160)});
    bullpen_core::timeline::record(&repo.dir, s(t, "id"), "accept_refused", fields);
}

/// How many times an auto-accept refused for a conflict sends a task back to rebase
/// before it leaves it for the person.
pub const CONFLICT_SENDS: i64 = 3;

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
#[path = "tests/test_auto_accept.rs"]
mod tests;
