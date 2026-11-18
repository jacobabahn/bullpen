use super::*;
use crate::testenv::Env;

/// A repo's state folder under env's: a checkout (made unless gone), its coordinators
/// and tasks of these statuses (BP-1, BP-2, ...).
fn repo(env: &Env, name: &str, gone: bool, coordinators: Value, statuses: &[&str]) -> PathBuf {
    let root = env.dir.join("checkouts").join(name);
    if !gone {
        std::fs::create_dir_all(&root).unwrap();
    }
    let dir = env.state().join("repos").join(name);
    std::fs::create_dir_all(dir.join("tasks")).unwrap();
    let cfg = json!({"root": root, "coordinators": coordinators, "prefix": "BP"});
    std::fs::write(dir.join("config.json"), cfg.to_string()).unwrap();
    for (i, st) in statuses.iter().enumerate() {
        task(&dir, json!({"id": format!("BP-{}", i + 1), "title": "t", "status": st}));
    }
    dir
}

fn task(dir: &Path, t: Value) {
    let id = t["id"].as_str().unwrap().to_string();
    std::fs::write(dir.join("tasks").join(format!("{id}.json")), t.to_string()).unwrap();
}

fn doing(env: &Env, block: &str, d: Value) {
    let path = env.state().join("doing").join(format!("{}.json", safe(block)));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, d.to_string()).unwrap();
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[test]
fn every_repo_with_its_checkout_its_tasks_and_counts() {
    let env = Env::new();
    repo(
        &env,
        "shop",
        false,
        json!([]),
        &[
            "working", "needs", "done", "ready", "stopped", "review", "finished", "todo",
        ],
    );
    repo(&env, "quiet", false, json!([]), &[]);
    repo(&env, "gone", true, json!([]), &["needs"]); // its checkout is gone: left out
    let fleet = read().unwrap();
    let names: Vec<String> = fleet.repos.iter().map(|r| r.repo.name()).collect();
    assert_eq!(names, ["quiet", "shop"]);
    let shop = &fleet.repos[1];
    assert_eq!(
        shop.counts,
        Counts {
            needs: 2,
            ready: 2,
            running: 2
        }
    );
    assert_eq!(shop.repo.cfg_str("prefix"), "BP");
    // open tasks in the board's order, closed ones only in `all`
    let ids: Vec<&str> = shop.open.iter().map(|t| s(&t.task, "id")).collect();
    assert_eq!(ids, ["BP-2", "BP-5", "BP-4", "BP-7", "BP-6", "BP-1", "BP-8"]);
    assert_eq!(shop.all.len(), 8);
    assert_eq!(shop.open[0].mark, "●");
    assert_eq!(fleet.repos[0].counts, Counts::default());
}

#[test]
fn a_task_says_what_its_agent_is_doing_and_a_coordinator_its_state() {
    let env = Env::new();
    let cs =
        json!([{"id": "c1", "block": "block:a", "session": "s"}, {"id": "c2", "block": "block:b", "session": "s"}]);
    let dir = repo(&env, "shop", false, cs, &[]);
    task(
        &dir,
        json!({"id": "BP-1", "title": "t", "status": "working", "block": "block:w"}),
    );
    task(&dir, json!({"id": "BP-2", "title": "t", "status": "todo"}));
    doing(
        &env,
        "block:w",
        json!({"state": "working", "text": "Reading the tests", "at": now()}),
    );
    doing(
        &env,
        "block:a",
        json!({"state": "working", "text": "Starting BP-1", "at": now()}),
    );
    let mut fleet = read().unwrap();
    let shop = &fleet.repos[0];
    assert_eq!(
        (shop.open[0].doing.as_str(), shop.open[0].words.as_str()),
        ("Reading the tests", "Reading the tests")
    );
    assert_eq!(shop.open[1].doing, "");
    assert_eq!(
        shop.open[1].words,
        crate::words::board_detail(&shop.repo.cfg, &shop.open[1].task, &shop.every, None).unwrap()
    );
    let c = &shop.coordinators;
    assert_eq!(
        (c[0].word(), c[0].text.as_str(), c[1].word()),
        ("working", "Starting BP-1", "idle")
    );
    // Rex says c2's pane is gone
    fleet.ask_alive(|_, block| Ok(block == "block:a"));
    let c = &fleet.repos[0].coordinators;
    assert_eq!((c[0].alive, c[1].alive, c[1].word()), (Some(true), Some(false), "gone"));
}

#[test]
fn a_task_file_that_doesnt_parse_is_named_and_left_out() {
    let env = Env::new();
    let dir = repo(&env, "shop", false, json!([]), &["needs"]);
    std::fs::write(dir.join("tasks/BP-2.json"), "{").unwrap();
    let fleet = read().unwrap();
    let shop = &fleet.repos[0];
    assert_eq!(shop.counts.needs, 1);
    assert_eq!(shop.broken.len(), 1);
    assert!(shop.broken[0].contains("BP-2.json doesn't parse"), "{:?}", shop.broken);
}

#[test]
fn a_doing_note_that_doesnt_parse_leaves_a_task_its_status_and_a_coordinator_unknown() {
    let env = Env::new();
    let cs = json!([{"id": "c1", "block": "block:a", "session": "s"}]);
    let dir = repo(&env, "shop", false, cs, &[]);
    task(
        &dir,
        json!({"id": "BP-1", "title": "t", "status": "working", "block": "block:w"}),
    );
    for block in ["block:a", "block:w"] {
        let path = env.state().join("doing").join(format!("{}.json", safe(block)));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "{").unwrap();
    }
    let shop = &read().unwrap().repos[0];
    // the task says where it stands, as with no note
    assert_eq!(shop.open[0].doing, "");
    assert_eq!(
        shop.open[0].words,
        board_sentence(&shop.repo.cfg, &shop.open[0].task, &shop.every, None, String::new())
    );
    // the coordinator isn't taken for idle
    assert!(shop.coordinators[0].unread);
    assert_eq!(shop.coordinators[0].word(), "unknown");
}

