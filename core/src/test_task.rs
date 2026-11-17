use super::*;
use crate::testenv::Env;

fn task(v: Value) -> Obj {
    v.as_object().unwrap().clone()
}

/// A task in status, stopped by `by` in the way `kind`, with more of its keys.
fn stopped(status: &str, by: &str, kind: &str, more: Value) -> Obj {
    let mut t = task(json!({"status": status, "stop": {"by": by, "kind": kind}}));
    t.extend(task(more));
    t
}

#[test]
fn test_status_words_are_the_boards() {
    let w = status_words;
    assert_eq!(w("needs"), "needs you");
    assert_eq!(w("stopped"), "needs you (stopped)");
    assert_eq!(w("triage"), "coordinator on it (triage)");
    assert_eq!(w("review"), "in review");
    assert_eq!(w("ready"), "ready to accept");
    assert_eq!(w("todo"), "queued (todo)");
    assert_eq!(w("dropped"), "closed (dropped)");
    let mut statuses: Vec<&str> = BOARD_WORDS.iter().map(|(s, _)| *s).collect();
    statuses.sort();
    let mut want = [
        "todo", "working", "review", "ready", "finished", "triage", "needs", "stopped", "done", "dropped",
    ];
    want.sort();
    assert_eq!(statuses, want);
}

#[test]
fn is_status_puts_the_words_in_a_sentence() {
    let t = |status: &str| task(json!({"id": "BP-3", "status": status}));
    assert_eq!(is_status(&t("todo")), "BP-3 is queued (todo)");
    assert_eq!(is_status(&t("stopped")), "BP-3 needs you (stopped)");
    assert_eq!(is_status(&t("needs")), "BP-3 needs you");
    assert_eq!(is_status(&t("triage")), "BP-3's coordinator is on it (triage)");
    assert_eq!(is_status(&t("odd")), "BP-3 is odd");
}

#[test]
fn test_whether_a_worker_is_between_turns_goes_by_its_stop() {
    let idle = worker_idle;
    let none = json!({});
    assert!(idle(&task(json!({"status": "working"}))));
    assert!(idle(&stopped("triage", "worker", "ended", none.clone())));
    assert!(idle(&stopped("triage", "worker", "not_sent", none.clone())));
    assert!(idle(&stopped("triage", "reviewer", "ended", none.clone())));
    assert!(idle(&stopped("needs", "reviewer", "prompt", none.clone())));
    assert!(!idle(&stopped("needs", "worker", "prompt", none.clone())));
    assert!(!idle(&stopped("triage", "worker", "failed", none.clone())));
    assert!(!idle(&stopped("triage", "reviewer", "limit", none.clone())));
    // the person asked about it: the question is its why, the stop still counts
    assert!(!idle(&stopped(
        "needs",
        "worker",
        "prompt",
        json!({"why": "Delete it?", "ask": true})
    )));
    assert!(idle(&task(
        json!({"status": "needs", "why": "Delete it?", "ask": true})
    )));
    assert!(idle(&stopped("stopped", "worker", "exited", none)));
}

#[test]
fn test_a_new_status_ends_ask_ok_and_the_stop() {
    let why = json!({"why": "Ended its turn without saying ready", "ok": true});
    let mut t = stopped("triage", "worker", "ended", why);
    ask(&mut t, "Delete it?");
    assert_eq!(
        (&t["status"], &t["why"], &t["ask"], &t["stop"]),
        (
            &json!("needs"),
            &json!("Delete it?"),
            &json!(true),
            &json!({"by": "worker", "kind": "ended"})
        )
    );
    assert!(!t.contains_key("ok"));
    ask(&mut t, "Or keep it?"); // asked again: still asked
    assert_eq!(t["ask"], json!(true));
    set_status(&mut t, "working", None, None);
    assert_eq!((&t["status"], &t["why"]), (&json!("working"), &Value::Null));
    assert!(!["ask", "ok", "stop"].iter().any(|k| t.contains_key(*k)));
    let mut t = stopped("triage", "reviewer", "ended", json!({}));
    ok(&mut t);
    assert_eq!((&t["status"], &t["ok"]), (&json!("review"), &json!(true)));
    assert!(!t.contains_key("stop"));
    let mut t = stopped("triage", "worker", "failed", json!({}));
    ok(&mut t);
    assert_eq!(t["status"], json!("working"));
}

