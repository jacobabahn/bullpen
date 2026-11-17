use super::*;
use crate::app::Job;
use crate::model::Action;
use crate::mission::testing::*;

fn two_repos(st: &State) -> (PathBuf, PathBuf) {
    let shop = st.repo("shop", json!([]), &[
        t("BP-1", "working", "Build", 30),
        t("BP-2", "needs", "Asks", 5),
        t("BP-3", "ready", "Done one", 60),
        t("BP-4", "todo", "Later", 1),
    ]);
    let web = st.repo("web", json!([]), &[t("BP-1", "stopped", "Exited", 20), t("BP-3", "ready", "Also done", 10)]);
    (shop, web)
}

#[test]
fn the_selection_moves_through_every_repos_tasks_in_the_lists_order() {
    let st = State::new("moves");
    two_repos(&st);
    let mut m = st.mission();
    assert_eq!(selected(&m), "web BP-1"); // the first: what has needed you longest
    let mut seen = vec![selected(&m)];
    for _ in 0..6 {
        press(&mut m, 'j');
        seen.push(selected(&m));
    }
    assert_eq!(seen, ["web BP-1", "shop BP-2", "shop BP-3", "web BP-3", "shop BP-1", "shop BP-4", "shop BP-4"]);
    key(&mut m, KeyCode::Home);
    assert_eq!(selected(&m), "web BP-1");
    press(&mut m, 'G');
    assert_eq!(selected(&m), "shop BP-4");
    press(&mut m, 'k');
    assert_eq!(selected(&m), "shop BP-1");
}

#[test]
fn n_goes_to_the_next_task_that_needs_you_in_any_repo_then_to_what_is_ready() {
    let st = State::new("next");
    two_repos(&st);
    let mut m = st.mission();
    press(&mut m, 'n');
    assert_eq!(selected(&m), "shop BP-2");
    press(&mut m, 'n');
    assert_eq!(selected(&m), "web BP-1"); // round again
    let st = State::new("next-ready");
    st.repo("web", json!([]), &[t("BP-1", "working", "a", 1), t("BP-2", "finished", "b", 1)]);
    let mut m = st.mission();
    press(&mut m, 'n');
    assert_eq!(selected(&m), "web BP-2");
    assert_eq!(m.msg, "Nothing needs you: BP-2 is finished.");
    let st = State::new("next-none");
    st.repo("web", json!([]), &[t("BP-1", "working", "a", 1)]);
    let mut m = st.mission();
    press(&mut m, 'n');
    assert_eq!(m.msg, "Nothing needs you.");
}

#[test]
fn a_key_on_a_task_goes_to_its_own_repos_board_though_another_repo_has_its_id() {
    let st = State::new("route");
    let (shop, web) = two_repos(&st);
    let mut m = st.mission();
    press(&mut m, 'j');
    press(&mut m, 'j');
    press(&mut m, 'j');
    assert_eq!(selected(&m), "web BP-3");
    press(&mut m, 'a');
    assert_eq!(m.boards[&web].pending, Some((Action::Accept, "BP-3".into())));
    assert!(m.boards[&shop].pending.is_none());
    // while it asks, every key goes to that board: y accepts there
    press(&mut m, 'y');
    assert_eq!(m.boards[&web].job("BP-3"), Some(Job::Accept));
    assert_eq!(m.boards[&shop].job("BP-3"), None);
    // the result comes back from that board, and is said
    let done = Msg::Done { id: "BP-3".into(), job: Job::Accept, ok: true, out: String::new() };
    m.handle(MMsg::Board(web.clone(), done));
    assert_eq!(m.msg, "Accepted BP-3.");
    assert!(m.boards[&web].running.is_empty());
}

#[test]
fn n_or_any_other_key_cancels_a_question_as_on_the_board() {
    let st = State::new("cancel");
    let (_, web) = two_repos(&st);
    let mut m = st.mission();
    press(&mut m, 'd'); // web BP-1, stopped
    assert_eq!(m.boards[&web].pending, Some((Action::Drop, "BP-1".into())));
    press(&mut m, 'j'); // not a move: it cancels
    assert!(m.boards[&web].pending.is_none());
    assert_eq!(m.msg, "Cancelled.");
    assert_eq!(selected(&m), "web BP-1");
}

