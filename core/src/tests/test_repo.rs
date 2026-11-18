use super::*;
use crate::testenv::Env;

fn task(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

fn read(path: &Path) -> Obj {
    json::read_obj(path).unwrap().unwrap()
}

#[test]
fn ids_paths_and_names() {
    let env = Env::new();
    let mut repo = Repo::new("/home/u/dev/my proj").unwrap();
    assert_eq!(repo.dir, env.state().join("repos/home-u-dev-my_proj"));
    assert_eq!(repo.name(), "my proj");
    assert_eq!(repo.norm("3"), "BP-3");
    assert_eq!(repo.norm("ab-3"), "AB-3");
    repo.cfg.insert("prefix".into(), json!("XY"));
    assert_eq!(repo.norm("12"), "XY-12");
    assert_eq!(repo.task_path("XY-1"), repo.dir.join("tasks/XY-1.json"));
    let wt = repo.worktree_dir(&task(json!({"id": "XY-2"})));
    assert_eq!(wt, PathBuf::from("/home/u/dev/my proj-bullpen/XY-2"));
    let err = repo.task("9").unwrap_err().0;
    assert_eq!(
        err,
        "no task XY-9 in /home/u/dev/my proj: `bullpen ls --all` lists them"
    );
}

#[test]
fn save_task_stamps_it_and_returns_what_it_was() {
    let _env = Env::new();
    let repo = Repo::new("/r").unwrap();
    let mut t = task(json!({"id": "BP-2", "status": "working", "note": "n", "stuck": 1, "auto_wait": true}));
    let old = repo.save_task(&mut t).unwrap();
    assert!(old.is_empty());
    let since = s(&t, "since").to_string();
    assert_eq!(since.len(), "2026-11-12T21:42:21.123456-06:00".len(), "{since}");
    assert_eq!(s(&t, "updated").len(), "2026-11-12T21:42:21-06:00".len());
    // a new status: its note, stuck and auto_wait go (note stays as null, where it was)
    let keys: Vec<&str> = t.keys().map(String::as_str).collect();
    assert_eq!(keys, ["id", "status", "note", "updated", "since"]);
    assert_eq!(t["note"], Value::Null);
    assert_eq!(read(&repo.task_path("BP-2")), t);
    // the same status: its since and note stay
    t.insert("note".into(), json!("still"));
    t.insert("stuck".into(), json!(2));
    let old = repo.save_task(&mut t).unwrap();
    assert_eq!(old["status"], json!("working"));
    assert_eq!(
        (s(&t, "since"), &t["note"], &t["stuck"]),
        (since.as_str(), &json!("still"), &json!(2))
    );
    // a since lost is stamped again
    t.shift_remove("since");
    repo.save_task(&mut t).unwrap();
    assert!(!s(&t, "since").is_empty());
    let text = std::fs::read_to_string(repo.task_path("BP-2")).unwrap();
    assert!(
        text.ends_with("}\n") && text.starts_with("{\n  \"id\": \"BP-2\""),
        "{text}"
    );
    assert!(repo.save_task(&mut task(json!({"status": "x"}))).is_err());
}

#[test]
fn tasks_come_by_number() {
    let _env = Env::new();
    let repo = Repo::new("/r").unwrap();
    for n in [10, 2, 1] {
        json::write(
            &repo.task_path(&format!("BP-{n}")),
            &json!({"id": format!("BP-{n}"), "status": "todo"}),
        )
        .unwrap();
    }
    let ids: Vec<String> = repo.tasks().unwrap().iter().map(|t| s(t, "id").into()).collect();
    assert_eq!(ids, ["BP-1", "BP-2", "BP-10"]);
    assert_eq!(s(&repo.task("bp-10").unwrap(), "id"), "BP-10");
    std::fs::write(repo.task_path("BP-3"), "{").unwrap();
    let err = repo.tasks().unwrap_err().0;
    assert!(err.contains("BP-3.json doesn't parse ("), "{err}");
}

#[test]
fn the_gh_program() {
    let _env = Env::new();
    let mut repo = Repo::new("/r").unwrap();
    std::env::remove_var("BULLPEN_GH");
    assert_eq!(repo.gh(), "gh");
    repo.cfg.insert("gh".into(), json!("/opt/gh"));
    assert_eq!(repo.gh(), "/opt/gh");
    std::env::set_var("BULLPEN_GH", "/env/gh");
    assert_eq!(repo.gh(), "/env/gh");
    std::env::remove_var("BULLPEN_GH");
}

/// A git repository in the test's folder, with one commit on branch.
fn git_repo(env: &Env, branch: &str) -> PathBuf {
    let root = env.dir.join("proj");
    std::fs::create_dir_all(&root).unwrap();
    let r = root.display().to_string();
    let git = |args: &[&str]| {
        let mut argv = vec![
            "git",
            "-C",
            &r,
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ];
        argv.extend_from_slice(args);
        run(&argv, true).unwrap();
    };
    git(&["init", "-q", "-b", branch]);
    git(&["commit", "-q", "--allow-empty", "-m", "first"]);
    root
}

#[test]
fn base_and_origin_come_from_git() {
    let env = Env::new();
    let root = git_repo(&env, "trunk");
    let canon = root.canonicalize().unwrap();
    assert_eq!(repo_root_of(&root).unwrap(), canon);
    assert_eq!(repo_root_of(&root.join(".git")).unwrap(), canon);
    assert_eq!(repo_root_of(Path::new("/")).unwrap_err().0, "not in a git repository");
    let mut repo = Repo::new(&root).unwrap();
    assert!(!repo.has_origin().unwrap());
    assert_eq!(repo.base().unwrap(), "trunk");
    repo.cfg.insert("base".into(), json!("dev"));
    assert_eq!(repo.base().unwrap(), "dev");
}

#[test]
fn panes_are_indexed_by_block() {
    let env = Env::new();
    let mut repo = Repo::new("/r").unwrap();
    repo.save().unwrap();
    index_block("blk/1", &repo, "worker").unwrap();
    let idx = env.state().join("blocks/blk_1.json");
    assert_eq!(
        std::fs::read_to_string(&idx).unwrap(),
        "{\n  \"repo\": \"/r\",\n  \"role\": \"worker\",\n  \"stuck_after\": 20\n}\n"
    );
    assert!(env.state().join("doing").is_dir());
    repo.cfg.insert("stuck_after".into(), json!(5));
    reindex(&repo).unwrap();
    assert_eq!(read(&idx)["stuck_after"], json!(5));
    mark_compacting("blk/1", true).unwrap();
    assert_eq!(read(&idx).keys().next_back().map(String::as_str), Some("compacting"));
    mark_compacting("blk/1", false).unwrap();
    assert!(!read(&idx).contains_key("compacting"));
    mark_compacting("nope", true).unwrap(); // an unknown pane: nothing
    assert!(!env.state().join("blocks/nope.json").exists());
    let (found, role) = lookup_block(Some("blk/1")).unwrap().unwrap();
    assert_eq!((found.root, role.as_str()), (PathBuf::from("/r"), "worker"));
    assert!(lookup_block(Some("other")).unwrap().is_none());
    assert!(lookup_block(None).unwrap().is_none());
    assert_eq!(find_repo(Some("blk/1")).unwrap().root, PathBuf::from("/r"));
    std::fs::write(env.state().join("doing/blk_1.json"), "{}").unwrap();
    unindex_block("blk/1").unwrap();
    unindex_block("blk/1").unwrap(); // gone already: fine
    assert!(!idx.exists() && !env.state().join("doing/blk_1.json").exists());
}

#[test]
fn find_repo_falls_back_to_the_latest_coordinator() {
    let env = Env::new();
    let mut a = Repo::new("/ra").unwrap();
    a.cfg
        .insert("coordinators".into(), json!([{"id": "c1", "block": "ba"}]));
    a.save().unwrap();
    let mut b = Repo::new("/rb").unwrap();
    b.cfg.insert(
        "coordinators".into(),
        json!([{"id": "c1", "block": null}, {"id": "c2", "block": "bb"}]),
    );
    b.save().unwrap();
    index_block("ba", &a, "coordinator").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    index_block("bb", &b, "coordinator").unwrap();
    // the tests run in this checkout, which has no bullpen state in the test's folder
    assert_eq!(find_repo(None).unwrap().root, PathBuf::from("/rb"));
    std::fs::remove_dir_all(env.state().join("blocks")).unwrap();
    assert_eq!(
        find_repo(None).unwrap_err().0,
        "no bullpen here: run `bullpen up` in a repository first"
    );
}

#[test]
fn known_repos_are_those_still_checked_out() {
    let env = Env::new();
    let here = git_repo(&env, "main");
    Repo::new(&here).unwrap().save().unwrap();
    Repo::new("/gone/repo").unwrap().save().unwrap();
    let broken = env.state().join("repos/broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("config.json"), "{").unwrap();
    let roots: Vec<PathBuf> = known_repos().unwrap().into_iter().map(|r| r.root).collect();
    assert_eq!(roots, [here]);
}
