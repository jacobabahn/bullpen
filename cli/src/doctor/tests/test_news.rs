use super::*;

#[test]
fn the_version_is_the_first_three_numbers() {
    assert_eq!(version_in("2.1.325 (Claude Code)\n"), Some((2, 1, 325)));
    assert_eq!(version_in("claude v2.0.9-beta"), Some((2, 0, 9)));
    assert_eq!(version_in(""), None);
    assert_eq!(version_in("version 2.1"), None);
    assert!((2, 1, 99) < CLAUDE_AT_LEAST && (2, 2, 0) > CLAUDE_AT_LEAST);
}
