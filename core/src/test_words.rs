//! test_helpers.py's cases for the board's words, under their names.

use super::*;
use chrono::Duration;
use serde_json::json;

fn task(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

/// A repo's settings: review_rounds 2, and cfg's, as test_helpers' repo() had them.
fn config(cfg: Value) -> Obj {
    let mut c = task(json!({"review_rounds": 2}));
    c.extend(task(cfg));
    c
}

/// A stamp `minutes` ago.
pub fn minutes_ago(minutes: i64) -> String {
    timeutil::isoformat(&(timeutil::now() - Duration::minutes(minutes)))
}

#[test]
fn test_board_order_puts_what_you_do_next_first() {
    let statuses = [
        "todo", "working", "ready", "stopped", "needs", "triage", "done", "review", "finished",
    ];
    let ts: Vec<Obj> = statuses
        .iter()
        .enumerate()
        .map(|(i, st)| task(json!({"id": format!("BP-{}", i + 1), "status": st})))
        .collect();
    let ids: Vec<String> = board_order(ts).iter().map(|t| s(t, "id").to_string()).collect();
    assert_eq!(
        ids,
        ["BP-4", "BP-5", "BP-6", "BP-3", "BP-9", "BP-8", "BP-2", "BP-1", "BP-7"]
    );
}

#[test]
fn test_the_boards_sentence() {
    let d = |t: Value, more: &[Value], cfg: Value| {
        let t = task(t);
        let mut tasks = vec![t.clone()];
        tasks.extend(more.iter().map(|m| task(m.clone())));
        board_detail(&config(cfg), &t, &every(&tasks), None).unwrap()
    };
    let none = json!({});
    assert_eq!(
        d(json!({"id": "BP-1", "status": "stopped"}), &[], none.clone()),
        "Its agent exited. r resumes it."
    );
    assert_eq!(
        d(
            json!({"id": "BP-1", "status": "finished", "summary": "# Recorded the demo\n\nmore"}),
            &[],
            none.clone()
        ),
        "Finished, nothing to merge: Recorded the demo. a closes it."
    );
    assert_eq!(
        d(
            json!({"id": "BP-1", "status": "finished", "keep_branch": true, "summary": "Ported the parser"}),
            &[],
            none.clone()
        ),
        "Finished, branch to be kept: Ported the parser. a closes it, keeping the branch."
    );
    assert_eq!(
        d(
            json!({"id": "BP-1", "status": "ready", "pr": "https://github.com/a/b/pull/41",
                   "review": {"verdict": "approved", "kind": "full"}, "gh": {"checks": "passing"}}),
            &[],
            none.clone()
        ),
        "PR #41, approved by full review. Checks passing."
    );
    assert_eq!(
        d(
            json!({"id": "BP-1", "status": "review", "review": {"kind": "followup", "round": 2}}),
            &[],
            none.clone()
        ),
        "Follow-up review on round 2 of 2."
    );
    assert_eq!(
        d(
            json!({"id": "BP-2", "status": "todo", "after": ["BP-1"]}),
            &[json!({"id": "BP-1", "status": "ready"})],
            none.clone()
        ),
        "Waits on BP-1."
    );
    assert_eq!(
        d(
            json!({"id": "BP-1", "status": "triage", "why": "Ended its turn without saying ready"}),
            &[],
            none.clone()
        ),
        "Ended its turn without ready."
    );
    assert_eq!(
        d(
            json!({"id": "BP-1", "status": "working", "base": "main",
                   "review": {"verdict": "approved", "conflict": ["a.txt"]}}),
            &[],
            none.clone()
        ),
        "Approved, rebasing onto main: its branch conflicted."
    );
    assert_eq!(
        d(
            json!({"id": "BP-1", "status": "todo", "worker_model": "opus"}),
            &[],
            json!({"worker_model": "sonnet"})
        ),
        "Not started. s starts it. Model: opus."
    );
    assert_eq!(
        d(
            json!({"id": "BP-1", "status": "needs", "why": "Asks: x?", "note": "Looking"}),
            &[],
            none
        ),
        "Looking"
    );
}

#[test]
fn test_the_marks_are_the_boards() {
    // one mark per status, the board's too (it shows mark()): each its own
    let marks: std::collections::HashSet<&str> = MARKED.iter().map(|st| mark(st)).collect();
    assert_eq!(marks.len(), MARKED.len());
    assert!(!marks.contains(" "));
    // ready and closed are told apart
    assert_eq!((mark("ready"), mark("done")), ("✔", "·"));
}

#[test]
fn test_a_working_task_only_shows_what_github_sent_it_back_for() {
    let gh = json!({"state": "open", "checks": "passing", "new": 0});
    let mut sent = gh.clone();
    sent["sent"] = json!(true);
    assert_eq!(pr_phrase(&task(json!({"status": "working", "gh": gh}))), "");
    assert_eq!(
        pr_phrase(&task(json!({"status": "working", "gh": sent}))),
        "checks passing"
    );
    assert_eq!(pr_phrase(&task(json!({"status": "ready", "gh": gh}))), "checks passing");
}

#[test]
fn test_only_the_agent_a_task_waits_on_is_stuck() {
    // (is_limited: core/src/test_task.rs)
    let t = task(json!({"status": "working", "stuck": {"who": "worker", "since": minutes_ago(25)}}));
    assert!(is_stuck(&t));
    assert_eq!(stuck_phrase(&t, None), "no progress for 25 min");
    let mut review = t.clone();
    review.insert("status".into(), json!("review"));
    assert!(!is_stuck(&review));
    assert!(!is_stuck(&task(json!({"status": "working"}))));
    // a record of the wrong shape is none
    assert!(!is_stuck(&task(json!({"status": "working", "stuck": 1}))));
}

#[test]
fn test_a_task_closed_with_its_branch_kept() {
    let cfg = config(json!({}));
    let pushed = task(json!({"id": "BP-2", "status": "done", "kept": {"branch": "rust", "remote": "github"}}));
    let every = every(&[pushed.clone()]);
    assert_eq!(
        status_detail(&cfg, &pushed, "", None),
        "Closed, branch kept: rust on github."
    );
    let here = task(json!({"status": "done", "kept": {"branch": "rust", "remote": null}}));
    assert_eq!(
        status_detail(&cfg, &here, "", None),
        "Closed, branch kept here: rust, not pushed."
    );
    // what waits on it keeps waiting: its work isn't on the base branch
    let after = task(json!({"id": "BP-3", "status": "todo", "after": ["BP-2"]}));
    assert_eq!(waits_phrase(&after, &every), "Waits on BP-2 (branch kept, not merged)");
}

#[test]
fn test_a_task_from_the_backlog_says_where_it_came_from() {
    let plain = task(json!({"id": "BP-1", "status": "todo"}));
    assert_eq!((from_mark(&plain), from_detail(&plain).as_str()), (" ", ""));
    let by_hand = task(json!({"id": "BP-2", "status": "todo", "from": "L7", "origin": "backlog", "started_by": "c1"}));
    assert_eq!(
        (from_mark(&by_hand), from_detail(&by_hand).as_str()),
        ("↻", "From the backlog (L7).")
    );
    // last in the sentence for one started by hand, first for one that started itself
    assert_eq!(
        from_around(&by_hand, &["Not started.", "", "Model: opus."]),
        "Not started. Model: opus. From the backlog (L7)."
    );
    let itself =
        task(json!({"id": "BP-3", "status": "working", "from": "L8", "origin": "check", "started_by": "itself"}));
    assert_eq!(from_around(&itself, &["Working."]), "Started by itself (L8). Working.");
    // closed, it keeps its mark but its sentence doesn't say it
    let done = task(json!({"id": "BP-4", "status": "done", "from": "L9", "origin": "backlog"}));
    assert_eq!((from_mark(&done), from_detail(&done).as_str()), ("↻", ""));
}

#[test]
fn a_ready_tasks_sentence_ends_with_its_risk() {
    let risk = json!({"level": "high", "why": "changes how accept merges"});
    let t = task(
        json!({"id": "BP-1", "status": "ready", "pr": "https://github.com/a/b/pull/41",
                        "review": {"verdict": "approved", "kind": "full"}, "risk": risk}),
    );
    assert_eq!(
        status_detail(&config(json!({})), &t, "", None),
        "PR #41, approved by full review. High risk: changes how accept merges."
    );
    // once it's working again, its old label isn't said
    let mut working = t.clone();
    working.insert("status".into(), json!("working"));
    assert!(!status_detail(&config(json!({})), &working, "", None).contains("risk"));
}

#[test]
fn a_worker_waiting_on_its_job_says_so() {
    let job = |what: Value| task(json!({"id": "BP-1", "status": "working", "job": {"what": what, "since": "x"}}));
    assert_eq!(
        status_detail(&config(json!({})), &job(json!("cargo test")), "", None),
        "Waiting on its job (cargo test): it takes another turn when that ends."
    );
    assert_eq!(
        status_detail(&config(json!({})), &job(Value::Null), "", None),
        "Waiting on its background job: it takes another turn when that ends."
    );
}
