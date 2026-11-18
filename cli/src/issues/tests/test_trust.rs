use super::*;

fn ev(event: &str, label: &str, by: &str) -> Value {
    json!({"event": event, "label": {"name": label}, "actor": {"login": by}})
}

#[test]
fn the_label_was_added_by_whoever_added_it_last() {
    let pages = [
        json!([ev("labeled", "bullpen", "me"), ev("unlabeled", "bullpen", "me")]),
        json!([
            ev("labeled", "Bullpen", "bob"),
            ev("labeled", "bug", "me"),
            ev("commented", "", "me")
        ]),
    ];
    assert_eq!(labeled_by(&pages, "bullpen").as_deref(), Some("bob"));
    assert_eq!(labeled_by(&pages[..1], "bullpen").as_deref(), Some("me"));
    assert_eq!(labeled_by(&pages, "other"), None);
    assert_eq!(labeled_by(&[json!({"message": "Not Found"})], "bullpen"), None);
}
