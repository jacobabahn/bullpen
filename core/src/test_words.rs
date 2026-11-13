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
fn test_only_the_agent_a_task_waits_on_is_stuck_or_limited() {
    let t = task(json!({"status": "working", "stuck": {"who": "worker", "since": minutes_ago(25)}}));
    assert!(is_stuck(&t));
    assert_eq!(stuck_phrase(&t, None), "no progress for 25 min");
    let mut review = t.clone();
    review.insert("status".into(), json!("review"));
    assert!(!is_stuck(&review));
    assert!(!is_stuck(&task(json!({"status": "working"}))));
    let lim = json!({"who": "worker", "resets": "2026-11-11T15:00:00-06:00", "text": "x"});
    assert!(is_limited(&task(json!({"status": "working", "limit": lim}))));
    assert!(!is_limited(&task(json!({"status": "ready", "limit": lim}))));
    // a record of the wrong shape is none
    assert!(!is_stuck(&task(json!({"status": "working", "stuck": 1}))));
    assert!(!is_limited(&task(json!({"status": "working", "limit": "x"}))));
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
