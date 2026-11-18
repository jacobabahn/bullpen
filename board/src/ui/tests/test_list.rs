use crate::model::{self, Layout, Task};
use crate::ui::testing::*;
use crate::ui::{draw, AMBER, TINT};
use crate::App;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

#[test]
fn page_down_and_up_move_a_screenful_at_42_columns() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let tasks: Vec<Task> = (1..=30).map(|n| ready(&format!("BP-{n}"))).collect();
    let mut a = app(tasks, 0, Layout::Compact);
    let s = screen(&mut a, 42, 24);
    let shown = a.page;
    assert!(shown > 3 && shown < 30, "a page: {shown}");
    assert!(s.iter().any(|r| r.contains(&format!("BP-{shown} "))), "{s:#?}");
    let key = |a: &mut App, code: KeyCode, m: KeyModifiers| {
        a.key(KeyEvent::new(code, m));
        screen(a, 42, 24)
    };
    // a page down selects the task a page on, and the list scrolls to it
    let s = key(&mut a, KeyCode::PageDown, KeyModifiers::NONE);
    let sel = format!("BP-{}", 1 + shown);
    assert_eq!(a.sel.as_deref(), Some(sel.as_str()));
    assert!(!s.iter().any(|r| r.contains("BP-1 ")), "scrolled: {s:#?}");
    assert!(s.iter().any(|r| r.contains(&format!("{sel} "))));
    // half a page (of what shows now: the heading's row has scrolled off) with
    // ctrl+u, and ctrl+d doesn't drop
    let half = a.page / 2;
    key(&mut a, KeyCode::Char('u'), KeyModifiers::CONTROL);
    assert_eq!(a.sel, Some(format!("BP-{}", 1 + shown - half)));
    key(&mut a, KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert_eq!(a.sel.as_deref(), Some(sel.as_str()));
    assert!(a.pending.is_none());
    // G and End go to the last, Home to the first, and the page keys stop at the ends
    let s = key(&mut a, KeyCode::Char('G'), KeyModifiers::SHIFT);
    assert_eq!(a.sel.as_deref(), Some("BP-30"));
    assert!(s.iter().any(|r| r.contains("BP-30 ")), "{s:#?}");
    key(&mut a, KeyCode::PageDown, KeyModifiers::NONE);
    assert_eq!(a.sel.as_deref(), Some("BP-30"));
    let s = key(&mut a, KeyCode::Home, KeyModifiers::NONE);
    assert_eq!(a.sel.as_deref(), Some("BP-1"));
    assert!(s.iter().any(|r| r.contains("BP-1 ")));
    key(&mut a, KeyCode::PageUp, KeyModifiers::NONE);
    assert_eq!(a.sel.as_deref(), Some("BP-1"));
    key(&mut a, KeyCode::End, KeyModifiers::NONE);
    assert_eq!(a.sel.as_deref(), Some("BP-30"));
}

#[test]
fn a_page_in_the_grouped_layout_counts_the_tasks_with_their_details() {
    let tasks: Vec<Task> = (1..=30).map(|n| ready(&format!("BP-{n}"))).collect();
    let mut a = app(tasks, 0, Layout::Grouped);
    let s = screen(&mut a, 42, 24);
    let titles = s.iter().filter(|r| r.contains("Task BP-")).count();
    assert_eq!(a.page, titles, "{s:#?}");
}

#[test]
fn the_header_says_what_is_hidden_and_o_labels_them() {
    let c = |id: &str, who: &str| ready(id).with(json!({"coordinator": who}));
    let mut a = app(vec![c("BP-1", "c1"), c("BP-2", "c2"), c("BP-3", "c2")], 1, Layout::Grouped);
    a.cfg.coordinators = vec!["c1".into(), "c2".into()];
    a.me = Some("c1".into());
    let s = screen(&mut a, 60, 14);
    assert!(s[0].contains("repo · c1 · +2 in c2 · o shows"), "{s:#?}");
    assert!(s[0].ends_with("1 ready"), "{s:#?}"); // counts what's shown
    assert!(!s.iter().any(|r| r.contains("BP-2")), "{s:#?}");
    a.cfg.board_others = vec!["c1".into()];
    let s = screen(&mut a, 60, 14);
    assert!(!s[0].contains("o shows") && s[0].ends_with("3 ready"), "{s:#?}");
    assert!(s.iter().any(|r| r.contains("BP-2  c2 · Task BP-2")), "{s:#?}");
    assert!(s.iter().any(|r| r.contains("BP-1  Task BP-1")), "{s:#?}");
}

