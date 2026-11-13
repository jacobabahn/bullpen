use super::*;
use crate::model::testing::*;

#[test]
fn keys_fit_the_task() {
    assert_eq!(task_keys(Some(&task("BP-1", "ready")))[0], ("a", "accept"));
    // ⏎ opens the pane, with a question or without: it says so for both
    assert_eq!(task_keys(Some(&task("BP-1", "needs")))[0], ("⏎", "open"));
    let asked = Task { ask: Some(true), ..task("BP-1", "needs") };
    assert_eq!(task_keys(Some(&asked))[..2], [("i", "answer"), ("⏎", "open")]);
    // stopped on its reviewer, ⏎ opens the reviewer, as in review
    let on_reviewer = |st: &str| Task { why: Some("Reviewer ended its turn without a verdict".into()), ..task("BP-1", st) };
    assert_eq!(task_keys(Some(&on_reviewer("needs")))[0], ("⏎", "reviewer"));
    assert_eq!(task_keys(Some(&on_reviewer("triage")))[0], ("⏎", "reviewer"));
    assert_eq!(task_keys(Some(&task("BP-1", "triage")))[0], ("⏎", "open"));
    assert!(task_keys(None).is_empty());
    let wt = |st: &str| Task { worktree: Some("/w".into()), ..task("BP-1", st) };
    assert_eq!(task_keys(Some(&wt("working"))).last(), Some(&("v", "diff")));
    assert!(!task_keys(Some(&task("BP-1", "todo"))).contains(&("v", "diff")));
    assert!(!task_keys(Some(&wt("done"))).contains(&("v", "diff")));
}

fn approved() -> Option<Review> {
    Some(Review { round: Some(1), verdict: Some("approved".into()), kind: None, block: None })
}

fn checks(c: &str) -> Option<Gh> {
    Some(Gh { checks: Some(c.into()), ..Default::default() })
}

#[test]
fn confirm_names_the_merge() {
    let t = Task { pr: Some("https://x/pull/7".into()), review: approved(), ..task("BP-1", "ready") };
    assert_eq!(confirm_text(Action::Accept, &t, &merge("squash")), "Accept and squash-merge?");
    assert!(!confirm_risky(Action::Accept, &t, &merge("squash")));
    assert_eq!(confirm_text(Action::Drop, &t, &merge("rebase")), "Drop it and close PR #7?");
    assert!(confirm_risky(Action::Drop, &t, &merge("rebase")), "a drop is always red");
    let nothing = Task { why: Some("Nothing to merge (no commits)".into()), ..task("BP-1", "ready") };
    assert_eq!(confirm_text(Action::Accept, &nothing, &merge("rebase")), "Accept and clean up?");
    assert!(!confirm_risky(Action::Accept, &nothing, &merge("rebase")), "nothing to review");
    let fin = task("BP-1", "finished");
    assert!(!confirm_risky(Action::Close, &fin, &merge("rebase")));
}

#[test]
fn an_accept_names_its_risks_worst_first() {
    let cfg = merge("rebase");
    let pr = Some("https://x/pull/42".into());
    let t = Task { pr: pr.clone(), gh: checks("failing"), review: changes(2), ..task("BP-8", "ready") };
    assert_eq!(confirm_risks(Action::Accept, &t, &cfg), vec![("Checks failing", "Checks failing"), ("Review asks for changes", "Changes asked")]);
    assert!(confirm_risky(Action::Accept, &t, &cfg));
    assert_eq!(
        confirm_texts(Action::Accept, &t, &cfg),
        vec![
            "Checks failing, review asks for changes. Accept and rebase-merge anyway?",
            "Checks failing. Accept and rebase-merge anyway?",
            "Checks failing. Accept anyway?",
            "Checks failing. Accept?",
            "Checks failing.",
        ]
    );
    let t = Task { pr: pr.clone(), gh: checks("passing"), review: changes(2), ..task("BP-8", "ready") };
    assert_eq!(confirm_text(Action::Accept, &t, &cfg), "Review asks for changes. Accept and rebase-merge anyway?");
    assert_eq!(confirm_texts(Action::Accept, &t, &cfg).last().unwrap(), "Changes asked.");
    let t = Task { pr: pr.clone(), gh: checks("running"), review: approved(), ..task("BP-8", "ready") };
    assert_eq!(confirm_text(Action::Accept, &t, &cfg), "Checks still running. Accept and rebase-merge anyway?");
    assert_eq!(confirm_texts(Action::Accept, &t, &cfg).last().unwrap(), "Checks running.");
    // no verdict: not reviewed, unless the repo has review off
    let t = Task { pr, ..task("BP-8", "ready") };
    assert_eq!(confirm_text(Action::Accept, &t, &cfg), "Not reviewed. Accept and rebase-merge anyway?");
    let off = Config { review: false, ..cfg.clone() };
    assert_eq!(confirm_text(Action::Accept, &t, &off), "Accept and rebase-merge?");
    assert!(!confirm_risky(Action::Accept, &t, &off));
    assert!(Config::from_value(&serde_json::json!({})).review);
    assert!(!Config::from_value(&serde_json::json!({"review": false})).review);
}

#[test]
fn a_drop_names_the_agent_it_stops() {
    let cfg = merge("rebase");
    let pr = Some("https://x/pull/42".into());
    let worker = |st: &str| Task { block: Some("w3".into()), ..task("BP-3", st) };
    let reviewer = |st: &str, why: &str| Task {
        why: Some(why.into()),
        review: Some(Review { block: Some("r3".into()), ..Default::default() }),
        ..worker(st)
    };
    let t = Task { pr: pr.clone(), ..worker("working") };
    assert_eq!(
        confirm_texts(Action::Drop, &t, &cfg),
        vec![
            "Its worker is still running. Drop it and close PR #42?",
            "Worker running. Drop it and close PR #42?",
            "Worker running. Drop it?",
            "Worker running. Drop?",
            "Worker running.",
        ]
    );
    for st in ["needs", "triage"] {
        assert_eq!(confirm_text(Action::Drop, &worker(st), &cfg), "Its worker is still running. Drop it?", "{st}");
    }
    // stopped on its reviewer: the reviewer is the one a drop stops
    for st in ["review", "needs", "triage"] {
        let t = reviewer(st, "Reviewer needs you: a prompt");
        assert_eq!(confirm_text(Action::Drop, &t, &cfg), "Its reviewer is still running. Drop it?", "{st}");
    }
    // its pane closed: nothing running to name
    assert_eq!(confirm_text(Action::Drop, &task("BP-3", "working"), &cfg), "Drop it?");
    let exited = Task { review: Some(Review::default()), ..reviewer("triage", "Reviewer exited without a verdict") };
    assert_eq!(confirm_text(Action::Drop, &exited, &cfg), "Drop it?");
    for st in ["stopped", "ready", "todo"] {
        assert_eq!(confirm_text(Action::Drop, &worker(st), &cfg), "Drop it?", "{st}");
    }
    assert_eq!(first_fit(&confirm_texts(Action::Drop, &t, &cfg), 23), "Worker running. Drop?");
    assert_eq!(first_fit(&confirm_texts(Action::Drop, &t, &cfg), 24), "Worker running. Drop it?");
    assert_eq!(first_fit(&confirm_texts(Action::Drop, &t, &cfg), 15), "Worker running.");
    assert_eq!(first_fit(&confirm_texts(Action::Drop, &t, &cfg), 10), "Worker ru…");
}