#[test]
fn t_types_a_line_for_the_tasks_worker_and_esc_drops_it() {
    let st = State::new("tell");
    let (shop, _) = two_repos(&st);
    let mut m = st.mission();
    press(&mut m, 'j'); // shop BP-2
    press(&mut m, 't');
    assert!(matches!(&m.boards[&shop].mode, Mode::Tell { id, reply: false, .. } if id == "BP-2"));
    for c in "jk".chars() {
        press(&mut m, c); // typed, not moves
    }
    assert_eq!(selected(&m), "shop BP-2");
    let Mode::Tell { text, .. } = &m.boards[&shop].mode else { panic!() };
    assert_eq!(text.text(), "jk");
    key(&mut m, KeyCode::Esc);
    assert!(matches!(m.boards[&shop].mode, Mode::Normal));
}

#[test]
fn enter_and_s_and_r_act_as_the_board_does() {
    let st = State::new("enter");
    let (shop, web) = two_repos(&st);
    let mut m = st.mission();
    press(&mut m, 'G'); // shop BP-4, queued
    key(&mut m, KeyCode::Enter);
    assert_eq!(m.msg, "Not started yet: s starts it.");
    press(&mut m, 's');
    assert_eq!(m.boards[&shop].job("BP-4"), Some(Job::Start));
    key(&mut m, KeyCode::Home); // web BP-1, stopped
    press(&mut m, 'r');
    assert_eq!(m.boards[&web].job("BP-1"), Some(Job::Resume));
}

#[test]
fn a_task_that_leaves_the_list_hands_the_selection_to_the_one_in_its_place() {
    let st = State::new("settle");
    let (shop, _) = two_repos(&st);
    let mut m = st.mission();
    press(&mut m, 'j');
    press(&mut m, 'j');
    assert_eq!(selected(&m), "shop BP-3");
    let mut done = t("BP-3", "done", "Done one", 0);
    done["since"] = json!("2020-01-01T00:00:00+00:00");
    std::fs::write(shop.join("tasks/BP-3.json"), done.to_string()).unwrap();
    m.reload();
    assert_eq!(selected(&m), "web BP-3");
}

#[test]
fn q_asks_first() {
    let st = State::new("quit");
    two_repos(&st);
    let mut m = st.mission();
    press(&mut m, 'q');
    assert!(m.quitting);
    assert!(press(&mut m, 'n'));
    assert!(!m.quitting);
    assert_eq!(m.msg, "Cancelled.");
    press(&mut m, 'q');
    assert!(!press(&mut m, 'y'));
}

#[test]
fn a_command_that_cant_run_says_why() {
    let st = State::new("cantrun");
    two_repos(&st);
    let mut m = st.mission();
    press(&mut m, '/');
    assert_eq!(m.msg.kind, crate::app::Kind::Failed);
    assert!(m.msg.text.contains("can't run"), "{}", m.msg.text);
    press(&mut m, 'v');
    assert!(m.msg.text.contains("can't run"), "{}", m.msg.text);
}

#[test]
fn rex_says_which_coordinators_are_alive() {
    let st = State::new("alive");
    let dir = st.repo("shop", json!([{"id": "c1", "block": "block:a", "session": "s"}]), &[]);
    let mut m = st.mission();
    assert_eq!(m.fleet.repos[0].coordinators[0].alive, None);
    m.handle(MMsg::Alive(vec![(dir, "c1".into(), false)]));
    assert_eq!(m.fleet.repos[0].coordinators[0].word(), "gone");
    m.reload(); // kept on the next read
    assert_eq!(m.fleet.repos[0].coordinators[0].word(), "gone");
}

#[test]
fn no_repos_and_nothing_to_act_on() {
    let st = State::new("empty");
    let mut m = st.mission();
    assert!(m.current().is_none());
    key(&mut m, KeyCode::Enter);
    assert_eq!(m.msg, "Nothing open in any repository.");
}

/// Mission control on these tasks in one repo, `shop`, whose boards run a bullpen that
/// logs its arguments: the log's path, and the scratch folder that holds both.
fn logging(name: &str, tasks: &[Value]) -> (State, Mission, PathBuf) {
    let st = State::new(name);
    st.repo("shop", json!([]), tasks);
    let (exe, log) = (st.0.join("bullpen"), st.0.join("log"));
    crate::app::testing::program(&exe, &format!("#!/bin/sh\n[ \"$1\" = --ready ] && exit 0\necho \"$*\" >> {}\n", log.display()));
    let mut m = st.mission();
    for app in m.boards.values_mut() {
        app.bullpen = exe.clone();
    }
    (st, m, log)
}

