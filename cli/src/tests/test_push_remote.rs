use super::*;

#[test]
fn a_rejected_push_says_how_to_catch_up() {
    let why = push_reason(" ! [rejected]        main -> main (fetch first)\n", "backup", "main");
    assert!(why.starts_with("rejected, backup has commits main doesn't"), "{why}");
}

#[test]
fn a_failure_is_its_first_fatal_line_without_hints() {
    let out = "hint: try this\nsomething first\nfatal: could not read from remote\nerror: more\n";
    assert_eq!(push_reason(out, "b", "main"), "fatal: could not read from remote");
    assert_eq!(push_reason("hint: x\n  plain words \n", "b", "main"), "plain words");
    assert_eq!(push_reason("hint: only\n", "b", "main"), "hint: only");
    assert_eq!(push_reason("", "b", "main"), "git push failed");
    assert_eq!(
        push_reason(&format!("fatal: {}", "x".repeat(300)), "b", "main")
            .chars()
            .count(),
        200
    );
}
