use super::*;
use crate::model::testing::*;

#[test]
fn ready_and_closed_have_marks_you_tell_apart() {
    assert_eq!(mark("ready"), "✔");
    assert_eq!(mark("done"), "·");
}

#[test]
fn another_coordinators_task_is_theirs_only_while_it_is_listed() {
    let mut cfg = Config::default_for_test();
    let owned = |o: Option<&str>| Task { coordinator: o.map(String::from), ..task("BP-1", "working") };
    cfg.coordinators = vec!["c1".into()];
    assert!(!cfg.theirs(&owned(Some("c2")), Some("c1"))); // one coordinator: all are its
    cfg.coordinators = vec!["c1".into(), "c2".into()];
    assert!(cfg.theirs(&owned(Some("c2")), Some("c1")));
    assert!(!cfg.theirs(&owned(Some("c1")), Some("c1")));
    assert!(!cfg.theirs(&owned(Some("c3")), Some("c1"))); // its owner is gone: everyone's
    assert!(!cfg.theirs(&owned(None), Some("c1")));
    assert!(!cfg.theirs(&owned(Some("c2")), None)); // a board that isn't anyone's
}

/// Two coordinators: c1's and c2's tasks, one with no owner, one whose owner is gone.
fn two_boards() -> (Config, Vec<Task>) {
    let mut cfg = Config::default_for_test();
    cfg.coordinators = vec!["c1".into(), "c2".into()];
    let owned = |id: &str, status: &str, o: Option<&str>| Task { coordinator: o.map(String::from), ..task(id, status) };
    let tasks = vec![
        owned("BP-1", "working", Some("c1")),
        owned("BP-2", "needs", Some("c2")),
        owned("BP-3", "ready", Some("c2")),
        owned("BP-4", "done", Some("c2")),
        owned("BP-5", "todo", None),
        owned("BP-6", "review", Some("c9")),
    ];
    (cfg, tasks)
}

fn shown_ids<'a>(tasks: &'a [Task], cfg: &Config, me: Option<&str>, closed: bool) -> Vec<&'a str> {
    let mut ids: Vec<&str> = shown_groups(tasks, cfg, me, closed).into_iter().flat_map(|(_, ts)| ts).map(|t| t.id.as_str()).collect();
    ids.sort_by_key(|id| id[3..].parse::<u32>().unwrap());
    ids
}

#[test]
fn a_board_shows_only_its_own_coordinators_tasks() {
    let (cfg, tasks) = two_boards();
    // c2's are left off c1's board; its own, the ownerless and the gone owner's stay
    assert_eq!(shown_ids(&tasks, &cfg, Some("c1"), true), vec!["BP-1", "BP-5", "BP-6"]);
    assert_eq!(shown_ids(&tasks, &cfg, Some("c2"), false), vec!["BP-2", "BP-3", "BP-5", "BP-6"]);
    // a board that isn't anyone's shows everything
    assert_eq!(shown_ids(&tasks, &cfg, None, false), vec!["BP-1", "BP-2", "BP-3", "BP-5", "BP-6"]);
}

#[test]
fn ownerless_and_gone_owners_tasks_stay_on_every_board() {
    let (cfg, tasks) = two_boards();
    for me in ["c1", "c2"] {
        let ids = shown_ids(&tasks, &cfg, Some(me), false);
        assert!(ids.contains(&"BP-5") && ids.contains(&"BP-6"), "{me}: {ids:?}");
    }
    assert_eq!(shown_title(&tasks[4], &cfg, Some("c1")), "");
    assert_eq!(shown_title(&tasks[5], &cfg, Some("c1")), "");
}

