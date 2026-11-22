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
fn a_task_waits_on_its_reviewers_pane_when_stopped_on_it() {
    let more = json!({"review": {"block": "blk2", "session": "s"}, "block": "blk1"});
    let mut x = stopped("needs", "reviewer", "ended", more);
    assert_eq!(waited_on(&x), Some(("reviewer", Agent::on_rex("s", "blk2"))));
    x.insert("stop".into(), json!({"by": "worker", "kind": "failed"}));
    assert_eq!(waited_on(&x), Some(("worker", Agent::on_rex("", "blk1"))));
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
    assert!(!running(&t(json!({"status": "ready", "block": "b1"})), None).unwrap());
    // waits on no agent
}

#[test]
fn where_a_pane_was_placed() {
    let x = t(json!({"pane": "tabs", "review": {"pane": "background"}}));
    assert_eq!(placed(&x, "worker"), "tabs");
    assert_eq!(placed(&x, "reviewer"), "background");
    assert_eq!(placed(&t(json!({"pane": null})), "worker"), "");
}

#[test]
fn a_reviewer_is_on_its_reviews_host_in_its_workers_session_unless_its_own() {
    let x = t(json!({"session": "s", "block": "b1", "host": "rex", "review": {"block": "b2"}}));
    assert_eq!(agent(&x, "reviewer"), Agent::on_rex("s", "b2"));
    assert_eq!(agent(&x, "worker"), Agent::on_rex("s", "b1"));
    let own = t(json!({"session": "s", "block": "b1", "review": {"block": "b2", "session": "r"}}));
    assert_eq!(agent(&own, "reviewer"), Agent::on_rex("r", "b2"));
}