#[test]
fn the_header_counts_the_other_repos_in_the_room_left() {
    let mut a = app(vec![ready("BP-1")], 1, Layout::Grouped);
    a.others.counts = vec![model::RepoCount { name: "shop".into(), needs: 1, ready: 2 }];
    let s = screen(&mut a, 60, 10);
    assert_eq!(s[0], " repo · shop: 1 needs you, 2 ready                 1 ready", "{s:#?}");
    // at the board's usual 42 columns this repo's count shrinks to its mark first
    let s = screen(&mut a, 42, 10);
    assert_eq!(s[0], " repo · shop: 1 needs you, 2 ready    ✔1", "{s:#?}");
    let s = screen(&mut a, 30, 10);
    assert_eq!(s[0], " repo · shop ●1 ✔2   1 ready", "{s:#?}");
    let s = screen(&mut a, 24, 10); // cut, never under 8 columns
    assert_eq!(s[0], " repo · shop ●1 ✔2  ✔1", "{s:#?}");
    let s = screen(&mut a, 23, 10);
    assert_eq!(s[0], " repo · shop ●1…   ✔1", "{s:#?}");
    let s = screen(&mut a, 20, 10); // the other repo's marks stay whole before this one's count
    assert_eq!(s[0], " repo · shop ●1 ✔2", "{s:#?}");
    let s = screen(&mut a, 17, 10); // no room for them: this repo's count comes back
    assert_eq!(s[0], " repo   1 ready", "{s:#?}");
}

#[test]
fn a_crowded_top_line_drops_o_shows_before_the_counts() {
    // c1's board at 42 columns, with c3's tasks hidden and another repo ready
    let c = |id: &str, who: &str, status: &str| ready(id).with(json!({"coordinator": who, "status": status}));
    let tasks = vec![c("BP-1", "c1", "needs"), c("BP-2", "c1", "ready"), c("BP-3", "c3", "working"), c("BP-4", "c3", "ready")];
    let mut a = app(tasks, 1, Layout::Grouped);
    a.repo_name = "bullpen".into();
    a.cfg.coordinators = vec!["c1".into(), "c3".into()];
    a.me = Some("c1".into());
    let s = screen(&mut a, 60, 10); // room for it all
    assert_eq!(s[0], " bullpen · c1 · +2 in c3 · o shows   1 needs you · 1 ready", "{s:#?}");
    let s = screen(&mut a, 42, 10);
    assert_eq!(s[0], " bullpen · c1 · +2 in c3           ●1 ✔1", "{s:#?}");
    a.others.counts = vec![model::RepoCount { name: "jj-evolved".into(), needs: 0, ready: 1 }];
    let s = screen(&mut a, 42, 10);
    assert_eq!(s[0], " bullpen · c1 · +2 in c3 · jj-evolved ✔1", "{s:#?}");
    let s = screen(&mut a, 50, 10);
    assert_eq!(s[0], " bullpen · c1 · +2 in c3 · jj-evolved ✔1   ●1 ✔1", "{s:#?}");
}

fn busy(id: &str, status: &str, text: &str) -> Task {
    let at = model::now().timestamp() - 5;
    let doing = json!({"state": "working", "text": text, "at": at}).as_object().cloned();
    let mut t = ready(id).with(json!({"since": model::now().to_rfc3339(), "review": {"round": 1}, "status": status}));
    t.doing = doing;
    t
}

