use crate::model::{Item, Layout};
use crate::ui::testing::*;
use crate::ui::AMBER;

fn item(id: &str, state: &str, brief: bool) -> Item {
    Item { id: id.into(), title: format!("Idea {id}"), state: state.into(), by: "BP-9".into(), brief, ..Default::default() }
}

fn queued(id: &str) -> crate::model::Task {
    task(id, "todo").with(json!({"title": format!("Task {id}")}))
}

#[test]
fn the_later_group_is_one_line_under_queued() {
    let mut a = app(vec![queued("BP-1"), task("BP-2", "done")], 0, Layout::Grouped);
    a.later = vec![item("L1", "kept", true), item("L2", "kept", false)];
    a.closed = true;
    a.settle();
    let s = screen(&mut a, 50, 20);
    let at = |text: &str| s.iter().position(|r| r.contains(text)).unwrap_or_else(|| panic!("{text}: {s:#?}"));
    assert_eq!(s[at("Later ·")], " Later · 2 · next: Idea L1");
    // after the open groups, before Closed, with a blank row on each side
    assert!(at("Queued") < at("Later ·") && at("Later ·") < at("Closed"), "{s:#?}");
    assert_eq!(s[at("Later ·") - 1], "");
    assert_eq!(tone_of(&mut a, 50, 20, "Later ·"), Some(ratatui::style::Color::Reset));
    // something there asks for a yes: it says so, in amber
    a.later.push(item("L5", "asks", false));
    let s = screen(&mut a, 50, 20);
    assert!(s.contains(&" Later · 3 · 1 to decide (L5)".to_string()), "{s:#?}");
    assert_eq!(tone_of(&mut a, 50, 20, "Later ·"), Some(AMBER));
    // an empty backlog has no line
    a.later.clear();
    let s = screen(&mut a, 50, 20);
    assert!(!s.iter().any(|r| r.contains("Later")), "{s:#?}");
}

#[test]
fn expanded_each_item_has_a_row_and_the_selected_one_its_keys() {
    let mut a = app(vec![queued("BP-10")], 0, Layout::Grouped);
    a.later = vec![item("L1", "kept", true), item("L2", "asks", false)];
    a.later_open = true;
    a.sel = Some("L2".into());
    let s = screen(&mut a, 60, 20);
    let at = |text: &str| s.iter().position(|r| r.contains(text)).unwrap_or_else(|| panic!("{text}: {s:#?}"));
    assert_eq!(s[at("Later  2")], " Later  2");
    // IDs share the tasks' column
    assert_eq!(s[at("Idea L1")], " ◇ L1     Idea L1");
    assert!(s[at("Idea L2")].starts_with(" ? L2     Idea L2"), "{s:#?}");
    assert_eq!(s[at("Idea L2") + 1], "   Asks for a yes (from BP-9): y keeps it, n drops it. No");
    assert_eq!(s[at("Idea L2") + 2], "   brief: s asks the coordinator for one.");
    assert_eq!(tone_of(&mut a, 60, 20, "? L2"), Some(AMBER));
    assert_eq!(row(&s, 0), " y keep  n drop  s start  J K move                   ? keys");
    // the compact layout has the rows too
    a.cfg.layout = Layout::Compact;
    let s = screen(&mut a, 60, 20);
    assert!(s.iter().any(|r| r.starts_with(" ? L2     Idea L2")), "{s:#?}");
}

#[test]
fn a_task_from_the_backlog_has_a_dim_arrow_after_its_mark() {
    let from = queued("BP-3").with(json!({"from": "L7", "origin": "backlog", "started_by": "c1"}));
    let mut a = app(vec![from, queued("BP-4")], 0, Layout::Grouped);
    a.sel = Some("BP-4".into());
    let s = screen(&mut a, 60, 20);
    assert!(s.iter().any(|r| r.starts_with(" ◌↻BP-3  Task BP-3")), "{s:#?}");
    assert!(s.iter().any(|r| r.starts_with(" ◌ BP-4  Task BP-4")), "{s:#?}");
    assert!(s.iter().any(|r| r.contains("From the backlog (L7).")), "{s:#?}");
}

#[test]
fn a_board_with_no_tasks_still_shows_the_later_line() {
    let mut a = app(vec![], 0, Layout::Grouped);
    a.later = vec![item("L1", "kept", false)];
    let s = screen(&mut a, 50, 20);
    assert_eq!(s[2..6], [" No tasks yet.", " Tell the coordinator what you want done.", "", " Later · 1 · next: Idea L1"].map(String::from), "{s:#?}");
}
