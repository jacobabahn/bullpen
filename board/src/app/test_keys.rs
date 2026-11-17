use super::*;
use crate::app::testing::*;
use crate::app::Msg;
use std::path::PathBuf;

#[test]
fn a_closes_a_finished_task_after_y() {
    let mut a = App::for_test(vec![task("BP-1", "finished")]);
    a.settle();
    press(&mut a, 'a');
    assert_eq!(a.pending, Some((Action::Close, "BP-1".into())));
    press(&mut a, 'y');
    assert_eq!(a.job("BP-1"), Some(Job::Close));
    assert_eq!(Job::Close.args("BP-1"), vec!["accept", "BP-1"]);
    // n cancels
    let mut a = App::for_test(vec![task("BP-1", "finished")]);
    a.settle();
    press(&mut a, 'a');
    press(&mut a, 'n');
    assert!(a.running.is_empty() && a.pending.is_none());
    assert_eq!(a.msg, "Cancelled.");
}

#[test]
fn a_accepts_a_ready_task_after_y() {
    let mut a = App::for_test(vec![task("BP-1", "ready")]);
    a.settle();
    press(&mut a, 'a');
    assert_eq!(a.pending, Some((Action::Accept, "BP-1".into())));
    press(&mut a, 'y');
    assert_eq!(a.job("BP-1"), Some(Job::Accept));
    assert_eq!(Job::Accept.args("BP-1"), vec!["accept", "BP-1"]);
}

#[test]
fn a_on_a_task_that_isnt_done_says_so() {
    let mut a = App::for_test(vec![task("BP-1", "working")]);
    a.settle();
    press(&mut a, 'a');
    assert!(a.pending.is_none());
    assert_eq!(a.msg, "BP-1 isn't ready yet.");
    let mut a = App::for_test(vec![task("BP-1", "done")]);
    a.closed = true;
    a.settle();
    press(&mut a, 'a');
    assert!(a.pending.is_none());
    assert_eq!(a.msg, "BP-1 is closed already.");
}

#[test]
fn c_always_goes_to_the_coordinator() {
    for status in ["ready", "finished", "needs"] {
        let mut a = App::for_test(vec![task("BP-1", status)]);
        a.settle();
        press(&mut a, 'c');
        assert!(a.pending.is_none() && a.running.is_empty(), "{status}");
        assert!(a.msg.text.contains("can't run"), "{status}: {}", a.msg.text); // it tried `bullpen go`
    }
}

#[test]
fn the_help_scrolls_and_any_other_key_closes_it_without_quitting() {
    let mut a = App::for_test(vec![task("BP-1", "ready")]);
    a.height = 14;
    press(&mut a, '?');
    assert!(matches!(a.mode, Mode::Help { top: 0 }));
    press(&mut a, 'j');
    press(&mut a, 'j');
    press(&mut a, 'k');
    assert!(matches!(a.mode, Mode::Help { top: 1 }));
    a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(matches!(a.mode, Mode::Help { top: 2 }));
    press(&mut a, ' '); // a page: the height less the title, gap, foot and gap
    assert!(matches!(a.mode, Mode::Help { top: 12 }));
    press(&mut a, 'b');
    press(&mut a, 'b');
    assert!(matches!(a.mode, Mode::Help { top: 0 }));
    // q closes the list; it doesn't quit the board
    assert!(a.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)));
    assert!(matches!(a.mode, Mode::Normal));
    for close in [KeyCode::Esc, KeyCode::Char('?'), KeyCode::Char('a')] {
        press(&mut a, '?');
        a.key(KeyEvent::new(close, KeyModifiers::NONE));
        assert!(matches!(a.mode, Mode::Normal), "{close:?}");
    }
    assert!(a.pending.is_none(), "the key that closes it does nothing else");
}

#[test]
fn i_on_a_prompt_shows_it_and_a_number_answers_it() {
    let mut a = App::for_test(vec![needs("BP-1", false, "prompt")]);
    a.settle();
    press(&mut a, 'i');
    assert!(matches!(&a.mode, Mode::Answer { id, prompt: None, .. } if id == "BP-1"));
    a.peeking = false; // the read it started can't reach this test
    a.prompted("BP-1".into(), Some(prompt(&[("1", "Yes"), ("2", "No")])), String::new());
    // a key that's no option does nothing
    press(&mut a, '7');
    assert!(matches!(a.mode, Mode::Answer { .. }));
    // an option runs `bullpen answer BP-1 2 --prompt ab12` (there's no bullpen here)
    press(&mut a, '2');
    assert!(matches!(a.mode, Mode::Normal));
    assert!(a.msg.text.contains("can't run"), "{}", a.msg.text);
    // esc goes back without answering
    a.answer(&needs("BP-1", false, "prompt"));
    a.answer_key(KeyCode::Esc);
    assert!(matches!(a.mode, Mode::Normal));
}