#[test]
fn a_working_task_shows_what_its_agent_is_doing() {
    let tasks = vec![
        busy("BP-1", "working", "Running the board's tests with cargo test in board/"),
        busy("BP-2", "review", "Reading the diff"),
        { let mut t = busy("BP-3", "working", ""); t.doing = None; t },
    ];
    let mut a = app(tasks.clone(), 0, Layout::Grouped);
    let s = screen(&mut a, 44, 16);
    assert_eq!(s[2..12], [
        " In review  1",
        " ◜ BP-2  Task BP-2                     now",
        "   Reading the diff",
        "",
        " Working  2",
        " ⠋ BP-1  Task BP-1                     now",
        "   Running the board's tests with cargo …",
        " ⠋ BP-3  Task BP-3                     now",
        "",
        "",
    ].map(String::from), "{s:#?}");

    // compact: in the detail at the bottom, above the rest
    let mut a = app(tasks, 0, Layout::Compact);
    a.sel = Some("BP-1".into());
    let s = screen(&mut a, 44, 16);
    assert_eq!(s[10..13], [
        " BP-1  Task BP-1",
        " Running the board's tests with cargo test…",
        "",
    ].map(String::from), "{s:#?}");
    a.sel = Some("BP-2".into());
    let s = screen(&mut a, 44, 16);
    assert_eq!(s[11..13], [" Reading the diff", " Reviewer on round 1 of 2."].map(String::from), "{s:#?}");
    a.sel = Some("BP-3".into());
    assert!(!screen(&mut a, 44, 16).iter().any(|r| r.contains("Running")));
}

#[test]
fn a_ready_task_says_whether_its_toured_and_a_tour_being_written_spins() {
    let run = |id: &str, state: &str, commit: &str, opened: Option<&str>| {
        ready(id).with(json!({"tour": {"state": state, "commit": commit, "opened": opened}}))
    };
    let tasks = vec![
        ready("BP-1").with(json!({"diffstat": "3 files, +12 −1"})),
        run("BP-2", "written", "abc", Some("abc")),
        run("BP-3", "written", "def", Some("abc")),
        run("BP-4", "writing", "", None),
        run("BP-5", "failed", "", None).with(json!({"note": "tour failed: its tour doesn't parse"})),
    ];
    let stale = |opened: Option<&str>| ready("BP-6").with(json!({"tour": {"state": "written", "commit": "abc", "opened": opened, "stale": true}}));
    // a tour of code the worker has changed since is no tour of this code
    let old = stale(None);
    assert_eq!(model::stat_line(&old), "Not toured yet");
    // even once its old tour is opened anyway
    let opened = stale(Some("abc"));
    assert_eq!(model::stat_line(&opened), "Not toured yet");
    let mut a = app(tasks, 0, Layout::Grouped);
    let s = screen(&mut a, 60, 24);
    // a spinner turns in place of BP-4's mark
    assert!(!s[12].starts_with(" ✔") && s[12].ends_with("BP-4  Task BP-4"), "{s:#?}");
    let s: Vec<String> = s.into_iter().enumerate().map(|(i, r)| if i == 12 { r[r.find("BP-4").unwrap()..].into() } else { r }).collect();
    assert_eq!(s[2..18], [
        " Ready to accept  5",
        " ✔ BP-1  Task BP-1",
        "   Done.",
        "   3 files, +12 −1 · Not toured yet",
        " ✔ BP-2  Task BP-2",
        "   Done.",
        "   Toured",
        " ✔ BP-3  Task BP-3",
        "   Done.",
        "   Tour written, not opened yet",
        "BP-4  Task BP-4",
        "   Done.",
        "   Writing its tour…",
        " ✔ BP-5  Task BP-5",
        "   tour failed: its tour doesn't parse",
        "",
    ].map(String::from), "{s:#?}");
    assert!(row(&s, 0).starts_with(" a accept  g tour"), "{s:#?}");
}

