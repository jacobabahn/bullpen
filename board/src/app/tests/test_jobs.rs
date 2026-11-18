use super::*;
use crate::app::testing::*;

#[test]
fn a_prompt_that_cant_be_read_closes_the_answer() {
    let mut a = App::for_test(vec![needs("BP-1", false, "prompt")]);
    a.settle();
    press(&mut a, 'i');
    a.prompted("BP-2".into(), None, "other".into()); // another task's: ignored
    assert!(matches!(a.mode, Mode::Answer { .. }));
    a.prompted("BP-1".into(), None, "BP-1 isn't waiting at a prompt".into());
    assert!(matches!(a.mode, Mode::Normal));
    assert_eq!(a.msg, "BP-1 isn't waiting at a prompt");
    // a screen with no prompt now stays open, with nothing to press
    press(&mut a, 'i');
    a.prompted("BP-1".into(), Some(Prompt { question: None, ..prompt(&[]) }), String::new());
    press(&mut a, '1');
    assert!(matches!(a.mode, Mode::Answer { .. }));
}

#[test]
fn after_an_accept_the_next_task_that_needs_you_is_selected() {
    let mut a = App::for_test(vec![task("BP-1", "ready"), task("BP-2", "needs"), task("BP-3", "ready"), task("BP-4", "finished")]);
    a.sel = Some("BP-3".into());
    a.settle();
    a.done("BP-3".into(), Job::Accept, true, String::new());
    assert_eq!(a.msg, "Accepted BP-3.");
    assert_eq!(a.sel.as_deref(), Some("BP-2"));
    // with nothing needing you, the next one ready to accept, then a finished one,
    // even past "Coordinator on it" and a task in review
    let mut a = App::for_test(vec![task("BP-1", "ready"), task("BP-2", "triage"), task("BP-3", "ready"), task("BP-4", "finished"), task("BP-5", "review")]);
    a.sel = Some("BP-1".into());
    a.settle();
    a.done("BP-1".into(), Job::Accept, true, String::new());
    assert_eq!(a.sel.as_deref(), Some("BP-3"));
    a.done("BP-3".into(), Job::Accept, true, String::new());
    assert_eq!(a.sel.as_deref(), Some("BP-4")); // BP-1's still listed until the board reads again
    // a closed one too; with nothing else to accept or close the selection stays
    let mut a = App::for_test(vec![task("BP-4", "finished")]);
    a.sel = Some("BP-4".into());
    a.done("BP-4".into(), Job::Close, true, String::new());
    assert_eq!((a.msg.text.as_str(), a.sel.as_deref()), ("Closed BP-4.", Some("BP-4")));
    // a failed accept, or a selection already moved elsewhere, stays put
    let mut a = App::for_test(vec![task("BP-1", "ready"), task("BP-2", "needs"), task("BP-3", "ready")]);
    a.sel = Some("BP-1".into());
    a.done("BP-1".into(), Job::Accept, false, "merge refused".into());
    assert_eq!(a.sel.as_deref(), Some("BP-1"));
    a.done("BP-3".into(), Job::Accept, true, String::new());
    assert_eq!(a.sel.as_deref(), Some("BP-1"));
}

#[test]
fn an_accept_that_changed_the_keys_says_so_in_place_of_accepted() {
    let mut a = App::for_test(vec![task("BP-1", "ready")]);
    a.done("BP-1".into(), Job::Accept, true, "Keys changed: run `rex config reload`".into());
    assert_eq!(a.msg, "Keys changed: run `rex config reload`");
    // any other last line is still "Accepted"
    let mut a = App::for_test(vec![task("BP-1", "ready")]);
    a.done("BP-1".into(), Job::Accept, true, "BP-1 done (merged into main: 0123456789ab)".into());
    assert_eq!(a.msg, "Accepted BP-1.");
}

#[test]
fn a_failed_peek_says_why_without_bullpens_prefix() {
    let mut a = App::for_test(vec![task("BP-3", "stopped")]);
    a.mode = Mode::Overlay { title: "BP-3".into(), lines: vec!["…".into()], peek: Some("BP-3".into()) };
    a.peeked("BP-3".into(), said(&last_line("bullpen: BP-3's agent isn't running\n")), false);
    assert!(a.peek_failed);
    assert!(matches!(&a.mode, Mode::Overlay { lines, .. } if lines == &["BP-3's agent isn't running."]));
    // once it reads the screen again, it's live
    a.peeked("BP-3".into(), "$ cargo test".into(), true);
    assert!(!a.peek_failed);
}