/// What the logging bullpen was asked, waiting a little for one run in the background.
fn calls(log: &Path, want: usize) -> Vec<String> {
    for _ in 0..200 {
        let got: Vec<String> = std::fs::read_to_string(log).unwrap_or_default().lines().map(String::from).collect();
        if got.len() >= want {
            return got;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    std::fs::read_to_string(log).unwrap_or_default().lines().map(String::from).collect()
}

fn with_pane(id: &str, status: &str, title: &str) -> Value {
    let mut v = t(id, status, title, 3);
    v["block"] = json!(format!("blk-{id}"));
    v["session"] = json!("session:shop");
    v
}

#[test]
fn p_peeks_at_the_workers_screen_over_mission_control_and_esc_closes_it() {
    let (_st, mut m, log) = logging("peek", &[with_pane("BP-1", "working", "Build")]);
    press(&mut m, 'p');
    let app = m.active().unwrap();
    assert!(matches!(&app.mode, Mode::Overlay { peek: Some(id), .. } if id == "BP-1"));
    let s = screen(&mut m, 100, 20);
    assert_eq!(s[0], " BP-1  Build");
    assert!(s[19].contains("esc returns"), "{}", s[19]);
    m.peek(); // what the loop does before each draw: read the screen
    let got = calls(&log, 1);
    assert!(got[0].starts_with("peek BP-1 --lines ") && got[0].ends_with(" --vt"), "{got:?}");
    // keys stay in the peek: j isn't a move
    press(&mut m, 'j');
    assert!(matches!(m.active().unwrap().mode, Mode::Overlay { .. }));
    key(&mut m, KeyCode::Esc);
    assert!(matches!(m.active().unwrap().mode, Mode::Normal));
    assert!(screen(&mut m, 100, 20)[0].starts_with(" MISSION CONTROL"));
}

#[test]
fn p_on_a_task_in_review_peeks_at_its_reviewer() {
    let (_st, mut m, log) = logging("peek-review", &[with_pane("BP-1", "review", "Build")]);
    press(&mut m, 'p');
    assert_eq!(screen(&mut m, 100, 20)[0], " BP-1  Build · its reviewer");
    m.peek();
    let got = calls(&log, 1);
    assert!(got[0].starts_with("peek BP-1 --lines ") && got[0].ends_with(" --vt --review"), "{got:?}");
    press(&mut m, 'q');
    assert!(matches!(m.active().unwrap().mode, Mode::Normal));
    assert!(!m.quitting, "q left the peek; it didn't ask to quit");
}

#[test]
fn enter_goes_to_the_tasks_pane_and_has_rex_show_its_session() {
    let (_st, mut m, log) =
        logging("enter-go", &[with_pane("BP-1", "working", "Build"), with_pane("BP-2", "review", "Check")]);
    assert_eq!(selected(&m), "shop BP-2"); // in review, listed first
    key(&mut m, KeyCode::Enter);
    assert_eq!(m.msg, "Opened BP-2's reviewer.");
    press(&mut m, 'j');
    key(&mut m, KeyCode::Enter);
    assert_eq!(m.msg, "Opened BP-1.");
    // from a peek, ⏎ does the same
    press(&mut m, 'p');
    key(&mut m, KeyCode::Enter);
    assert!(matches!(m.active().unwrap().mode, Mode::Normal));
    let got: Vec<String> = calls(&log, 4).into_iter().filter(|c| c.starts_with("go ")).collect();
    assert_eq!(got, ["go BP-2 --review --switch", "go BP-1 --switch", "go BP-1 --switch"]);
}

#[test]
fn enter_on_a_task_with_no_pane_says_so_and_runs_nothing() {
    let (_st, mut m, log) = logging("enter-none", &[t("BP-1", "todo", "Later", 1), t("BP-2", "finished", "Done", 1)]);
    // the finished one is listed first (ready), the queued one last
    assert_eq!(selected(&m), "shop BP-2");
    key(&mut m, KeyCode::Enter);
    assert_eq!(m.msg, "BP-2 has no pane open.");
    press(&mut m, 'G');
    assert_eq!(selected(&m), "shop BP-1");
    key(&mut m, KeyCode::Enter);
    assert_eq!(m.msg, "Not started yet: s starts it.");
    assert!(!log.exists(), "{:?}", std::fs::read_to_string(&log));
}