#[test]
fn the_selected_task_says_all_it_has_to_say_from_under_its_id() {
    let stopped = ready("BP-2").with(json!({"status": "stopped"}));
    let mut a = app(vec![asking("BP-1"), stopped, asking("BP-3"), ready("BP-4")], 0, Layout::Grouped);
    a.sel = Some("BP-1".into());
    let mut term = Terminal::new(TestBackend::new(42, 24)).unwrap();
    term.draw(|f| draw(f, &mut a)).unwrap();
    let buf = term.backend().buffer().clone();
    let s: Vec<String> = (0..24).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
    // all of it under the ID for the selected task (up to eight lines), two for the others
    assert_eq!(s[3..17], [
        "▌● BP-1  Architecture round 2: review",
        "▌  Pick: candidate 4 (one rule for who",
        "▌  sees to a task), 3 (reviews that ask",
        "▌  before they block), or both. 4",
        "▌  changes how triage works for every",
        "▌  coordinator; 3 only touches the",
        "▌  reviewer's prompt and is easy to",
        "▌  undo.",
        "▌■ BP-2  Task BP-2",
        "▌  Its agent exited. r resumes it.",
        "▌● BP-3  Architecture round 2: review",
        "▌  Pick: candidate 4 (one rule for who",
        "▌  sees to a task), 3 (reviews that ask…",
        "",
    ].map(String::from), "{s:#?}");
    // what needs you is at normal weight, and so is what a ready task says
    assert!(!buf[(3, 4)].modifier.contains(Modifier::DIM));
    assert!(!buf[(3, 12)].modifier.contains(Modifier::DIM)); // stopped
    assert!(!buf[(3, 14)].modifier.contains(Modifier::DIM));
    let done = s.iter().position(|r| r == "   Done.").unwrap();
    assert!(!buf[(3, done as u16)].modifier.contains(Modifier::DIM));

    // selecting another task gives it the room, and BP-1 goes back to two lines
    a.sel = Some("BP-3".into());
    let s = screen(&mut a, 42, 24);
    assert_eq!(s[4..7], [
        "▌  Pick: candidate 4 (one rule for who",
        "▌  sees to a task), 3 (reviews that ask…",
        "▌■ BP-2  Task BP-2",
    ].map(String::from), "{s:#?}");
    assert_eq!(s[8], "▌● BP-3  Architecture round 2: review", "{s:#?}");
    assert_eq!(s[15], "▌  undo.", "{s:#?}");
}

#[test]
fn another_coordinators_question_stays_dim() {
    let c = |id: &str, who: &str| asking(id).with(json!({"coordinator": who}));
    let mut a = app(vec![c("BP-1", "c1"), c("BP-2", "c2"), ready("BP-3")], 0, Layout::Grouped);
    a.cfg.coordinators = vec!["c1".into(), "c2".into()];
    a.cfg.board_others = vec!["c1".into()];
    a.me = Some("c1".into());
    a.sel = Some("BP-3".into());
    let mut term = Terminal::new(TestBackend::new(42, 24)).unwrap();
    term.draw(|f| draw(f, &mut a)).unwrap();
    let buf = term.backend().buffer().clone();
    let s: Vec<String> = (0..24).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
    let mine = s.iter().position(|r| r.contains("BP-1")).unwrap() + 1;
    let theirs = s.iter().position(|r| r.contains("BP-2")).unwrap() + 1;
    assert!(s[theirs].starts_with("▌  Pick:"), "{s:#?}");
    assert!(!buf[(3, mine as u16)].modifier.contains(Modifier::DIM), "{s:#?}");
    assert!(buf[(3, theirs as u16)].modifier.contains(Modifier::DIM), "{s:#?}");

    // selected, it's on the tint like any selected task, and not dim there
    a.sel = Some("BP-2".into());
    let mut term = Terminal::new(TestBackend::new(42, 24)).unwrap();
    term.draw(|f| draw(f, &mut a)).unwrap();
    let cell = &term.backend().buffer()[(3, theirs as u16)];
    assert!(!cell.modifier.contains(Modifier::DIM));
    assert_eq!(cell.bg, TINT);
}

