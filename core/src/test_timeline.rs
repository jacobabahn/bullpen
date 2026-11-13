use super::*;

fn task(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

fn events(es: &[Obj]) -> Vec<&str> {
    es.iter().map(|e| s(e, "event")).collect()
}

#[test]
fn test_a_new_task_is_added() {
    let mut t = task(json!({"id": "BP-1", "status": "todo"}));
    let es = changes(&Obj::new(), &mut t);
    assert_eq!(events(&es), ["added"]);
    assert_eq!(es[0]["status"], "todo");
}

#[test]
fn test_a_status_change_says_from_to_and_its_stop() {
    let old = task(json!({"status": "working"}));
    let mut t = task(json!({"status": "triage", "why": "Failed", "stop": {"by": "worker", "kind": "failed"}}));
    let es = changes(&old, &mut t);
    assert_eq!(events(&es), ["status"]);
    assert_eq!((s(&es[0], "from"), s(&es[0], "to")), ("working", "triage"));
    assert_eq!(es[0]["stop"], json!({"by": "worker", "kind": "failed"}));
    assert_eq!(es[0]["why"], "Failed");
    assert!(changes(&t.clone(), &mut t).is_empty()); // the same again: nothing
}

#[test]
fn test_the_back_mark_goes_on_a_trip_back_and_off_the_task() {
    let old = task(json!({"status": "review"}));
    let mut t = task(json!({"status": "working"}));
    send_back(&mut t, "review");
    let es = changes(&old, &mut t);
    assert_eq!(es[0]["back"], "review");
    assert!(!t.contains_key(BACK));
    // a send that didn't reach the worker stops it: no trip back
    let mut t = task(json!({"status": "triage", "stop": {"by": "worker", "kind": "not_sent"}}));
    send_back(&mut t, "review");
    let es = changes(&old, &mut t);
    assert!(!es[0].contains_key("back"));
    assert!(!t.contains_key(BACK));
}

#[test]
fn test_an_end_of_turn_but_not_of_a_compaction() {
    let old = task(json!({"status": "working", "last": "working|"}));
    let mut t = task(json!({"status": "working", "last": "idle|"}));
    assert_eq!(events(&changes(&old, &mut t)), ["turn_ended"]);
    let old = task(json!({"status": "review", "last": "working|", "compacting": "x"}));
    let mut t = task(json!({"status": "review", "last": "done|"}));
    assert!(changes(&old, &mut t).is_empty());
}

#[test]
fn test_reviews_start_and_finish() {
    let old = task(
        json!({"status": "working", "review": {"round": 1, "kind": "full", "verdict": "changes", "block": null, "commit": "a"}}),
    );
    let mut t = task(
        json!({"status": "review", "review": {"round": 2, "kind": "followup", "verdict": null, "block": "b2", "commit": "b"}}),
    );
    let es = changes(&old, &mut t);
    assert_eq!(events(&es), ["status", "review_started"]);
    assert_eq!(
        (es[1]["round"].clone(), es[1]["kind"].clone()),
        (json!(2), json!("followup"))
    );
    // its pane found again after a Rex restart: no new review
    let old = t.clone();
    let mut r = obj(&t, "review").clone();
    r.insert("block".into(), json!("b3"));
    t.insert("review".into(), Value::Object(r.clone()));
    assert!(changes(&old, &mut t).is_empty());
    // the verdict
    let old = t.clone();
    r.insert("verdict".into(), json!("approved"));
    r.insert("block".into(), Value::Null);
    t.insert("review".into(), Value::Object(r));
    t.insert("status".into(), json!("ready"));
    let es = changes(&old, &mut t);
    assert_eq!(events(&es), ["review_finished", "status"]);
    assert_eq!(es[0]["verdict"], "approved");
}

#[test]
fn test_closing_says_how() {
    let old = task(json!({"status": "ready"}));
    let mut t = task(json!({"status": "done", "auto_accepted": false}));
    assert_eq!(changes(&old, &mut t)[0]["outcome"], "accepted");
    let mut t = task(json!({"status": "done", "kept": {"branch": "x"}}));
    assert_eq!(changes(&old, &mut t)[0]["outcome"], "kept");
    let mut t = task(json!({"status": "done", "closed": true}));
    assert_eq!(changes(&old, &mut t)[0]["outcome"], "closed");
}

#[test]
fn test_append_and_read_back() {
    let dir = std::env::temp_dir().join(format!("bullpen-timeline-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(read(&dir, "BP-1").is_none());
    append(&dir, "BP-1", &[entry("added", json!({"status": "todo"}))]);
    record(&dir, "BP-1", "accept_refused", json!({"reason": "conflict"}));
    let es = read(&dir, "BP-1").unwrap();
    assert_eq!(events(&es), ["added", "accept_refused"]);
    assert_eq!(es[1]["reason"], "conflict");
    let _ = std::fs::remove_dir_all(&dir);
}
