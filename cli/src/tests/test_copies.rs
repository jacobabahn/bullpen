use super::*;
use serde_json::json;

fn coordinator(v: serde_json::Value) -> Obj {
    v.as_object().cloned().unwrap_or_default()
}

#[test]
fn a_coordinator_without_a_recorded_copy_counts_as_ours() {
    assert_eq!(copy_of(&coordinator(json!({"id": "c1"}))), None);
    assert!(ours(&coordinator(json!({"id": "c1", "bullpen": ""}))));
    let c = coordinator(json!({"id": "c1", "bullpen": "/nowhere/bullpen"}));
    assert_eq!(copy_of(&c), Some(PathBuf::from("/nowhere/bullpen")));
    assert!(!ours(&c));
    assert!(!there(Path::new("/nowhere/bullpen")));
}

#[test]
fn a_recorded_copy_is_this_one() {
    let mut c = coordinator(json!({"id": "c1"}));
    record(&mut c);
    assert!(ours(&c));
    assert!(same(&copy_of(&c).unwrap(), &home()));
}

#[test]
fn which_is_older_cant_be_told_without_git() {
    let dir = std::env::temp_dir();
    assert_eq!(age(Path::new("/nowhere/a"), &dir).0, "different from");
    assert_eq!(commits(1), "1 commit");
    assert_eq!(commits(10), "10 commits");
}
