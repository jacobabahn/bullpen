use super::*;

#[test]
fn test_spans_in_words() {
    assert_eq!(span(40), "40s");
    assert_eq!(span(14 * 60 + 5), "14m");
    assert_eq!(span(2 * 3600 + 5 * 60), "2h 05m");
    assert_eq!(span(3 * 86400 + 2 * 3600), "3d 2h");
}

#[test]
fn test_since_takes_a_date_a_stamp_or_days_back() {
    let at = parse("2026-11-13T12:00:00+00:00").unwrap();
    assert_eq!(since("7d", at), Some(at - chrono::Duration::days(7)));
    assert!(since("2026-11-01", at).is_some());
    assert_eq!(
        since("2026-11-01T00:00:00+00:00", at),
        parse("2026-11-01T00:00:00+00:00")
    );
    assert!(since("soon", at).is_none());
}

#[test]
fn test_medians() {
    assert_eq!(median(vec![]), None);
    assert_eq!(median(vec![5, 1, 3]), Some(3));
    assert_eq!(median(vec![4, 1, 3, 2]), Some(2));
}

#[test]
fn test_entries_in_words() {
    let o = |v: Value| v.as_object().unwrap().clone();
    assert_eq!(
        describe(&o(json!({"event": "status", "from": "todo", "to": "working"}))),
        "started"
    );
    assert_eq!(
        describe(&o(
            json!({"event": "status", "from": "review", "to": "working", "back": "review"})
        )),
        "sent back to work (review)"
    );
    assert_eq!(
        describe(&o(json!({"event": "review_started", "round": 2, "kind": "followup"}))),
        "follow-up review started (round 2)"
    );
    assert_eq!(
        describe(&o(
            json!({"event": "status", "from": "ready", "to": "done", "outcome": "accepted"})
        )),
        "accepted"
    );
}
