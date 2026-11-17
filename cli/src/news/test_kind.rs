use super::*;

#[test]
fn a_line_whose_row_asks_wakes_and_one_that_needs_nothing_rides_along() {
    for &k in ALL {
        if NOT_YET.contains(&k) {
            assert!(
                k.wakes() && !k.row().asks(),
                "{k:?} is settled now: take it out of NOT_YET"
            );
            continue;
        }
        assert_eq!(k.wakes(), k.row().asks(), "{k:?} ({:?})", k.row());
    }
}

#[test]
fn each_kind_has_an_event_of_its_own_that_reads_back() {
    for &k in ALL {
        let e = k.event();
        assert!(!e.is_empty() && !e.contains([' ', ':']), "{e}");
        assert_eq!(Kind::of_event(e), Some(k));
        assert_eq!(<&str>::from(k), e);
    }
    assert_eq!(Kind::of_event("a-new-one"), None);
}

#[test]
fn every_kind_is_listed() {
    // a new kind fails to compile here until it is named, and then until it is in ALL
    fn named(k: Kind) -> Kind {
        match k {
            Kind::Ready
            | Kind::NotAutoAccepted
            | Kind::Finished
            | Kind::Accepted
            | Kind::Closed
            | Kind::Kept
            | Kind::Dropped
            | Kind::Started
            | Kind::Working
            | Kind::TourSent
            | Kind::Rebased
            | Kind::NotRebased
            | Kind::RebaseSent
            | Kind::Keys
            | Kind::AcceptFailed
            | Kind::Queue
            | Kind::QueueFailed
            | Kind::Pickup
            | Kind::PickupAsk
            | Kind::Later
            | Kind::NeedsYou
            | Kind::Ask
            | Kind::Answer
            | Kind::Quiet
            | Kind::Failed
            | Kind::Stopped
            | Kind::NotSent
            | Kind::Stuck
            | Kind::Limit
            | Kind::LimitReset
            | Kind::Risk
            | Kind::Overlap
            | Kind::PrFeedback
            | Kind::PrMerged
            | Kind::PrClosed
            | Kind::Job
            | Kind::Push
            | Kind::Guide
            | Kind::Issues
            | Kind::DroppedDependency
            | Kind::Build
            | Kind::Board => k,
        }
    }
    assert_eq!(ALL.len(), 42);
    assert!(ALL.iter().all(|&k| named(k) == k));
}

#[test]
fn old_logs_events_read_as_they_did() {
    // an event that meant several things kept its word for what rode along
    for e in [
        "pickup",
        "queue",
        "rebased",
        "accepted",
        "closed",
        "started",
        "kept",
        "dropped",
        "later",
        "tour-sent",
    ] {
        assert!(!Kind::of_event(e).unwrap().wakes(), "{e}");
    }
    for e in [
        "ready",
        "quiet",
        "needs-you",
        "pr-feedback",
        "guide",
        "working",
        "risk",
        "issues",
    ] {
        assert!(Kind::of_event(e).unwrap().wakes(), "{e}");
    }
}
