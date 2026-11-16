use super::*;
use crate::json::dumps_obj;
use crate::testenv::Env;
use serde_json::json;

/// What python3 left in a repo's state from before triage, stops and the layers,
/// after `core.Repo("/nonexistent/proj")`: the files before, and after.
const GLOBAL: &str = r#"{"merge": "merge", "max_running": 5, "prefix": "ZZ", "root": "/x"}
"#;
const OLD_CONFIG: &str = r#"{"prefix": "XY", "next": 4, "max_running": 3, "merge": "squash", "notify": "needs", "coordinator": {"block": "b1", "session": "s1"}, "board": {"block": "b2", "session": "s1"}, "worker": ["claude", "--permission-mode", "auto"], "custom": 1}
"#;
const NEW_CONFIG: &str = r#"{
  "prefix": "XY",
  "next": 4,
  "base": null,
  "merge": "squash",
  "push_remote": null,
  "news": "type",
  "coordinators": [
    {
      "id": "c1",
      "block": "b1",
      "session": "s1",
      "board": {
        "block": "b2",
        "session": "s1"
      }
    }
  ],
  "next_coordinator": 2,
  "custom": 1,
  "root": "/nonexistent/proj",
  "triage_migrated": true,
  "stops_migrated": true,
  "layered": true
}
"#;
const TASKS: [(&str, &str, &str); 6] = [
    (
        "XY-1",
        r#"{"id": "XY-1", "status": "needs", "why": "Failed: boom", "updated": "u"}
"#,
        r#"{
  "id": "XY-1",
  "status": "triage",
  "why": "Failed: boom",
  "updated": "u",
  "stop": {
    "by": "worker",
    "kind": "failed"
  }
}
"#,
    ),
    (
        "XY-2",
        r#"{"id": "XY-2", "status": "needs", "why": "Needs you (permission): Allow?"}
"#,
        r#"{
  "id": "XY-2",
  "status": "needs",
  "why": "Needs you (permission): Allow?",
  "stop": {
    "by": "worker",
    "kind": "prompt"
  }
}
"#,
    ),
    (
        "XY-3",
        r#"{"id": "XY-3", "status": "needs", "why": "Delete it?", "ask": true, "ask_of": "Ended its turn without saying ready"}
"#,
        r#"{
  "id": "XY-3",
  "status": "needs",
  "why": "Delete it?",
  "ask": true,
  "stop": {
    "by": "worker",
    "kind": "ended"
  }
}
"#,
    ),
    (
        "XY-10",
        r#"{"id": "XY-10", "status": "working", "why": null}
"#,
        r#"{"id": "XY-10", "status": "working", "why": null}
"#,
    ),
    (
        "XY-4",
        r#"{"id": "XY-4", "status": "stopped", "why": "Its pane was gone", "stop": {"by": "worker", "kind": "pane_gone"}}
"#,
        r#"{"id": "XY-4", "status": "stopped", "why": "Its pane was gone", "stop": {"by": "worker", "kind": "pane_gone"}}
"#,
    ),
    (
        "XY-5",
        r#"{"id": "XY-5", "status": "triage", "why": null, "ask_of": "x"}
"#,
        r#"{
  "id": "XY-5",
  "status": "triage",
  "why": null
}
"#,
    ),
];

fn repo_dir(env: &Env) -> std::path::PathBuf {
    env.state().join("repos/nonexistent-proj")
}

#[test]
fn an_old_repo_is_migrated_as_python_migrates_it() {
    let env = Env::new();
    env.global(GLOBAL);
    let dir = repo_dir(&env);
    std::fs::create_dir_all(dir.join("tasks")).unwrap();
    std::fs::write(dir.join("config.json"), OLD_CONFIG).unwrap();
    for (id, before, _) in TASKS {
        std::fs::write(dir.join(format!("tasks/{id}.json")), before).unwrap();
    }
    let repo = Repo::new("/nonexistent/proj").unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("config.json")).unwrap(), NEW_CONFIG);
    for (id, _, after) in TASKS {
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("tasks/{id}.json"))).unwrap(),
            after,
            "{id}"
        );
    }
    let sources: Vec<&str> = ["merge", "max_running", "prefix", "review", "base"]
        .iter()
        .map(|k| config_source(&repo, k).unwrap())
        .collect();
    assert_eq!(sources, ["repo", "global", "repo", "default", "default"]);
    let ids: Vec<String> = repo.tasks().unwrap().iter().map(|t| s(t, "id").to_string()).collect();
    assert_eq!(ids, ["XY-1", "XY-2", "XY-3", "XY-4", "XY-5", "XY-10"]);
    // a migrated repo isn't migrated again: its tasks are left as they are
    std::fs::write(
        dir.join("tasks/XY-7.json"),
        r#"{"id": "XY-7", "status": "needs", "why": "Failed"}"#,
    )
    .unwrap();
    Repo::new("/nonexistent/proj").unwrap();
    let t7 = std::fs::read_to_string(dir.join("tasks/XY-7.json")).unwrap();
    assert_eq!(t7, r#"{"id": "XY-7", "status": "needs", "why": "Failed"}"#);
}

