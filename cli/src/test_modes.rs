use super::*;
use serde_json::json;

fn task(v: serde_json::Value) -> Obj {
    v.as_object().cloned().unwrap()
}

#[test]
fn a_published_task_goes_by_its_record() {
    assert_eq!(recorded_remote(&task(json!({"local": true}))), Some(false));
    assert_eq!(recorded_remote(&task(json!({"local": false}))), Some(true));
    // the record wins over a PR
    assert_eq!(recorded_remote(&task(json!({"local": true, "pr": "u"}))), Some(false));
}

#[test]
fn a_pr_from_before_the_record_is_remote_mode() {
    assert_eq!(
        recorded_remote(&task(json!({"pr": "https://github.com/me/p/pull/1"}))),
        Some(true)
    );
    assert_eq!(recorded_remote(&task(json!({"local": null, "pr": "u"}))), Some(true));
}

#[test]
fn an_unpublished_task_goes_by_the_repo() {
    assert_eq!(recorded_remote(&task(json!({}))), None);
    assert_eq!(recorded_remote(&task(json!({"local": null, "pr": null}))), None);
}

#[test]
fn what_a_failed_git_said_is_one_line() {
    let p = Done {
        code: 1,
        stdout: "out\n".into(),
        stderr: "fatal:  no\n  such remote\n".into(),
    };
    assert_eq!(failed(&p), "fatal: no such remote");
    let p = Done {
        code: 1,
        stdout: " only\nout ".into(),
        stderr: String::new(),
    };
    assert_eq!(failed(&p), "only out");
}