#[test]
fn an_asked_task_remembers_where_it_was_asked_from() {
    let mut t = task(json!({"status": "review", "why": "round 2", "from_ready": true}));
    ask(&mut t, "Merge it?");
    assert_eq!(t["asked_from"], json!({"status": "review", "why": "round 2"}));
    ask(&mut t, "Really?");
    assert_eq!(t["asked_from"], json!({"status": "review", "why": "round 2"}));
    let mut s = stopped("triage", "worker", "ended", json!({}));
    ask(&mut s, "Delete it?");
    assert!(!s.contains_key("asked_from")); // a stopped task keeps its stop instead
    back_to_work(&mut t);
    assert_eq!(t["status"], json!("working"));
    assert!(!t.contains_key("from_ready") && !t.contains_key("asked_from"));
    // a stop given replaces the one it had; a status that keeps it stopped keeps it
    set_status(
        &mut s,
        "triage",
        Some("x"),
        Some(json!({"by": "worker", "kind": "failed"})),
    );
    assert_eq!(s["stop"], json!({"by": "worker", "kind": "failed"}));
    set_status(&mut s, "needs", Some("y"), None);
    assert_eq!(s["stop"], json!({"by": "worker", "kind": "failed"}));
}

#[test]
fn whose_stop_it_is() {
    assert!(reviewers_stop(&stopped("needs", "reviewer", "prompt", json!({}))));
    assert!(!reviewers_stop(&stopped("triage", "reviewer", "limit", json!({}))));
    assert!(!reviewers_stop(&stopped("triage", "worker", "ended", json!({}))));
    assert!(!reviewers_stop(&task(json!({"status": "review"}))));
    let mut t = stopped("needs", "worker", "prompt", json!({}));
    assert!(at_prompt(&t));
    t["stop"] = json!({"kind": "prompt", "by": "worker"}); // the same, in another order
    assert!(at_prompt(&t));
    t["stop"] = json!({"by": "worker", "kind": "prompt", "more": 1});
    assert!(!at_prompt(&t));
}

#[test]
fn a_task_is_on_its_reviewer_by_its_stop() {
    assert!(on_reviewer(&stopped("needs", "reviewer", "prompt", json!({}))));
    assert!(on_reviewer(&stopped("triage", "reviewer", "exited", json!({}))));
    assert!(on_reviewer(&stopped(
        "needs",
        "reviewer",
        "ended",
        json!({"ask": true})
    )));
    assert!(!on_reviewer(&stopped("triage", "reviewer", "limit", json!({})))); // seen to as its worker's
    assert!(!on_reviewer(&stopped("triage", "worker", "ended", json!({}))));
    assert!(!on_reviewer(&task(json!({"status": "review"}))));
    // its why's wording makes no difference
    let why = json!({"why": "Reviewer stopped at the usage limit"});
    assert!(!on_reviewer(&stopped("triage", "reviewer", "limit", why)));
}

