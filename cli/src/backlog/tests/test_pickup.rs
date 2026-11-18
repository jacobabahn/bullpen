use super::*;
use serde_json::json;

fn task(id: &str, status: &str, by: &str, created: &str) -> Obj {
    let v = json!({"id": id, "status": status, "started_by": by, "created": created});
    v.as_object().unwrap().clone()
}

const DAY: &str = "2026-11-15T10:00:00";

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 15).unwrap()
}

#[test]
fn caps_hold_pickup_back_in_order() {
    let caps = [3, 1, 2, 4];
    assert_eq!(held(&[], caps, today()), None);
    // anything that needs the person comes first, a hand-started task too
    let needs = [task("BP-1", "needs", "person", DAY)];
    assert_eq!(held(&needs, caps, today()), Some(Held::Needed));
    let stopped = [task("BP-1", "stopped", "", DAY)];
    assert_eq!(held(&stopped, caps, today()), Some(Held::Needed));
    // two self-started tasks waiting for the person pause it
    let pile = [
        task("BP-1", "ready", ITSELF, DAY),
        task("BP-2", "finished", ITSELF, DAY),
    ];
    assert_eq!(
        held(&pile, caps, today()),
        Some(Held::Paused(vec!["BP-1".into(), "BP-2".into()]))
    );
    // one in review is the one running
    let one = [task("BP-1", "review", ITSELF, DAY)];
    assert_eq!(held(&one, caps, today()), Some(Held::NoRoom));
    // max_running working, hand-started ones count
    let full: Vec<Obj> = (1..=3)
        .map(|n| task(&format!("BP-{n}"), "working", "person", DAY))
        .collect();
    assert_eq!(held(&full, caps, today()), Some(Held::NoRoom));
    // the day's budget counts closed ones too, and only today's
    let mut spent: Vec<Obj> = (1..=4).map(|n| task(&format!("BP-{n}"), "done", ITSELF, DAY)).collect();
    assert_eq!(held(&spent, caps, today()), Some(Held::Spent));
    spent[0].insert("created".into(), json!("2026-11-14T10:00:00"));
    assert_eq!(held(&spent, caps, today()), None);
}

#[test]
fn ids_read_as_a_list() {
    assert_eq!(and_list(&["BP-1".into()]), "BP-1");
    assert_eq!(and_list(&["BP-1".into(), "BP-2".into()]), "BP-1 and BP-2");
    assert_eq!(and_list(&["A".into(), "B".into(), "C".into()]), "A, B and C");
}

#[test]
fn the_pile_stands_while_something_else_holds_pickup() {
    let mut tasks = vec![
        task("BP-1", "ready", ITSELF, DAY),
        task("BP-2", "ready", ITSELF, DAY),
        task("BP-3", "needs", "person", DAY),
    ];
    assert_eq!(held(&tasks, [3, 1, 2, 4], today()), Some(Held::Needed));
    assert_eq!(pile(&tasks, 2), Some(vec!["BP-1".into(), "BP-2".into()]));
    tasks[0].insert("status".into(), json!("done"));
    assert_eq!(pile(&tasks, 2), None);
}
