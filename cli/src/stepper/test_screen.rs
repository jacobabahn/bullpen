//! test_helpers.py's stepper cases, with their names.

use super::*;
use serde_json::json;

fn as_obj(v: Value) -> Obj {
    v.as_object().cloned().unwrap_or_default()
}

fn comment(id: i64, commit: &str, text: &str) -> Obj {
    as_obj(json!({"id": id, "step": 1, "title": "One", "commit": commit, "text": text}))
}

fn stepper(comments: Vec<Obj>) -> Stepper {
    let tour = as_obj(json!({"commit": "c1", "steps": [
        {"title": "One", "note": "Why one.", "hunks": [{"file": "a", "lines": [1, 2]}]},
        {"title": "Two", "note": "", "hunks": [{"file": "b"}]}]}));
    let diff: DiffFn = Box::new(|i, _w| (0..30).map(|n| format!("diff line {n} of step {}", i + 1)).collect());
    Stepper::new("BP-1", "Fix it", &tour, comments, 0, diff)
}

/// The lines without their colours (and, with trim, the spaces at their ends).
fn plain(lines: &[String], trim: bool) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let mut out = String::new();
            let mut rest = line.as_str();
            while let Some(i) = rest.find("\x1b[") {
                out.push_str(&rest[..i]);
                let after = &rest[i + 2..];
                let end = after.find(|c: char| !(c.is_ascii_digit() || c == ';'));
                match end {
                    Some(e) if after[e..].starts_with('m') => rest = &after[e + 1..],
                    _ => {
                        out.push_str("\x1b[");
                        rest = after;
                    }
                }
            }
            out.push_str(rest);
            if trim {
                out.trim_end().to_string()
            } else {
                out
            }
        })
        .collect()
}

fn typed(st: &mut Stepper, text: &str) {
    for c in text.chars() {
        st.key(&c.to_string(), 20);
    }
}

#[test]
fn test_stepper_keys() {
    let mut st = stepper(vec![]);
    assert_eq!(st.key("n", 20), None);
    assert_eq!(st.at, 1);
    st.key("\x1b[C", 20); // → past the last step: the summary
    assert!(st.at_end());
    st.key("n", 20); // no further
    assert!(st.at_end());
    st.key("p", 20);
    st.key("\x1b[D", 20);
    assert_eq!(st.at, 0);
    st.key("j", 20);
    st.key(" ", 10);
    assert_eq!(st.top, 9);
    st.key("k", 20);
    assert_eq!(st.top, 8);
    st.key("n", 20);
    assert_eq!(st.top, 0); // a new step starts at its top
    assert_eq!(st.key("c", 20), None);
    typed(&mut st, "Line 3 is off");
    st.key("\x7f", 20);
    assert_eq!(st.key("q", 20), None); // typed, not quit
    assert_eq!(st.key("\r", 20), Some(Act::Comment("Line 3 is ofq".into())));
    st.key("c", 20);
    st.key("x", 20);
    assert_eq!(st.key("\x1b", 20), None); // esc drops what's typed
    assert_eq!((st.mode, st.msg.as_str()), (Mode::Read, "Comment not saved."));
    st.key("c", 20);
    assert_eq!(st.key("\r", 20), None); // nothing typed: nothing saved
    assert_eq!(st.key("\x1b", 20), Some(Act::Close));
    assert_eq!(st.key("q", 20), Some(Act::Close));
}

#[test]
fn test_stepper_end_sends_or_accepts() {
    let mut st = stepper(vec![]);
    st.go(2);
    assert_eq!(st.key("s", 20), None);
    assert!(st.msg.contains("No comments to send"));
    assert_eq!(st.key("a", 20), Some(Act::Accept)); // nothing unsent: no warning
    let mut st = stepper(vec![comment(1, "c1", "Why?")]);
    st.go(2);
    assert_eq!(st.key("a", 20), None);
    assert!(st.msg.contains("1 comment not sent: a again accepts anyway"));
    assert_eq!(st.key("a", 20), Some(Act::Accept));
    st.key("a", 20);
    st.key("j", 20); // another key in between: it asks again
    assert_eq!(st.key("a", 20), None);
    assert_eq!(st.key("s", 20), Some(Act::Send));
    assert_eq!(st.key("c", 20), None); // no comments on the summary
    assert_eq!(st.mode, Mode::Read);
}

#[test]
fn test_stepper_passes_the_boards_keys_through() {
    let mut st = stepper(vec![]);
    // ⏎ and v close it and open the task's pane or diff, as on the board
    assert_eq!(st.key("\r", 20), Some(Act::Open));
    assert_eq!(st.key("v", 20), Some(Act::Diff));
    // t types a line for the worker, sent now; esc drops it
    assert_eq!(st.key("t", 20), None);
    assert_eq!(st.prompt(), "Tell BP-1: ");
    typed(&mut st, "Use the helper");
    assert_eq!(st.key("\r", 20), Some(Act::Tell("Use the helper".into())));
    assert_eq!(st.mode, Mode::Read);
    st.key("t", 20);
    st.key("x", 20);
    assert_eq!(st.key("\x1b", 20), None);
    assert_eq!((st.mode, st.msg.as_str()), (Mode::Read, "Not sent."));
    st.key("t", 20);
    let out = plain(&st.screen(60, 12), false);
    assert_eq!(out.last().unwrap().trim(), "⏎ sends it now · esc cancels");
    st.key("\x1b", 20);
    // a before the end accepts too, on a second a
    assert_eq!(st.key("a", 20), None);
    assert_eq!(st.msg, "a again accepts BP-1 before the end.");
    assert_eq!(st.key("a", 20), Some(Act::Accept));
    let mut st = stepper(vec![comment(1, "c1", "Why?")]);
    assert_eq!(st.key("a", 20), None);
    assert_eq!(st.msg, "1 comment not sent: a again accepts BP-1 before the end.");
    let out = plain(&st.screen(42, 12), true);
    assert_eq!(out.last().unwrap(), " n p · c comment · t tell · esc close"); // the shortest keys at 42
    st.key("n", 20); // another key in between: it asks again
    assert_eq!(st.key("a", 20), None);
    // c and s keep their own meanings: a comment, and at the end, send
    assert_eq!(st.key("c", 20), None);
    assert_eq!(st.mode, Mode::Comment);
}

