use crate::model::{Action, Layout, Task};
use crate::ui::draw;
use crate::ui::testing::*;
use crate::{Mode, Note};
use ratatui::backend::TestBackend;
use ratatui::layout::Position;
use ratatui::style::Color;
use ratatui::Terminal;

#[test]
fn a_risky_confirm_names_the_risk_in_red() {
    let t = |id: &str, n: u32, verdict: &str, checks: &str| {
        ready(id).with(json!({"pr": format!("https://x/pull/{n}"), "review": {"round": 1, "verdict": verdict}, "gh": {"checks": checks}}))
    };
    let safe = t("BP-7", 41, "approved", "passing");
    let risky = t("BP-8", 42, "changes", "failing");
    let working = ready("BP-9").with(json!({"pr": "https://x/pull/43", "status": "working", "block": "w9"}));
    let mut a = app(vec![safe, risky, working], 1, Layout::Grouped);

    a.pending = Some((Action::Accept, "BP-8".into()));
    let s = screen(&mut a, 42, 24);
    // the task keeps its title, y / n for its age, and the whole question is above the keys
    assert!(s.iter().any(|r| r == " ✔ BP-8  Task BP-8                 y / n"), "{s:#?}");
    assert_eq!(
        s[20..23],
        [
            " BP-8: Checks failing, review asks for",
            " changes. Accept and rebase-merge anyway?",
            " y accept anyway   any other key cancels",
        ]
        .map(String::from),
        "{s:#?}"
    );
    assert_eq!(tone_of(&mut a, 42, 24, "BP-8: Checks failing"), Some(Color::Red));
    assert_eq!(tone_of(&mut a, 42, 24, "Task BP-8"), Some(Color::Red));
    assert_eq!(tone_of(&mut a, 42, 24, "y accept anyway"), Some(Color::Red));
    // with fewer rows, the shorter words that fit
    let s = screen(&mut a, 42, 11);
    assert_eq!(row(&s, 2), " BP-8: Checks failing. Accept anyway?", "{s:#?}");
    assert!(s.iter().any(|r| r.contains("BP-8  Task BP-8")), "{s:#?}");

    // a safe accept stays green, its key row plain
    a.pending = Some((Action::Accept, "BP-7".into()));
    let s = screen(&mut a, 42, 24);
    assert_eq!(row(&s, 2), " BP-7: Accept and rebase-merge?", "{s:#?}");
    assert_eq!(row(&s, 1), " y accept   any other key cancels");
    assert_eq!(tone_of(&mut a, 42, 24, "BP-7: Accept"), Some(Color::Green));
    assert_eq!(tone_of(&mut a, 42, 24, "y accept"), Some(Color::Reset));

    // a drop says whose agent it stops, and the key row is red too
    a.pending = Some((Action::Drop, "BP-9".into()));
    let s = screen(&mut a, 42, 24);
    assert_eq!(s[20..22], [" BP-9: Its worker is still running. Drop", " it and close PR #43?"].map(String::from), "{s:#?}");
    assert_eq!(row(&s, 1), " y drop and close PR #43   n cancels");
    assert_eq!(tone_of(&mut a, 42, 24, "y drop"), Some(Color::Red));
    assert!(s.iter().any(|r| r.contains("BP-9  Task BP-9") && r.ends_with("y / n")), "{s:#?}");
}

#[test]
fn a_message_wraps_to_three_rows_in_its_colour_above_the_keys() {
    let ts: Vec<Task> = (1..=4).map(|i| ready(&format!("BP-{i}"))).collect();
    let mut a = app(ts, 1, Layout::Grouped);
    let quiet = screen(&mut a, 42, 16);
    a.msg = Note::failed(
        "BP-1 can't be accepted: its branch has conflicts with main in board/src/ui.rs. Rebase it, then accept it again.",
    );
    let mut term = Terminal::new(TestBackend::new(42, 16)).unwrap();
    term.draw(|f| draw(f, &mut a)).unwrap();
    let buf = term.backend().buffer().clone();
    let s: Vec<String> =
        (0..16).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
    // three rows, the last cut, then the keys and the capsules' row
    assert_eq!(
        s[11..14],
        [
            " BP-1 can't be accepted: its branch has",
            " conflicts with main in board/src/ui.rs.",
            " Rebase it, then accept it again.",
        ]
        .map(String::from),
        "{s:#?}"
    );
    assert!(row(&s, 1).starts_with(" a accept"), "{s:#?}");
    assert_eq!(row(&s, 0), "");
    assert_eq!(buf[(1, 11)].fg, Color::Red);
    assert_eq!(buf[(1, 13)].fg, Color::Red);
    // the list gives up two rows for it, and says there's more below
    assert_eq!(s[10], "", "{s:#?}");
    assert!(s[9].ends_with('↓'), "{s:#?}");
    assert_eq!(s[2..9], quiet[2..9], "{s:#?}");

    // a longer one is cut at three rows
    a.msg = Note::failed("word ".repeat(40));
    let s = screen(&mut a, 42, 16);
    assert!(s[13].ends_with('…') && s[10].is_empty(), "{s:#?}");

    // what worked is green, and one row
    a.msg = Note::done("Accepted BP-1.");
    term.draw(|f| draw(f, &mut a)).unwrap();
    let buf = term.backend().buffer().clone();
    assert_eq!(buf[(1, 13)].symbol(), "A");
    assert_eq!(buf[(1, 13)].fg, Color::Green);

    // in the compact layout the detail panel moves up over the list instead
    a.cfg.layout = Layout::Compact;
    a.msg = Note::failed("word ".repeat(12));
    let s = screen(&mut a, 42, 16);
    assert!(s[13].starts_with(" word") && s[12].starts_with(" word") && s[14].starts_with(" a accept"), "{s:#?}");
    assert!(s[7].starts_with(" ───"), "{s:#?}");
    assert!(s[8].starts_with(" BP-1  Task BP-1"), "{s:#?}");
}

