use super::*;
use serde_json::json;

fn task(v: serde_json::Value) -> Obj {
    v.as_object().cloned().unwrap()
}

#[test]
fn a_ready_task_shows_its_label_and_others_keep_it() {
    let risk = json!({"level": "high", "why": "changes how accept merges"});
    let ready = task(json!({"status": "ready", "risk": risk}));
    assert_eq!(shown(&ready), Some(("high", "changes how accept merges")));
    let done = task(json!({"status": "done", "risk": risk}));
    assert_eq!(shown(&done), None);
    assert_eq!(stored(&done), Some(("high", "changes how accept merges")));
}

#[test]
fn no_label_or_an_unknown_level_shows_nothing() {
    assert_eq!(shown(&task(json!({"status": "ready"}))), None);
    assert_eq!(
        shown(&task(json!({"status": "ready", "risk": {"level": "huge"}}))),
        None
    );
}

#[test]
fn the_label_and_the_order() {
    assert_eq!(label("low", "docs only"), "risk: low · docs only");
    assert_eq!(label("medium", ""), "risk: medium");
    assert!(rank("high") > rank("medium") && rank("medium") > rank("low"));
    assert_eq!(level_sgr("high"), "1;31");
}
