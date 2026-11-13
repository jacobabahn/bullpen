use super::*;
use crate::app::testing::*;
use crate::app::Job;
use crate::model::Action;

#[test]
fn a_board_reloads_once_its_program_is_renamed_into_place_and_its_idle() {
    let d = Scratch::new("reexec");
    let exe = d.0.join("bullpen-board");
    program(&exe, "#!/bin/sh\n");
    let mut a = App::for_test(vec![task("BP-1", "ready")]);
    a.exe = Some((exe.clone(), ExeId::of(&exe).unwrap()));
    assert_eq!(a.replaced(), None); // the same file
    // a build writes a copy, then renames it over the program
    let tmp = d.0.join(".bullpen-board.1");
    program(&tmp, "#!/bin/sh\necho new\n");
    std::fs::rename(&tmp, &exe).unwrap();
    a.running.push(("BP-1".into(), Job::Accept));
    assert_eq!(a.replaced(), None); // not while a command runs
    a.running.clear();
    a.mode = Mode::Tell { id: "BP-1".into(), text: "half a".into(), reply: false };
    assert_eq!(a.replaced(), None); // nor while you type
    a.mode = Mode::Stale { id: "BP-1".into() };
    assert_eq!(a.replaced(), None); // nor while it asks about a stale tour
    a.mode = Mode::Normal;
    a.pending = Some((Action::Accept, "BP-1".into()));
    assert_eq!(a.replaced(), None); // nor while it asks y/n
    a.pending = None;
    assert_eq!(a.replaced(), Some(exe.clone()));
}

#[test]
fn a_missing_or_empty_program_isnt_reloaded() {
    let d = Scratch::new("missing");
    let exe = d.0.join("bullpen-board");
    program(&exe, "#!/bin/sh\n");
    let mut a = App::for_test(vec![]);
    a.exe = Some((exe.clone(), ExeId::of(&exe).unwrap()));
    std::fs::remove_file(&exe).unwrap();
    assert_eq!(a.replaced(), None);
    std::fs::write(&exe, "").unwrap();
    assert_eq!(a.replaced(), None);
    a.exe = None; // started from a relative path, or reloading failed
    program(&exe, "#!/bin/sh\necho new\n");
    assert_eq!(a.replaced(), None);
}

#[test]
fn a_reload_keeps_the_selection_f_and_the_coordinator() {
    let mut a = App::for_test(vec![task("BP-1", "ready"), task("BP-2", "done")]);
    a.me = Some("c2".into());
    a.sel = Some("BP-2".into());
    press(&mut a, 'f');
    let args = parse_args(a.restart_args()).unwrap();
    assert_eq!(
        args,
        Args {
            dir: a.dir.clone(),
            root: a.root.clone(),
            bullpen: a.bullpen.clone(),
            me: Some("c2".into()),
            select: Some("BP-2".into()),
            closed: true,
            reloaded: true,
        }
    );
    a.closed = false;
    a.me = None;
    a.sel = None;
    let args = parse_args(a.restart_args()).unwrap();
    assert_eq!((args.me, args.select, args.closed), (None, None, false));
}

#[test]
fn f_says_whether_closed_tasks_show_and_an_old_boards_finished_still_works() {
    let mut a = App::for_test(vec![task("BP-1", "ready"), task("BP-2", "done")]);
    press(&mut a, 'f');
    assert!(a.closed);
    assert_eq!(a.msg, "Showing closed tasks too.");
    press(&mut a, 'f');
    assert_eq!((a.closed, a.msg.text.as_str()), (false, "Hiding closed tasks."));
    // a board from before --closed restarts the new one with --finished
    let argv = ["--dir", "d", "--root", "r", "--bullpen", "b", "--finished", "on"].map(String::from);
    assert!(parse_args(argv).unwrap().closed);
}
