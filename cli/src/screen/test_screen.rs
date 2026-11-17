use super::*;

#[test]
fn shells_as_the_regex_finds_them() {
    for yes in [
        "  2 background shells · ↓ to manage",
        "1 shell still running",
        "3 Background Shells",
        "x 1 shell",
    ] {
        assert!(shell_re_search(yes), "{yes}");
    }
    for no in ["a1 shell", "2 shellfish", "12shells", "no shells here"] {
        assert!(!shell_re_search(no), "{no}");
    }
}

#[test]
fn only_the_footer_says_a_shell_runs() {
    assert!(shell_running("❯ \n  2 background shells · ↓ to manage\n"));
    // a Monitor counts too (Claude Code 2.1.326's footer), a subagent doesn't
    assert!(shell_running(
        "❯ \n  ⏵⏵ auto mode on · 1 monitor · ← 1 agent · ↓ to manage\n"
    ));
    assert!(shell_running("done · 2 monitors still running\n"));
    assert!(!shell_running(
        "❯ \n  ⏵⏵ auto mode on · /tasks to see subagents · ← 1 agent\n"
    ));
    assert!(!shell_running("❯ \n  12 monitoring\n"));
    assert!(!shell_running(&format!(
        "1 shell still running\n{}",
        "line\n".repeat(6)
    )));
}

#[test]
fn compacting_as_the_regex_matches_it() {
    assert!(compact_re_match("✽ Compacting conversation… (esc to interrupt)"));
    assert!(compact_re_match("Running Pre-Compact hooks"));
    assert!(compact_re_match("·· \tRunning PreCompact hooks"));
    assert!(!compact_re_match("a Compacting conversation"));
    assert!(!compact_re_match("··· Compacting conversation"));
    assert!(compacting_in("Compacting conversation…", ""));
    assert!(!compacting_in("Reading", "nothing\n"));
}

#[test]
fn the_screen_says_working_blocked_or_idle() {
    assert_eq!(screen_state("Thinking…\n(esc to interrupt)\n"), Some("working"));
    assert_eq!(screen_state("Do you want to proceed?\n❯ 1. Yes\n"), Some("blocked"));
    assert_eq!(screen_state("done\n❯ \n"), Some("idle"));
    assert_eq!(screen_state("$ ls\n"), None);
}
