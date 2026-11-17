use super::*;

const CMD: &str = "/b/bin/bullpen _worker_stop /s/turns/BP-1.json 2>/dev/null || true";

fn obj(v: Value) -> Obj {
    v.as_object().cloned().unwrap()
}

#[test]
fn put_adds_one_stop_entry_and_leaves_the_rest() {
    let theirs = json!({"hooks": [{"type": "command", "command": "say done"}]});
    let mut st = obj(json!({"permissions": {"allow": []}, "hooks": {"Stop": [theirs.clone()]}}));
    assert!(put(&mut st, CMD));
    assert_eq!(st["hooks"]["Stop"][0], theirs);
    assert_eq!(
        st["hooks"]["Stop"][1],
        json!({"hooks": [{"type": "command", "command": CMD, "timeout": 10}]})
    );
    assert!(has(&st, CMD));
    assert!(!put(&mut st, CMD)); // current: nothing to change
    let other = "/c/bin/bullpen _worker_stop /s/turns/BP-1.json 2>/dev/null || true";
    assert!(!has(&st, other));
    assert!(put(&mut st, other)); // another copy of bullpen: brought up to date, still once
    assert_eq!(st["hooks"]["Stop"].as_array().unwrap().len(), 2);
    assert!(has(&st, other));
}

#[test]
fn put_keeps_one_of_two_and_leaves_a_shape_it_doesnt_know() {
    let mine = json!({"type": "command", "command": CMD, "timeout": 10});
    let mut st = obj(json!({"hooks": {"Stop": [{"hooks": [mine.clone()]}, {"hooks": [mine.clone()]}]}}));
    assert!(put(&mut st, CMD));
    assert_eq!(st["hooks"]["Stop"], json!([{"hooks": [mine]}]));
    let mut odd = obj(json!({"hooks": "nope"}));
    assert!(!put(&mut odd, CMD));
    assert_eq!(odd["hooks"], json!("nope"));
}

#[test]
fn the_command_never_fails_the_turn() {
    // exit 2 from a Stop hook would keep the worker's turn going
    assert!(CMD.ends_with("|| true"));
    assert!(ours(&json!({"command": CMD})));
    assert!(!ours(&json!({"command": "/b/bin/bullpen news --wake"})));
}
