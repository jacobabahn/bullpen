use super::*;
use serde_json::json;

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn a_new_id_is_a_version_4_uuid() {
    let id = new_id();
    let parts: Vec<&str> = id.split('-').collect();
    assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12]);
    assert!(id
        .chars()
        .all(|c| c == '-' || c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert!(parts[2].starts_with('4'));
    assert!("89ab".contains(&parts[3][..1]));
    assert_ne!(new_id(), id);
}

#[test]
fn only_claude_code_is_started_with_a_session_id() {
    let (id, flags) = start(&strs(&["/usr/bin/claude", "--permission-mode", "auto"])).unwrap();
    assert_eq!(flags, ["--session-id".to_string(), id]);
    assert!(start(&strs(&["codex"])).is_none());
}

#[test]
fn resuming_takes_the_workers_own_conversation() {
    let claude = strs(&["claude"]);
    let t = json!({"chat": "abc"}).as_object().unwrap().clone();
    assert_eq!(resume(&claude, &t), (Some("abc".into()), strs(&["--resume", "abc"])));
    // none recorded, or not Claude Code: the newest in the worktree, as before
    assert_eq!(resume(&claude, &Obj::new()), (None, strs(&["--continue"])));
    assert_eq!(resume(&strs(&["codex"]), &t), (None, strs(&["--continue"])));
}
