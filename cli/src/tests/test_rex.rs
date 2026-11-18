use super::*;

fn view() -> Obj {
    let v = json!({"windows": [
        {"window_id": "w1", "layers": [
            {"layer_id": "tiled-w1", "kind": "tiled", "blocks": [{"block_id": "blk1", "label": "bullpen: proj"}]},
            {"layer_id": "lyr2", "kind": "floating", "state": "minimized",
             "blocks": [{"block_id": "blk3", "label": "bullpen: BP-1 · ctrl+esc hides"}]},
        ]},
        {"window_id": "w2", "layers": [
            {"layer_id": "tiled-w2", "kind": "tiled", "blocks": [{"block_id": "blk4", "label": "bullpen: BP-2"},
                                                                 {"block_id": "blk5", "label": "bullpen: proj"}]},
        ]},
    ]});
    v.as_object().unwrap().clone()
}

#[test]
fn test_find_by_id_or_by_label_with_or_without_the_hint() {
    let v = view();
    let hit = find_pane(Some(&v), "blk3", "").unwrap();
    assert_eq!(
        (hit.window.as_deref(), hit.kind(), hit.layer_id(), hit.block.as_str()),
        (Some("w1"), "floating", "lyr2".into(), "blk3")
    );
    assert_eq!(find_pane(Some(&v), "", "bullpen: BP-1").unwrap().block, "blk3"); // the hint, or not
    assert_eq!(find_pane(Some(&v), "", "bullpen: BP-2").unwrap().block, "blk4");
    assert_eq!(find_pane(Some(&v), "", "bullpen: nobody"), None);
    assert_eq!(find_pane(Some(&v), "nosuch", "bullpen: BP-2"), None); // an ID is looked for alone
    assert_eq!(find_pane(Some(&v), "", ""), None);
    assert_eq!(find_pane(None, "blk3", ""), None);
    let both: Vec<_> = find_panes(Some(&v), "bullpen: proj")
        .into_iter()
        .map(|w| (w.window.unwrap(), w.block))
        .collect();
    assert_eq!(both, [("w1".into(), "blk1".into()), ("w2".into(), "blk5".into())]);
}

#[test]
fn a_label_is_found_with_or_without_the_hint_only() {
    let b = |label: &str| json!({"label": label}).as_object().unwrap().clone();
    assert!(is_label(&b("bullpen: BP-3"), "bullpen: BP-3"));
    assert!(is_label(&b("bullpen: BP-3 · ctrl+esc hides"), "bullpen: BP-3"));
    assert!(!is_label(&b("bullpen: BP-3 review"), "bullpen: BP-3"));
    assert!(!is_label(&b(""), ""));
    assert!(!is_label(&Obj::new(), "bullpen: BP-3"));
}

#[test]
fn a_vt_screens_gaps_are_spaces_and_its_colours_can_go() {
    assert_eq!(
        forward_as_spaces("a\x1b[3Cb\x1b[Cc\x1b[0Cd\x1b[1;2me"),
        "a   b cd\x1b[1;2me"
    );
    assert_eq!(without_sgr("\x1b[1;38:5:2mred\x1b[m \x1b[2K"), "red \x1b[2K");
    assert_eq!(without_sgr("\x1b["), "\x1b[");
}

#[test]
fn with_state_carries_bullpens_settings_into_a_pane() {
    let argv = vec!["bullpen".to_string(), "board".into()];
    let keys = ["BULLPEN_STATE", "BULLPEN_GH", "BULLPEN_REX", "BULLPEN_BOARD"];
    let was: Vec<_> = keys.iter().map(|k| std::env::var_os(k)).collect();
    for k in keys {
        std::env::remove_var(k);
    }
    assert_eq!(with_state(&argv), argv);
    std::env::set_var("BULLPEN_REX", "/x/rex");
    std::env::set_var("BULLPEN_STATE", "/s");
    assert_eq!(
        with_state(&argv),
        ["env", "BULLPEN_STATE=/s", "BULLPEN_REX=/x/rex", "bullpen", "board"]
    );
    for (k, v) in keys.iter().zip(was) {
        match v {
            Some(v) => std::env::set_var(k, v),
            None => std::env::remove_var(k),
        }
    }
}
