//! A reviewer's `verdict` (bullpen/auto_accept.py's cmd_verdict): approve the task, or
//! send its findings back to the worker. With auto-accept, an approval runs Python's
//! `_auto_accept` (the accept, or its wait on the PR's checks) until T9 moves it.

use crate::agents::pane;
use crate::args::{pos, Arg, Args, Nargs};
use crate::jobs::{background, Job};
use crate::news::ready_news;
use crate::ready::{summary_text, SUMMARY, SUMMARY_FILE};
use crate::review::{review_kind, rounds};
use crate::rex::rex_close;
use crate::saves::{run_later, save};
use crate::start::launch::hand_back;
use crate::stops::stop;
use bullpen_core::json::{obj, py_str, s, truthy};
use bullpen_core::paths::self_bin;
use bullpen_core::repo::{repo_root, unindex_block};
use bullpen_core::settings::WAITING;
use bullpen_core::task::{auto_accepts, clip, commits, set_status, where_};
use bullpen_core::{fail, Repo, Res};
use serde_json::{json, Value};
use std::io::Write;

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

/// An approved task with auto-accept: Python's `_auto_accept` accepts it now, or once
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

/// Close a pane this command may run in: last, after the work it set off (run_later),
/// since closing it may end this process.
fn close_last(session: &str, block: &str) -> Res<()> {
    run_later();
    let _ = std::io::stdout().flush();
    rex_close(session, block)
}