#[test]
fn a_reader_kept_reads_a_file_again_only_when_it_changed() {
    let env = Env::new();
    let dir = repo(&env, "shop", false, json!([]), &["needs"]);
    let path = dir.join("tasks/BP-1.json");
    // a file from a while ago: kept once read
    let old = SystemTime::now() - Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let mut r = Reader::default();
    assert_eq!(r.read(&env.state()).unwrap().repos[0].counts.needs, 1);
    // the same time and size: not read again (written behind its back, to show it)
    let same = r#"{"id":"BP-1","title":"t","status":"ready"}"#; // as long as "needs"
    std::fs::write(
        &path,
        format!(
            "{same:width$}",
            width = std::fs::metadata(&path).unwrap().len() as usize
        ),
    )
    .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    assert_eq!(r.read(&env.state()).unwrap().repos[0].counts.needs, 1);
    // a new time: read again
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::now())
        .unwrap();
    assert_eq!(r.read(&env.state()).unwrap().repos[0].counts.ready, 1);
    // a task gone is forgotten
    std::fs::remove_file(&path).unwrap();
    assert!(r.read(&env.state()).unwrap().repos[0].open.is_empty());
    assert!(!r.files.contains_key(&path));
}

#[test]
fn the_snapshot_as_json() {
    let env = Env::new();
    let cs = json!([{"id": "c1", "block": "block:a", "session": "s"}]);
    let dir = repo(&env, "shop", false, cs, &["ready"]);
    let mut fleet = read().unwrap();
    fleet.ask_alive(|_, _| Ok(true));
    let v = fleet.to_json();
    let r = &v["repos"][0];
    assert_eq!(r["name"], "shop");
    assert_eq!(r["root"], json!(env.dir.join("checkouts/shop")));
    assert_eq!(r["dir"], json!(dir));
    assert_eq!(r["counts"], json!({"needs": 0, "ready": 1, "running": 0}));
    assert_eq!(r["coordinators"][0]["alive"], true);
    assert_eq!(r["coordinators"][0]["word"], "idle");
    assert_eq!(r["tasks"][0]["mark"], "✔");
    assert_eq!(r["tasks"][0]["task"]["id"], "BP-1");
    assert_eq!(r["settings"]["prefix"], "BP");
    let keys: Vec<&String> = r.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "name",
            "root",
            "dir",
            "coordinators",
            "counts",
            "tasks",
            "broken",
            "settings"
        ]
    );
}

#[test]
fn the_repos_it_cant_read_are_named_apart() {
    let env = Env::new();
    repo(&env, "shop", false, json!([]), &["needs"]);
    repo(&env, "gone", true, json!([]), &[]);
    let broken = env.state().join("repos").join("broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("config.json"), "{").unwrap();
    let names: Vec<String> = repos(&env.state()).unwrap().iter().map(Repo::name).collect();
    assert_eq!(names, ["shop"]);
    assert_eq!(
        unread(&env.state()),
        [Unread {
            dir: broken,
            root: None
        }]
    );
}

#[test]
fn a_broken_global_config_fails_the_snapshot_and_each_repo_is_unread() {
    let env = Env::new();
    let dir = repo(&env, "shop", false, json!([]), &[]);
    env.global("{oops");
    assert!(repos(&env.state()).is_err());
    assert!(read().is_err());
    let root = env.dir.join("checkouts").join("shop");
    assert_eq!(unread(&env.state()), [Unread { dir, root: Some(root) }]);
}

#[test]
fn one_repos_tasks_with_what_their_agents_are_doing_and_a_half_written_file_left_out() {
    let env = Env::new();
    let dir = repo(&env, "shop", false, json!([]), &[]);
    task(&dir, json!({"id": "BP-10", "status": "working", "block": "block:w"}));
    task(
        &dir,
        json!({"id": "BP-9", "status": "review", "block": "block:w9", "review": {"block": "block:r"}}),
    );
    task(&dir, json!({"id": "BP-8", "status": "done", "block": "block:d"}));
    std::fs::write(dir.join("tasks/BP-7.json"), "{\"id\": ").unwrap();
    doing(&env, "block:w", json!({"text": "Worker"}));
    doing(&env, "block:r", json!({"text": "Reviewer"}));
    doing(&env, "block:d", json!({"text": "Closed long ago"}));
    std::fs::write(env.state().join("doing/block_w9.json"), "{").unwrap();
    let mut r = Reader::default();
    let got = r.read_repo(&dir);
    let ids: Vec<&str> = got.all.iter().map(|t| s(t, "id")).collect();
    assert_eq!(ids, ["BP-8", "BP-9", "BP-10"]);
    assert_eq!(got.broken.len(), 1);
    let said = |b: &str| got.doing.get(b).map(|d| s(d, "text").to_string());
    assert_eq!(said("block:w").as_deref(), Some("Worker"));
    // in review: its reviewer's; closed: nobody's
    assert_eq!(said("block:r").as_deref(), Some("Reviewer"));
    assert_eq!(got.doing.len(), 2);
    // the file written in full is read next time
    task(&dir, json!({"id": "BP-7", "status": "todo"}));
    let got = r.read_repo(&dir);
    assert_eq!((got.all.len(), got.broken.len()), (4, 0));
}
