use super::*;

fn task(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bullpen-cli-whose-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_recorded_session_says_whose_turn_it_was() {
    let t = task(json!({"chat": "w1", "review": {"chat": "r1"}}));
    let never = || -> Option<String> { panic!("the session told") };
    assert_eq!(whose(&t, "BP-3", "w1", never), Some(Agent::Worker));
    assert_eq!(whose(&t, "BP-3", "r1", never), Some(Agent::Reviewer));
}

#[test]
fn an_unknown_session_is_told_by_its_title() {
    let t = task(json!({"chat": "w1", "review": {"chat": "r1"}}));
    let named = |n: &str| Some(n.to_string());
    assert_eq!(whose(&t, "BP-3", "r2", || named("BP-3 review")), Some(Agent::Reviewer));
    assert_eq!(whose(&t, "BP-3", "w2", || named("BP-3: Fix it")), Some(Agent::Worker));
    assert_eq!(whose(&t, "BP-3", "x", || named("BP-30: Another")), None);
    assert_eq!(whose(&t, "BP-3", "x", || named("bullpen: repo")), None);
    // no title to read: untold, which writes the turn record as before and moves no chat
    assert_eq!(whose(&t, "BP-3", "x", || None), Some(Agent::Untold));
    assert!(Agent::Untold.writes_record() && Agent::Worker.writes_record() && !Agent::Reviewer.writes_record());
}

#[test]
fn the_title_is_the_transcripts_custom_title() {
    let dir = temp("title");
    let f = dir.join("s.jsonl");
    std::fs::write(
        &f,
        "{\"type\":\"mode\"}\n{\"type\":\"custom-title\",\"customTitle\":\"BP-3 review\",\"sessionId\":\"s\"}\n",
    )
    .unwrap();
    assert_eq!(title(&f).as_deref(), Some("BP-3 review"));
    std::fs::write(&f, "{\"type\":\"mode\"}\n").unwrap();
    assert_eq!(title(&f), None);
    assert_eq!(title(&dir.join("gone.jsonl")), None);
}

#[test]
fn only_the_agent_whose_turn_it_was_has_its_chat_moved() {
    let mut t = task(json!({"chat": "w1", "review": {"chat": "r1"}}));
    assert!(follow(&mut t, Agent::Reviewer, "r2"));
    assert_eq!((s(&t, "chat"), s(obj(&t, "review"), "chat")), ("w1", "r2"));
    assert!(!follow(&mut t, Agent::Reviewer, "r2"));
    assert!(follow(&mut t, Agent::Worker, "w2"));
    assert_eq!(s(&t, "chat"), "w2");
    assert!(!follow(&mut t, Agent::Worker, ""));
    assert!(!follow(&mut t, Agent::Untold, "u"));
    let mut bare = task(json!({}));
    assert!(!follow(&mut bare, Agent::Reviewer, "r"));
}

#[test]
fn settle_moves_the_chat_in_the_tasks_file() {
    let dir = temp("settle");
    std::fs::create_dir_all(dir.join("tasks")).unwrap();
    let path = dir.join("tasks/BP-3.json");
    write_obj(
        &path,
        &task(json!({"id": "BP-3", "chat": "w1", "review": {"chat": "r1"}})),
    )
    .unwrap();
    let transcript = dir.join("r2.jsonl");
    std::fs::write(
        &transcript,
        "{\"type\":\"custom-title\",\"customTitle\":\"BP-3 review\"}\n",
    )
    .unwrap();
    let input = task(json!({"session_id": "r2", "transcript_path": transcript.display().to_string()}));
    assert_eq!(settle(&dir.join("turns/BP-3.json"), &input), Some(Agent::Reviewer));
    let t = read_obj(&path).unwrap().unwrap();
    assert_eq!((s(&t, "chat"), s(obj(&t, "review"), "chat")), ("w1", "r2"));
    // a task there's no file for: untold
    assert_eq!(settle(&dir.join("turns/BP-9.json"), &input), Some(Agent::Untold));
}
