use super::*;

#[test]
fn a_records_stamp_task_and_event() {
    let t = "2026-11-16T10:00:00-08:00";
    assert_eq!(
        record_head(&format!("{t} [c1,c2] BP-3 ready: bullpen · BP-3 is ready: x")),
        Some((t, "BP-3", "ready"))
    );
    assert_eq!(
        record_head(&format!("{t} BP-3 needs-you: bullpen · a: b")),
        Some((t, "BP-3", "needs-you"))
    );
    assert_eq!(record_head("\tdiff line"), None);
    assert_eq!(record_head("junk"), None);
}

#[test]
fn an_events_kind_says_whether_it_wakes_and_an_unknown_one_wakes() {
    for event in [
        "started",
        "rebased",
        "accepted",
        "closed",
        "pickup",
        "queue",
        "tour-sent",
        "not-auto-accepted",
    ] {
        assert!(!wakes(event), "{event}");
    }
    for event in [
        "ready",
        "quiet",
        "needs-you",
        "keys",
        "pickup-ask",
        "queue-failed",
        "not-rebased",
        "a-new-one",
    ] {
        assert!(wakes(event), "{event}");
    }
}