#[test]
fn i_on_an_ask_types_an_answer_for_the_coordinator() {
    let mut a = App::for_test(vec![needs("BP-1", true, "ended")]);
    a.settle();
    press(&mut a, 'i');
    assert!(matches!(&a.mode, Mode::Tell { id, reply: true, .. } if id == "BP-1"));
    for c in "Postgres".chars() {
        press(&mut a, c);
    }
    a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(a.mode, Mode::Normal));
    assert!(a.msg.text.contains("can't run"), "{}", a.msg.text); // it tried `bullpen reply`
}

#[test]
fn the_tell_line_edits_takes_a_paste_whole_and_has_no_cap() {
    let d = Scratch::new("tell-edit");
    let mut a = App::for_test(vec![task("BP-1", "working")]);
    (a.bullpen, a.root) = (fake_bullpen(&d, "none"), d.0.clone());
    a.settle();
    let ctrl = |a: &mut App, c: char| a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    press(&mut a, 't');
    for c in "use the old table".chars() {
        press(&mut a, c);
    }
    ctrl(&mut a, 'w'); // the last word goes; ctrl+w isn't typed as w
    key(&mut a, KeyCode::Home);
    for c in "please ".chars() {
        press(&mut a, c);
    }
    // a pasted newline is a space: it doesn't send the line
    a.handle(Msg::Paste("keep\nit, ".into()));
    assert!(matches!(a.mode, Mode::Tell { .. }));
    // past the old 500-character cap, nothing is dropped
    key(&mut a, KeyCode::End);
    let long = "x".repeat(600);
    a.handle(Msg::Paste(long.clone()));
    let Mode::Tell { text, .. } = &a.mode else { panic!() };
    assert_eq!(text.text(), format!("please keep it, use the old {long}"));
    // ctrl+u cuts to the start; ⏎ sends what's left
    ctrl(&mut a, 'u');
    for c in "ok".chars() {
        press(&mut a, c);
    }
    key(&mut a, KeyCode::Enter);
    assert_eq!(calls(&d), ["tell BP-1 ok"]);
    // a paste with no line to go into isn't taken as keys
    a.handle(Msg::Paste("dq".into()));
    assert!(matches!(a.mode, Mode::Normal) && a.pending.is_none());
}

#[test]
fn i_with_nothing_to_answer_says_so() {
    let mut a = App::for_test(vec![needs("BP-1", false, "ended"), task("BP-2", "working")]);
    a.settle();
    press(&mut a, 'i');
    assert!(matches!(a.mode, Mode::Normal));
    assert_eq!(a.msg, "BP-1 asks nothing here: ⏎ opens it.");
    press(&mut a, 'j');
    press(&mut a, 'i');
    assert_eq!(a.msg, "BP-2 doesn't need you.");
}

/// An app whose bullpen is this script.
fn with_bullpen(d: &Scratch, tasks: Vec<Task>, script: &str) -> App {
    let exe = d.0.join("bullpen");
    program(&exe, script);
    let mut a = App::for_test(tasks);
    (a.bullpen, a.root) = (exe, d.0.clone());
    a.settle();
    a
}

