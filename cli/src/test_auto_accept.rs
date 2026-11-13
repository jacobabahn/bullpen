use super::*;
use serde_json::json;

fn pr(v: Value) -> Obj {
    v.as_object().cloned().unwrap()
}

#[test]
fn no_checks_is_no_summary() {
    assert_eq!(checks(&pr(json!({}))), (None, vec![]));
    assert_eq!(checks(&pr(json!({"statusCheckRollup": null}))), (None, vec![]));
}

#[test]
fn a_failed_check_is_failing_and_named() {
    let rollup = json!({"statusCheckRollup": [
        {"name": "test", "conclusion": "FAILURE", "status": "COMPLETED"},
        {"context": "ci/lint", "state": "ERROR"},
        {"conclusion": "TIMED_OUT"},
        {"name": "build", "status": "IN_PROGRESS"},
    ]});
    let failed: Vec<String> = ["test", "ci/lint", "check"].map(String::from).into();
    assert_eq!(checks(&pr(rollup)), (Some("failing"), failed));
}

#[test]
fn an_unfinished_check_is_running() {
    let running =
        json!({"statusCheckRollup": [{"name": "a", "conclusion": "SUCCESS"}, {"name": "b", "status": "QUEUED"}]});
    assert_eq!(checks(&pr(running)).0, Some("running"));
    let pending = json!({"statusCheckRollup": [{"context": "ci", "state": "PENDING"}]});
    assert_eq!(checks(&pr(pending)).0, Some("running"));
    let expected = json!({"statusCheckRollup": [{"context": "ci", "state": "EXPECTED"}]});
    assert_eq!(checks(&pr(expected)).0, Some("running"));
}

#[test]
fn finished_checks_that_didnt_fail_are_passing() {
    let done = json!({"statusCheckRollup": [{"name": "a", "conclusion": "SUCCESS", "status": "COMPLETED"},
                                            {"name": "b", "conclusion": "SKIPPED"}, {"context": "c", "state": "SUCCESS"}]});
    assert_eq!(checks(&pr(done)), (Some("passing"), vec![]));
}
