use super::*;
use serde_json::json;

fn task(v: serde_json::Value) -> Obj {
    v.as_object().cloned().unwrap()
}

#[test]
fn only_a_ready_task_with_a_change_and_no_word_of_risk_is_unlabelled() {
    assert!(unlabelled(&task(json!({"status": "ready"}))));
    assert!(!unlabelled(&task(json!({"status": "ready", "risk": {"level": "low"}}))));
    assert!(!unlabelled(&task(json!({"status": "ready", "risk": null})))); // said: it has none
    assert!(!unlabelled(&task(
        json!({"status": "ready", "why": "Nothing to merge (no commits)"})
    )));
    assert!(!unlabelled(&task(json!({"status": "review"}))));
    assert!(!unlabelled(&task(json!({"status": "finished"}))));
}

#[test]
fn the_findings_are_the_approving_rounds() {
    let briefs = Path::new("/state/briefs");
    let review = |r: serde_json::Value| task(json!({"id": "BP-3", "review": r}));
    let f = "/state/briefs/BP-3.review-2.findings.md";
    assert_eq!(
        approval_path(
            briefs,
            &review(json!({"verdict": "approved", "kind": "full", "round": 2, "findings": f}))
        ),
        Some(PathBuf::from(f))
    );
    // a rebase check's own findings say nothing of risk: the round it follows does
    let rebase = "/state/briefs/BP-3.review-2.rebase.findings.md";
    assert_eq!(
        approval_path(
            briefs,
            &review(json!({"verdict": "approved", "kind": "rebase", "round": 2, "findings": rebase}))
        ),
        Some(PathBuf::from(f))
    );
    assert_eq!(
        approval_path(
            briefs,
            &review(json!({"verdict": "changes", "round": 2, "findings": f}))
        ),
        None
    );
    assert_eq!(approval_path(briefs, &task(json!({"id": "BP-3"}))), None);
}

#[test]
fn a_brief_names_the_copy_whose_ready_its_worker_runs() {
    let brief = "Then run one of:\n\n    /home/me/wt/bullpen/bin/bullpen ready BP-3 --summary-file <that file>\n    \
                 /home/me/wt/bullpen/bin/bullpen finish BP-3 --summary-file <that file>\n";
    assert_eq!(told_copy(brief, "BP-3"), Some(PathBuf::from("/home/me/wt/bullpen")));
    assert_eq!(told_copy(brief, "BP-30"), None);
    assert_eq!(told_copy("run `bullpen ready BP-3 --summary x`", "BP-3"), None);
    assert_eq!(
        told_copy("    '/home/me/my wt/bin/bullpen' ready BP-3 --summary-file f", "BP-3"),
        Some(PathBuf::from("/home/me/my wt"))
    );
}