#[test]
fn a_failure_stays_until_the_next_action_on_a_task() {
    let d = Scratch::new("failure");
    let fails = "#!/bin/sh\necho 'bullpen: the coordinator has no pane' >&2\nexit 1\n";
    let mut a = with_bullpen(&d, vec![task("BP-1", "needs"), task("BP-2", "ready")], fails);
    press(&mut a, 'c');
    assert_eq!(a.msg, Note::failed("the coordinator has no pane"));
    // looking around keeps it: moving, n, f, the help and back
    for c in ['j', 'k', 'n', 'f', 'f', 'x', '?', 'q'] {
        press(&mut a, c);
        assert_eq!(a.msg, "the coordinator has no pane", "after {c}");
    }
    key(&mut a, KeyCode::Down);
    assert_eq!(a.msg.kind, Kind::Failed);
    // esc puts it away
    key(&mut a, KeyCode::Esc);
    assert!(a.msg.is_empty());
    // an action on a task does too, saying its own thing
    press(&mut a, 'c');
    a.sel = Some("BP-1".into());
    press(&mut a, 'a');
    assert_eq!(a.msg, Note::info("BP-1 isn't ready yet."));
    // which, not being a failure, goes with the next key
    press(&mut a, 'j');
    assert!(a.msg.is_empty());

    // a background command's result: green when it worked, red and kept when not
    a.done("BP-2".into(), Job::Accept, true, String::new());
    assert_eq!(a.msg, Note::done("Accepted BP-2."));
    press(&mut a, 'k');
    assert!(a.msg.is_empty());
    a.done("BP-2".into(), Job::Accept, false, "BP-2's branch doesn't merge cleanly".into());
    assert_eq!(a.msg, Note::failed("BP-2's branch doesn't merge cleanly"));
    press(&mut a, 'j');
    assert_eq!(a.msg.kind, Kind::Failed);
    // a y/n answered is an action
    press(&mut a, 'a');
    assert!(a.msg.is_empty() && a.pending.is_some());
}

#[test]
fn slash_opens_the_picker_or_says_why_not() {
    let d = Scratch::new("slash");
    let mut a = with_bullpen(&d, vec![task("BP-1", "ready")], "#!/bin/sh\n");
    a.bullpen = fake_bullpen(&d, "current");
    press(&mut a, '/');
    assert_eq!(calls(&d), ["pick --pane"]);
    assert!(a.msg.is_empty() && matches!(a.mode, Mode::Normal));
    let d = Scratch::new("slash-fails");
    let mut a = with_bullpen(&d, vec![], "#!/bin/sh\necho 'bullpen: no Rex session here' >&2\nexit 1\n");
    press(&mut a, '/');
    assert_eq!(a.msg, Note::failed("no Rex session here"));
}

#[test]
fn ctrl_letters_dont_act_as_the_letters() {
    let mut a = App::for_test(vec![task("BP-1", "ready"), task("BP-2", "ready")]);
    a.settle();
    for c in ['a', 'q', 'l', 'k'] {
        a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }
    assert!(a.pending.is_none() && matches!(a.mode, Mode::Normal) && a.msg.is_empty());
    a.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert_eq!(a.sel.as_deref(), Some("BP-2"));
    assert!(a.pending.is_none());
}

#[test]
fn l_and_o_say_what_they_did() {
    let d = Scratch::new("say");
    let c = |id: &str, who: &str| owned(id, "needs", who);
    let mut a = with_bullpen(&d, vec![c("BP-1", "c1"), c("BP-2", "c2")], "#!/bin/sh\nexit 0\n");
    a.cfg.coordinators = vec!["c1".into(), "c2".into()];
    a.me = Some("c1".into());
    press(&mut a, 'o');
    assert_eq!(a.msg, Note::info("Showing c2's tasks too."));
    press(&mut a, 'o');
    assert_eq!(a.msg, "Hiding c2's tasks.");
    a.cfg.coordinators.push("c3".into());
    press(&mut a, 'o');
    assert_eq!(a.msg, "Showing other coordinators' tasks too.");
    press(&mut a, 'l');
    assert_eq!(a.msg, Note::info("Compact layout."));
    press(&mut a, 'l');
    assert_eq!(a.msg, "Grouped layout.");
    // a setting that can't be saved says why, in red
    let d = Scratch::new("say-fails");
    let mut a = with_bullpen(&d, vec![], "#!/bin/sh\necho 'bullpen: config.json is read-only' >&2\nexit 1\n");
    press(&mut a, 'l');
    assert_eq!(a.msg, Note::failed("config.json is read-only"));
}

fn peek_at(a: &mut App, id: &str) {
    a.sel = Some(id.into());
    a.peeking = true; // no read in the background: it can't reach this test
    press(a, 'p');
    assert!(matches!(&a.mode, Mode::Overlay { peek: Some(p), .. } if p == id));
}