#[test]
fn a_task_waits_on_its_reviewer_in_review_or_for_its_stop() {
    assert_eq!(waits_on(&task(json!({"status": "review"}))), Some("reviewer"));
    assert_eq!(
        waits_on(&stopped("needs", "reviewer", "prompt", json!({}))),
        Some("reviewer")
    );
    assert_eq!(
        waits_on(&stopped("triage", "reviewer", "limit", json!({}))),
        Some("worker")
    );
    assert_eq!(
        whose_turn(&stopped("triage", "reviewer", "limit", json!({}))),
        Some("worker")
    );
    assert_eq!(
        waits_on(&stopped("triage", "worker", "exited", json!({}))),
        Some("worker")
    );
    assert_eq!(waits_on(&stopped("stopped", "worker", "pane_gone", json!({}))), None); // its pane is gone
    assert_eq!(waits_on(&task(json!({"status": "working"}))), Some("worker"));
    for status in ["todo", "ready", "finished", "done", "dropped"] {
        assert_eq!(waits_on(&task(json!({"status": status}))), None);
        assert_eq!(whose_turn(&task(json!({"status": status}))), None);
    }
    let lim = json!({"who": "reviewer", "text": "limit"});
    assert_eq!(waits_on(&task(json!({"status": "review", "limit": lim}))), None); // waits on the usage limit
    assert_eq!(
        whose_turn(&task(json!({"status": "review", "limit": lim}))),
        Some("reviewer")
    );
    let worker_lim = json!({"who": "worker", "text": "limit"});
    assert_eq!(waits_on(&task(json!({"status": "working", "limit": worker_lim}))), None);
}

#[test]
fn only_the_agent_a_task_waits_on_is_limited() {
    let lim = json!({"who": "worker", "resets": "2026-11-11T15:00:00-06:00", "text": "x"});
    assert!(is_limited(&task(json!({"status": "working", "limit": lim}))));
    assert!(!is_limited(&task(json!({"status": "ready", "limit": lim}))));
    assert!(!is_limited(&task(json!({"status": "review", "limit": lim}))));
    let lim = json!({"who": "reviewer", "text": "x"});
    assert!(is_limited(&task(json!({"status": "review", "limit": lim}))));
    // a record of the wrong shape, or an empty one, is none
    assert!(!is_limited(&task(json!({"status": "working", "limit": {}}))));
    assert!(!is_limited(&task(json!({"status": "working", "limit": "x"}))));
    assert!(!is_limited(&task(json!({"status": "working", "limit": true}))));
}

#[test]
fn what_needs_you_and_what_waits_on_your_accept() {
    let needs: Vec<&str> = crate::words::MARKED.into_iter().filter(|st| needs_you(st)).collect();
    assert_eq!(needs, ["needs", "stopped"]);
    let accept: Vec<&str> = crate::words::MARKED
        .into_iter()
        .filter(|st| awaits_accept(st))
        .collect();
    assert_eq!(accept, ["ready", "finished"]);
    let nothing = json!({"status": "ready", "why": "Nothing to merge (no commits)"});
    assert!(nothing_to_merge(&task(nothing)));
    assert!(!nothing_to_merge(&task(json!({"status": "ready", "why": "Approved"}))));
    assert!(!nothing_to_merge(&task(json!({"status": "ready"}))));
    let review = json!({"status": "review", "why": "Nothing to merge (no commits)"});
    assert!(!nothing_to_merge(&task(review)));
}

#[test]
fn statuses_and_numbers() {
    assert!(is_closed("done") && is_closed("dropped") && !is_closed("ready"));
    assert!(is_waiting("triage") && !is_waiting("stopped"));
    assert!(is_stopped("stopped") && !is_stopped("working"));
    assert_eq!(task_num(&task(json!({"id": "BP-12"}))), 12);
    assert_eq!(task_num(&task(json!({"id": "AB-CD-7"}))), 7);
    let ts = vec![task(json!({"status": "done"})), task(json!({"status": "todo"}))];
    assert_eq!(listed(ts.clone(), false).len(), 1);
    assert_eq!(listed(ts, true).len(), 2);
    assert_eq!(stamp(&task(json!({"since": "s", "updated": "u"}))), Some("s"));
    assert_eq!(stamp(&task(json!({"since": null, "updated": "u"}))), Some("u"));
    assert_eq!(stamp(&task(json!({}))), None);
}

