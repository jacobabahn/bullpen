use super::*;
use bullpen_core::timeutil::parse;
use serde_json::json;

#[test]
fn limit_phrase_says_when_it_resets() {
    let at = parse("2026-05-04T10:00:00+00:00");
    assert_eq!(limit_phrase(None, at), "reset time unknown");
    assert_eq!(limit_phrase(Some(&json!(null)), at), "reset time unknown");
    assert!(limit_phrase(Some(&json!("2026-05-04T15:00:00+00:00")), at).starts_with("resets "));
}
