use super::*;

#[test]
fn a_tasks_group_goes_by_its_status() {
    for st in ["needs", "stopped", "ready", "finished"] {
        assert_eq!(task_group(st), "needs", "{st}");
    }
    for st in ["working", "review", "triage", "todo"] {
        assert_eq!(task_group(st), "active", "{st}");
    }
    for st in ["done", "dropped"] {
        assert_eq!(task_group(st), "done", "{st}");
    }
}

#[test]
fn a_coordinator_at_a_prompt_needs_you() {
    let mut c = Coordinator {
        state: "blocked".into(),
        alive: Some(true),
        ..Default::default()
    };
    assert_eq!(coordinator_group(&c), "needs");
    c.state = "working".into();
    assert_eq!(coordinator_group(&c), "active");
    c.alive = Some(false);
    assert_eq!(coordinator_group(&c), "active");
}
