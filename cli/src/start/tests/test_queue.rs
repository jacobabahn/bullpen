use super::*;
use serde_json::{json, Value};

fn tasks(v: Value) -> super::super::Tasks {
    v.as_array()
        .unwrap()
        .iter()
        .map(|t| (t["id"].as_str().unwrap().to_string(), t.as_object().unwrap().clone()))
        .collect()
}

#[test]
fn the_queue_takes_todo_tasks_whose_dependencies_are_all_done_in_id_order() {
    let every = tasks(json!([
        {"id": "BP-10", "status": "todo", "after": ["BP-1"]},
        {"id": "BP-9", "status": "todo", "after": ["BP-1", "BP-2"]},
        {"id": "BP-1", "status": "done"},
        {"id": "BP-2", "status": "done", "closed": true},
        {"id": "BP-3", "status": "todo"},
        {"id": "BP-4", "status": "todo", "after": ["BP-5"]},
        {"id": "BP-5", "status": "working"},
        {"id": "BP-6", "status": "todo", "after": ["BP-404"]},
        {"id": "BP-7", "status": "stopped", "after": ["BP-1"]},
        {"id": "BP-8", "status": "todo", "after": []},
    ]));
    let ids: Vec<String> = queued(&every).iter().map(|t| s(t, "id").to_string()).collect();
    assert_eq!(ids, ["BP-9", "BP-10"]);
}

#[test]
fn why_names_how_the_slot_was_freed() {
    assert_eq!(why("BP-1", "accepted"), "BP-1 was accepted");
    assert_eq!(why("BP-1", "closed"), "BP-1 was closed");
    assert_eq!(why("BP-1", "review"), "BP-1 went to review, freeing a slot");
    assert_eq!(why("BP-1", "ready"), "BP-1 is ready, freeing a slot");
    assert_eq!(why("BP-1", "finished"), "BP-1 finished, freeing a slot");
    assert_eq!(why("BP-1", "dropped"), "BP-1 was dropped, freeing a slot");
}

#[test]
fn running_count_counts_only_working_tasks() {
    let every: Vec<Obj> = tasks(json!([
        {"id": "BP-1", "status": "working"},
        {"id": "BP-2", "status": "review"},
        {"id": "BP-3", "status": "working"},
    ]))
    .into_values()
    .collect();
    assert_eq!(running_count(&every), 2);
}
