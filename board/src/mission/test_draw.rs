use crate::mission::testing::*;

pub(crate) fn sample(st: &State) {
    let cs = json!([{"id": "c3", "block": "block:c3", "session": "s"}]);
    st.repo("bullpen", cs, &[
        t("BP-126", "stopped", "Measure the test suite's time", 3).as_object().cloned().map(|mut o| { o.insert("why".into(), json!("Its worker exited.")); serde_json::Value::Object(o) }).unwrap(),
        t("BP-127", "ready", "bullpen stats", 22).as_object().cloned().map(|mut o| { o.insert("diffstat".into(), json!("9 files, +612 −40")); o.insert("summary".into(), json!("Adds bullpen stats.\n\nWhere each task's time went.")); serde_json::Value::Object(o) }).unwrap(),
        t("BP-129", "review", "close --keep-branch docs", 4),
        t("BP-130", "working", "Design: mission control", 31),
        t("BP-132", "todo", "Board split, step 1", 0),
    ]);
    st.repo("jj-evolved", json!([]), &[
        t("BP-12", "needs", "Split the rebase planner", 14).as_object().cloned().map(|mut o| { o.insert("why".into(), json!("Asks: Allow `jj git push --all`?")); o.insert("stop".into(), json!({"by": "worker", "kind": "prompt"})); serde_json::Value::Object(o) }).unwrap(),
        t("BP-13", "working", "Evolve across merges", 48),
    ]);
}

fn has(s: &[String], text: &str) -> bool {
    s.iter().any(|l| l.contains(text))
}

fn line_with<'a>(s: &'a [String], text: &str) -> &'a str {
    s.iter().find(|l| l.contains(text)).unwrap_or_else(|| panic!("no {text:?} in\n{}", s.join("\n")))
}

#[test]
fn wide_two_frames_and_a_line_of_detail_under_every_task() {
    let st = State::new("wide");
    sample(&st);
    let mut m = st.mission();
    let s = screen(&mut m, 150, 30);
    assert!(s[0].starts_with(" MISSION CONTROL   2 need you · 1 ready · 3 of 7 running · 2 repos"), "{}", s[0]);
    assert!(s[1].contains("Inbox and repos") && s[1].contains("Detail"), "{}", s[1]);
    let at = |t: &str| s.iter().position(|l| l.contains(t)).unwrap();
    // the inbox: what needs you, longest wait first, each with its repo; then what's ready
    assert!(at("NEEDS YOU") < at("jj-evolved BP-12") && at("jj-evolved BP-12") < at("bullpen    BP-126"));
    assert!(at("bullpen    BP-126") < at("READY") && at("READY") < at("bullpen    BP-127"));
    // then each repo, with its coordinators and its other tasks, each with what it's doing
    assert!(line_with(&s, "c3 ○ idle").contains("5 open"));
    assert!(at("c3 ○ idle") < at("BP-129  close --keep-branch docs"));
    assert_eq!(at("Reviewer on round 1 of 2"), at("BP-129  close") + 1);
    assert!(line_with(&s, "jj-evolved   no coordinator").contains("2 open"));
    // the selected task's detail, beside
    assert!(line_with(&s, "NEEDS YOU").contains("BP-12  Split the rebase planner"), "beside the list");
    assert!(has(&s, "● needs you · 14m"));
    assert!(s[s.len() - 1].starts_with(" i answer  ⏎ open  t tell  p peek  n next  / find  q quit"), "{}", s[s.len() - 1]);
}

#[test]
fn under_wide_the_repos_tasks_get_a_line_each() {
    let st = State::new("two");
    sample(&st);
    let mut m = st.mission();
    let s = screen(&mut m, 100, 24);
    assert!(s[1].contains("Inbox and repos") && s[1].contains("Detail"));
    assert!(has(&s, "Asks: Allow")); // the inbox keeps its detail
    assert!(!has(&s, "Reviewer on round")); // the repos' tasks don't
    assert!(has(&s, "BP-129  close --keep-branch docs"));
}

