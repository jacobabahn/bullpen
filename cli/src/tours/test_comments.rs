use super::*;

#[test]
fn drafts_are_the_files_comments_or_none() {
    let c = json!({"next": 3, "comments": [{"id": 1}, {"id": 2}]});
    assert_eq!(drafts(c.as_object().unwrap()).len(), 2);
    assert!(drafts(&Obj::new()).is_empty());
}
