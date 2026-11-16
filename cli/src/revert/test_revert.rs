use super::*;

fn tasks(v: Value) -> Vec<Obj> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_object().cloned().unwrap())
        .collect()
}

#[test]
fn only_an_accepted_task_with_something_merged_can_be_reverted() {
    let why = |v: Value| not_accepted(&tasks(json!([v]))[0]);
    assert_eq!(why(json!({"id": "BP-1", "status": "done", "merged": "abc"})), None);
    assert_eq!(
        why(json!({"id": "BP-1", "status": "done", "pr": "https://x/pull/7"})),
        None
    );
    let said = |v: Value| why(v).unwrap();
    assert!(said(json!({"id": "BP-1", "status": "working"})).ends_with("only an accepted task can be reverted"));
    assert!(said(json!({"id": "BP-1", "status": "dropped"})).contains("was dropped"));
    assert!(said(json!({"id": "BP-1", "status": "done", "closed": true})).contains("nothing to merge"));
    assert!(said(json!({"id": "BP-1", "status": "done"})).contains("nothing to merge"));
    assert!(said(json!({"id": "BP-1", "status": "done", "kept": {"as": "x"}, "merged": "abc"})).contains("kept"));
}

#[test]
fn an_open_revert_comes_before_an_accepted_one_and_a_dropped_one_counts_for_nothing() {
    let all = tasks(json!([
        {"id": "BP-1", "status": "done", "merged": "abc"},
        {"id": "BP-2", "status": "dropped", "reverts": "BP-1"},
        {"id": "BP-3", "status": "done", "closed": true, "reverts": "BP-1"},
    ]));
    assert_eq!(reverted(&all, "BP-1"), None);
    assert_eq!(already(&all, "BP-1"), None);
    let mut all = all;
    all.extend(tasks(
        json!([{"id": "BP-4", "status": "done", "merged": "def", "reverts": "BP-1"}]),
    ));
    assert_eq!(reverted(&all, "BP-1"), Some(Reverted::By("BP-4".into(), None)));
    assert!(already(&all, "BP-1")
        .unwrap()
        .contains("`bullpen revert BP-4` brings it back"));
    all.extend(tasks(json!([{"id": "BP-5", "status": "review", "reverts": "BP-1"}])));
    assert_eq!(reverted(&all, "BP-1"), Some(Reverted::Open("BP-5".into())));
    assert_eq!(
        already(&all, "BP-1").unwrap(),
        "BP-1 already has an open revert task: BP-5 (in review)"
    );
}

#[test]
fn a_task_whose_revert_was_reverted_can_be_reverted_again() {
    let all = tasks(json!([
        {"id": "BP-1", "status": "done", "merged": "abc"},
        {"id": "BP-2", "status": "done", "merged": "def", "reverts": "BP-1"},
        {"id": "BP-3", "status": "done", "merged": "ghi", "reverts": "BP-2"},
    ]));
    assert_eq!(
        reverted(&all, "BP-1"),
        Some(Reverted::By("BP-2".into(), Some("BP-3".into())))
    );
    assert_eq!(already(&all, "BP-1"), None);
    assert_eq!(
        show_lines(&all[0], &all),
        [" reverted: reverted by BP-2 (which BP-3 reverted)"]
    );
    assert_eq!(
        show_lines(&all[1], &all),
        ["  reverts: BP-1", " reverted: reverted by BP-3"]
    );
}
