use super::*;
use serde_json::json;

fn t(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

fn stopped(status: &str, by: &str, kind: &str, more: Value) -> Obj {
    let mut o = t(json!({"id": "BP-3", "status": status, "why": "x", "stop": {"by": by, "kind": kind}}));
    o.extend(t(more));
    o
}

#[test]
fn a_task_is_on_its_reviewer_by_its_stop() {
    assert!(on_reviewer(&stopped("needs", "reviewer", "prompt", json!({}))));
    assert!(on_reviewer(&stopped("triage", "reviewer", "exited", json!({}))));
    assert!(on_reviewer(&stopped("needs", "reviewer", "ended", json!({"ask": true}))));
    assert!(!on_reviewer(&stopped("triage", "reviewer", "limit", json!({})))); // seen to as its worker's
    assert!(!on_reviewer(&stopped("triage", "worker", "ended", json!({}))));
    assert!(!on_reviewer(&t(json!({"status": "review"}))));
    let more = json!({"review": {"block": "blk2", "session": "s"}, "block": "blk1"});
    let mut x = stopped("needs", "reviewer", "ended", more);
    assert_eq!(waited_on(&x), Some(("reviewer", "s".into(), "blk2".into())));
    x.insert("stop".into(), json!({"by": "worker", "kind": "failed"}));
    assert_eq!(waited_on(&x), Some(("worker", "".into(), "blk1".into())));
}

#[test]
fn a_task_waits_on_its_reviewer_in_review_or_for_its_stop() {
    assert_eq!(waits_on(&t(json!({"status": "review"}))), Some("reviewer"));
    assert_eq!(waits_on(&stopped("needs", "reviewer", "prompt", json!({}))), Some("reviewer"));
    assert_eq!(waits_on(&stopped("triage", "reviewer", "limit", json!({}))), Some("worker"));
    assert_eq!(waits_on(&stopped("triage", "worker", "exited", json!({}))), Some("worker"));
    assert_eq!(waits_on(&stopped("stopped", "worker", "pane_gone", json!({}))), None); // its pane is gone
    assert_eq!(waits_on(&t(json!({"status": "working"}))), Some("worker"));
    for status in ["todo", "ready", "finished", "done", "dropped"] {
        assert_eq!(waits_on(&t(json!({"status": status}))), None);
    }
    let lim = json!({"who": "reviewer", "text": "limit"});
    assert_eq!(waits_on(&t(json!({"status": "review", "limit": lim}))), None); // waits on the usage limit
    assert_eq!(whose_turn(&t(json!({"status": "review", "limit": lim}))), Some("reviewer"));
    let worker_lim = json!({"who": "worker", "text": "limit"});
    assert_eq!(waits_on(&t(json!({"status": "working", "limit": worker_lim}))), None);
}

#[test]
fn a_reviewer_pane_falls_back_to_its_workers_session() {
    let mut x = t(json!({"status": "review", "session": "s1", "block": "b1", "review": {"block": "b2"}}));
    assert_eq!(pane(&x, "worker"), ("s1".into(), "b1".into()));
    assert_eq!(pane(&x, "reviewer"), ("s1".into(), "b2".into()));
    x["review"]["session"] = json!("s2");
    assert_eq!(pane(&x, "reviewer"), ("s2".into(), "b2".into()));
    assert_eq!(pane(&t(json!({"status": "todo"})), "reviewer"), ("".into(), "".into()));
    assert_eq!(waited_on(&t(json!({"status": "review", "block": "b1"}))), None); // no reviewer pane
    assert!(!running(&t(json!({"status": "ready", "block": "b1"})), None).unwrap()); // waits on no agent
}

#[test]
fn only_the_agent_a_task_waits_on_is_limited() {
    let lim = json!({"who": "worker", "resets": "2026-11-11T15:00:00-06:00", "text": "x"});
    assert!(is_limited(&t(json!({"status": "working", "limit": lim}))));
    assert!(!is_limited(&t(json!({"status": "ready", "limit": lim}))));
    assert!(!is_limited(&t(json!({"status": "working", "limit": {}}))));
}

#[test]
fn where_a_pane_was_placed() {
    let x = t(json!({"pane": "tabs", "review": {"pane": "background"}}));
    assert_eq!(placed(&x, "worker"), "tabs");
    assert_eq!(placed(&x, "reviewer"), "background");
    assert_eq!(placed(&t(json!({"pane": null})), "worker"), "");
}
