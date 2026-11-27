use super::*;
use crate::testenv::Env;
use serde_json::{json, Value};
use std::io::Write as _;

fn fixture(name: &str) -> String {
    let path = format!("{}/src/chat/tests/fixtures/{name}.jsonl", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

/// A transcript file with these lines, in a project folder, made `age` seconds old.
fn transcript(dir: &Path, id: &str, title: &str, age: u64) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(format!("{id}.jsonl"));
    let head = json!({"type": "custom-title", "customTitle": title, "sessionId": id});
    std::fs::write(
        &path,
        format!("{head}\n{}\n", json!({"type": "mode", "mode": "normal"})),
    )
    .unwrap();
    let when = SystemTime::now() - std::time::Duration::from_secs(age);
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(when)
        .unwrap();
    path
}

fn append(path: &Path, text: &str) {
    File::options()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

#[test]
fn a_working_folders_transcripts_are_in_a_folder_named_for_its_path() {
    let p = Path::new("/p");
    assert_eq!(
        folder(p, Path::new("/home/me/dev/bullpen-bullpen/BP-232")),
        p.join("-home-me-dev-bullpen-bullpen-BP-232")
    );
    assert_eq!(
        folder(p, Path::new("/home/me/.local/x_y")),
        p.join("-home-me--local-x-y")
    );
}

#[test]
fn claude_codes_projects_folder() {
    let env = Env::new();
    let was = std::env::var_os("CLAUDE_CONFIG_DIR");
    std::env::remove_var("CLAUDE_CONFIG_DIR");
    assert_eq!(projects(), env.dir.join("home/.claude/projects"));
    std::env::set_var("CLAUDE_CONFIG_DIR", env.dir.join("claude"));
    assert_eq!(projects(), env.dir.join("claude/projects"));
    match was {
        Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
        None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
    }
}

#[test]
fn the_session_id_recorded_for_a_worker_and_its_reviewer() {
    let t = json!({"id": "BP-3", "chat": "w-id", "review": {"chat": "r-id"}});
    let t = t.as_object().unwrap();
    assert_eq!(recorded(t, false), "w-id");
    assert_eq!(recorded(t, true), "r-id");
    assert_eq!(recorded(json!({"id": "BP-3"}).as_object().unwrap(), true), "");
}

#[test]
fn a_recorded_session_is_found_in_any_project_folder_else_the_newest_titled_one() {
    let env = Env::new();
    let projects = env.dir.join("projects");
    let cwd = Path::new("/w/BP-3");
    let here = folder(&projects, cwd);
    let worker = transcript(
        &here,
        "0a1b2c3d-0000-4000-8000-000000000001",
        "BP-3: Fix the queue",
        300,
    );
    let newer = transcript(
        &here,
        "0a1b2c3d-0000-4000-8000-000000000002",
        "BP-3: Fix the queue (renamed)",
        100,
    );
    let review = transcript(&here, "0a1b2c3d-0000-4000-8000-000000000003", "BP-3 review", 10);
    let elsewhere = transcript(&projects.join("-other"), "0a1b2c3d-0000-4000-8000-000000000004", "x", 0);
    let w = Agent::Worker("BP-3".into());
    // the newest of the worker's two, never the reviewer's (newer still)
    assert_eq!(find(&projects, "", cwd, &w), Some((newer.clone(), Source::Title)));
    std::fs::remove_file(&newer).unwrap();
    assert_eq!(find(&projects, "", cwd, &w), Some((worker, Source::Title)));
    let r = Agent::Reviewer("BP-3".into());
    assert_eq!(find(&projects, "", cwd, &r), Some((review.clone(), Source::Title)));
    // recorded: wherever its file is
    let id = "0a1b2c3d-0000-4000-8000-000000000004";
    assert_eq!(find(&projects, id, cwd, &w), Some((elsewhere, Source::Recorded)));
    // a recorded ID with no file yet, or that isn't an ID: by title
    assert_eq!(
        find(&projects, "0a1b2c3d-dead", cwd, &r),
        Some((review.clone(), Source::Title))
    );
    assert_eq!(find(&projects, "../-other/x", cwd, &r), Some((review, Source::Title)));
    assert_eq!(find(&projects, "", cwd, &Agent::Worker("BP-30".into())), None);
    assert_eq!(find(&projects, "", Path::new("/nowhere"), &w), None);
}

#[test]
fn a_coordinators_name_and_a_transcripts_title() {
    let c = Agent::Coordinator("bullpen".into());
    assert!(c.named("bullpen: bullpen"));
    assert!(!c.named("bullpen: bullpen board"));
    assert!(Agent::Worker("BP-3".into()).named("BP-3: Anything"));
    assert!(!Agent::Worker("BP-3".into()).named("BP-30: Anything"));
    assert!(!Agent::Worker("BP-3".into()).named("BP-3 review"));
    let env = Env::new();
    let path = env.dir.join("t.jsonl");
    std::fs::write(&path, fixture("bp230-review")).unwrap();
    assert_eq!(title(&path).as_deref(), Some("BP-230 review"));
    std::fs::write(&path, "{\"type\":\"mode\"}\n").unwrap();
    assert_eq!(title(&path), None);
}

#[test]
fn a_half_written_last_line_waits_for_the_next_read() {
    let env = Env::new();
    let path = env.dir.join("s.jsonl");
    let whole = fixture("bp230-review");
    let cut = whole.len() - 40; // inside the last line
    std::fs::write(&path, &whole[..cut]).unwrap();
    let (lines, offset) = read_from(&path, 0).unwrap();
    assert_eq!(lines.len(), whole.lines().count() - 1);
    assert_eq!(offset as usize, whole[..cut].rfind('\n').unwrap() + 1);
    assert_eq!(read_from(&path, offset).unwrap(), (vec![], offset));
    append(&path, &whole[cut..]);
    let (lines, end) = read_from(&path, offset).unwrap();
    assert_eq!(lines, [whole.lines().last().unwrap()]);
    assert_eq!(end as usize, whole.len());
}

#[test]
fn a_file_that_grows_between_two_reads_sends_the_new_items_and_those_changed() {
    let env = Env::new();
    let path = env.dir.join("ask.jsonl");
    let whole = fixture("coordinator-ask");
    let lines: Vec<&str> = whole.lines().collect();
    let answer = lines.iter().position(|l| l.contains("The user answered")).unwrap();
    std::fs::write(&path, lines[..answer].join("\n") + "\n").unwrap();
    let mut t = Transcript::new(path.clone(), Source::Recorded);
    let first = t.read().unwrap();
    let kinds: Vec<Value> = first.iter().map(|i| i.json()["kind"].clone()).collect();
    assert_eq!(kinds, [json!("you"), json!("question")]);
    assert_eq!(first[1].json()["answers"], Value::Null); // asked, not answered yet
    assert!(t.read().unwrap().is_empty()); // nothing new
    append(&path, &(lines[answer..].join("\n") + "\n"));
    let second = t.read().unwrap();
    assert_eq!(second[0].id, first[1].id); // the question, answered
    assert!(second[0].json()["answers"].is_object());
    assert_eq!(second[1].json()["kind"], "tools");
    assert_eq!(t.offset as usize, whole.len());
    assert_eq!(t.chat.items().len(), 3);
}

#[test]
fn a_file_written_anew_is_read_from_its_start() {
    let env = Env::new();
    let path = env.dir.join("s.jsonl");
    std::fs::write(&path, fixture("bp230-review")).unwrap();
    let mut t = Transcript::new(path.clone(), Source::Title);
    assert_eq!(t.read().unwrap().len(), 4);
    let user = json!({"type": "user", "uuid": "n1", "message": {"content": "hello"}});
    std::fs::write(&path, format!("{user}\n")).unwrap();
    let items = t.read().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(t.chat.items().len(), 1);
}