#[test]
fn a_long_tell_shows_the_part_around_the_cursor_at_42_columns() {
    let mut a = app(vec![asking("BP-31")], 0, Layout::Grouped);
    let mut text = crate::edit::LineEdit::from("move the session table to postgres and keep the old one");
    use ratatui::crossterm::event::{KeyCode, KeyEvent};
    let k = |t: &mut crate::edit::LineEdit, code| t.key(KeyEvent::from(code));
    a.mode = Mode::Tell { id: "BP-31".into(), text: text.clone(), reply: false };
    let mut term = Terminal::new(TestBackend::new(42, 16)).unwrap();
    term.draw(|f| draw(f, &mut a)).unwrap();
    let last = |term: &Terminal<TestBackend>| (0..42).map(|x| term.backend().buffer()[(x, 15)].symbol()).collect::<String>();
    // the end, with the cursor after it, inside the line
    assert_eq!(last(&term).trim_end(), " Tell BP-31: tgres and keep the old one");
    assert_eq!(term.get_cursor_position().unwrap(), Position::new(39, 15));
    // home: the start, the cursor on its first letter
    k(&mut text, KeyCode::Home);
    a.mode = Mode::Tell { id: "BP-31".into(), text: text.clone(), reply: false };
    term.draw(|f| draw(f, &mut a)).unwrap();
    assert_eq!(last(&term), " Tell BP-31: move the session table to p  ");
    assert_eq!(term.get_cursor_position().unwrap(), Position::new(13, 15));
    // the answer line scrolls the same way
    k(&mut text, KeyCode::End);
    a.mode = Mode::Tell { id: "BP-31".into(), text, reply: true };
    term.draw(|f| draw(f, &mut a)).unwrap();
    let row = (0..42).map(|x| term.backend().buffer()[(x, 14)].symbol()).collect::<String>();
    assert_eq!(row.trim_end(), " Answer:  postgres and keep the old one");
    assert_eq!(term.get_cursor_position().unwrap(), Position::new(39, 14));
}

#[test]
fn a_stale_tour_asks_on_the_last_row() {
    let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
    a.mode = Mode::Stale { id: "BP-1".into() };
    a.msg = Note::info("BP-1's tour is stale: the code changed.");
    let s = screen(&mut a, 70, 10);
    assert_eq!(row(&s, 2), " BP-1's tour is stale: the code changed.");
    assert_eq!(row(&s, 1), " n new tour   o the old one   esc cancels");
}

#[test]
fn the_key_line_never_drops_its_question_mark() {
    let keys = |t: &Task, w: u16| {
        let mut a = app(vec![t.clone()], 0, Layout::Grouped);
        a.sel = Some("BP-1".into());
        row(&screen(&mut a, w, 12), 0).to_string()
    };
    let t = ready("BP-1").with(json!({"worktree": "/w"}));
    assert_eq!(keys(&t, 60), format!(" a accept  g tour  d drop  ⏎ open  v diff{}? keys", " ".repeat(12)));
    // no room for "? keys": a bare ? at the right edge
    assert_eq!(keys(&t, 46), " a accept  g tour  d drop  ⏎ open  v diff   ?");
    // less room: the task's last keys go, never the ?
    assert_eq!(keys(&t, 42), " a accept  g tour  d drop  ⏎ open       ?");
    assert_eq!(keys(&t, 20), " a accept         ?");
}
