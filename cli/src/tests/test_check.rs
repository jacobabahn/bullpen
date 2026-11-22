use super::*;
use serde_json::json;

fn agent(state: &'static str, kind: &str, age: Option<f64>) -> State {
    State {
        state,
        kind: kind.into(),
        msg: String::new(),
        age,
    }
}

fn task(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

#[test]
fn needs_label_is_the_prompt_kind_or_the_why_in_a_few_words() {
    assert_eq!(needs_label(Some(&json!("Needs you (permission): run x"))), "permission");
    assert_eq!(needs_label(Some(&json!("Reviewer needs you (plan): ok?"))), "plan");
    assert_eq!(needs_label(Some(&json!("Failed: it broke"))), "failed: it broke");
    assert_eq!(needs_label(None), "waiting");
    assert_eq!(
        needs_label(Some(&json!("Failed: ".to_string() + &"x".repeat(80))))
            .chars()
            .count(),
        60
    );
}

#[test]
fn a_shell_counts_once_it_has_run_past_the_limit() {
    assert_eq!(shell_minutes(&agent("shell", "", Some(1200.0)), 15.0), Some(20));
    assert_eq!(shell_minutes(&agent("shell", "", Some(600.0)), 15.0), None);
    assert_eq!(shell_minutes(&agent("idle", "", Some(1200.0)), 15.0), None);
    assert_eq!(shell_minutes(&agent("shell", "", None), 0.0), None);
}

#[test]
fn a_worker_drifts_by_what_it_does_against_its_status() {
    let working = task(json!({"id": "BP-1", "status": "working"}));
    let (line, fix) = worker_drift(&working, &agent("idle", "", None), 15.0).unwrap();
    assert_eq!(
        (line.as_str(), fix),
        (
            "BP-1 is marked working but its agent is idle at its prompt: `bullpen peek` it",
            "idle"
        )
    );
    assert_eq!(
        worker_drift(&working, &agent("gone", "", None), 15.0).unwrap().1,
        "gone"
    );
    assert_eq!(
        worker_drift(&working, &agent("shell", "", Some(1200.0)), 15.0)
            .unwrap()
            .1,
        "shell"
    );
    assert!(worker_drift(&working, &agent("shell", "", Some(60.0)), 15.0).is_none());
    assert!(worker_drift(&working, &agent("working", "", None), 15.0).is_none());
    let ok = task(json!({"id": "BP-1", "status": "working", "ok": true}));
    assert!(worker_drift(&ok, &agent("idle", "", None), 15.0).is_none());
    assert_eq!(
        worker_drift(&ok, &agent("blocked", "plan", None), 15.0).unwrap().1,
        "blocked"
    );
    let asked = task(json!({"id": "BP-1", "status": "needs", "ask": true}));
    assert!(worker_drift(&asked, &agent("working", "", None), 15.0).is_none());
    let at_prompt = task(json!({"id": "BP-1", "status": "needs", "stop": {"by": "worker", "kind": "prompt"}}));
    assert!(worker_drift(&at_prompt, &agent("blocked", "", None), 15.0).is_none());
    assert_eq!(
        worker_drift(&at_prompt, &agent("working", "", None), 15.0).unwrap().1,
        "working"
    );
}

#[test]
fn a_reviewer_drifts_without_a_verdict() {
    let review = task(json!({"id": "BP-2", "status": "review"}));
    let (line, fix) = reviewer_drift(&review, &agent("idle", "", None), 15.0).unwrap();
    assert_eq!(
        (line.as_str(), fix),
        ("BP-2 is in review but its reviewer is idle without a verdict", "idle")
    );
    assert_eq!(
        reviewer_drift(&review, &agent("error", "", None), 15.0).unwrap().1,
        "error"
    );
    assert_eq!(
        reviewer_drift(&review, &agent("shell", "", Some(1200.0)), 15.0)
            .unwrap()
            .1,
        "shell"
    );
    assert!(reviewer_drift(&review, &agent("shell", "", Some(60.0)), 15.0).is_none());
    let needs = task(json!({"id": "BP-2", "status": "needs", "why": "Reviewer needs you (plan): ok?"}));
    assert_eq!(
        reviewer_drift(&needs, &agent("working", "", None), 15.0).unwrap().0,
        "BP-2 is marked needs (plan) but its reviewer is working again"
    );
    assert_eq!(
        reviewer_drift(&needs, &agent("gone", "", None), 15.0).unwrap().0,
        "BP-2 is marked needs but its reviewer's pane is gone"
    );
}