#[test]
fn showing_the_others_brings_their_tasks_back_labelled() {
    let (mut cfg, mut tasks) = two_boards();
    cfg.board_others = vec!["c1".into()];
    assert!(cfg.shows_others(Some("c1")) && !cfg.shows_others(Some("c2")));
    // c1's board: everything, closed ones only with f
    assert_eq!(shown_ids(&tasks, &cfg, Some("c1"), false), vec!["BP-1", "BP-2", "BP-3", "BP-5", "BP-6"]);
    assert_eq!(shown_ids(&tasks, &cfg, Some("c1"), true), vec!["BP-1", "BP-2", "BP-3", "BP-4", "BP-5", "BP-6"]);
    // in their usual groups
    let groups = shown_groups(&tasks, &cfg, Some("c1"), false);
    let group = |id: &str| groups.iter().find(|(_, ts)| ts.iter().any(|t| t.id == id)).unwrap().0;
    assert_eq!((group("BP-2"), group("BP-3")), ("Needs you", "Ready to accept"));
    // labelled with their coordinator; its own aren't
    tasks[1].title = "Fix it".into();
    assert_eq!(shown_title(&tasks[1], &cfg, Some("c1")), "c2 · Fix it");
    tasks[0].title = "Mine".into();
    assert_eq!(shown_title(&tasks[0], &cfg, Some("c1")), "Mine");
    // c2's board still shows only its own
    assert_eq!(shown_ids(&tasks, &cfg, Some("c2"), false), vec!["BP-2", "BP-3", "BP-5", "BP-6"]);
    assert!(hidden(&tasks, &cfg, Some("c1"), true).is_empty());
}

#[test]
fn the_header_counts_what_is_hidden() {
    let (mut cfg, tasks) = two_boards();
    // BP-2 and BP-3 open; BP-4 is closed, so it counts only with f
    assert_eq!(hidden(&tasks, &cfg, Some("c1"), false), vec![("c2".to_string(), 2)]);
    assert_eq!(hidden(&tasks, &cfg, Some("c1"), true), vec![("c2".to_string(), 3)]);
    assert_eq!(hidden_hint(&hidden(&tasks, &cfg, Some("c1"), false), true), "+2 in c2 · o shows");
    assert_eq!(hidden_hint(&hidden(&tasks, &cfg, Some("c1"), false), false), "+2 in c2");
    // c2 hides c1's one task
    assert_eq!(hidden_hint(&hidden(&tasks, &cfg, Some("c2"), false), true), "+1 in c1 · o shows");
    // several others
    cfg.coordinators.push("c3".into());
    let mut more = tasks.clone();
    more.push(Task { coordinator: Some("c3".into()), ..task("BP-7", "working") });
    assert_eq!(hidden_hint(&hidden(&more, &cfg, Some("c1"), false), true), "+3 in c2, c3 · o shows");
    // nothing hidden: nothing said
    let mine: Vec<Task> = tasks.iter().filter(|t| t.coordinator.as_deref() != Some("c2")).cloned().collect();
    assert_eq!(hidden_hint(&hidden(&mine, &cfg, Some("c1"), false), true), "");
}

#[test]
fn one_coordinator_shows_everything_as_before() {
    let (mut cfg, tasks) = two_boards();
    cfg.coordinators = vec!["c1".into()];
    // c2 isn't listed: its tasks are everyone's
    assert_eq!(shown_ids(&tasks, &cfg, Some("c1"), true), vec!["BP-1", "BP-2", "BP-3", "BP-4", "BP-5", "BP-6"]);
    assert!(hidden(&tasks, &cfg, Some("c1"), true).is_empty());
    assert_eq!(shown_title(&tasks[1], &cfg, Some("c1")), "");
}

#[test]
fn groups_follow_what_you_do_next() {
    let tasks: Vec<Task> = ["todo", "working", "ready", "needs", "stopped", "review", "done", "triage"]
        .iter()
        .enumerate()
        .map(|(i, st)| task(&format!("BP-{}", i + 1), st))
        .collect();
    let groups: Vec<(&str, Vec<&str>)> = board_groups(&tasks, false)
        .into_iter()
        .map(|(h, ts)| (h, ts.iter().map(|t| t.id.as_str()).collect()))
        .collect();
    assert_eq!(
        groups,
        vec![
            ("Needs you", vec!["BP-4", "BP-5"]),
            ("Coordinator on it", vec!["BP-8"]),
            ("Ready to accept", vec!["BP-3"]),
            ("In review", vec!["BP-6"]),
            ("Working", vec!["BP-2"]),
            ("Queued", vec!["BP-1"]),
        ]
    );
    assert_eq!(board_groups(&tasks, true).last().unwrap().0, "Closed");
}

#[test]
fn groups_sort_by_number_not_text() {
    let tasks = vec![task("BP-10", "todo"), task("BP-9", "todo")];
    let ids: Vec<&str> = board_groups(&tasks, false)[0].1.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["BP-9", "BP-10"]);
}
