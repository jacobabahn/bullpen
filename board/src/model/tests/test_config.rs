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

#[test]
fn the_look_is_text_unless_the_setting_says_image_and_the_font_is_a_path_when_set() {
    let c = config(json!({}));
    assert_eq!((c.look, c.font.clone()), (Look::Text, None));
    let c = config(json!({"board_look": "image", "board_font": "/fonts/A.ttf"}));
    assert_eq!((c.look, c.font.as_deref()), (Look::Image, Some(std::path::Path::new("/fonts/A.ttf"))));
    assert_eq!(config(json!({"board_look": "pictures", "board_font": ""})).font, None);
    assert_eq!(config(json!({"board_look": "pictures"})).look, Look::Text);
}

#[test]
fn the_text_size_is_the_settings_and_medium_when_unset_or_unknown() {
    assert_eq!(config(json!({})).text_size, TextSize::Medium);
    assert_eq!(config(json!({"board_text_size": "small"})).text_size, TextSize::Small);
    assert_eq!(config(json!({"board_text_size": "large"})).text_size, TextSize::Large);
    assert_eq!(config(json!({"board_text_size": "huge"})).text_size, TextSize::Medium);
}

#[test]
fn motion_is_on_unless_the_setting_turns_it_off() {
    assert!(config(json!({})).motion);
    assert!(!config(json!({"board_motion": false})).motion);
    assert!(config(json!({"board_motion": "off"})).motion); // the wrong shape: the default
}

#[test]
fn the_board_s_tests_read_no_settings_of_this_machine_s() {
    bullpen_core::isolated::assert_isolated();
    let c = Config::load(Path::new("/nonexistent/bullpen-board-test"));
    assert_eq!(c.obj(), &bullpen_core::settings::defaults());
    assert_eq!((c.look, c.layout, c.text_size), (Look::Text, Layout::Grouped, TextSize::Medium));
}
