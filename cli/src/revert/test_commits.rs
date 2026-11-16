use super::*;
use serde_json::json;

fn task(v: Value) -> Obj {
    let mut t = v.as_object().cloned().unwrap();
    t.insert("id".into(), json!("BP-3"));
    t
}

#[test]
fn a_recorded_merge_method_says_the_shape() {
    let rebase = task(json!({"merged_how": "rebase", "merged_from": "abc", "commits": 3}));
    assert_eq!(shape(&rebase, 1), Ok(Shape::Since("abc".into())));
    // a PR: what was merged, not what the last `ready` counted
    let pr = task(json!({"merged_how": "rebase", "commits": 3, "merged_commits": 4}));
    assert_eq!(shape(&pr, 1), Ok(Shape::Last(4)));
    assert_eq!(
        shape(&task(json!({"merged_how": "squash", "commits": 3})), 1),
        Ok(Shape::One)
    );
    assert_eq!(
        shape(&task(json!({"merged_how": "merge", "commits": 3})), 2),
        Ok(Shape::Merge)
    );
}

#[test]
fn a_record_that_doesnt_fit_its_commit_is_refused() {
    let said = shape(&task(json!({"merged_how": "merge", "commits": 1})), 1).unwrap_err();
    assert!(
        said.contains("merged by a merge commit") && said.contains("1 parent,"),
        "{said}"
    );
    let said = shape(&task(json!({"merged_how": "squash"})), 2).unwrap_err();
    assert!(said.contains("2 parents, not 1"), "{said}");
    let said = shape(&task(json!({"merged_how": "rebase", "commits": 3})), 1).unwrap_err();
    assert!(said.contains("how many commits its PR had"), "{said}");
}

#[test]
fn an_older_record_gives_only_what_is_certain() {
    assert_eq!(shape(&task(json!({"commits": 4})), 2), Ok(Shape::Merge));
    assert_eq!(shape(&task(json!({"commits": 1})), 1), Ok(Shape::One));
    let grew = shape(&task(json!({"commits": 1, "merged_commits": 2})), 1).unwrap_err();
    assert!(grew.contains("2 commits"), "{grew}");
    let said = shape(&task(json!({"commits": 4})), 1).unwrap_err();
    assert!(
        said.contains("doesn't say how it was merged") && said.contains("4 commits"),
        "{said}"
    );
    let said = shape(&task(json!({})), 1).unwrap_err();
    assert!(said.contains("an unknown number of commits"), "{said}");
}
