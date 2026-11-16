use super::*;
use crate::model::testing::json;

fn item(id: &str, state: &str) -> Item {
    Item::from_obj(json!({"id": id, "title": format!("Idea {id}"), "state": state, "by": "BP-3"}).as_object().unwrap())
}

#[test]
fn the_later_line_says_what_is_next_or_what_waits_on_you() {
    assert_eq!(later_line(&[]), (String::new(), false));
    let kept = [item("L1", "kept"), item("L2", "kept")];
    assert_eq!(later_line(&kept), ("Later · 2 · next: Idea L1".into(), false));
    let some_ask = [item("L1", "kept"), item("L4", "asks"), item("L6", "asks")];
    assert_eq!(later_line(&some_ask), ("Later · 3 · 2 to decide (L4, L6)".into(), true));
}

#[test]
fn an_item_says_what_its_keys_do() {
    let asks = item("L4", "asks");
    assert_eq!(item_mark(&asks), "?");
    assert_eq!(item_detail(&asks), "Asks for a yes (from BP-3): y keeps it, n drops it. No brief: s asks the coordinator for one.");
    assert_eq!(item_keys(&asks), vec![("y", "keep"), ("n", "drop"), ("s", "start"), ("J K", "move")]);
    let kept = Item { brief: true, skipped: "overlaps BP-9".into(), ..item("L1", "kept") };
    assert_eq!(item_mark(&kept), "◇");
    assert_eq!(item_detail(&kept), "Skipped: overlaps BP-9. Has a brief: s starts it.");
    assert_eq!(item_keys(&kept), vec![("s", "start"), ("n", "drop"), ("J K", "move")]);
}

#[test]
fn the_board_reads_the_open_items_in_order() {
    let d = std::env::temp_dir().join(format!("bullpen-board-later-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    assert!(load_later(&d).is_empty());
    let items = json!({"next": 4, "items": [
        {"id": "L3", "title": "Third", "state": "kept", "brief": "L3.md"},
        {"id": "L1", "title": "First", "state": "started", "task": "BP-2"},
        {"id": "L2", "title": "Second", "state": "asks", "by": "BP-5"},
    ]});
    std::fs::write(d.join("backlog.json"), items.to_string()).unwrap();
    let got = load_later(&d);
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(got.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["L3", "L2"]);
    assert!(got[0].brief && !got[1].brief);
    assert!(is_item("L3") && !is_item("BP-3"));
}
