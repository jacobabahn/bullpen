use super::*;

fn t(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

#[test]
fn dep_ids_split_and_dedupe() {
    let repo = Repo {
        root: "/r".into(),
        dir: "/nowhere".into(),
        cfg: t(json!({"prefix": "BP"})),
        own: Default::default(),
    };
    let ids = dep_ids(&repo, &["8, bp-9", " 8 BP-10,,", ""]);
    assert_eq!(ids, vec!["BP-8", "BP-9", "BP-10"]);
}

#[test]
fn dep_path_finds_a_cycle() {
    let mut tasks = Tasks::new();
    for (id, after) in [
        ("BP-1", json!(["BP-2"])),
        ("BP-2", json!(["BP-3", "BP-1"])),
        ("BP-3", json!([])),
    ] {
        tasks.insert(id.into(), t(json!({"id": id, "after": after})));
    }
    let path = dep_path(&tasks, "BP-1", "BP-3", &mut HashSet::new());
    assert_eq!(path, Some(vec!["BP-1".into(), "BP-2".into(), "BP-3".into()]));
    assert_eq!(dep_path(&tasks, "BP-3", "BP-1", &mut HashSet::new()), None);
}

#[test]
fn a_slot_is_freed_by_leaving_working_for_review_ready_or_finished_or_by_closing() {
    let st = |s: &str| t(json!({"status": s}));
    for to in ["review", "ready", "finished"] {
        assert_eq!(slot_freed(&st("working"), &st(to)), Some(to));
    }
    assert_eq!(slot_freed(&st("ready"), &st("done")), Some("accepted"));
    assert_eq!(
        slot_freed(&st("finished"), &t(json!({"status": "done", "closed": true}))),
        Some("closed")
    );
    assert_eq!(slot_freed(&st("todo"), &st("dropped")), Some("dropped"));
    // a stop frees nothing: its agent comes back to the task
    for to in ["needs", "triage", "stopped"] {
        assert_eq!(slot_freed(&st("working"), &st(to)), None);
    }
    assert_eq!(slot_freed(&st("review"), &st("ready")), None);
    assert_eq!(slot_freed(&Obj::new(), &st("todo")), None);
    assert_eq!(slot_freed(&st("done"), &st("dropped")), None);
}
