use super::*;
use serde_json::json;

#[test]
fn a_prompt_that_only_names_its_kind_says_nothing_more() {
    assert_eq!(prompt_msg("permission", " Permission prompt "), "");
    assert_eq!(prompt_msg("input", "INPUT"), "");
    assert_eq!(prompt_msg("permission", " Allow rm? "), "Allow rm?");
}

#[test]
fn prompt_news_says_who_is_at_what() {
    assert_eq!(
        prompt_news("BP-3", "permission", "Allow rm?"),
        "BP-3 is at a permission prompt: Allow rm?"
    );
    assert_eq!(
        prompt_news("BP-3's reviewer", "question", ""),
        "BP-3's reviewer is at a prompt (question)"
    );
}

#[test]
fn only_the_quiet_agent_is_unstuck() {
    let mut t = json!({"id": "BP-1", "stuck": {"who": "worker", "since": "x"}})
        .as_object()
        .unwrap()
        .clone();
    assert!(!unstick(&mut t, "reviewer"));
    assert!(t.contains_key("stuck"));
    assert!(unstick(&mut t, "worker"));
    assert!(!t.contains_key("stuck"));
    assert!(!unstick(&mut t, "worker"));
}

#[test]
fn a_role_names_its_task_first() {
    assert_eq!(role_task("BP-3 review"), "BP-3");
    assert_eq!(role_task("BP-3"), "BP-3");
    assert_eq!(role_task(""), "");
}
