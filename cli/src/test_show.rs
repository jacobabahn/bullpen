//! test_helpers.py's cases for `ls`'s lines, `show`'s next steps and the task line, under their names.

use super::*;
use bullpen_core::timeutil::{isoformat, now};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn task(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

/// A repo with only a config (review_rounds 2), as test_helpers' repo() is.
fn repo() -> Repo {
    let cfg = task(json!({"review_rounds": 2}));
    Repo {
        root: PathBuf::from("/r"),
        dir: PathBuf::from("/nowhere"),
        cfg,
        own: BTreeSet::new(),
    }
}

fn ago(minutes: u64) -> String {
    isoformat(&(now() - std::time::Duration::from_secs(minutes * 60)))
}

#[test]
fn test_a_task_line_fits_the_width_and_keeps_its_age() {
    let t = task(
        json!({"id": "BP-3", "status": "needs", "title": "Fix the login redirect after sign-in",
                        "why": "Asks: python3 or python? The Makefile uses both.", "since": ago(12)}),
    );
    let e = every(std::slice::from_ref(&t));
    let line = |cut, color| task_line(&repo(), &t, &e, 0, cut, color).unwrap();
    assert_eq!(
        line(None, false),
        "● BP-3  Fix the login redirect after sign-in  12m  Asks: python3 or python? The Makefile uses both."
    );
    assert_eq!(line(Some(42), false), "● BP-3  Fix the login redirect after…  12m"); // no room for the sentence
    assert_eq!(
        line(Some(70), false),
        "● BP-3  Fix the login redirect after sign-in  12m  Asks: python3 or p…"
    );
    let colour = line(None, true);
    assert!(colour.starts_with("\x1b[1;33m●\x1b[0m ")); // the mark amber, as on the board
    assert!(colour.contains("\x1b[1;31m12m")); // needs you for over 5 minutes: the age turns red
}

#[test]
fn test_a_narrow_terminal_gets_the_boards_rows() {
    let a = task(
        json!({"id": "BP-3", "status": "needs", "title": "Fix the login redirect after sign-in",
                        "why": "Asks: python3 or python? The Makefile uses both and the CI image only has python3.",
                        "since": ago(12)}),
    );
    let b = task(json!({"id": "BP-16", "status": "todo", "title": "Emails: digest", "since": ago(1)}));
    let r = repo();
    let e = every(&[a.clone(), b.clone()]);
    let lines = |cols| ls_lines(&r, vec![b.clone(), a.clone()], cols, false, &e).unwrap();
    assert_eq!(
        lines(Some(42)),
        [
            "● BP-3   Fix the login redirect aft…  12m",
            "         Asks: python3 or python? The",
            "         Makefile uses both and the CI …",
            "◌ BP-16  Emails: digest                1m",
            "         Not started. s starts it."
        ]
    );
    assert!(lines(Some(42)).iter().all(|l| len(l) <= 41));
    assert_eq!(lines(Some(200)).len(), 2); // wide: one line each
    assert_eq!(lines(None).len(), 2); // piped: whole
    let wide = lines(Some(80)); // room for the sentence: one cut line each, not rows
    assert_eq!(
        wide,
        [
            "● BP-3   Fix the login redirect after sign-in  12m  Asks: python3 or python? T…",
            "◌ BP-16  Emails: digest  1m  Not started. s starts it."
        ]
    );
    assert!(wide.iter().all(|l| len(l) <= 79));
    let c = task(json!({"id": "BP-5", "status": "todo", "title": "Later",
                        "note": "One two three four five six seven eight"}));
    let ec = every(std::slice::from_ref(&c));
    assert_eq!(task_rows(&r, &c, &ec, 0, 20, false, None).unwrap().len(), 2); // the board's one line
    assert_eq!(task_rows(&r, &c, &ec, 0, 20, false, Some(99)).unwrap().len(), 5);
    // show's: all of it
}

#[test]
fn test_next_says_what_you_can_do_now() {
    let n = |more: Value| {
        let mut t = task(json!({"id": "BP-4"}));
        t.extend(task(more));
        next_steps(&t, &Every::new())
    };
    assert_eq!(
        n(json!({"status": "needs", "stop": {"kind": "prompt"}})),
        "`bullpen answer BP-4 <option>` (`bullpen peek BP-4` shows the prompt), or i on the board"
    );
    assert_eq!(
        n(json!({"status": "needs", "ask": true})),
        "`bullpen reply BP-4 <answer>`, or i on the board"
    );
    assert!(n(json!({"status": "ready"})).contains("`bullpen accept BP-4`"));
    assert!(n(json!({"status": "finished"})).contains("`bullpen close BP-4`"));
    assert!(n(json!({"status": "stopped"})).contains("`bullpen resume BP-4`"));
    assert_eq!(n(json!({"status": "done"})), "nothing: it's closed");
}

#[test]
fn notes_are_indented_under_their_label() {
    let notes = "- one thing\n  goes on\n\n- another";
    assert_eq!(
        notes_lines(notes),
        "    notes: - one thing\n             goes on\n\n           - another"
    );
}
