use super::*;
use serde_json::json;

/// A fixture (a trimmed copy of one of this repository's transcripts) read whole.
fn fixture(name: &str) -> Chat {
    let path = format!("{}/src/chat/tests/fixtures/{name}.jsonl", env!("CARGO_MANIFEST_DIR"));
    let mut chat = Chat::new();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        chat.line(line);
    }
    chat
}

fn kinds(chat: &Chat) -> Vec<String> {
    chat.items()
        .iter()
        .map(|i| i.json()["kind"].as_str().unwrap().to_string())
        .collect()
}

fn of_kind(chat: &Chat, kind: &str) -> Vec<Value> {
    chat.items()
        .iter()
        .map(|i| i.json())
        .filter(|j| j["kind"] == kind)
        .collect()
}

fn feed(chat: &mut Chat, records: &[Value]) {
    for r in records {
        chat.line(&r.to_string());
    }
}

fn user(uuid: &str, text: &str) -> Value {
    json!({"type": "user", "uuid": uuid, "timestamp": "2026-11-24T05:54:30.000Z", "message": {"content": text}})
}

fn call(uuid: &str, msg: &str, id: &str, name: &str, input: Value, at: &str) -> Value {
    json!({"type": "assistant", "uuid": uuid, "timestamp": at, "cwd": "/w",
        "message": {"id": msg, "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}})
}

fn text(uuid: &str, msg: &str, words: &str) -> Value {
    json!({"type": "assistant", "uuid": uuid, "timestamp": "2026-11-24T05:55:00.000Z",
        "message": {"id": msg, "content": [{"type": "text", "text": words}]}})
}

fn result(uuid: &str, id: &str, error: bool, content: &str, at: &str) -> Value {
    json!({"type": "user", "uuid": uuid, "timestamp": at,
        "message": {"content": [{"type": "tool_result", "tool_use_id": id, "is_error": error, "content": content}]}})
}

#[test]
fn a_workers_transcript_bp_230() {
    let chat = fixture("bp230-worker");
    assert_eq!(
        chat.title.as_deref(),
        Some("BP-230: The run queue keeps going when a news line fails")
    );
    assert_eq!(chat.mode.as_deref(), Some("auto"));
    let k = kinds(&chat);
    assert_eq!(k[..4], ["brief", "tools", "said", "tools"]);
    assert_eq!(k.last().unwrap(), "worked");
    assert_eq!(k.iter().filter(|k| *k == "said").count(), 7);
    let first = chat.items()[0].json();
    assert_eq!(first["file"], "BP-230.full.md");
    let tools = of_kind(&chat, "tools");
    assert_eq!(tools[0]["summary"], "Ran 6 commands");
    assert_eq!(tools[0]["ms"], 15106);
    assert_eq!(tools[0]["doing"], Value::Null); // every call finished
    let calls = tools[0]["calls"].as_array().unwrap();
    assert_eq!(
        calls[0]["what"],
        "cat /home/me/.local/state/bullpen/repos/home-me-dev-bullpen/briefs/BP-230.full.md"
    );
    assert!(calls.iter().all(|c| c["state"] == "done"));
    // an Edit that failed: its error without Claude Code's tags, a path from the worktree
    let edit = tools[3]["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Edit")
        .unwrap()
        .clone();
    assert_eq!(edit["what"], "tests/test_queue.py");
    assert_eq!(edit["state"], "failed");
    assert!(
        edit["error"]
            .as_str()
            .unwrap()
            .starts_with("String to replace not found"),
        "{edit}"
    );
    assert_eq!(tools[3]["summary"], "Edited 1 file, ran 3 commands");
    let worked = of_kind(&chat, "worked");
    assert_eq!(worked[0]["text"], "Worked for 4 min 38 s");
}

#[test]
fn a_reviewers_transcript_bp_230() {
    let chat = fixture("bp230-review");
    assert_eq!(chat.title.as_deref(), Some("BP-230 review"));
    assert_eq!(kinds(&chat), ["brief", "tools", "said", "tools"]);
    assert_eq!(chat.items()[0].json()["file"], "BP-230.review-1.md");
    let tools = of_kind(&chat, "tools");
    assert_eq!(tools[0]["summary"], "Ran 5 commands");
}

#[test]
fn a_compacted_workers_transcript_bp_232() {
    let chat = fixture("bp232-worker");
    let k = kinds(&chat);
    assert_eq!(k[0], "brief");
    assert_eq!(k.last().unwrap(), "compacted"); // the summary after it is the model's, not the chat's
    assert_eq!(k.iter().filter(|k| *k == "worked").count(), 2);
    let worked = of_kind(&chat, "worked");
    assert_eq!(worked[0]["text"], "Worked for 1 h 29 min");
    // no image notes, no queued notifications, no compaction summary
    assert!(!k.contains(&"queued".to_string()));
    let all = chat.items().iter().map(|i| i.json().to_string()).collect::<String>();
    assert!(!all.contains("[Image: original"));
    assert!(!all.contains("This session is being continued"));
    // the hand-back after the review is typed text with no mark: a bubble of its own
    let you = of_kind(&chat, "you");
    assert_eq!(you.len(), 1);
    assert!(you[0]["text"]
        .as_str()
        .unwrap()
        .starts_with("The reviewer asked for changes"));
    let compacted = of_kind(&chat, "compacted");
    assert_eq!(compacted[0]["text"], "Conversation compacted");
}

#[test]
fn a_coordinators_transcript_with_news_lines() {
    let chat = fixture("coordinator");
    let news: Vec<(String, String)> = of_kind(&chat, "news")
        .iter()
        .map(|n| {
            (
                n["text"].as_str().unwrap().into(),
                n["task"].as_str().unwrap_or("").into(),
            )
        })
        .collect();
    // by monitor, by the Stop hook (two in one), typed, taken in mid-turn
    let want = [
        (
            "BP-35 is ready (1 commit, local branch), approved by full review: Help and docs cleanup",
            "BP-35",
        ),
        ("BP-227 accepted (merged into main, rebase)", "BP-227"),
        (
            "BP-2 approved by review and auto-accepted (merged into main, rebase)",
            "BP-2",
        ),
        ("BP-3 started (BP-2 was accepted)", "BP-3"),
        (
            "BP-4 approved by review with 2 notes and auto-accepted (merged into main, rebase)",
            "BP-4",
        ),
        ("BP-232 accepted (merged into main, rebase)", "BP-232"),
    ];
    for (text, task) in want {
        assert!(news.contains(&(text.into(), task.into())), "{text} in {news:#?}");
    }
    assert_eq!(news.len(), 9);
    assert!(news[2].0.starts_with("After BP-227: BP-230 rebased"));
    // the person's messages; no queued bubble outlives its message
    let you: Vec<Value> = of_kind(&chat, "you").iter().map(|y| y["text"].clone()).collect();
    assert_eq!(you.len(), 3);
    assert_eq!(you[2], "yes, start the design pass");
    assert!(!kinds(&chat).contains(&"queued".to_string()));
    // a call still running at the end of the file: its line says what it does
    let tools = of_kind(&chat, "tools");
    let last = tools.last().unwrap();
    assert_eq!(last["doing"], "Read BP-233's summary and list its mockups");
    assert_eq!(last["summary"], "Read 1 file, used 1 tool, ran 1 command");
}

#[test]
fn an_ask_user_question_is_a_question_item_answered_by_its_result() {
    let chat = fixture("coordinator-ask");
    assert_eq!(kinds(&chat), ["you", "question", "tools"]);
    let q = &of_kind(&chat, "question")[0];
    assert_eq!(q["questions"][0]["header"], "Where");
    assert_eq!(q["questions"][0]["multiSelect"], false);
    assert!(q["questions"][0]["options"].as_array().unwrap().len() >= 2);
    let answers = q["answers"].as_object().unwrap();
    assert_eq!(answers.len(), 1);
    assert!(answers
        .values()
        .next()
        .unwrap()
        .as_str()
        .unwrap()
        .starts_with("I want this to be autonomous"));
    assert_eq!(q["error"], Value::Null);
}

#[test]
fn a_question_with_no_result_yet_is_open_and_one_declined_says_why() {
    let mut chat = Chat::new();
    let input = json!({"questions": [{"question": "Which?", "header": "Pick", "multiSelect": false,
        "options": [{"label": "A", "description": "a"}, {"label": "B", "description": "b"}]}]});
    feed(
        &mut chat,
        &[call("a1", "m1", "q1", "AskUserQuestion", input, "2026-11-24T05:55:00Z")],
    );
    let q = chat.items()[0].json();
    assert_eq!((q["kind"].as_str(), q["id"].as_str()), (Some("question"), Some("q1")));
    assert_eq!(q["answers"], Value::Null);
    feed(
        &mut chat,
        &[result(
            "u1",
            "q1",
            true,
            "The user doesn't want to proceed",
            "2026-11-24T05:56:00Z",
        )],
    );
    let q = chat.items()[0].json();
    assert_eq!(q["error"], "The user doesn't want to proceed");
}

#[test]
fn a_run_of_calls_across_messages_is_one_line_until_the_agent_says_something() {
    let mut chat = Chat::new();
    feed(
        &mut chat,
        &[
            call(
                "a1",
                "m1",
                "t1",
                "Bash",
                json!({"command": "ls\nmore", "description": ""}),
                "2026-11-24T05:55:00.000Z",
            ),
            call(
                "a2",
                "m1",
                "t2",
                "Read",
                json!({"file_path": "/w/cli/src/main.rs"}),
                "2026-11-24T05:55:01.000Z",
            ),
            result("u1", "t1", false, "a\nb", "2026-11-24T05:55:02.000Z"),
            result("u2", "t2", false, "fn main", "2026-11-24T05:55:03.000Z"),
            json!({"type": "assistant", "uuid": "a3", "message": {"id": "m2", "content": [{"type": "thinking", "thinking": ""}]}}),
            call(
                "a4",
                "m2",
                "t3",
                "Bash",
                json!({"command": "false", "description": "Fail on purpose"}),
                "2026-11-24T05:55:04.000Z",
            ),
        ],
    );
    let line = chat.items()[0].json();
    assert_eq!(chat.items().len(), 1);
    assert_eq!(line["summary"], "Ran 2 commands, read 1 file");
    assert_eq!(line["doing"], "Fail on purpose");
    assert_eq!(line["calls"][0]["what"], "ls");
    assert_eq!(line["calls"][0]["doing"], "Running ls");
    assert_eq!(line["calls"][1]["what"], "cli/src/main.rs");
    let out = json!({"type": "user", "uuid": "u3", "timestamp": "2026-11-24T05:55:10.500Z",
        "message": {"content": [{"type": "tool_result", "tool_use_id": "t3", "is_error": true, "content": "Exit code 1\nboom"}]},
        "toolUseResult": {"stdout": "line 1\nline 2", "stderr": "boom"}});
    feed(
        &mut chat,
        &[
            out,
            text("a5", "m3", "It failed."),
            call("a6", "m3", "t4", "Grep", json!({"pattern": "fn x"}), "T"),
        ],
    );
    let line = chat.items()[0].json();
    assert_eq!(line["ms"], 10500); // the first call to the last result
    assert_eq!(line["doing"], Value::Null);
    assert_eq!(line["calls"][2]["state"], "failed");
    assert_eq!(line["calls"][2]["error"], "Exit code 1\nboom");
    assert_eq!(line["calls"][2]["tail"], "line 1\nline 2\nboom");
    assert_eq!(kinds(&chat), ["tools", "said", "tools"]);
    assert_eq!(chat.items()[2].json()["calls"][0]["what"], "fn x");
}

#[test]
fn an_api_messages_text_blocks_are_one_item_and_a_record_seen_twice_adds_nothing() {
    let mut chat = Chat::new();
    feed(
        &mut chat,
        &[
            text("a1", "m1", "First."),
            text("a2", "m1", "Second."),
            text("a3", "m2", "Third."),
        ],
    );
    let said: Vec<Value> = chat.items().iter().map(|i| i.json()["text"].clone()).collect();
    assert_eq!(said, [json!("First.\n\nSecond."), json!("Third.")]);
    feed(&mut chat, &[user("u1", "again"), user("u1", "again")]);
    assert_eq!(kinds(&chat), ["said", "said", "you"]);
}

#[test]
fn a_result_for_a_call_that_isnt_in_the_chat_is_dropped() {
    let mut chat = Chat::new();
    feed(&mut chat, &[result("u1", "toolu_gone", false, "x", "T")]);
    assert!(chat.items().is_empty());
}

#[test]
fn typed_text_is_the_brief_your_bubble_the_coordinators_or_news() {
    let mut chat = Chat::new();
    feed(
        &mut chat,
        &[
            user("u0", "<local-command-stdout>Restarted</local-command-stdout>"),
            user(
                "u1",
                "You are the worker for bullpen task BP-3. Read your brief at /s/briefs/BP-3.full.md and follow it.",
            ),
            user("u2", "You are the worker, said again"),
            user("u3", "Coordinator: rebase onto main"),
            user("u4", "bullpen · After BP-4: BP-3 rebased"),
            user(
                "u5",
                "<task-notification>\n<summary>Background command done</summary>\n</task-notification>",
            ),
        ],
    );
    let items: Vec<Value> = chat.items().iter().map(|i| i.json()).collect();
    assert_eq!(kinds(&chat), ["brief", "you", "coordinator", "news"]);
    assert_eq!(items[0]["file"], "BP-3.full.md");
    assert_eq!(items[2]["text"], "rebase onto main");
    assert_eq!(
        (items[3]["text"].as_str(), items[3]["task"].as_str()),
        (Some("After BP-4: BP-3 rebased"), Some("BP-4"))
    );
}

#[test]
fn pasted_text_folds_to_its_line_count() {
    let text = "What do you think?\n\n<pasted_content id=\"d536\">\none\ntwo\nthree\n</pasted_content>\nThanks";
    let want = json!([{"text": "What do you think?"}, {"pasted": "one\ntwo\nthree", "lines": 3}, {"text": "Thanks"}]);
    assert_eq!(Value::Array(parts(text)), want);
    assert_eq!(Value::Array(parts("plain")), json!([{"text": "plain"}]));
}

#[test]
fn a_message_typed_mid_turn_is_queued_until_it_is_taken_in() {
    let mut chat = Chat::new();
    feed(
        &mut chat,
        &[
            call(
                "a1",
                "m1",
                "t1",
                "Bash",
                json!({"command": "sleep 9"}),
                "2026-11-24T05:55:00Z",
            ),
            json!({"type": "queue-operation", "operation": "enqueue", "timestamp": "T", "content": "also do X"}),
            json!({"type": "queue-operation", "operation": "enqueue", "timestamp": "T", "content": "<task-notification>x"}),
        ],
    );
    assert_eq!(kinds(&chat), ["tools", "queued"]);
    let queued = chat.items()[1].id.clone();
    chat.take_changed();
    // the agent's line goes on past it
    feed(
        &mut chat,
        &[call(
            "a2",
            "m2",
            "t2",
            "Bash",
            json!({"command": "ls"}),
            "2026-11-24T05:55:09Z",
        )],
    );
    assert_eq!(chat.items()[0].json()["calls"].as_array().unwrap().len(), 2);
    // taken in mid-turn: an attachment brings what was typed
    let taken = json!({"type": "queue-operation", "operation": "remove", "content": "also do X"});
    let absorbed = json!({"type": "attachment", "uuid": "t9", "timestamp": "T",
        "attachment": {"type": "queued_command", "prompt": "also do X"}});
    feed(&mut chat, &[taken, absorbed]);
    let changed: Vec<Value> = chat.take_changed().iter().map(Item::json).collect();
    assert_eq!(changed[1]["id"].as_str(), Some(queued.as_str()));
    assert_eq!(changed[1]["gone"], true);
    assert_eq!(changed[2]["kind"], "you");
    assert_eq!(kinds(&chat), ["tools", "you"]);
}

#[test]
fn changed_items_are_the_new_ones_and_those_a_result_completed() {
    let mut chat = Chat::new();
    feed(
        &mut chat,
        &[
            user("u1", "go"),
            call("a1", "m1", "t1", "Bash", json!({"command": "ls"}), "T"),
        ],
    );
    assert_eq!(chat.take_changed().len(), 2);
    assert!(chat.take_changed().is_empty());
    feed(&mut chat, &[result("u2", "t1", false, "ok", "T")]);
    let changed = chat.take_changed();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].id, "t1");
    assert_eq!(changed[0].json()["calls"][0]["state"], "done");
}

#[test]
fn an_error_and_the_dividers() {
    let mut chat = Chat::new();
    feed(
        &mut chat,
        &[
            json!({"type": "system", "subtype": "api_error", "level": "error", "content": "API Error: 529", "uuid": "s1"}),
            json!({"type": "system", "subtype": "turn_duration", "durationMs": 26801, "uuid": "s2"}),
            json!({"type": "system", "subtype": "compact_boundary", "uuid": "s3"}),
        ],
    );
    let items: Vec<Value> = chat.items().iter().map(|i| i.json()).collect();
    assert_eq!(items[0]["text"], "API Error: 529");
    assert_eq!(items[1]["text"], "Worked for 26 s");
    assert_eq!(items[2]["kind"], "compacted");
}

#[test]
fn lengths_of_time_in_words() {
    assert_eq!(span(800), "0 s");
    assert_eq!(span(26_801), "26 s");
    assert_eq!(span(278_141), "4 min 38 s");
    assert_eq!(span(600_000), "10 min");
    assert_eq!(span(659_000), "10 min");
    assert_eq!(span(3_600_000), "1 h");
    assert_eq!(span(5_354_954), "1 h 29 min");
}

#[test]
fn a_tool_line_in_words() {
    let c = |name: &str| ToolCall {
        id: "x".into(),
        name: name.into(),
        what: String::new(),
        doing: String::new(),
        state: State::Done,
        error: None,
        tail: None,
    };
    let calls: Vec<ToolCall> = ["Bash", "Read", "Bash", "Edit", "Agent", "Monitor", "Read"]
        .map(c)
        .into();
    assert_eq!(
        summary(&calls),
        "Ran 2 commands, read 2 files, edited 1 file, ran 1 agent, used 1 tool"
    );
    assert_eq!(summary(&calls[..1]), "Ran 1 command");
}

#[test]
fn the_task_a_news_line_names() {
    assert_eq!(task_named("BP-233 added L26 to the backlog"), Some("BP-233".into()));
    assert_eq!(task_named("After BP-227: BP-230 rebased"), Some("BP-227".into()));
    assert_eq!(task_named("L26 added, nothing else"), None);
    assert_eq!(task_named("a non-task-1 word"), None);
}
