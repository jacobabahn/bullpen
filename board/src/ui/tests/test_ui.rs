use super::*;
use crate::model::{Action, Layout, Task};
use crate::ui::testing::*;
use crate::{App, Mode, Note};
use std::time::Instant;

#[test]
fn the_last_row_is_left_for_rexs_capsules() {
    let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
    let s = screen(&mut a, 46, 12);
    assert_eq!(row(&s, 0), "");
    assert!(row(&s, 1).starts_with(" a accept  g tour  d drop"), "{s:#?}");
    assert!(row(&s, 1).ends_with("? keys"), "{s:#?}");

    a.msg = Note::done("Accepted BP-1");
    let s = screen(&mut a, 40, 12);
    assert_eq!(row(&s, 2), " Accepted BP-1");
    assert_eq!(row(&s, 0), "");

    a.pending = Some((Action::Drop, "BP-1".into()));
    let s = screen(&mut a, 40, 12);
    assert_eq!(row(&s, 1), " y drop   any other key cancels");
    assert_eq!(row(&s, 0), "");

    a.pending = None;
    a.mode = Mode::Tell { id: "BP-1".into(), text: "hi".into(), reply: false };
    let s = screen(&mut a, 40, 12);
    assert_eq!(row(&s, 1), " Tell BP-1: hi");
    assert_eq!(row(&s, 0), "");

    a.mode = Mode::Overlay { title: "Keys".into(), lines: vec!["one".into()], peek: None };
    let s = screen(&mut a, 40, 12);
    assert_eq!(row(&s, 1), " any key returns");
    assert_eq!(row(&s, 0), "");
}

#[test]
fn the_working_and_review_marks_turn_five_times_a_second_and_a_command_spinner_fast() {
    let working = ready("BP-1").with(json!({"status": "working"}));
    let review = ready("BP-2").with(json!({"status": "review"}));
    let mut a = app(vec![working, review, ready("BP-3")], 0, Layout::Grouped);
    let marks = |a: &mut App, ms: u64| {
        a.started = Instant::now() - std::time::Duration::from_millis(ms);
        let s = screen(a, 42, 24);
        ["BP-1", "BP-2"].map(|id| s.iter().find(|r| r.contains(id)).unwrap().chars().nth(1).unwrap())
    };
    // the same frame through the first fifth of a second, the next one after it
    assert_eq!(marks(&mut a, 0), marks(&mut a, 150));
    let (first, later) = (marks(&mut a, 0), marks(&mut a, 250));
    assert_ne!(first[0], later[0]);
    assert_ne!(first[1], later[1]);
    // a command's spinner is feedback: it moves every tenth of a second
    a.started = Instant::now();
    assert_ne!(spinner(&a), { a.started = Instant::now() - std::time::Duration::from_millis(150); spinner(&a) });
    // the board redraws often enough to show every frame of the marks
    a.tasks.retain(|t| t.status == "review");
    assert!(a.tick().as_millis() * 2 <= super::MARK_MS, "{:?}", a.tick());
}

#[test]
fn a_gap_of_none_is_the_old_layout() {
    let mut a = app(vec![ready("BP-1")], 0, Layout::Grouped);
    a.msg = Note::done("Accepted BP-1");
    let s = screen(&mut a, 46, 12);
    assert_eq!(row(&s, 1), " Accepted BP-1");
    assert!(row(&s, 0).starts_with(" a accept  g tour  d drop"), "{s:#?}");
    assert!(row(&s, 0).ends_with("? keys"), "{s:#?}");
}

#[test]
fn the_compact_panel_and_scroll_mark_move_up_too() {
    let tasks: Vec<Task> = (1..=9).map(|i| ready(&format!("BP-{i}"))).collect();
    let lines = |gap| {
        let mut a = app(tasks.clone(), gap, Layout::Compact);
        screen(&mut a, 40, 16)
    };
    let (old, new) = (lines(0), lines(1));
    // everything from the top down is the same; the bottom rows move up by one
    assert_eq!(row(&new, 0), "");
    for k in 0..8 {
        assert_eq!(row(&new, k + 1), row(&old, k), "row {k} from the bottom: {new:#?}");
    }
    assert!(row(&new, 7).starts_with(" ─"), "the panel's rule: {new:#?}");
    assert!(row(&new, 8).ends_with("↓"), "the scroll mark: {new:#?}");
    // and the list has one row fewer
    assert_eq!(&new[..7], &old[..7]);
}

#[test]
fn a_short_board_drops_the_gap_before_the_keys() {
    // grouped and short: header, blank, one row and the keys
    for h in [5u16, 4, 2] {
        let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
        let s = screen(&mut a, 40, h);
        assert!(row(&s, 0).starts_with(" a accept"), "height {h}: {s:#?}");
    }
    let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
    let s = screen(&mut a, 40, 6);
    assert_eq!(row(&s, 0), "");
    assert!(row(&s, 1).starts_with(" a accept"), "{s:#?}");
    assert!(s[2].contains("BP-1"), "the task still shows: {s:#?}");
}

#[test]
fn a_short_board_gives_the_list_its_spare_rows() {
    let tasks = vec![asks("BP-1"), asks("BP-2"), ready("BP-3")];
    let mut a = app(tasks.clone(), 1, Layout::Grouped);
    let s = screen(&mut a, 42, 10);
    // header, blank, seven list rows, the keys, the capsules' row
    assert_eq!(s[1..9], [
        "",
        " Needs you  2",
        "▌● BP-1  Task BP-1",
        "▌  Asks: python3 or python?",
        "▌● BP-2  Task BP-2",
        "▌  Asks: python3 or python?",
        "                                        ↓",
        " ⏎ open  t tell  p peek            ? keys",
    ].map(String::from), "{s:#?}");
    assert_eq!(row(&s, 0), "");
    // the scroll mark is past the ages, and the list moves down under it
    a.sel = Some("BP-3".into());
    let s = screen(&mut a, 42, 10);
    assert!(s[1].ends_with('↑') && s[3].contains("BP-3"), "{s:#?}");

    // a message takes the key row, and the list keeps its rows
    a.sel = None;
    a.msg = Note::done("Accepted BP-0.");
    let s = screen(&mut a, 42, 10);
    assert_eq!(row(&s, 1), " Accepted BP-0.");
    assert!(s[7].ends_with('↓'), "{s:#?}");
    // a y/n question gets a row of its own above the keys
    a.msg = Note::default();
    a.pending = Some((Action::Drop, "BP-1".into()));
    let s = screen(&mut a, 42, 10);
    assert_eq!(row(&s, 2), " BP-1: Drop it?");
    assert_eq!(row(&s, 1), " y drop   any other key cancels");

    // a tall board keeps its blank rows
    let mut a = app(tasks, 1, Layout::Grouped);
    let s = screen(&mut a, 42, 12);
    assert_eq!(s[8], "", "{s:#?}");
    assert!(s[7].ends_with('↓') && !s[7].contains("Ready"), "{s:#?}");
}