#[test]
fn settings_layer_defaults_then_global_then_the_repo() {
    let env = Env::new();
    // the global file can't hold what only a repo has, nor bullpen's own notes
    env.global(r#"{"merge": "squash", "review_rounds": 4, "prefix": "ZZ", "next": 9, "layered": false}"#);
    let dir = repo_dir(&env);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.json"),
        r#"{"review_rounds": 1, "prefix": "AB", "layered": true}"#,
    )
    .unwrap();
    let cfg = load_config(&dir).unwrap();
    assert_eq!(cfg["merge"], json!("squash")); // global over the default
    assert_eq!(cfg["review_rounds"], json!(1)); // the repo's over the global
    assert_eq!(cfg["prefix"], json!("AB"));
    assert_eq!(cfg["next"], json!(1)); // the global's next is ignored
    assert_eq!(cfg["max_running"], json!(3)); // the default
    let keys: Vec<&String> = cfg.keys().collect();
    assert_eq!(keys.first().map(|k| k.as_str()), Some("prefix")); // the table's order, the repo's extras after
    assert_eq!(keys.last().map(|k| k.as_str()), Some("layered"));
    let global = read_global().unwrap();
    assert_eq!(
        dumps_obj(&global),
        "{\n  \"merge\": \"squash\",\n  \"review_rounds\": 4\n}"
    );
    let inherited = inherited_config().unwrap();
    assert_eq!(
        (&inherited["merge"], &inherited["prefix"]),
        (&json!("squash"), &json!("BP"))
    );
}

#[test]
fn a_repo_writes_only_its_own_settings() {
    let env = Env::new();
    env.global(r#"{"merge": "squash"}"#);
    let mut repo = Repo::new("/a/b/").unwrap();
    assert_eq!(repo.dir, env.state().join("repos/a-b"));
    assert!(!repo.dir.join("config.json").exists()); // a new repo writes nothing until saved
    repo.set("review", json!(false));
    repo.set("merge", json!("rebase"));
    repo.save().unwrap();
    let saved = std::fs::read_to_string(repo.dir.join("config.json")).unwrap();
    let want = r#"{
  "prefix": "BP",
  "next": 1,
  "base": null,
  "merge": "rebase",
  "push_remote": null,
  "review": false,
  "news": "type",
  "issues_repo": null,
  "coordinators": [],
  "next_coordinator": 1,
  "root": "/a/b",
  "triage_migrated": true,
  "stops_migrated": true,
  "layered": true
}
"#;
    assert_eq!(saved, want);
    assert_eq!(config_source(&repo, "merge").unwrap(), "repo");
    repo.unset("merge").unwrap();
    assert_eq!(repo.cfg["merge"], json!("squash")); // the global one again
    assert_eq!(config_source(&repo, "merge").unwrap(), "global");
    assert_eq!(config_source(&repo, "prefix").unwrap(), "default"); // written down, but the default
    repo.set("prefix", json!("QQ"));
    assert_eq!(config_source(&repo, "prefix").unwrap(), "repo");
}

#[test]
fn the_old_coordinator_becomes_c1() {
    let env = Env::new();
    let dir = repo_dir(&env);
    std::fs::create_dir_all(&dir).unwrap();
    let old = r#"{"coordinator": {"block": "b1", "session": "s1"}, "board": {"block": "b2"}, "layered": true}"#;
    std::fs::write(dir.join("config.json"), old).unwrap();
    let cfg = load_config(&dir).unwrap();
    assert!(!cfg.contains_key("coordinator") && !cfg.contains_key("board"));
    assert_eq!(
        cfg["coordinators"],
        json!([{"id": "c1", "block": "b1", "session": "s1", "board": {"block": "b2"}}])
    );
    assert_eq!(cfg["next_coordinator"], json!(2));
    // one that has coordinators already keeps them; a board alone goes
    let both = r#"{"coordinator": {"block": "b1"}, "coordinators": [{"id": "c3"}], "board": null}"#;
    std::fs::write(dir.join("config.json"), both).unwrap();
    let cfg = load_config(&dir).unwrap();
    assert_eq!(cfg["coordinators"], json!([{"id": "c3"}]));
    assert_eq!(cfg["next_coordinator"], json!(1));
    assert!(!cfg.contains_key("board"));
}

#[test]
fn a_broken_global_config_says_so() {
    let env = Env::new();
    let path = env.dir.join("config/bullpen/config.json").display().to_string();
    env.global("[1]");
    assert_eq!(
        read_global_file().unwrap_err().0,
        format!("{path} isn't a JSON object ({{...}}): fix it, or move it away")
    );
    env.global("{oops");
    let err = read_global_file().unwrap_err().0;
    assert!(
        err.starts_with(&format!("{path} doesn't parse (")) && err.ends_with("): fix it, or move it away"),
        "{err}"
    );
    std::fs::remove_file(&path).unwrap();
    assert!(read_global_file().unwrap().is_empty());
    std::fs::create_dir_all(&path).unwrap();
    assert_eq!(
        read_global_file().unwrap_err().0,
        format!("{path} is a folder, not a file: move it away")
    );
}

#[test]
fn test_old_whys_say_the_stop() {
    let f = |why: Option<&str>| stop_from_why(why);
    let stop = |by: &str, kind: &str| Some(json!({"by": by, "kind": kind}));
    assert_eq!(f(Some("Needs you (permission): Allow?")), stop("worker", "prompt"));
    assert_eq!(f(Some("Reviewer needs you (input)")), stop("reviewer", "prompt"));
    assert_eq!(
        f(Some("Failed (coordinator didn't resolve it)")),
        stop("worker", "failed")
    );
    assert_eq!(f(Some("Reviewer exited without a verdict")), stop("reviewer", "exited"));
    assert_eq!(f(Some("Its pane was gone")), stop("worker", "pane_gone"));
    assert_eq!(
        f(Some("Couldn't be continued after the usage limit: x")),
        stop("worker", "limit")
    );
    assert_eq!(f(Some("Something else")), stop("worker", "other"));
    assert_eq!(f(None), None);
    assert_eq!(f(Some("")), None);
}
