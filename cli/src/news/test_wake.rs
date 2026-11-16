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
fn bookkeeping_rides_along_and_anything_else_wakes() {
    for event in [
        "started",
        "rebased",
        "accepted",
        "closed",
        "pickup",
        "tour-sent",
        "keys",
    ] {
        assert!(!wakes(event), "{event}");
    }
    for event in [
        "ready",
        "quiet",
        "stuck",
        "needs-you",
        "pr-feedback",
        "guide",
        "working",
        "risk",
        "issues",
        "a-new-one",
    ] {
        assert!(wakes(event), "{event}");
    }
}
