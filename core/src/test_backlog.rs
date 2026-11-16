use super::*;
use crate::testenv::Env;

fn ids(b: &Backlog) -> Vec<&str> {
    b.items.iter().map(|i| s(i, "id")).collect()
}

#[test]
fn item_ids() {
    assert_eq!(norm_id("l7").as_deref(), Some("L7"));
    assert_eq!(norm_id("L007").as_deref(), Some("L7"));
    assert_eq!(norm_id("7").as_deref(), Some("L7"));
    assert_eq!(norm_id("L"), None);
    assert_eq!(norm_id("Later"), None);
    assert!(is_item_id("l12"));
    assert!(!is_item_id("12"));
    assert!(!is_item_id("Lint the docs"));
}

#[test]
fn adds_in_order_and_on_top_of_the_open_ones() {
    let mut b = Backlog {
        next: 1,
        items: Vec::new(),
    };
    assert_eq!(b.add("One", "person", "kept", false), "L1");
    b.add("Two", "person", "kept", false);
    b.item_mut("L1").unwrap().insert("state".into(), json!("started"));
    assert_eq!(b.add("Urgent", "c1", "kept", true), "L3");
    assert_eq!(ids(&b), ["L1", "L3", "L2"]);
    let item = b.item("L3").unwrap();
    assert_eq!((s(item, "by"), s(item, "state")), ("c1", "kept"));
    assert_eq!(item.get("brief"), Some(&Value::Null));
    assert_eq!(b.open().map(|i| s(i, "id")).collect::<Vec<_>>(), ["L3", "L2"]);
    assert!(b.item("L9").unwrap_err().0.contains("no item L9"));
}

#[test]
fn moves_among_the_open_items() {
    let mut b = Backlog {
        next: 1,
        items: Vec::new(),
    };
    for t in ["a", "b", "c", "d"] {
        b.add(t, "person", "kept", false);
    }
    b.item_mut("L2").unwrap().insert("state".into(), json!("dropped"));
    // open: L1 L3 L4
    assert!(b.move_item("L4", Move::Up).unwrap());
    assert_eq!(ids(&b), ["L1", "L2", "L4", "L3"]);
    assert!(b.move_item("L1", Move::Down).unwrap());
    assert_eq!(ids(&b), ["L2", "L4", "L1", "L3"]);
    assert!(b.move_item("L3", Move::Top).unwrap());
    assert_eq!(ids(&b), ["L2", "L3", "L4", "L1"]);
    assert!(b.move_item("L3", Move::Bottom).unwrap());
    assert_eq!(ids(&b), ["L2", "L4", "L1", "L3"]);
    assert!(!b.move_item("L3", Move::Down).unwrap());
    assert!(!b.move_item("L4", Move::Top).unwrap());
    assert!(b
        .move_item("L2", Move::Top)
        .unwrap_err()
        .0
        .contains("only an open item moves"));
}

#[test]
fn saved_under_its_lock_and_read_back() {
    let _env = Env::new();
    let repo = Repo::new("/home/u/dev/proj").unwrap();
    assert_eq!(
        load(&repo).unwrap(),
        Backlog {
            next: 1,
            items: Vec::new()
        }
    );
    let id = change(&repo, |b| Ok(b.add("Tidy", "person", "kept", false))).unwrap();
    assert_eq!(id, "L1");
    let failed: Res<()> = change(&repo, |b| {
        b.add("Lost", "person", "kept", false);
        fail("no")
    });
    assert!(failed.is_err());
    let b = load(&repo).unwrap();
    assert_eq!((b.next, ids(&b)), (2, vec!["L1"]));
    let text = std::fs::read_to_string(path(&repo)).unwrap();
    assert!(text.starts_with("{\n  \"next\": 2,\n  \"items\": [\n    {\n      \"id\": \"L1\","));
}
