use super::*;
use serde_json::json;

fn o(v: serde_json::Value) -> Obj {
    v.as_object().unwrap().clone()
}

fn ids(cs: &[&Obj]) -> Vec<String> {
    cs.iter().map(|c| s(c, "id").to_string()).collect()
}

#[test]
fn the_list_is_oldest_first_and_skips_what_isnt_a_coordinator() {
    let cfg = o(json!({"coordinators": [{"id": "c1"}, "x", {"id": "c2"}]}));
    assert_eq!(ids(&list(&cfg)), ["c1", "c2"]);
    assert!(list(&o(json!({}))).is_empty());
}

#[test]
fn its_running_owner_sees_to_a_task_else_every_running_coordinator() {
    let cfg = o(json!({"coordinators": [{"id": "c1"}, {"id": "c2"}, {"id": "c3"}]}));
    let task = |owner: Option<&str>| o(json!({"id": "BP-1", "coordinator": owner}));
    let all = |_: &Obj| Ok(true);
    let not_c2 = |c: &Obj| Ok(s(c, "id") != "c2");
    let sees = |t: &Obj, alive: &dyn Fn(&Obj) -> Res<bool>| ids(&sees_to(list(&cfg), t, alive).unwrap());
    assert_eq!(sees(&task(Some("c2")), &all), ["c2"]);
    assert_eq!(sees(&task(Some("c2")), &not_c2), ["c1", "c3"]); // its owner's pane is gone
    assert_eq!(sees(&task(Some("c9")), &all), ["c1", "c2", "c3"]); // its owner is no longer listed
    assert_eq!(sees(&task(None), &not_c2), ["c1", "c3"]);
    assert!(sees(&task(Some("c1")), &|_| Ok(false)).is_empty()); // none running: it needs the person
}