#[test]
fn the_selected_task_has_its_id_reversed_and_its_whole_block_tinted() {
    let mut a = app(vec![asking("BP-1"), ready("BP-12")], 0, Layout::Grouped);
    a.sel = Some("BP-1".into());
    let mut term = Terminal::new(TestBackend::new(42, 24)).unwrap();
    term.draw(|f| draw(f, &mut a)).unwrap();
    let buf = term.backend().buffer().clone();
    let s: Vec<String> = (0..24).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
    let title = s.iter().position(|r| r.contains("BP-1 ")).unwrap() as u16;
    assert_eq!(s[title as usize], "▌● BP-1   Architecture round 2: review", "{s:#?}");
    // the ID itself in reverse video, and not its padding: that shows in any theme
    for x in 3..7 {
        assert!(buf[(x, title)].modifier.contains(Modifier::REVERSED | Modifier::BOLD), "column {x}");
        assert_eq!(buf[(x, title)].bg, Color::Reset, "column {x}");
    }
    assert!(!buf[(7, title)].modifier.contains(Modifier::REVERSED));
    // the rest of its title row and every detail row on the tint, from column 3
    // to the right edge, so the whole block reads as selected
    let details = (title + 1..24).take_while(|&y| s[y as usize].starts_with("▌  ")).count() as u16;
    assert_eq!(details, 7, "{s:#?}");
    for y in title..=title + details {
        let from = if y == title { 7 } else { 3 };
        for x in from..40 {
            assert_eq!(buf[(x, y)].bg, TINT, "row {y}, column {x}: {s:#?}");
            assert!(!buf[(x, y)].modifier.contains(Modifier::DIM), "row {y}, column {x}");
        }
        assert_eq!(buf[(40, y)].bg, Color::Reset, "row {y}");
        // the amber bar stays as it is
        assert_eq!(buf[(0, y)].symbol(), "▌");
        assert_eq!(buf[(0, y)].fg, AMBER);
        assert_eq!(buf[(0, y)].bg, Color::Reset);
    }
    // another task: no reverse, no tint; a ready task's details at normal weight
    let other = s.iter().position(|r| r.contains("BP-12")).unwrap() as u16;
    assert!(!buf[(3, other)].modifier.contains(Modifier::REVERSED));
    for y in other..other + 3 {
        assert!((0..42).all(|x| buf[(x, y)].bg == Color::Reset), "row {y}");
    }
    assert!(!buf[(3, other + 1)].modifier.contains(Modifier::DIM));

    // in the compact layout, the title row alone, the same way
    a.cfg.layout = Layout::Compact;
    let mut term = Terminal::new(TestBackend::new(42, 24)).unwrap();
    term.draw(|f| draw(f, &mut a)).unwrap();
    let buf = term.backend().buffer().clone();
    let y = (0..24).find(|&y| buf[(3, y)].symbol() == "B" && buf[(6, y)].symbol() == "1").unwrap();
    assert!(buf[(3, y)].modifier.contains(Modifier::REVERSED));
    assert_eq!(buf[(10, y)].bg, TINT);
    assert_eq!(buf[(10, y + 1)].bg, Color::Reset);
}

#[test]
fn a_selected_task_low_on_the_board_scrolls_its_details_into_view() {
    let mut a = app((1..=7).map(|i| asking(&format!("BP-{i}"))).collect(), 0, Layout::Grouped);
    a.sel = Some("BP-7".into());
    let s = screen(&mut a, 42, 16);
    let i = s.iter().position(|r| r.contains("BP-7")).unwrap();
    assert_eq!(s[i + 7], "▌  undo.", "{s:#?}");
    assert!(s[1].ends_with('↑'), "{s:#?}");
}

#[test]
fn the_details_at_42_columns_say_when_a_task_comes_to_you_and_open_for_needs() {
    let ago = |m| (model::now() - chrono::Duration::minutes(m)).to_rfc3339();
    let t = |id: &str, st: &str, title: &str, why: &str, m| {
        task(id, st).with(json!({"title": title, "why": Some(why).filter(|w| !w.is_empty()), "since": ago(m)}))
    };
    let finished = t("BP-64", "finished", "Record the demo video", "", 30).with(json!({"summary": "# Recorded the demo video"}));
    let mut a = app(vec![
        t("BP-61", "needs", "Fix the login redirect", "Failed (coordinator didn't resolve it)", 4),
        t("BP-62", "triage", "Retry flaky upload test", "Ended its turn without saying ready", 3),
        finished,
    ], 1, Layout::Grouped);
    a.settle();
    let s = screen(&mut a, 42, 24);
    let at = |head: &str| s.iter().position(|r| r.contains(head)).unwrap();
    // not selected, a task its coordinator is on still gets room for when it's yours
    let i = at("BP-62");
    assert_eq!(s[i + 1..i + 3], ["   Ended its turn without ready. Yours", "   in 7 min."].map(String::from), "{s:#?}");
    // a finished task doesn't spend its two lines on "a closes it": the key row says that
    let i = at("BP-64");
    assert_eq!(s[i + 1], "   Finished, nothing to merge: Recorded", "{s:#?}");
    assert_eq!(s[i + 2], "   the demo video.", "{s:#?}");
    // a needs task without a question: ⏎ opens it, and says so
    assert!(row(&s, 1).starts_with(" ⏎ open  t tell  p peek"), "{s:#?}");
}

