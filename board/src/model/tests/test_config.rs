use super::*;
use crate::model::testing::*;

fn config(v: Value) -> Config {
    let mut obj = bullpen_core::settings::defaults();
    obj.extend(v.as_object().cloned().unwrap_or_default());
    Config::from_obj(obj)
}

#[test]
fn the_board_reads_the_settings_bullpen_core_layers() {
    let c = config(json!({"merge": "merge", "board_layout": "compact", "coordinators": [{"id": "c1"}, {"id": "c2", "board_others": true}]}));
    assert_eq!(c.merge, "merge");
    assert_eq!(c.layout, Layout::Compact);
    assert_eq!(c.coordinators, vec!["c1".to_string(), "c2".to_string()]);
    assert_eq!(c.board_others, vec!["c2".to_string()]);
    assert_eq!(c.obj()["review_rounds"], 2); // for bullpen-core's sentences
    // a value of the wrong shape counts as the default
    let c = config(json!({"merge": 3, "review": "yes", "triage_minutes": -1, "coordinators": "c1"}));
    assert_eq!((c.merge.as_str(), c.review, c.triage_minutes), ("rebase", true, 10));
    assert!(c.coordinators.is_empty());
}

#[test]
fn the_bottom_gap_is_a_row_only_while_something_floats() {
    let gap = |v: Value| config(v).bottom_gap;
    assert_eq!(gap(json!({})), 1); // workers float by default
    assert_eq!(gap(json!({"workers": "background"})), 0); // and reviewers are in the background
    assert_eq!(gap(json!({"workers": "tabs", "reviewers": "float"})), 1);
    assert_eq!(gap(json!({"workers": "tabs", "reviewers": "same"})), 0);
    assert_eq!(gap(json!({"workers": "tabs", "board_bottom_gap": 2})), 2); // a setting wins
    assert_eq!(gap(json!({"board_bottom_gap": 0})), 0);
}
