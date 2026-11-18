use super::*;

fn obj(v: Value) -> Obj {
    v.as_object().cloned().unwrap()
}

/// What Claude Code 2.1.326 gave a Stop hook, seen in a scratch session.
fn input(said: &str, jobs: Value) -> Obj {
    obj(json!({
        "session_id": "6047dc6f", "transcript_path": "/x.jsonl", "cwd": "/w",
        "hook_event_name": "Stop", "stop_hook_active": false,
        "last_assistant_message": said, "background_tasks": jobs, "session_crons": [],
    }))
}

#[test]
fn the_record_keeps_the_running_jobs_and_the_end_of_what_it_said() {
    let jobs = json!([
        {"id": "b1", "type": "shell", "status": "running", "description": "Run the tests", "command": "cargo test"},
        {"id": "b2", "type": "shell", "status": "completed", "description": "ls", "command": "ls"},
        {"id": "a1", "type": "subagent", "status": "running", "description": "Look", "agent_type": "general-purpose"},
        {"id": "d1", "type": "dream", "status": "running", "description": "x"},
    ]);
    let rec = record(&input("Waiting for the tests.", jobs));
    assert_eq!(rec["session"], "6047dc6f");
    assert_eq!(
        rec["jobs"],
        json!([{"command": "cargo test", "description": "Run the tests"}])
    );
    assert_eq!(rec["said"], "Waiting for the tests.");
    assert_eq!(jobs_phrase(&rec).as_deref(), Some("cargo test"));
    let long = format!("{}end", "x".repeat(5000));
    let rec = record(&input(&long, json!([])));
    assert_eq!(rec["said"].as_str().unwrap().chars().count(), SAID_MAX);
    assert!(rec["said"].as_str().unwrap().ends_with("end"));
    assert_eq!(jobs_phrase(&rec), None);
    assert_eq!(record(&obj(json!({})))["jobs"], json!([])); // older Claude Code: no jobs said
}

#[test]
fn several_jobs_are_named_by_the_first() {
    let rec = obj(json!({"jobs": [{"command": "npm run `dev`\nx", "description": ""}, {"command": "b"}]}));
    assert_eq!(jobs_phrase(&rec).as_deref(), Some("npm run 'dev' x and 1 more"));
    let rec = obj(json!({"jobs": [{"command": "", "description": "Watch the log"}]}));
    assert_eq!(jobs_phrase(&rec).as_deref(), Some("Watch the log"));
}

#[test]
fn the_question_is_the_last_sentence_asked() {
    assert_eq!(question("Which config file?").as_deref(), Some("Which config file?"));
    let said = "I read both.\n\nThe repo has two. Which config file should I use? I'd pick the first.\n";
    assert_eq!(question(said).as_deref(), Some("Which config file should I use?"));
    assert_eq!(
        question("**Should I `rm -rf` it?**").as_deref(),
        Some("Should I 'rm -rf' it?")
    );
    assert_eq!(question("Done. All tests pass."), None);
    assert_eq!(
        question("Use \"a\"\u{1b}[31m or b?").as_deref(),
        Some("Use 'a' [31m or b?")
    );
    let q = question(&format!("{}?", "why ".repeat(100))).unwrap();
    assert_eq!(q.chars().count(), QUESTION_MAX);
    assert!(!q.contains('\n'));
}

#[test]
fn the_line_says_what_it_waits_on() {
    let t = obj(json!({"id": "BP-5"}));
    let asked = obj(json!({"said": "Two configs here.\nWhich config file?"}));
    assert_eq!(
        ended_line("BP-5", &t, Some(&asked), None, false),
        "BP-5 ended its turn with a question: \"Which config file?\": `bullpen peek BP-5`"
    );
    let quiet = obj(json!({"said": "Stopping here."}));
    assert_eq!(
        ended_line("BP-5", &t, Some(&quiet), None, false),
        "BP-5 ended its turn without `ready`: read it with `bullpen peek BP-5`"
    );
    let mut waiting = t.clone();
    wait_on_job(&mut waiting, Some(&obj(json!({"jobs": [{"command": "cargo test"}]}))));
    assert_eq!(job_what(&waiting).as_deref(), Some("cargo test"));
    assert_eq!(
        ended_line("BP-5", &waiting, None, Some(25), false),
        "BP-5 ended its turn without `ready` 25 min ago, and still waits on its job (cargo test): \
         read it with `bullpen peek BP-5`"
    );
    assert_eq!(
        ended_line("BP-5", &waiting, Some(&asked), None, true),
        "BP-5 waited on its job (cargo test), which has ended, but hasn't taken a turn since: \
         read it with `bullpen peek BP-5`"
    );
    assert!(ended_line("BP-5", &t, None, Some(25), false).contains("a background shell of its own still runs"));
}

#[test]
fn a_wait_keeps_its_start() {
    let mut t = obj(json!({"job": {"what": null, "since": "2026-11-17T09:00:00-06:00"}}));
    wait_on_job(&mut t, None);
    assert_eq!(t["job"]["since"], "2026-11-17T09:00:00-06:00");
    assert_eq!(job_what(&t).as_deref(), Some(""));
}
