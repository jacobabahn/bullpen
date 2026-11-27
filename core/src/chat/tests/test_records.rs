use super::*;
use serde_json::json;

fn line(v: Value) -> String {
    v.to_string()
}

#[test]
fn an_unknown_record_type_or_a_line_that_isnt_json_says_nothing() {
    assert_eq!(
        parse(&line(json!({"type": "something-new", "uuid": "u1", "text": "hi"}))),
        vec![]
    );
    assert_eq!(parse(r#"{"type":"user","message":{"content":"half"#), vec![]);
    assert_eq!(parse(""), vec![]);
    assert_eq!(parse("[1, 2]"), vec![]);
    for t in [
        "session-owner",
        "api-request",
        "data-flow-records",
        "cost-state",
        "file-history-snapshot",
    ] {
        assert_eq!(parse(&line(json!({"type": t, "accountUuid": "x"}))), vec![], "{t}");
    }
}

#[test]
fn a_user_record_is_typed_text_a_notice_or_tool_results() {
    let typed = json!({"type": "user", "uuid": "u1", "timestamp": "T", "message": {"content": "hello"}});
    let want = Record::Typed {
        uuid: "u1".into(),
        at: "T".into(),
        text: "hello".into(),
    };
    assert_eq!(parse(&line(typed)), vec![want]);
    let blocks = json!({"type": "user", "uuid": "u2", "timestamp": "T",
        "message": {"content": [{"type": "text", "text": "a"}, {"type": "image"}, {"type": "text", "text": "b"}]}});
    let want = Record::Typed {
        uuid: "u2".into(),
        at: "T".into(),
        text: "a\nb".into(),
    };
    assert_eq!(parse(&line(blocks)), vec![want]);
    let notice = json!({"type": "user", "uuid": "u3", "timestamp": "T",
        "message": {"content": "<task-notification>\n<summary>done</summary>\n</task-notification>"}});
    assert!(matches!(&parse(&line(notice))[..], [Record::Notice { .. }]));
    let result = json!({"type": "user", "uuid": "u4", "timestamp": "T2",
        "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "is_error": true,
            "content": [{"type": "text", "text": "Exit code 1"}]}]},
        "toolUseResult": {"stdout": "out", "stderr": "err"}});
    let want = Outcome {
        at: "T2".into(),
        id: "t1".into(),
        error: true,
        text: "Exit code 1".into(),
        detail: json!({"stdout": "out", "stderr": "err"}),
    };
    assert_eq!(parse(&line(result)), vec![Record::Result(want)]);
}

#[test]
fn meta_notes_the_compaction_summary_and_sidechains_are_hidden() {
    for extra in [
        json!({"isMeta": true}),
        json!({"isCompactSummary": true}),
        json!({"isSidechain": true}),
    ] {
        let mut r = json!({"type": "user", "uuid": "u1", "message": {"content": "[Image: original 576x3275]"}});
        r.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert_eq!(parse(&line(r)), vec![]);
    }
    let side =
        json!({"type": "assistant", "isSidechain": true, "message": {"content": [{"type": "text", "text": "x"}]}});
    assert_eq!(parse(&line(side)), vec![]);
}

#[test]
fn an_assistant_record_is_its_text_and_calls_and_never_its_thinking() {
    let r = json!({"type": "assistant", "uuid": "a1", "timestamp": "T", "cwd": "/w",
        "message": {"id": "msg_1", "content": [
            {"type": "thinking", "thinking": "", "signature": "xyz"},
            {"type": "text", "text": "Reading it."},
            {"type": "text", "text": "  "},
            {"type": "tool_use", "id": "toolu_1", "name": "Read", "input": {"file_path": "/w/a.rs"}}]}});
    let said = Record::Said {
        uuid: "a1".into(),
        at: "T".into(),
        message: "msg_1".into(),
        text: "Reading it.".into(),
    };
    let call = Record::Call(Call {
        uuid: "a1".into(),
        at: "T".into(),
        message: "msg_1".into(),
        id: "toolu_1".into(),
        name: "Read".into(),
        input: json!({"file_path": "/w/a.rs"}),
        cwd: "/w".into(),
    });
    assert_eq!(parse(&line(r)), vec![said, call]);
}

#[test]
fn system_records_say_a_turns_length_a_compaction_or_an_error() {
    let turn =
        json!({"type": "system", "subtype": "turn_duration", "durationMs": 278141, "uuid": "s1", "timestamp": "T"});
    assert_eq!(
        parse(&line(turn)),
        vec![Record::Worked {
            uuid: "s1".into(),
            at: "T".into(),
            ms: 278141
        }]
    );
    let compact = json!({"type": "system", "subtype": "compact_boundary", "uuid": "s2", "timestamp": "T"});
    assert_eq!(
        parse(&line(compact)),
        vec![Record::Compacted {
            uuid: "s2".into(),
            at: "T".into()
        }]
    );
    let error =
        json!({"type": "system", "subtype": "api_error", "level": "error", "content": "Overloaded", "uuid": "s3"});
    assert_eq!(
        parse(&line(error)),
        vec![Record::Error {
            uuid: "s3".into(),
            at: "".into(),
            text: "Overloaded".into()
        }]
    );
    for sub in ["stop_hook_summary", "away_summary", "informational", "local_command"] {
        let r = json!({"type": "system", "subtype": sub, "level": "info", "content": "x"});
        assert_eq!(parse(&line(r)), vec![], "{sub}");
    }
}

#[test]
fn the_queue_and_a_message_taken_in_mid_turn() {
    let q = json!({"type": "queue-operation", "operation": "enqueue", "timestamp": "T", "content": "wait"});
    assert_eq!(
        parse(&line(q)),
        vec![Record::Queued {
            at: "T".into(),
            text: "wait".into()
        }]
    );
    let r = json!({"type": "queue-operation", "operation": "remove", "content": "wait"});
    assert_eq!(parse(&line(r)), vec![Record::Unqueued { text: "wait".into() }]);
    assert_eq!(
        parse(&line(json!({"type": "queue-operation", "operation": "dequeue"}))),
        vec![]
    );
    let taken = json!({"type": "attachment", "uuid": "t1", "timestamp": "T",
        "attachment": {"type": "queued_command", "prompt": "bullpen · BP-3 started", "origin": {"kind": "human"}}});
    let want = Record::Typed {
        uuid: "t1".into(),
        at: "T".into(),
        text: "bullpen · BP-3 started".into(),
    };
    assert_eq!(parse(&line(taken)), vec![want]);
    let other = json!({"type": "attachment", "attachment": {"type": "skill_listing", "content": "x"}});
    assert_eq!(parse(&line(other)), vec![]);
}

#[test]
fn the_title_and_permission_mode_feed_the_header() {
    let t = json!({"type": "custom-title", "customTitle": "BP-3: Fix it", "sessionId": "s"});
    assert_eq!(parse(&line(t)), vec![Record::Title("BP-3: Fix it".into())]);
    let m = json!({"type": "permission-mode", "permissionMode": "auto"});
    assert_eq!(parse(&line(m)), vec![Record::Mode("auto".into())]);
}
