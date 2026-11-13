use super::*;
use serde_json::json;

fn e(at: &str, v: Value) -> Obj {
    let mut o = v.as_object().unwrap().clone();
    o.insert("at".into(), json!(format!("2026-11-13T{at}:00+00:00")));
    o
}

fn st(to: &str, from: &str) -> Value {
    json!({"event": "status", "from": from, "to": to})
}

/// A task that starts, asks a question, goes to review, is sent back, is reviewed
/// again and accepted.
fn flow() -> Vec<Obj> {
    vec![
        e("10:00", json!({"event": "added", "status": "todo"})),
        e("10:05", st("working", "todo")),
        e(
            "10:20",
            json!({"event": "status", "from": "working", "to": "needs", "stop": {"by": "worker", "kind": "prompt"}}),
        ),
        e("10:30", st("working", "needs")),
        e("11:00", st("review", "working")),
        e("11:00", json!({"event": "review_started", "round": 1, "kind": "full"})),
        e(
            "11:20",
            json!({"event": "review_finished", "round": 1, "kind": "full", "verdict": "changes"}),
        ),
        e(
            "11:20",
            json!({"event": "status", "from": "review", "to": "working", "back": "review"}),
        ),
        e("11:40", json!({"event": "turn_ended"})),
        e("11:40", st("review", "working")),
        e(
            "11:40",
            json!({"event": "review_started", "round": 2, "kind": "followup"}),
        ),
        e(
            "11:50",
            json!({"event": "review_finished", "round": 2, "kind": "followup", "verdict": "approved"}),
        ),
        e("11:50", st("ready", "review")),
        e(
            "12:30",
            json!({"event": "status", "from": "ready", "to": "done", "outcome": "accepted"}),
        ),
    ]
}

#[test]
fn test_the_time_in_each_phase() {
    let now = parse("2026-11-14T00:00:00+00:00").unwrap();
    let t = totals(&flow(), now);
    let time = obj(&t, "time");
    let mins = |k: &str| time[k].as_i64().unwrap() / 60;
    assert_eq!(mins("queued"), 5);
    assert_eq!(mins("working"), 15 + 30 + 20);
    assert_eq!(mins("person"), 10 + 40); // the question, and ready until accepted
    assert_eq!(mins("review"), 20 + 10);
    assert_eq!(mins("review_wait"), 0);
    assert_eq!((mins("stalled"), mins("rebasing"), mins("coordinator")), (0, 0, 0));
    assert_eq!(t["review_rounds"], 2);
    assert_eq!(t["reviews"], json!({"full": 1, "followup": 1}));
    assert_eq!(t["trips_back"], json!({"review": 1}));
    assert_eq!(
        (t["questions"].clone(), t["turns"].clone(), t["stalls"].clone()),
        (json!(1), json!(1), json!(0))
    );
}

#[test]
fn test_rebasing_stalls_and_an_open_task_up_to_now() {
    let es = vec![
        e("10:00", st("ready", "review")),
        e("10:10", json!({"event": "accept_refused", "reason": "conflict"})),
        e(
            "10:10",
            json!({"event": "status", "from": "ready", "to": "working", "back": "rebase"}),
        ),
        e(
            "10:20",
            json!({"event": "status", "from": "working", "to": "stopped", "stop": {"by": "worker", "kind": "exited"}}),
        ),
        e("10:25", st("working", "stopped")),
        e(
            "10:30",
            json!({"event": "status", "from": "working", "to": "triage", "stop": {"by": "worker", "kind": "ended"}}),
        ),
        e("10:40", st("working", "triage")),
    ];
    let now = parse("2026-11-13T10:50:00+00:00").unwrap();
    let t = totals(&es, now);
    let time = obj(&t, "time");
    let mins = |k: &str| time[k].as_i64().unwrap() / 60;
    assert_eq!(mins("person"), 10);
    assert_eq!(mins("rebasing"), 10 + 5 + 10); // up to now: still rebasing
    assert_eq!(mins("stalled"), 5);
    assert_eq!(mins("coordinator"), 10);
    assert_eq!(t["trips_back"], json!({"rebase": 1}));
    assert_eq!(t["accept_refused"], json!({"conflict": 1}));
    assert_eq!(t["stalls"], 1);
}

#[test]
fn test_lines_from_the_diffstat() {
    let t = json!({"diffstat": "3 files, +40 \u{2212}12"})
        .as_object()
        .unwrap()
        .clone();
    assert_eq!(lines(&t), Some((40, 12)));
    assert_eq!(lines(&Obj::new()), None);
}
