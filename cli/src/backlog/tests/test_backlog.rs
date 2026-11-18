use super::*;

fn task(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

#[test]
fn show_says_where_a_task_came_from() {
    assert_eq!(from_line(&task(json!({"id": "BP-1"}))), None);
    let t = task(json!({"from": "L7", "origin": "backlog", "started_by": "c3"}));
    assert_eq!(from_line(&t).unwrap(), "     from: the backlog (L7), started by c3");
    let t = task(json!({"from": "L7", "origin": "backlog", "started_by": "person"}));
    assert_eq!(from_line(&t).unwrap(), "     from: the backlog (L7), started by you");
    let t = task(json!({"from": "L9", "origin": "check", "started_by": "itself"}));
    assert_eq!(from_line(&t).unwrap(), "     from: L9 (check), started by itself");
}
