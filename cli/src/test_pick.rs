//! test_helpers.py's picker cases.

use super::*;

fn picker(lines: &[&str], colors: &[&str]) -> Picker {
    Picker::new(
        lines.iter().map(|s| s.to_string()).collect(),
        colors.iter().map(|s| s.to_string()).collect(),
    )
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn test_the_picker_shows_marks_in_colour() {
    let mut p = picker(&["● BP-1  Fix login", "◌ BP-2  Cover logout"], &["1;33", "2"]);
    let out = p.screen(80, 5);
    assert_eq!(out[2], "\x1b[7m> \x1b[1;33m●\x1b[22;39m BP-1  Fix login\x1b[0m"); // still reversed after
    assert_eq!(out[3], "  \x1b[2m◌\x1b[22;39m BP-2  Cover logout");
    assert_eq!(p.lines, strings(&["● BP-1  Fix login", "◌ BP-2  Cover logout"]));
    // what's searched has no colour
}

#[test]
fn test_fuzzy_matches_each_word_in_order_ignoring_case() {
    let f = fuzzy_score;
    assert!(f("lgn", "Fix the LOGIN redirect").is_some());
    assert!(f("ngl", "Fix the login redirect").is_none()); // out of order
    assert!(f("redirect fix", "Fix the login redirect").is_some()); // words in any order
    assert!(f("fix logout", "Fix the login redirect").is_none()); // every word must match
    assert!(f("digest", "Migrate sessions table").is_none()); // each word in one word
    assert!(f("rl", "Rate-limit the endpoint").is_some());
    assert_eq!(f("", "anything"), Some(0));
}

#[test]
fn test_fuzzy_ranks_letters_together_and_word_starts_first() {
    let pf = |q: &str, lines: &[&str]| pick_filter(q, &strings(lines));
    assert_eq!(pf("log", &["lxoxg", "a login"]), [1, 0]); // together
    assert_eq!(pf("log", &["l o g", "a login"]), [1]); // not across words
    assert_eq!(pf("re", &["more", "redirect"]), [1, 0]); // the start of a word
    assert_eq!(pf("42", &["BP-142", "BP-42"]), [1, 0]);
    assert_eq!(pf("bp", &["BP-1", "BP-2"]), [0, 1]); // ties keep their order
    assert_eq!(pf("x", &["a", "b"]), Vec::<usize>::new());
}

#[test]
fn test_picker_keys() {
    let mut p = picker(
        &[
            "BP-1  working  Fix login",
            "BP-2  queued  Cover logout",
            "BP-3  ready  Docs",
        ],
        &[],
    );
    assert_eq!(p.shown, [0, 1, 2]);
    for k in ["l", "o"] {
        p.key(k);
    }
    assert_eq!((p.query().as_str(), p.shown.clone(), p.sel), ("lo", vec![0, 1], 0));
    p.key("\x1b[B");
    p.key("\x1b[B"); // can't go past the last
    assert_eq!(p.sel, 1);
    assert_eq!(p.key("\r"), Some(Act::Choose));
    assert_eq!(p.chosen(), Some(1));
    p.key("\x7f");
    assert_eq!((p.query().as_str(), p.sel), ("l", 0)); // a new search starts at the top
    p.key("\x10"); // ctrl+p: can't go above the first
    assert_eq!(p.sel, 0);
    for k in [" ", "z", "z"] {
        p.key(k);
    }
    assert!(p.shown.is_empty());
    assert_eq!(p.key("\r"), None); // nothing to choose
    p.key("\x17"); // ctrl+w: the last word
    assert_eq!(p.query(), "l ");
    p.key("\x15"); // ctrl+u: all of it
    assert_eq!((p.query().as_str(), p.shown.clone()), ("", vec![0, 1, 2]));
    assert_eq!(p.key("\x04"), Some(Act::Cancel)); // ctrl+d with nothing typed
    assert_eq!(p.key("\x1b"), Some(Act::Cancel));
    assert_eq!(p.key("\x03"), Some(Act::Cancel));
    p.key("\x1b[C"); // another escape sequence does nothing
    assert_eq!(p.query(), "");
}

#[test]
fn test_picker_screen_keeps_the_selected_task_in_view() {
    let lines: Vec<String> = (1..=10).map(|n| format!("BP-{n}  queued  Task number {n}")).collect();
    let mut p = Picker::new(lines, Vec::new());
    for _ in 0..6 {
        p.key("\x0e"); // ctrl+n
    }
    let out = p.screen(13, 5); // the query, a hint and three tasks
    assert_eq!(out[0], "jump to> ");
    assert!(out[1].contains("10/10"));
    assert_eq!(
        out[2..],
        ["  BP-5  queu…", "  BP-6  queu…", "\x1b[7m> BP-7  queu…\x1b[0m"]
    );
    assert!(picker(&[], &[]).screen(80, 5)[1].contains("no open tasks in any repository"));
}

#[test]
fn test_picker_says_a_failure_in_the_normal_weight_after_the_count() {
    let mut p = picker(&["◌ BP-1  Fix login", "◌ BP-2  Cover logout"], &[]);
    assert_eq!(p.screen(42, 5)[1], "\x1b[2m2/2  ⏎ go · esc close · ↑↓ move\x1b[0m");
    p.msg = "no board: run `bullpen up`".into();
    assert_eq!(p.screen(42, 5)[1], "\x1b[2m2/2\x1b[0m  no board: run `bullpen up`");
    assert_eq!(p.screen(20, 5)[1], "\x1b[2m2/2\x1b[0m  no board: run …"); // cut to the width
}

#[test]
fn test_the_picker_edits_its_query_at_the_cursor() {
    let mut p = picker(&["BP-1  working  Fix login", "BP-2  queued  Cover logout"], &[]);
    for k in ["l", "g", "n"] {
        p.key(k);
    }
    p.key("\x1b[D");
    p.key("\x1b[B"); // down moves the selection, not the cursor
    assert_eq!(p.sel, if p.shown.len() > 1 { 1 } else { 0 });
    p.key("o");
    assert_eq!((p.query().as_str(), p.sel), ("lgon", 0)); // a new search starts at the top
    assert_eq!(p.typed(42), ("lgon".into(), 13)); // the cursor before n
    p.key("\x1b[D");
    assert_eq!(p.sel, 0); // moving the cursor isn't a new search
    assert_eq!(p.screen(42, 5)[0], "jump to> lgon");
}
