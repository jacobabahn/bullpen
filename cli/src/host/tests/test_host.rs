use super::*;

fn rec(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

#[test]
fn a_record_without_a_host_is_on_rex() {
    let a = Agent::of(&rec(json!({"session": "s1", "block": "blk1"})));
    assert_eq!(a.host.name(), "rex");
    assert_eq!(a, Agent::on_rex("s1", "blk1"));
    // a session or block that's None is ""
    let none = Agent::of(&rec(json!({"session": null, "block": null})));
    assert_eq!((none.within.as_str(), none.id.as_str()), ("", ""));
}

#[test]
fn a_host_this_build_doesnt_know_is_taken_for_rex() {
    let a = Agent::of(&rec(json!({"host": "elsewhere", "block": "blk1"})));
    assert_eq!(a.host.name(), "rex");
}

#[test]
fn a_host_is_found_by_its_name() {
    assert_eq!(named("rex").map(|h| h.name()), Some("rex"));
    assert!(named("").is_none());
    assert!(named("t3").is_none());
}

#[test]
fn record_keeps_what_of_reads_back() {
    let mut r = rec(json!({"round": 1}));
    Agent::on_rex("s2", "blk9").record(&mut r);
    assert_eq!(
        Value::Object(r.clone()),
        json!({"round": 1, "block": "blk9", "session": "s2", "host": "rex"})
    );
    assert_eq!(Agent::of(&r), Agent::on_rex("s2", "blk9"));
}
