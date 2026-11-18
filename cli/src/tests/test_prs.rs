use super::*;

fn pr(v: Value) -> Obj {
    v.as_object().cloned().unwrap()
}

#[test]
fn people_s_comments_and_reviews_are_feedback() {
    let p = pr(json!({
        "comments": [
            {"id": "c1", "author": {"login": "ann"}, "body": "  Please   rename it. "},
            {"id": "c2", "author": {"login": "ci-bot[bot]"}, "body": "Coverage 90%"},
            {"id": "c3", "author": {"login": "bob", "is_bot": true}, "body": "beep"},
            {"url": "u4", "author": null, "body": "who?"},
            {"id": "c5", "author": {"login": "ann"}, "body": "   "},
        ],
        "reviews": [
            {"id": "r1", "author": {"login": "cat"}, "state": "APPROVED", "body": "LGTM"},
            {"id": "r2", "author": {"login": "cat"}, "state": "CHANGES_REQUESTED", "body": "Fix the test."},
            {"id": "r3", "author": {"login": "dan"}, "state": "CHANGES_REQUESTED", "body": ""},
            {"submittedAt": "t5", "author": {"login": "eve"}, "state": "COMMENTED", "body": ""},
            {"id": "r6", "author": {"login": "eve"}, "state": "COMMENTED", "body": "Nice"},
        ],
    }));
    let got: Vec<(Value, String, String)> = feedback(&p);
    let want = [
        (json!("c1"), "ann", "Please rename it."),
        (json!("u4"), "someone", "who?"),
        (json!("r2"), "cat", "requested changes: Fix the test."),
        (json!("r3"), "dan", "requested changes"),
        (json!("t5"), "eve", "left comments on the code"),
        (json!("r6"), "eve", "reviewed: Nice"),
    ];
    let want: Vec<(Value, String, String)> = want.into_iter().map(|(k, w, s)| (k, w.into(), s.into())).collect();
    assert_eq!(got, want);
}

#[test]
fn seen_keys_are_kept_sorted_once() {
    assert_eq!(
        sorted(vec![json!("b"), json!("a"), json!("b")]),
        vec![json!("a"), json!("b")]
    );
}
