use super::*;

#[test]
fn repo_settings_over_global_ones() {
    let obj = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap().as_object().unwrap().clone();
    let g = obj(r#"{"merge":"squash","review_rounds":4,"board_layout":"compact","coordinators":[{"id":"x"}]}"#);
    let r = obj(r#"{"merge":"merge","coordinators":[{"id":"c1"}]}"#);
    let c = Config::from_value(&layered(g.clone(), r));
    assert_eq!(c.merge, "merge");
    assert_eq!(c.review_rounds, 4);
    assert_eq!(c.layout, Layout::Compact);
    assert_eq!(c.coordinators, vec!["c1".to_string()]);
    let c = Config::from_value(&layered(g, serde_json::Map::new()));
    assert!(c.coordinators.is_empty()); // never the global file's
    assert_eq!(Config::from_value(&serde_json::Value::Null).merge, "rebase");
}
