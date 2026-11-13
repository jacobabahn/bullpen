use super::*;
use crate::model::testing::*;

#[test]
fn a_field_of_the_wrong_shape_reads_as_missing() {
    let t: Task = serde_json::from_str(r#"{"id":"BP-1","status":"ready","merged":true,"gh":"?","diffstat":3}"#).unwrap();
    assert_eq!((t.merged, t.diffstat, t.gh.is_none()), (None, None, true));
}

#[test]
fn a_task_that_needs_you_says_what_i_answers() {
    let stop = |kind: &str| Some(Stop { by: "worker".into(), kind: kind.into() });
    let t = |status: &str, ask: Option<bool>, kind: &str| Task { ask, stop: stop(kind), ..task("BP-1", status) };
    assert_eq!(t("needs", None, "prompt").question(), Some(Question::Prompt));
    assert_eq!(t("needs", Some(true), "prompt").question(), Some(Question::Ask)); // the coordinator's first
    assert_eq!(t("needs", Some(true), "ended").question(), Some(Question::Ask));
    assert_eq!(t("needs", None, "ended").question(), None); // an escalated triage: open its pane
    assert_eq!(t("working", None, "prompt").question(), None);
    let read: Task = serde_json::from_str(r#"{"id":"BP-1","status":"needs","ask":true,"stop":{"by":"reviewer","kind":"prompt"}}"#).unwrap();
    assert_eq!((read.question(), read.stop.unwrap().by.as_str()), (Some(Question::Ask), "reviewer"));
}

#[test]
fn a_prompt_is_read_from_what_bullpen_prompt_prints() {
    let out = r#"{"who": "worker", "question": "Do you want to proceed?", "options": [["1", "Yes"], ["2", "No"]], "id": "ab12", "screen": ["a", "b"]}"#;
    let p = Prompt::parse(out).unwrap();
    assert_eq!((p.who.as_str(), p.question.as_deref(), p.id.as_deref()), ("worker", Some("Do you want to proceed?"), Some("ab12")));
    assert_eq!(p.option('2'), Some(("2", "No")));
    assert_eq!(p.option('3'), None);
    let yn = Prompt { options: vec![("y".into(), "Yes".into()), ("n".into(), "No".into())], ..Default::default() };
    assert_eq!(yn.option('Y'), Some(("y", "Yes")));
    let none = Prompt::parse(r#"{"who": "worker", "question": null, "options": [], "id": null, "screen": []}"#).unwrap();
    assert_eq!((none.question, none.options.len()), (None, 0));
    assert_eq!(Prompt::parse("bullpen: BP-1 isn't waiting at a prompt"), None);
}

#[test]
fn stuck_and_limit_of_the_wrong_shape_read_as_missing() {
    let t: Task = serde_json::from_str(r#"{"id":"BP-1","status":"working","stuck":1,"limit":"x"}"#).unwrap();
    assert!(!t.is_stuck() && !t.is_limited());
}
