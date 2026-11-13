use super::*;
use crate::app::testing::*;

#[test]
fn selection_and_n_skip_the_other_coordinators_tasks() {
    let mut a = App::for_test(vec![owned("BP-1", "needs", "c1"), owned("BP-2", "needs", "c2"), owned("BP-3", "needs", "c1")]);
    a.cfg.coordinators = vec!["c1".into(), "c2".into()];
    a.me = Some("c1".into());
    a.sel = Some("BP-2".into());
    a.settle();
    assert_eq!(a.sel.as_deref(), Some("BP-1")); // BP-2 isn't shown: off it
    press(&mut a, 'j');
    assert_eq!(a.sel.as_deref(), Some("BP-3"));
    press(&mut a, 'n');
    assert_eq!(a.sel.as_deref(), Some("BP-1"));
    press(&mut a, 'n');
    assert_eq!(a.sel.as_deref(), Some("BP-3"));
    // o shows them: BP-2 comes between (the setting can't be saved here; it still toggles)
    press(&mut a, 'o');
    assert!(a.cfg.shows_others(Some("c1")));
    assert_eq!(a.order().iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["BP-1", "BP-2", "BP-3"]);
    press(&mut a, 'k');
    assert_eq!(a.sel.as_deref(), Some("BP-2"));
    // and o again hides them
    press(&mut a, 'o');
    assert!(!a.cfg.shows_others(Some("c1")));
    a.settle();
    assert_eq!(a.sel.as_deref(), Some("BP-1"));
}

#[test]
fn o_with_one_coordinator_says_so() {
    let mut a = App::for_test(vec![owned("BP-1", "needs", "c1")]);
    a.cfg.coordinators = vec!["c1".into()];
    a.me = Some("c1".into());
    press(&mut a, 'o');
    assert!(a.cfg.board_others.is_empty());
    assert_eq!(a.msg, "One coordinator: every task is shown.");
}

#[test]
fn n_goes_to_what_needs_you_then_to_what_is_ready() {
    let mut a = App::for_test(vec![task("BP-1", "working"), task("BP-2", "triage"), task("BP-3", "ready"), task("BP-4", "finished"), task("BP-5", "needs")]);
    a.settle();
    assert_eq!(a.sel.as_deref(), Some("BP-5")); // the board's first: needs you
    press(&mut a, 'n');
    assert_eq!((a.sel.as_deref(), a.msg.text.as_str()), (Some("BP-5"), ""));
    // nothing needs you: ready, then finished, round again, saying why
    a.tasks.retain(|t| t.id != "BP-5");
    a.settle();
    press(&mut a, 'n');
    assert_eq!((a.sel.as_deref(), a.msg.text.as_str()), (Some("BP-3"), "Nothing needs you: BP-3 is ready."));
    press(&mut a, 'n');
    assert_eq!((a.sel.as_deref(), a.msg.text.as_str()), (Some("BP-4"), "Nothing needs you: BP-4 is finished."));
    press(&mut a, 'n');
    assert_eq!(a.sel.as_deref(), Some("BP-3"));
    // a stopped task needs you too
    a.tasks.push(task("BP-6", "stopped"));
    press(&mut a, 'n');
    assert_eq!((a.sel.as_deref(), a.msg.text.as_str()), (Some("BP-6"), ""));
    // nothing to do at all
    let mut a = App::for_test(vec![task("BP-1", "working"), task("BP-2", "triage")]);
    a.settle();
    press(&mut a, 'n');
    assert_eq!(a.msg, "Nothing needs you.");
}

#[test]
fn go_board_selects_a_task_through_the_select_file() {
    let d = Scratch::new("select");
    let mut a = App::for_test(vec![owned("BP-1", "needs", "c1"), owned("BP-2", "todo", "c1")]);
    (a.dir, a.me) = (d.0.clone(), Some("c1".into()));
    a.settle();
    a.pending = Some((Action::Accept, "BP-1".into()));
    let file = d.0.join("select").join("c1");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let ask = |id: &str| std::fs::write(&file, format!("{id}\n")).unwrap();
    ask("BP-2");
    a.take_select();
    assert_eq!(a.sel.as_deref(), Some("BP-2"));
    assert!(a.pending.is_none()); // the question was about BP-1
    assert!(!file.exists()); // read once
    ask("BP-9"); // a task it doesn't have: stays put
    a.take_select();
    assert_eq!(a.sel.as_deref(), Some("BP-2"));
    ask("BP-1"); // left over from long ago: dropped
    let old = std::time::SystemTime::now() - Duration::from_secs(60);
    std::fs::File::options().write(true).open(&file).unwrap().set_modified(old).unwrap();
    a.take_select();
    assert_eq!(a.sel.as_deref(), Some("BP-2"));
    assert!(!file.exists());
    a.cfg.coordinators = vec!["c1".into(), "c2".into()];
    a.tasks.push(owned("BP-3", "todo", "c2"));
    ask("BP-3"); // shown only with o: say so, and stay put
    a.take_select();
    assert_eq!(a.sel.as_deref(), Some("BP-2"));
    assert_eq!(a.msg, "BP-3 is c2's: o shows it.");
    a.cfg.board_others = vec!["c1".into()];
    ask("BP-3");
    a.take_select();
    assert_eq!(a.sel.as_deref(), Some("BP-3"));
    std::fs::write(d.0.join("select").join("c2"), "BP-1").unwrap(); // another coordinator's board's
    a.take_select();
    assert_eq!(a.sel.as_deref(), Some("BP-3"));
}
