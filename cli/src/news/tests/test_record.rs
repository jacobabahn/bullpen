use super::*;

const AT: &str = "2026-11-15T14:03:00-06:00";

fn rec(line: &str) -> Record {
    Record::parse(line).unwrap()
}

#[test]
fn a_line_in_its_parts() {
    let r = rec(&format!("{AT} [c1,c2] BP-3 ready: bullpen · BP-3 is ready: Fix it"));
    assert_eq!(r.stamp, AT);
    assert_eq!(r.to, vec!["c1", "c2"]);
    assert_eq!((r.task.as_str(), r.event.as_str()), ("BP-3", "ready"));
    assert_eq!(r.said, "bullpen · BP-3 is ready: Fix it");
    assert_eq!(r.text(), "BP-3 is ready: Fix it");
    let r = rec(&format!("{AT} - guide: bullpen · Your guide changed"));
    assert!(r.to.is_empty());
    assert_eq!(r.text(), "Your guide changed");
    // what it says may have colons of its own
    assert_eq!(rec(&format!("{AT} BP-3 needs-you: bullpen · a: b")).event, "needs-you");
    // a [to] that isn't one is the task
    assert_eq!(rec(&format!("{AT} [c1 ready: said")).task, "[c1");
    for junk in [
        "\t+ a diff line",
        "junk",
        &format!("{AT} BP-3 ready:x"),
        &format!("{AT} BP-3 a:b: x"),
    ] {
        assert_eq!(Record::parse(junk), None, "{junk}");
    }
}

#[test]
fn a_records_kind_says_whether_it_wakes_and_an_unknown_event_wakes() {
    let wakes = |event: &str| rec(&format!("{AT} BP-3 {event}: bullpen · x")).wakes();
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

#[test]
fn whose_news() {
    let listed = vec!["c1".to_string(), "c2".to_string()];
    let is_for = |line: &str, cid: &str| rec(line).is_for(cid, &listed);
    assert!(is_for("t [c1,c2] BP-1 x: bullpen · hi", "c2"));
    assert!(!is_for("t [c1] BP-1 x: bullpen · hi", "c2"));
    assert!(is_for("t [c7] BP-1 x: bullpen · hi", "c2")); // for a coordinator no longer listed
    assert!(is_for("t BP-1 x: bullpen · [c1]", "c2")); // after the mark it's words
    assert!(is_for("t [c1] BP-1 x: bullpen · hi", "")); // no coordinator: all news
}

#[test]
fn records_wait_for_their_end() {
    let recs = raw(b"a\n\tb\nc\n\td\npart");
    assert_eq!(recs, vec![vec![&b"a\n"[..], b"\tb\n"]]);
    let recs = raw(b"a\n\tb\nc\n");
    assert_eq!(recs, vec![vec![&b"a\n"[..], b"\tb\n"], vec![&b"c\n"[..]]]);
    assert!(raw(b"").is_empty());
}

#[test]
fn what_a_reader_is_shown() {
    let listed = vec!["c1".to_string(), "c2".to_string()];
    let data = b"t [c1] BP-1 x: bullpen \xc2\xb7 hi\n\t+ a\nt [c2] BP-1 x: bullpen \xc2\xb7 not yours\njunk\n";
    let recs = raw(data);
    let shown: Vec<_> = recs.iter().map(|r| shown(r, "c1", &listed)).collect();
    assert_eq!(
        shown,
        [
            Some(vec!["bullpen · hi".to_string(), "+ a".to_string()]),
            None,
            Some(vec!["junk".to_string()]), // not a news line: everyone's, whole
        ]
    );
    assert_eq!(shown_of(b"\torphan\n"), None); // a diff line without its line
    let r = Record::read(&recs[0]).unwrap();
    assert_eq!(r.more, ["+ a"]);
}

fn shown_of(data: &[u8]) -> Option<Vec<String>> {
    shown(&raw(data)[0], "c1", &[])
}