#[test]
fn test_news_doesnt_say_local_branch_in_local_mode() {
    assert_eq!(where_(&task(json!({"status": "ready", "pr": null, "local": true}))), "");
    assert_eq!(where_(&task(json!({"pr": "https://github.com/o/r/pull/12"}))), "PR #12");
    assert_eq!(
        where_(&task(json!({"pr": "https://github.com/o/r/pull/12", "local": true}))),
        ""
    );
}

#[test]
fn pr_short_finds_the_number() {
    assert_eq!(pr_short("https://github.com/o/r/pull/12"), "PR #12");
    assert_eq!(pr_short("https://github.com/o/pull/r/pull/34/files"), "PR #34");
    assert_eq!(pr_short("feature-branch"), "feature-branch");
    assert_eq!(pr_short(""), "");
}

#[test]
fn a_tasks_own_settings() {
    let _env = Env::new();
    let mut repo = Repo::new("/x/y").unwrap();
    repo.cfg.insert("worker_model".into(), json!("opus"));
    assert_eq!(
        own_model(&repo.cfg, &task(json!({"worker_model": "sonnet"}))),
        Some("sonnet")
    );
    assert_eq!(own_model(&repo.cfg, &task(json!({"worker_model": "opus"}))), None);
    assert_eq!(own_model(&repo.cfg, &task(json!({}))), None);
    let mut t = task(json!({}));
    assert!(!auto_accepts(&repo, &t));
    set_auto_accept(&mut t, None);
    assert!(!t.contains_key("auto_accept"));
    set_auto_accept(&mut t, Some(true));
    assert!(auto_accepts(&repo, &t));
    repo.cfg.insert("auto_accept".into(), json!(true));
    set_auto_accept(&mut t, Some(false));
    assert!(!auto_accepts(&repo, &t));
    t.insert("auto_accept".into(), Value::Null);
    assert!(auto_accepts(&repo, &t));
}

fn cmd(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

#[test]
fn models_in_agent_commands() {
    let c = cmd(&["/usr/bin/claude", "--model", "opus", "-p"]);
    assert!(is_claude(&c));
    assert!(!is_claude(&cmd(&["codex"])) && !is_claude(&[]));
    assert_eq!(model_in(&c), Some("opus".into()));
    assert_eq!(model_in(&cmd(&["claude", "--model=haiku"])), Some("haiku".into()));
    assert_eq!(model_in(&cmd(&["claude", "--model"])), None);
    assert_eq!(
        with_model(&c, Some("sonnet")),
        cmd(&["/usr/bin/claude", "-p", "--model", "sonnet"])
    );
    assert_eq!(with_model(&c, Some("default")), cmd(&["/usr/bin/claude", "-p"]));
    assert_eq!(with_model(&c, None), c);
    assert_eq!(
        with_model(&cmd(&["codex", "--model", "x"]), Some("y")),
        cmd(&["codex", "--model", "x"])
    );
    assert_eq!(model_phrase(&cmd(&["claude"]), Some("opus")), "opus");
    assert_eq!(model_phrase(&cmd(&["claude"]), None), "default (Claude Code picks)");
    assert_eq!(
        model_phrase(&cmd(&["codex"]), None),
        "not set by bullpen: the command is codex, not claude"
    );
    assert_eq!(
        model_phrase(&[], None),
        "not set by bullpen: the command is empty, not claude"
    );
}

#[test]
fn words_for_people() {
    assert_eq!(clip("  a\n b  ", 300), "a b");
    assert_eq!(clip("abcdef", 4), "abc…");
    assert_eq!(clip("abcd", 4), "abcd");
    assert_eq!(commits(1), "1 commit");
    assert_eq!(commits(0), "0 commits");
    assert_eq!(slug("Fix the Board's --pane flag!", 40), "fix-the-board-s-pane-flag");
    assert_eq!(slug("Ünïcode only ÉÉ", 40), "n-code-only");
    assert_eq!(slug("!!!", 40), "task");
    assert_eq!(slug("abcde fghij", 6), "abcde");
}
