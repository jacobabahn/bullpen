use super::*;

fn v(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|i| i.to_string()).collect()
}

#[test]
fn the_accepts_are_named_as_a_list_in_words() {
    assert_eq!(and_list(&v(&["BP-3"])), "BP-3");
    assert_eq!(and_list(&v(&["BP-3", "BP-5"])), "BP-3 and BP-5");
    assert_eq!(and_list(&v(&["BP-3", "BP-5", "BP-6"])), "BP-3, BP-5 and BP-6");
}

#[test]
fn take_returns_the_accepts_and_clears_them() {
    let mut t = Obj::new();
    t.insert(KEY.into(), json!(["BP-1", "BP-2"]));
    assert_eq!(take(&mut t), v(&["BP-1", "BP-2"]));
    assert!(t.get(KEY).is_none());
    assert!(take(&mut t).is_empty());
}

/// A repo of its own in the temp folder, with no config.
fn repo(name: &str) -> Repo {
    let dir = std::env::temp_dir().join(format!("bullpen-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tasks")).unwrap();
    Repo {
        root: dir.clone(),
        dir,
        cfg: Obj::new(),
        own: Default::default(),
    }
}

fn on_disk(repo: &Repo, t: Value) {
    write_obj(&repo.task_path("BP-2"), t.as_object().unwrap()).unwrap();
}

#[test]
fn mark_adds_each_accept_to_the_task_as_it_is_on_disk() {
    let repo = repo("mark");
    on_disk(
        &repo,
        json!({"id": "BP-2", "status": "review", "review": {"block": "b1", "verdict": null}}),
    );
    mark(&repo, "BP-2", "BP-1").unwrap();
    mark(&repo, "BP-2", "BP-3").unwrap();
    mark(&repo, "BP-2", "BP-3").unwrap();
    let t = repo.task("BP-2").unwrap();
    assert_eq!(ids(&t), v(&["BP-1", "BP-3"]));
    assert_eq!(s(obj(&t, "review"), "block"), "b1"); // the rest as it was
    std::fs::remove_dir_all(&repo.dir).unwrap();
}

#[test]
fn mark_leaves_a_verdict_saved_since_the_accept_read_the_task() {
    let repo = repo("verdict");
    // the accept read it in review; its reviewer approved it before the mark
    on_disk(
        &repo,
        json!({"id": "BP-2", "status": "ready", "review": {"block": null, "verdict": "approved"}}),
    );
    mark(&repo, "BP-2", "BP-1").unwrap();
    let t = repo.task("BP-2").unwrap();
    assert_eq!(
        (s(&t, "status"), s(obj(&t, "review"), "verdict")),
        ("ready", "approved")
    );
    assert!(t.get(KEY).is_none());
    std::fs::remove_dir_all(&repo.dir).unwrap();
}