#[test]
fn a_peek_returns_on_esc_or_q_and_stays_on_other_keys() {
    let mut a = App::for_test(vec![task("BP-1", "working"), task("BP-2", "ready")]);
    for k in [KeyCode::Esc, KeyCode::Char('q')] {
        peek_at(&mut a, "BP-1");
        for c in "jkn?lsrpgc".chars() {
            assert!(a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
            assert!(matches!(a.mode, Mode::Overlay { .. }), "{c} left the peek");
        }
        assert!(a.key(KeyEvent::new(k, KeyModifiers::NONE)));
        assert!(matches!(a.mode, Mode::Normal));
        assert_eq!(a.sel.as_deref(), Some("BP-1"));
        assert!(a.pending.is_none()); // q went back; it didn't ask to quit
    }
}

#[test]
fn t_i_a_d_in_a_peek_go_back_and_act_on_the_peeked_task() {
    let mut a = App::for_test(vec![task("BP-1", "working"), task("BP-2", "ready"), needs("BP-3", true, "worker")]);
    peek_at(&mut a, "BP-1");
    a.sel = Some("BP-2".into()); // the selection moved under the peek (a select from cmd+;)
    press(&mut a, 't');
    assert!(matches!(&a.mode, Mode::Tell { id, reply: false, .. } if id == "BP-1"));
    a.mode = Mode::Normal;
    peek_at(&mut a, "BP-2");
    press(&mut a, 'a');
    assert!(matches!(a.mode, Mode::Normal));
    assert_eq!(a.pending, Some((Action::Accept, "BP-2".into())));
    press(&mut a, 'n');
    peek_at(&mut a, "BP-2");
    press(&mut a, 'd');
    assert_eq!(a.pending, Some((Action::Drop, "BP-2".into())));
    press(&mut a, 'n');
    peek_at(&mut a, "BP-3");
    press(&mut a, 'i');
    assert!(matches!(&a.mode, Mode::Tell { id, reply: true, .. } if id == "BP-3"));
}

#[test]
fn enter_and_v_in_a_peek_go_back_and_open_the_pane_or_diff() {
    let d = Scratch::new("peek-acts");
    let exe = fake_bullpen(&d, "current");
    let mut a = App::for_test(vec![task("BP-1", "working").with(json!({"block": "b1"}))]);
    (a.bullpen, a.root) = (exe, d.0.clone());
    peek_at(&mut a, "BP-1");
    a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(a.mode, Mode::Normal));
    assert_eq!(a.msg, Note::done("Opened BP-1."));
    peek_at(&mut a, "BP-1");
    press(&mut a, 'v');
    assert!(matches!(a.mode, Mode::Normal));
    assert_eq!(a.msg, Note::done("Opened BP-1's diff."));
    assert_eq!(calls(&d), ["go BP-1", "diff BP-1 --pane"]);
}

#[test]
fn enter_and_peek_go_to_the_agent_whose_turn_it_is() {
    let d = Scratch::new("whose-turn");
    let exe = fake_bullpen(&d, "current");
    let stopped = |id: &str, kind: &str, why: &str| {
        let stop = json!({"by": "reviewer", "kind": kind});
        task(id, "triage").with(json!({"why": why, "stop": stop, "block": "b1", "review": {"block": "b2"}}))
    };
    let tasks = vec![
        stopped("BP-1", "ended", "Reviewer ended its turn without a verdict"),
        // a reviewer stopped at the usage limit: the worker's to see to, as the command line has it
        stopped("BP-2", "limit", "Reviewer stopped at the usage limit"),
    ];
    let mut a = App::for_test(tasks);
    (a.bullpen, a.root) = (exe, d.0.clone());
    for (id, opened, reviewer) in [("BP-1", "Opened BP-1's reviewer.", true), ("BP-2", "Opened BP-2.", false)] {
        a.sel = Some(id.into());
        key(&mut a, KeyCode::Enter);
        assert_eq!(a.msg, Note::done(opened));
        peek_at(&mut a, id);
        assert_eq!(a.peek_review, reviewer, "{id}");
        key(&mut a, KeyCode::Esc);
    }
    assert_eq!(calls(&d), ["go BP-1 --review", "go BP-2"]);
}

