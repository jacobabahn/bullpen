use super::*;

#[test]
fn one_task_waiting_on_a_dropped_one_stays_queued() {
    let line = dropped_line(&["BP-4".into()], "BP-2");
    assert_eq!(
        line,
        "BP-4 waits on BP-2, which was dropped, so it stays queued. Ask the person what to do: \
         `bullpen after BP-4 --none` (or other tasks) to go ahead without it, then `bullpen start BP-4`; \
         or drop it"
    );
}

#[test]
fn several_waiting_are_named_and_the_first_is_the_example() {
    let line = dropped_line(&["BP-4".into(), "BP-7".into()], "BP-2");
    assert!(
        line.starts_with("BP-4, BP-7 wait on BP-2, which was dropped, so they stay queued."),
        "{line}"
    );
    assert!(line.contains("`bullpen after BP-4 --none`"), "{line}");
    assert!(line.ends_with("or drop them"), "{line}");
}

#[test]
fn a_task_waits_on_what_its_after_names() {
    let t = |v: serde_json::Value| v.as_object().cloned().unwrap();
    assert!(waits_on(&t(json!({"after": ["BP-1", "BP-2"]})), "BP-2"));
    assert!(!waits_on(&t(json!({"after": ["BP-1"]})), "BP-2"));
    assert!(!waits_on(&t(json!({"after": null})), "BP-2"));
    assert!(!waits_on(&t(json!({})), "BP-2"));
}
