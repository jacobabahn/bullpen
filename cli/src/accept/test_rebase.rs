use super::*;
use serde_json::json;

fn set(files: &[&str]) -> BTreeSet<String> {
    files.iter().map(|f| f.to_string()).collect()
}

#[test]
fn lines_both_sides_added_at_one_place_are_kept_base_first() {
    let file = "a\n<<<<<<< HEAD\nfrom main\n||||||| base\n=======\nfrom the task\n>>>>>>> task\nb\n";
    assert_eq!(both_sides(file).as_deref(), Some("a\nfrom main\nfrom the task\nb\n"));
}

#[test]
fn a_conflict_over_lines_in_the_base_is_left_be() {
    let file = "<<<<<<< HEAD\nmain's\n||||||| base\nold\n=======\ntask's\n>>>>>>> task\n";
    assert_eq!(both_sides(file), None);
}

#[test]
fn markers_out_of_place_or_none_are_left_be() {
    assert_eq!(both_sides("just\nlines\n"), None); // no conflict
    assert_eq!(both_sides("<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> t\n"), None); // no base part
    assert_eq!(both_sides("<<<<<<< HEAD\nx\n||||||| b\n=======\ny\n"), None); // not ended
}

#[test]
fn a_marker_is_seven_of_one_character_then_a_space_or_the_end() {
    assert_eq!(marker("<<<<<<< HEAD"), Some('<'));
    assert_eq!(marker("======="), Some('='));
    assert_eq!(marker("||||||| merged common ancestors"), Some('|'));
    assert_eq!(marker("<<<<<<<< eight"), None);
    assert_eq!(marker("=======x"), None);
    assert_eq!(marker("<<<<<<"), None);
    // outside a conflict, "=======" is just a line
    assert_eq!(
        both_sides("a\n=======\nb\n<<<<<<< H\n||||||| b\n=======\nc\n>>>>>>> t\n").as_deref(),
        Some("a\n=======\nb\nc\n")
    );
}

#[test]
fn lines_keep_their_ends() {
    assert_eq!(lines_with_ends("a\r\nb\n\nc"), ["a\r\n", "b\n", "\n", "c"]);
    assert_eq!(lines_with_ends("x\u{2028}y\r"), ["x\u{2028}", "y\r"]);
    assert!(lines_with_ends("").is_empty());
}

#[test]
fn what_a_working_task_is_told() {
    assert_eq!(conflict_or(set(&[]), true, vec![]), Need::Alone("no overlap"));
    assert_eq!(
        conflict_or(set(&["b", "a"]), false, vec!["c".into()]),
        Need::Conflict(vec!["a".into(), "b".into()])
    );
    assert_eq!(
        conflict_or(set(&[]), false, vec!["c".into()]),
        Need::Uncommitted(vec!["c".into()])
    );
    assert_eq!(conflict_or(set(&[]), false, vec![]), Need::Alone("no conflict"));
}

fn task(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

#[test]
fn a_working_task_with_a_conflict_is_told_what_landed_and_to_keep_both() {
    let accepted = task(json!({"id": "BP-1", "title": "Fix login"}));
    assert_eq!(
        told(
            "main",
            &accepted,
            &Need::Conflict(vec!["a.txt".into(), "b.txt".into()]),
            "origin/main"
        ),
        "main moved: BP-1 (Fix login) was just accepted, and your branch conflicts with it in a.txt, b.txt. \
         At a good point, run `git fetch origin`, then rebase your branch onto origin/main; where both sides \
         changed the same place, keep both."
    );
}

#[test]
fn a_working_task_with_uncommitted_changes_is_told_to_commit_then_rebase_and_of_no_conflicts() {
    let accepted = task(json!({"id": "BP-1"}));
    let msg = told("main", &accepted, &Need::Uncommitted(vec!["a.txt".into()]), "main");
    assert_eq!(
        msg,
        "main moved: BP-1 was just accepted, and it changed a.txt, where you have uncommitted changes. \
         Commit them at a good point, then rebase your branch onto main."
    );
    assert!(!msg.contains("conflict"));
}

#[test]
fn show_says_a_ready_task_no_longer_rebases() {
    let t = task(json!({"id": "BP-3", "base": "main"}));
    assert_eq!(
        stale_line(&t, &["a.txt".into(), "b.txt".into()]),
        "   rebase: no longer rebases onto main: conflicts in a.txt, b.txt. `bullpen tell BP-3` it to rebase and \
         run ready again"
    );
}

#[test]
fn a_remote_base_is_fetched_first() {
    assert_eq!(
        onto("origin/main"),
        "run `git fetch origin`, then rebase your branch onto origin/main"
    );
    assert_eq!(onto("main"), "rebase your branch onto main");
}

#[test]
fn a_line_with_wide_characters_is_no_marker() {
    // a markdown table row or a quote: byte 7 falls inside a character
    assert_eq!(marker("| 日本語 |"), None);
    assert_eq!(marker("> 日本語"), None);
    assert_eq!(marker("=======日本"), None);
    assert_eq!(marker("<<<<<<< 日本語"), Some('<'));
    let file = "| 日本語 |\n<<<<<<< HEAD\n> 引用\n||||||| base\n=======\n| 表 |\n>>>>>>> t\n";
    assert_eq!(both_sides(file).as_deref(), Some("| 日本語 |\n> 引用\n| 表 |\n"));
}
