use crate::model::{self, Layout};
use crate::ui::draw;
use crate::ui::testing::*;
use crate::Mode;
use ratatui::backend::TestBackend;
use ratatui::layout::Position;
use ratatui::style::Modifier;
use ratatui::Terminal;

#[test]
fn a_peek_says_which_keys_act_from_it_and_q_asks_on_the_key_row() {
    let mut a = app(vec![ready("BP-1")], 0, Layout::Grouped);
    a.mode = Mode::Overlay { title: "BP-1  Fix it".into(), lines: vec!["$ cargo test".into()], peek: Some("BP-1".into()) };
    let s = screen(&mut a, 42, 12);
    assert_eq!(row(&s, 0), " live · t tell  ⏎ open  esc returns");
    let s = screen(&mut a, 80, 12);
    assert_eq!(row(&s, 0), " live · t tell  ⏎ open  i answer  a accept  v diff  esc returns");
    let s = screen(&mut a, 24, 12);
    assert_eq!(row(&s, 0), " live · esc returns");

    a.mode = Mode::Quit;
    let s = screen(&mut a, 42, 12);
    assert_eq!(row(&s, 0), " y quits the board   any other key stays");
    let s = screen(&mut a, 30, 12);
    assert_eq!(row(&s, 0), " y quits   n stays");
}

#[test]
fn a_prompt_shows_its_screen_then_its_options() {
    let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
    let p = model::Prompt {
        who: "worker".into(),
        question: Some("Do you want to proceed?".into()),
        options: vec![("1".into(), "Yes".into()), ("2".into(), "Yes, and always allow access to /tmp/x".into()), ("3".into(), "No".into())],
        id: Some("ab12".into()),
        screen: (1..=9).map(|n| format!("line {n}")).chain(["Do you want to proceed?".to_string()]).collect(),
    };
    a.mode = Mode::Answer { id: "BP-1".into(), title: "BP-1  Fix it".into(), prompt: Some(p.clone()) };
    let s = screen(&mut a, 40, 12);
    assert_eq!(s, [
        " BP-1  Fix it",
        "",
        " line 7",
        " line 8",
        " line 9",
        " Do you want to proceed?",
        "",
        " 1 Yes",
        " 2 Yes, and always allow access to /t…",
        " 3 No",
        " live · ⏎ opens its pane · esc returns",
        "",
    ].map(String::from), "{s:#?}");
    a.mode = Mode::Answer { id: "BP-1".into(), title: "BP-1  Fix it".into(), prompt: Some(model::Prompt { question: None, options: vec![], ..p }) };
    let s = screen(&mut a, 40, 12);
    assert_eq!(s[9..11], [" No prompt to answer on its screen now.", " live · ⏎ opens its pane · esc returns"].map(String::from), "{s:#?}");
}

#[test]
fn i_on_a_question_shows_all_of_it_above_the_answer() {
    let mut a = app(vec![asking("BP-31")], 1, Layout::Grouped);
    a.mode = Mode::Tell { id: "BP-31".into(), text: "Postgres".into(), reply: true };
    let mut term = Terminal::new(TestBackend::new(42, 16)).unwrap();
    term.draw(|f| draw(f, &mut a)).unwrap();
    let buf = term.backend().buffer().clone();
    let s: Vec<String> = (0..16).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
    assert_eq!(s, [
        " BP-31  Architecture round 2: review",
        "",
        " Pick: candidate 4 (one rule for who sees",
        " to a task), 3 (reviews that ask before",
        " they block), or both. 4 changes how",
        " triage works for every coordinator; 3",
        " only touches the reviewer's prompt and",
        " is easy to undo.",
        "",
        "",
        "",
        "",
        "",
        " Answer: Postgres",
        " ⏎ sends · esc returns",
        "",
    ].map(String::from), "{s:#?}");
    // the question at normal weight, the cursor after what's typed
    assert!(!buf[(1, 2)].modifier.contains(Modifier::DIM));
    assert_eq!(term.get_cursor_position().unwrap(), Position::new(17, 13));

    // too short for all of it: cut with " …", the answer line kept
    let s = screen(&mut a, 42, 8);
    assert_eq!(s[2..], [
        " Pick: candidate 4 (one rule for who sees",
        " to a task), 3 (reviews that ask before …",
        "",
        " Answer: Postgres",
        " ⏎ sends · esc returns",
        "",
    ].map(String::from), "{s:#?}");

    // a tell is still the one line at the bottom of the board
    a.mode = Mode::Tell { id: "BP-31".into(), text: "hi".into(), reply: false };
    assert_eq!(row(&screen(&mut a, 42, 16), 1), " Tell BP-31: hi");
}

#[test]
fn a_peek_that_cant_read_the_screen_says_why_and_isnt_live() {
    let mut a = app(vec![ready("BP-3")], 0, Layout::Grouped);
    a.mode = Mode::Overlay { title: "BP-3  Fix it".into(), lines: vec!["BP-3's agent isn't running.".into()], peek: Some("BP-3".into()) };
    a.peek_failed = true;
    let s = screen(&mut a, 42, 12);
    assert_eq!(s[2], " BP-3's agent isn't running.");
    assert_eq!(row(&s, 0), " t tell  ⏎ open  i answer  esc returns");
    a.peek_failed = false;
    let s = screen(&mut a, 42, 12);
    assert!(row(&s, 0).starts_with(" live · "), "{s:#?}");
}
