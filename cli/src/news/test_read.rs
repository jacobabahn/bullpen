use super::*;

#[test]
fn cursors_as_python_reads_them() {
    assert_eq!(py_int(" 12\n"), Some(12));
    assert_eq!(py_int("+1_000"), Some(1000));
    assert_eq!(py_int("-5"), Some(-5));
    for bad in ["junk", "", "1__0", "_1", "1.5", "1 2"] {
        assert_eq!(py_int(bad), None, "{bad}");
    }
}

#[test]
fn whose_news() {
    let listed = vec!["c1".to_string(), "c2".to_string()];
    assert!(news_for("t [c1,c2] BP-1 x: bullpen · hi", "c2", &listed));
    assert!(!news_for("t [c1] BP-1 x: bullpen · hi", "c2", &listed));
    assert!(news_for("t [c7] BP-1 x: bullpen · hi", "c2", &listed)); // for a coordinator no longer listed
    assert!(news_for("t BP-1 x: bullpen · [c1]", "c2", &listed)); // after the mark it's words
    assert!(news_for("t [c1] BP-1 x: bullpen · hi", "", &listed)); // no coordinator: all news
}

#[test]
fn records_wait_for_their_end() {
    let recs = news_records(b"a\n\tb\nc\n\td\npart");
    assert_eq!(recs, vec![vec![&b"a\n"[..], b"\tb\n"]]);
    let recs = news_records(b"a\n\tb\nc\n");
    assert_eq!(recs, vec![vec![&b"a\n"[..], b"\tb\n"], vec![&b"c\n"[..]]]);
    assert!(news_records(b"").is_empty());
}