#[test]
fn only_background_details_are_dim_and_a_stopped_tasks_age_turns_red() {
    let ago = |m| (model::now() - chrono::Duration::minutes(m)).to_rfc3339();
    let t = |id: &str, st: &str, m| {
        let why = matches!(st, "stopped" | "needs" | "triage").then_some("Failed (exit 1)");
        task(id, st).with(json!({"title": format!("Task {id}"), "why": why, "since": ago(m)}))
    };
    let mut a = app(vec![
        t("BP-1", "needs", 2),
        t("BP-2", "stopped", 6),
        t("BP-3", "stopped", 4),
        t("BP-4", "ready", 9),
        t("BP-5", "finished", 9),
        busy("BP-6", "working", "Running the tests").with(json!({"since": ago(9)})),
        t("BP-7", "todo", 9),
        t("BP-8", "triage", 1),
        t("BP-9", "review", 9),
        t("BP-10", "done", 9),
    ], 1, Layout::Grouped);
    a.closed = true; // closed tasks shown
    a.sel = Some("BP-1".into());
    let mut term = Terminal::new(TestBackend::new(42, 48)).unwrap();
    term.draw(|f| draw(f, &mut a)).unwrap();
    let buf = term.backend().buffer().clone();
    let s: Vec<String> = (0..48).map(|y| (0..42).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
    let title = |id: &str| s.iter().position(|r| r.contains(&format!("{id} "))).unwrap_or_else(|| panic!("{id}: {s:#?}")) as u16;
    let dim_detail = |id: &str| {
        let y = title(id) + 1;
        assert!(s[y as usize].len() > 3 && s[y as usize][..].contains(|c: char| c.is_alphanumeric()), "{id} has no detail: {s:#?}");
        buf[(3, y)].modifier.contains(Modifier::DIM)
    };
    // what you read and act on at normal weight
    for id in ["BP-2", "BP-3", "BP-4", "BP-5"] {
        assert!(!dim_detail(id), "{id}: {s:#?}");
    }
    // the background dim: working, queued, the coordinator's, in review, closed
    for id in ["BP-6", "BP-7", "BP-8", "BP-9", "BP-10"] {
        assert!(dim_detail(id), "{id}: {s:#?}");
    }
    // a stopped task's age is red after 5 minutes, as a needs task's is; not before
    let age = |id: &str| buf[(39, title(id))].clone();
    assert_eq!(s[title("BP-2") as usize].trim_end().rsplit(' ').next(), Some("6m"), "{s:#?}");
    assert_eq!(age("BP-2").fg, Color::Red);
    assert!(!age("BP-2").modifier.contains(Modifier::DIM));
    assert_ne!(age("BP-3").fg, Color::Red);
    assert!(age("BP-3").modifier.contains(Modifier::DIM));
    assert_ne!(age("BP-6").fg, Color::Red); // a working task's age never is
}

#[test]
fn a_board_with_tasks_but_none_shown_says_why() {
    let closed = |id: &str| ready(id).with(json!({"status": "done"}));
    let mut a = app(vec![closed("BP-1"), closed("BP-2")], 1, Layout::Grouped);
    let s = screen(&mut a, 42, 12);
    assert_eq!(s[2..4], [" Nothing open.", " f shows 2 closed."].map(String::from), "{s:#?}");
    a.closed = true;
    let s = screen(&mut a, 42, 12);
    assert_eq!(s[2], " Closed  2", "{s:#?}");

    // only another coordinator's: o shows them
    let c3 = |id: &str| ready(id).with(json!({"coordinator": "c3"}));
    let mut a = app(vec![c3("BP-3"), c3("BP-4"), closed("BP-5").with(json!({"coordinator": "c1"}))], 1, Layout::Grouped);
    a.cfg.coordinators = vec!["c1".into(), "c3".into()];
    a.me = Some("c1".into());
    let s = screen(&mut a, 42, 12);
    assert_eq!(s[2..5], [" Nothing of yours open.", " o shows 2 of c3's.", " f shows 1 closed."].map(String::from), "{s:#?}");

    // and none at all
    let mut a = app(vec![], 1, Layout::Grouped);
    let s = screen(&mut a, 42, 12);
    assert_eq!(s[2], " No tasks yet.", "{s:#?}");
}

#[test]
fn a_narrow_board_puts_one_space_after_the_id_and_the_age_on_the_detail() {
    let old = (model::now() - chrono::Duration::minutes(12)).to_rfc3339();
    let tasks = vec![
        asks("BP-3").with(json!({"title": "Fix the login redirect after sign-in", "since": old})),
        ready("BP-10").with(json!({"title": "Paginate the orders list", "why": null})),
    ];
    let mut a = app(tasks.clone(), 0, Layout::Grouped);
    let s = screen(&mut a, 30, 16);
    assert_eq!(s[2..10], [
        " Needs you  1",
        "▌● BP-3  Fix the login redi…",
        "▌  12m · Asks: python3 or",
        "▌  python?",
        "",
        " Ready to accept  1",
        " ✔ BP-10 Paginate the order…",
        "   Done.",
    ].map(String::from), "{s:#?}");
    // a needs task's age is still red after five minutes
    assert_eq!(tone_of(&mut a, 30, 14, "12m"), Some(Color::Red));

    // at 36 columns the age is back on the title
    let s = screen(&mut a, 36, 16);
    assert!(s[3].ends_with("12m") && s[3].contains("BP-3   Fix"), "{s:#?}");
    assert_eq!(s[4], "▌  Asks: python3 or python?", "{s:#?}");

    // compact: no detail lines, so the age stays on the title
    let mut a = app(tasks, 0, Layout::Compact);
    let s = screen(&mut a, 30, 20);
    assert!(s.iter().any(|r| r.starts_with("▌● BP-3  Fix the l") && r.ends_with("12m")), "{s:#?}");
}

#[test]
fn the_compact_layout_names_its_groups_dim() {
    let tasks = vec![asks("BP-1"), ready("BP-2"), ready("BP-3").with(json!({"status": "working"}))];
    let mut a = app(tasks, 0, Layout::Compact);
    a.sel = Some("BP-2".into());
    let s = screen(&mut a, 42, 20);
    assert_eq!(s[2..8], [
        " Needs you  1",
        "▌● BP-1  Task BP-1",
        " Ready to accept  1",
        " ✔ BP-2  Task BP-2",
        " Working  1",
        " ⠋ BP-3  Task BP-3",
    ].map(String::from), "{s:#?}");
    let mut term = Terminal::new(TestBackend::new(42, 20)).unwrap();
    term.draw(|f| draw(f, &mut a)).unwrap();
    assert!(term.backend().buffer()[(1, 4)].modifier.contains(Modifier::DIM));
}

#[test]
fn a_ready_task_has_its_risk_before_its_age_in_its_colour() {
    let risk = |id: &str, level: &str| ready(id).with(json!({"risk": {"level": level, "why": "changes how accept merges"}}));
    let mut a = app(vec![risk("BP-1", "high"), risk("BP-2", "medium"), risk("BP-3", "low"), ready("BP-4")], 0, Layout::Grouped);
    let s = screen(&mut a, 60, 24);
    let row = |id: &str| s.iter().find(|r| r.contains(id)).unwrap().clone();
    assert!(row("BP-1").contains("Task BP-1") && row("BP-1").ends_with("high"), "{s:#?}");
    assert!(row("BP-2").ends_with("medium") && row("BP-3").ends_with("low"), "{s:#?}");
    assert!(!row("BP-4").ends_with("low") && !row("BP-4").ends_with("high"), "{s:#?}");
    assert!(s.iter().any(|r| r.contains("High risk: changes how accept merges.")), "{s:#?}");
    assert_eq!(tone_of(&mut a, 60, 24, "high"), Some(Color::Red));
    assert_eq!(tone_of(&mut a, 60, 24, "medium"), Some(AMBER));
    // the title gives way to it on a narrow board
    let long = ready("BP-5").with(json!({"title": "A long title that has to be cut to fit", "risk": {"level": "high", "why": "x"}}));
    let mut a = app(vec![long], 0, Layout::Grouped);
    let s = screen(&mut a, 40, 12);
    let r = s.iter().find(|r| r.contains("BP-5")).unwrap();
    assert!(r.contains('…') && r.ends_with("high"), "{s:#?}");
}