#[test]
fn q_asks_before_quitting_the_board_and_ctrl_c_doesnt() {
    let mut a = App::for_test(vec![task("BP-1", "ready")]);
    assert!(a.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)));
    assert!(matches!(a.mode, Mode::Quit));
    assert!(a.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))); // a second q doesn't quit either
    assert!(matches!(a.mode, Mode::Normal));
    assert_eq!(a.msg, "Cancelled.");
    press(&mut a, 'q');
    assert!(!a.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)));
    let mut a = App::for_test(vec![task("BP-1", "ready")]);
    assert!(!a.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    // a board asking whether to quit isn't replaced under the question
    press(&mut a, 'q');
    assert!(a.replaced().is_none());
}

/// A bullpen that logs its arguments, says the tour's state is `state` to
/// `tour --show --json`, and answers `tour --pane` as the real one does.
fn fake_bullpen(d: &Scratch, state: &str) -> PathBuf {
    let exe = d.0.join("bullpen");
    let log = d.0.join("log");
    program(&exe, &format!(
        "#!/bin/sh\n[ \"$1\" = --ready ] && exit 0\necho \"$*\" >> {log}\ncase \"$*\" in\n\
         *--show*) echo '{{\"id\": \"'$2'\", \"state\": \"{state}\", \"why\": null, \"steps\": []}}' ;;\n\
         *--new*) echo \"writing a tour of $2: it opens when it's done\" ;;\n\
         *) echo \"opened $2's tour\" ;;\nesac\n",
        log = log.display()
    ));
    exe
}

fn calls(d: &Scratch) -> Vec<String> {
    std::fs::read_to_string(d.0.join("log")).unwrap_or_default().lines().map(String::from).collect()
}

#[test]
fn g_on_a_task_that_isnt_ready_or_in_review_says_why_not() {
    for status in ["working", "todo", "needs", "finished"] {
        let mut a = App::for_test(vec![task("BP-1", status)]);
        a.settle();
        press(&mut a, 'g');
        assert!(matches!(a.mode, Mode::Normal));
        let said = if status == "finished" { "BP-1 has no code to tour." } else { "BP-1 gets a tour once it's in review." };
        assert_eq!(a.msg, said);
    }
}

#[test]
fn g_opens_a_current_tour_or_has_one_written() {
    for (state, said) in [("current", "Opened BP-1's tour."), ("none", "Opened BP-1's tour."), ("failed", "Opened BP-1's tour.")] {
        let d = Scratch::new(&format!("g-{state}"));
        let mut a = App::for_test(vec![task("BP-1", "ready")]);
        (a.bullpen, a.root) = (fake_bullpen(&d, state), d.0.clone());
        a.settle();
        press(&mut a, 'g');
        assert!(matches!(a.mode, Mode::Normal), "{state}");
        assert_eq!(calls(&d), ["tour BP-1 --show --json", "tour BP-1 --pane"], "{state}");
        assert_eq!(a.msg, said, "{state}");
    }
    // being written: no need to look, `--pane` opens it once it's done
    let d = Scratch::new("g-writing");
    let mut a = App::for_test(vec![task("BP-1", "review").with(json!({"tour": {"state": "writing"}}))]);
    (a.bullpen, a.root) = (fake_bullpen(&d, "writing"), d.0.clone());
    a.settle();
    press(&mut a, 'g');
    assert_eq!(calls(&d), ["tour BP-1 --pane"]);
}

#[test]
fn g_on_a_stale_tour_asks_new_or_old() {
    let d = Scratch::new("g-stale");
    let mut a = App::for_test(vec![task("BP-1", "ready")]);
    (a.bullpen, a.root) = (fake_bullpen(&d, "stale"), d.0.clone());
    a.settle();
    press(&mut a, 'g');
    assert!(matches!(&a.mode, Mode::Stale { id } if id == "BP-1"));
    assert_eq!(a.msg, "BP-1's tour is stale: the code changed.");
    press(&mut a, 'n');
    assert!(matches!(a.mode, Mode::Normal));
    assert_eq!(calls(&d).last().unwrap(), "tour BP-1 --pane --new");
    assert_eq!(a.msg, "Writing a tour of BP-1: it opens when it's done.");
    press(&mut a, 'g');
    press(&mut a, 'o');
    assert_eq!(calls(&d).last().unwrap(), "tour BP-1 --pane");
    assert_eq!(a.msg, "Opened BP-1's tour.");
    press(&mut a, 'g');
    press(&mut a, 'x');
    assert_eq!(a.msg, "Cancelled.");
    assert_eq!(calls(&d).len(), 5); // three looks, two opens
}