#[test]
fn under_two_columns_one_with_the_selected_tasks_detail_in_the_list() {
    let st = State::new("one");
    sample(&st);
    let mut m = st.mission();
    let s = screen(&mut m, 60, 22);
    assert!(s[0].starts_with(" MISSION   ●2 ✔1 3/7"), "{}", s[0]);
    assert!(!has(&s, "Inbox and repos") && !has(&s, "┌"));
    assert!(has(&s, "Asks: Allow")); // the selected task's
    assert!(!has(&s, "Its agent exited")); // the others' go
    assert!(line_with(&s, "bullpen   c3 ○").contains("5 open")); // marks for the coordinators
    m.handle(crate::mission::MMsg::Key(ratatui::crossterm::event::KeyEvent::from(ratatui::crossterm::event::KeyCode::Char('j'))));
    let s = screen(&mut m, 60, 22);
    assert!(has(&s, "Its agent exited") && !has(&s, "Asks: Allow"));
}

#[test]
fn a_ready_tasks_detail_has_the_size_of_its_change_and_what_its_worker_said() {
    let st = State::new("readydetail");
    sample(&st);
    let mut m = st.mission();
    press(&mut m, 'j');
    press(&mut m, 'j');
    assert_eq!(selected(&m), "bullpen BP-127");
    let s = screen(&mut m, 150, 30);
    assert!(has(&s, "✔ ready to accept · 22m"));
    assert!(has(&s, "9 files, +612 −40 · Not toured yet"));
    let at = |t: &str| s.iter().position(|l| l.contains(t)).unwrap();
    assert!(at("What its worker said") < at("Adds bullpen stats.") && at("Adds bullpen stats.") < at("Where each task's time went."));
    assert!(s[s.len() - 1].starts_with(" a accept  g tour  d drop  ⏎ open"), "{}", s[s.len() - 1]);
}

#[test]
fn a_question_asks_on_the_tasks_row_and_the_key_row_says_what_y_does() {
    let st = State::new("question");
    sample(&st);
    let mut m = st.mission();
    press(&mut m, 'j');
    press(&mut m, 'j');
    press(&mut m, 'a');
    let s = screen(&mut m, 150, 30);
    assert!(line_with(&s, "bullpen    BP-127").contains("y / n"));
    // review is on and it has no verdict: the board's risky question, in red
    assert!(has(&s, "BP-127: Not reviewed. Accept and clean up anyway?"), "{}", s.join("\n"));
    assert!(s[s.len() - 1].starts_with(" y accept anyway   any other key cancels"));
}

#[test]
fn t_types_on_the_last_row_and_i_on_a_prompt_takes_the_screen() {
    let st = State::new("typing");
    sample(&st);
    let mut m = st.mission();
    press(&mut m, 't');
    press(&mut m, 'h');
    let s = screen(&mut m, 100, 24);
    assert_eq!(s[s.len() - 1], " Tell BP-12: h");
    key(&mut m, ratatui::crossterm::event::KeyCode::Esc);
    press(&mut m, 'i');
    let s = screen(&mut m, 100, 24);
    assert_eq!(s[0], " BP-12  Split the rebase planner");
    assert!(has(&s, "Reading its screen…"));
}

#[test]
fn with_no_repos_it_says_how_to_start_one() {
    let st = State::new("norepos");
    let mut m = st.mission();
    let s = screen(&mut m, 100, 20);
    assert!(has(&s, "No repositories yet.") && has(&s, "Nothing open in any repository."));
    assert!(s[0].contains("0 repos"));
}

#[test]
fn any_size_draws_without_a_panic() {
    let st = State::new("sizes");
    sample(&st);
    let mut m = st.mission();
    press(&mut m, 'q'); // a message row too
    m.msg = crate::app::Note::failed("A long failure that wraps over a few rows, on a screen far too small for it.");
    for (w, h) in [(1, 1), (10, 1), (20, 3), (79, 4), (80, 5), (140, 6), (300, 100)] {
        screen(&mut m, w, h);
    }
}

#[test]
fn a_ready_tasks_risk_shows_before_its_age_in_the_inbox() {
    let st = State::new("risk");
    let mut ready = t("BP-127", "ready", "bullpen stats", 22);
    ready["risk"] = json!({"level": "high", "why": "changes how accept merges"});
    st.repo("bullpen", json!([]), &[ready]);
    let mut m = st.mission();
    let s = screen(&mut m, 150, 30);
    let row = line_with(&s, "bullpen BP-127");
    assert!(row.contains("bullpen stats") && row.contains("high  22m"), "{row}");
    assert!(has(&s, "High risk: changes how accept merges."), "{}", s.join("\n"));
}
