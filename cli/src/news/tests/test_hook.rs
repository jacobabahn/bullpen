use super::*;

const CMD: &str = "/b/bin/bullpen news --wake";

fn obj(v: Value) -> Obj {
    v.as_object().cloned().unwrap()
}

#[test]
fn put_writes_both_entries_into_empty_settings() {
    let mut st = Obj::new();
    assert!(put(&mut st, CMD).unwrap());
    let entry = json!([{"hooks": [{"type": "command", "command": CMD, "asyncRewake": true, "timeout": 86400}]}]);
    assert_eq!(
        Value::Object(st.clone()),
        json!({"hooks": {"Stop": entry, "StopFailure": entry}})
    );
    assert!(!put(&mut st, CMD).unwrap()); // current: nothing to change
    assert!(current(&st, CMD));
    assert_eq!(
        found(&st),
        vec![("Stop", CMD.to_string()), ("StopFailure", CMD.to_string())]
    );
}

#[test]
fn put_leaves_everything_else_where_it_is() {
    let theirs = json!({"matcher": "", "hooks": [{"type": "command", "command": "say done"}]});
    let mut st = obj(json!({
        "zeta": 1,
        "permissions": {"allow": ["Bash(ls:*)"]},
        "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": []}], "Stop": [theirs.clone()]},
        "alpha": "é",
    }));
    assert!(put(&mut st, CMD).unwrap());
    assert_eq!(st.keys().collect::<Vec<_>>(), ["zeta", "permissions", "hooks", "alpha"]);
    let hooks = st["hooks"].as_object().unwrap();
    assert_eq!(hooks.keys().collect::<Vec<_>>(), ["PreToolUse", "Stop", "StopFailure"]);
    assert_eq!(hooks["Stop"][0], theirs);
    assert_eq!(hooks["Stop"][1]["hooks"][0]["command"], CMD);
    assert_eq!(hooks["PreToolUse"], json!([{"matcher": "Bash", "hooks": []}])); // empty, but not emptied by bullpen
}

#[test]
fn put_brings_an_old_path_up_to_date_in_place_and_drops_doubles() {
    let old = "env BULLPEN_STATE=/s /old/bin/bullpen news --wake";
    let mut st = obj(json!({"hooks": {
        "Stop": [
            {"hooks": [{"type": "command", "command": old, "asyncRewake": true, "timeout": 60},
                       {"type": "command", "command": "say done"}]},
            {"hooks": [{"type": "command", "command": CMD}]},
        ],
    }}));
    assert!(!current(&st, CMD));
    assert!(put(&mut st, CMD).unwrap());
    let stop = st["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 1); // the double's group went with it
    assert_eq!(stop[0]["hooks"][0], handler(CMD));
    assert_eq!(stop[0]["hooks"][1]["command"], "say done");
    assert!(current(&st, CMD));
}

#[test]
fn take_removes_only_bullpens_and_the_lists_it_emptied() {
    let mut st = obj(json!({"model": "opus", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "x"}]}]}}));
    let before = st.clone();
    put(&mut st, CMD).unwrap();
    assert!(take(&mut st).unwrap());
    assert_eq!(st, before);
    assert!(!take(&mut st).unwrap());

    let mut only = Obj::new();
    put(&mut only, CMD).unwrap();
    assert!(take(&mut only).unwrap());
    assert!(only.is_empty()); // `hooks` went too: bullpen emptied it

    // a command that only mentions it, or has more after it, isn't bullpen's
    let mut other = obj(json!({"hooks": {"Stop": [{"hooks": [
        {"type": "command", "command": "bullpen news --wake --timeout 3600"},
        {"type": "command", "command": "echo news --wake; true"}]}]}}));
    assert!(!take(&mut other).unwrap());
    assert!(found(&other).is_empty());
}

#[test]
fn settings_of_another_shape_are_refused_not_rewritten() {
    for bad in [json!({"hooks": []}), json!({"hooks": {"Stop": {}}})] {
        let mut st = obj(bad.clone());
        assert!(put(&mut st, CMD).is_err());
        assert!(take(&mut st).is_err());
        assert_eq!(Value::Object(st), bad);
    }
    // a group that isn't one is left alone
    let mut st = obj(json!({"hooks": {"Stop": ["what", {"hooks": "?"}]}}));
    assert!(put(&mut st, CMD).unwrap());
    assert_eq!(st["hooks"]["Stop"].as_array().unwrap().len(), 3);
}
