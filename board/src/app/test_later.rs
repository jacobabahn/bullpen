use super::*;
use crate::app::testing::*;
use crate::app::Kind;
use crate::model::{Item, Task};

fn item(id: &str, state: &str, brief: bool) -> Item {
    Item { id: id.into(), title: format!("Idea {id}"), state: state.into(), by: "BP-9".into(), brief, ..Default::default() }
}

/// A board on these tasks and items whose bullpen logs its arguments and says `out`.
fn board(d: &Scratch, tasks: Vec<Task>, out: &str) -> App {
    let exe = d.0.join("bullpen");
    let log = d.0.join("log");
    program(&exe, &format!("#!/bin/sh\n[ \"$1\" = --ready ] && exit 0\necho \"$*\" >> {}\necho '{out}'\n", log.display()));
    let mut a = App::for_test(tasks);
    (a.bullpen, a.root) = (exe, d.0.clone());
    a.later = vec![item("L1", "kept", true), item("L2", "asks", false), item("L3", "kept", false)];
    a.settle();
    a
}

fn calls(d: &Scratch) -> Vec<String> {
    std::fs::read_to_string(d.0.join("log")).unwrap_or_default().lines().map(String::from).collect()
}

#[test]
fn shift_l_expands_the_later_group_and_folds_it_back() {
    let mut a = App::for_test(vec![task("BP-1", "todo"), task("BP-2", "todo")]);
    a.settle();
    press(&mut a, 'L');
    assert!(!a.later_open);
    assert_eq!(a.msg, "The backlog is empty: `bullpen later \"...\"` adds an idea to it.");
    a.later = vec![item("L1", "kept", true), item("L2", "asks", false)];
    press(&mut a, 'L');
    assert!(a.later_open);
    assert_eq!(a.sel.as_deref(), Some("L1"));
    assert_eq!(a.current_item().map(|i| i.id.as_str()), Some("L1"));
    // no task is selected while an item is: the task keys do nothing to BP-1
    assert!(a.current().is_none());
    press(&mut a, 'd');
    assert!(a.pending.is_none());
    // j and k go on from the tasks into the items and back
    press(&mut a, 'j');
    assert_eq!(a.sel.as_deref(), Some("L2"));
    press(&mut a, 'k');
    press(&mut a, 'k');
    assert_eq!(a.sel.as_deref(), Some("BP-2"));
    press(&mut a, 'j');
    // folded, the selection goes back to the last task
    press(&mut a, 'L');
    assert!(!a.later_open);
    assert_eq!(a.sel.as_deref(), Some("BP-2"));
    // a selection left on an item's ID while folded isn't an item
    a.sel = Some("L1".into());
    assert!(a.current_item().is_none());
    a.settle();
    assert_eq!(a.sel.as_deref(), Some("BP-1"));
}

#[test]
fn y_keeps_n_drops_and_j_k_move_the_item_with_bullpen_later() {
    let d = Scratch::new("later-keys");
    let mut a = board(&d, vec![task("BP-1", "todo")], "L2 is on the list: Idea L2");
    press(&mut a, 'L');
    press(&mut a, 'j');
    assert_eq!(a.sel.as_deref(), Some("L2"));
    press(&mut a, 'y');
    assert_eq!(a.msg, Note::done("L2 is on the list: Idea L2."));
    press(&mut a, 'J');
    press(&mut a, 'K');
    // n drops it, and the selection goes on to the next item
    press(&mut a, 'n');
    assert_eq!(a.sel.as_deref(), Some("L3"));
    assert_eq!(calls(&d), ["later L2 --keep", "later L2 --down", "later L2 --up", "later L2 --drop"]);
    // the last item dropped: the one before it
    press(&mut a, 'n');
    assert_eq!(a.sel.as_deref(), Some("L2"));
}

#[test]
fn a_failed_change_says_why() {
    let d = Scratch::new("later-fails");
    let exe = d.0.join("bullpen");
    program(&exe, "#!/bin/sh\necho 'bullpen: L1 is a task already (BP-4): see to it there' >&2\nexit 1\n");
    let mut a = App::for_test(vec![]);
    (a.bullpen, a.root) = (exe, d.0.clone());
    a.later = vec![item("L1", "kept", true)];
    press(&mut a, 'L');
    press(&mut a, 'n');
    assert_eq!(a.msg, Note::failed("L1 is a task already (BP-4): see to it there"));
    assert_eq!(a.sel.as_deref(), Some("L1"), "it stays on the item");
}

#[test]
fn s_starts_an_item_with_a_brief_in_the_background() {
    let mut a = App::for_test(vec![task("BP-1", "todo")]);
    a.later = vec![item("L1", "kept", true)];
    press(&mut a, 'L');
    press(&mut a, 's');
    assert_eq!(a.job("L1"), Some(Job::StartItem));
    assert_eq!(Job::StartItem.args("L1"), ["later", "L1", "--start"]);
    // it worked: the new task is selected, and the board says what it is
    a.done("L1".into(), Job::StartItem, true, "L1 is BP-7 now, started in /w/BP-7 (branch b, pane 3)".into());
    assert_eq!(a.sel.as_deref(), Some("BP-7"));
    assert_eq!(a.msg, Note::done("Started L1: it's BP-7 now."));
    assert!(a.running.is_empty());
    // a start that failed says why
    a.done("L1".into(), Job::StartItem, false, "L1 is BP-7 now, but it didn't start: no session".into());
    assert_eq!(a.msg.kind, Kind::Failed);
    assert_eq!(a.msg, "L1 is BP-7 now, but it didn't start: no session");
}

#[test]
fn s_on_an_item_with_no_brief_asks_the_coordinator_for_one() {
    let d = Scratch::new("later-brief");
    let mut a = board(&d, vec![], "ok");
    a.me = Some("c2".into());
    press(&mut a, 'L');
    press(&mut a, 'j');
    press(&mut a, 's');
    assert!(a.running.is_empty());
    let said = calls(&d);
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(said[0].starts_with("say --coordinator c2 Please write a brief for L2 from the backlog (\"Idea L2\")"), "{said:?}");
    assert!(said[0].ends_with("`bullpen later L2 --start --brief-file FILE`."), "{said:?}");
    assert_eq!(said[1], "go");
    assert_eq!(a.msg, Note::done("Asked the coordinator to write L2's brief and start it."));
}

#[test]
fn enter_shows_the_item_in_full() {
    let d = Scratch::new("later-full");
    let mut a = board(&d, vec![], "L1  Idea L1");
    press(&mut a, 'L');
    key(&mut a, KeyCode::Enter);
    let Mode::Overlay { title, lines, peek } = &a.mode else { panic!("no overlay") };
    assert_eq!((title.as_str(), lines.clone(), peek.is_none()), ("L1  Idea L1", vec!["L1  Idea L1".to_string()], true));
    assert_eq!(calls(&d), ["later L1"]);
}
