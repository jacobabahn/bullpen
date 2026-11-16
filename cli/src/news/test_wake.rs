use super::*;

#[test]
fn a_records_stamp_and_event() {
    let t = "2026-11-16T10:00:00-08:00";
    assert_eq!(
        stamp_and_event(&format!("{t} [c1,c2] BP-3 ready: bullpen · BP-3 is ready: x")),
        Some((t, "ready"))
    );
    assert_eq!(
        stamp_and_event(&format!("{t} BP-3 needs-you: bullpen · a: b")),
        Some((t, "needs-you"))
    );
    assert_eq!(stamp_and_event("\tdiff line"), None);
    assert_eq!(stamp_and_event("junk"), None);
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
        "a-new-one",
    ] {
        assert!(wakes(event), "{event}");
    }
}