#[test]
fn test_stepper_screen() {
    let mut st = stepper(vec![comment(1, "c1", "Why?"), comment(2, "c0", "From an old tour.")]);
    let out = plain(&st.screen(60, 12), true);
    assert_eq!(out.len(), 12);
    assert!(
        out[0].starts_with(" BP-1  Fix it") && out[0].ends_with("step 1 of 2 · 2 comments"),
        "{out:?}"
    );
    assert_eq!(out[2..9], [" One", " a 1-2", "", " Why one.", "", " ▍ Why?", ""]);
    assert_eq!(out[9], " diff line 0 of step 1");
    assert!(out[10].contains("↓ 29 more"));
    assert!(out[11].contains("c comment"));
    st.go(2);
    let out = plain(&st.screen(60, 30), true);
    assert!(out[0].ends_with("end · 2 comments"));
    assert!(out.contains(&" Your 2 comments, not sent yet:".to_string()));
    assert!(out.contains(&" s send · a accept · p back · t tell · esc close".to_string())); // the short keys at 60
    let out = plain(&st.screen(100, 30), true);
    assert!(out.contains(&" s send them back · a accept · p back · t tell · v diff · ⏎ open · esc close".to_string()));
    st.go(0);
    st.key("G", 20);
    let out = plain(&st.screen(60, 12), true);
    assert_eq!(out[9], " diff line 29 of step 1"); // the bottom, no further
    st.key("c", 20);
    typed(&mut st, &"x".repeat(80));
    let out = plain(&st.screen(60, 12), true);
    // the prompt on its own row, what's typed wrapped under it
    let want = [
        " Comment on step 1:".to_string(),
        format!(" {}", "x".repeat(57)),
        format!(" {}", "x".repeat(23)),
    ];
    assert_eq!(out[8..11], want);
    assert_eq!(out[11], " ⏎ saves · ctrl+j new line · esc cancels");
    assert_eq!(st.cursor(60, 12), (11, 25));
    st.key("\x1b", 20);
    st.diff = Box::new(|_i, _w| vec![format!("\x1b[32m+{}\x1b[0m", "y".repeat(100)), "宽".repeat(40)]);
    st.cache.clear();
    st.top = 0;
    let out = plain(&st.screen(20, 20), true);
    assert!(out.contains(&format!(" +{}", "y".repeat(17)))); // cut before the last column
    assert!(out.contains(&format!(" {}", "宽".repeat(9))));
    assert!(
        out.iter()
            .filter(|l| !l.contains('宽'))
            .all(|l| l.chars().count() <= 19),
        "{out:?}"
    );
}

#[test]
fn test_clip_line_counts_columns_not_codes() {
    assert_eq!(clip_line("\x1b[1mabc\x1b[0mdef", 4), "\x1b[1mabc\x1b[0md");
    assert_eq!(clip_line("ab", 5), "ab");
    assert_eq!(clip_line("宽宽宽", 5), "宽宽");
}

#[test]
fn test_a_comment_can_have_lines_and_a_tell_is_one() {
    let mut st = stepper(vec![]);
    st.key("c", 20);
    st.key("\x1b[200~Use the helper:\n\n  helper(x)  \n\x1b[201~", 20);
    st.key("\n", 20);
    typed(&mut st, "Then test it ");
    let out = plain(&st.screen(42, 16), true);
    assert_eq!(out.len(), 16);
    assert_eq!(
        out[9..],
        [
            " Comment on step 1:",
            " Use the helper:",
            "",
            "   helper(x)",
            "",
            " Then test it",
            " ⏎ saves · ctrl+j new line · esc cancels"
        ]
    );
    assert_eq!(st.cursor(42, 16), (15, 15));
    assert_eq!(
        st.key("\r", 20),
        Some(Act::Comment("Use the helper:\n\n  helper(x)\n\nThen test it".into()))
    );
    st.key("t", 20);
    st.key("\x1b[200~one\ntwo\x1b[201~", 20); // a pasted line break doesn't send it
    assert_eq!(st.mode, Mode::Tell);
    st.key("\x1bb", 20); // alt+b: a word back
    st.key("\x17", 20);
    assert_eq!(st.key("\r", 20), Some(Act::Tell("two".into())));
}

#[test]
fn wrap_text_keeps_lines_and_hangs_list_items() {
    // stepper.wrap_text("1. one two three four\n\n- five six", 14, "  ") in Python
    assert_eq!(
        wrap_text("1. one two three four\n\n- five six", 14, "  "),
        ["  1. one two", "     three", "     four", "", "  - five six"]
    );
}
