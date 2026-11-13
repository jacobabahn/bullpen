use super::*;
use crate::model::testing::*;

#[test]
fn auto_accept_says_when_a_task_differs_or_waits() {
    let cfg = |on| Config { auto_accept: on, ..Config::default_for_test() };
    let approved = Some(Review { round: Some(1), verdict: Some("approved".into()), kind: None, block: None });
    let t = Task { pr: Some("https://github.com/me/repo/pull/4".into()), review: approved, ..task("BP-1", "ready") };
    assert_eq!(detail_with_auto(&t, &cfg(true), now()), "PR #4, approved by review.");
    let held = Task { auto_accept: Some(false), ..t.clone() };
    assert_eq!(detail_with_auto(&held, &cfg(true), now()), "PR #4, approved by review. Kept for you: no auto-accept.");
    assert_eq!(detail_with_auto(&held, &cfg(false), now()), "PR #4, approved by review.");
    let own = Task { auto_accept: Some(true), ..task("BP-1", "todo") };
    assert_eq!(detail_with_auto(&own, &cfg(false), now()), "Not started. s starts it. Auto-accepts once approved.");
    let waiting = Task { auto_wait: Some(true), ..t.clone() };
    assert_eq!(detail_with_auto(&waiting, &cfg(true), now()), "PR #4, approved by review. Auto-accepts once its checks pass.");
    assert_eq!(auto_phrase(&Task { status: "done".into(), ..held }, true), "");
}

#[test]
fn a_task_says_its_model_when_it_isnt_the_configs() {
    let mut cfg = Config::default_for_test();
    let own = Task { worker_model: Some("haiku".into()), ..task("BP-1", "todo") };
    assert_eq!(detail_with_auto(&own, &cfg, now()), "Not started. s starts it. Model: haiku.");
    assert_eq!(detail_with_auto(&task("BP-1", "todo"), &cfg, now()), "Not started. s starts it.");
    cfg.worker_model = Some("haiku".into());
    assert_eq!(detail_with_auto(&own, &cfg, now()), "Not started. s starts it.");
    assert_eq!(model_phrase(&Task { status: "done".into(), ..own }, &Config::default_for_test()), "");
}

#[test]
fn detail_says_what_a_task_needs() {
    let ended = Some("Ended its turn without saying ready".to_string());
    let t = Task { why: ended.clone(), note: Some("Asks: python3 or python?".into()), ..task("BP-1", "needs") };
    assert_eq!(task_detail(&t, 2, now()), "Asks: python3 or python?");
    let t = Task { why: ended, ..task("BP-1", "needs") };
    assert!(task_detail(&t, 2, now()).contains("probably with a question"));
    let t = Task { review: Some(Review { round: Some(2), verdict: None, kind: None, block: None }), ..task("BP-1", "review") };
    assert_eq!(task_detail(&t, 2, now()), "Reviewer on round 2 of 2.");
    let t = Task {
        pr: Some("https://x/pull/4".into()),
        review: Some(Review { round: None, verdict: Some("approved".into()), kind: None, block: None }),
        ..task("BP-1", "ready")
    };
    assert_eq!(task_detail(&t, 2, now()), "PR #4, approved by review.");
    let t = Task { review: Some(Review { round: Some(1), verdict: Some("changes".into()), kind: None, block: None }), ..task("BP-1", "working") };
    assert_eq!(task_detail(&t, 2, now()), "Fixing what review round 1 found.");
}

#[test]
fn detail_says_which_kind_of_review() {
    let kind = |k: &str, verdict: Option<&str>| Review {
        round: Some(1), verdict: verdict.map(Into::into), kind: Some(k.into()), block: None,
    };
    let t = Task { review: Some(kind("light", None)), ..task("BP-1", "review") };
    assert_eq!(task_detail(&t, 2, now()), "Light review on round 1 of 2.");
    let t = Task { review: Some(kind("followup", None)), ..task("BP-1", "review") };
    assert_eq!(task_detail(&t, 2, now()), "Follow-up review on round 1 of 2.");
    let t = Task { review: Some(kind("tour", None)), ..task("BP-1", "review") };
    assert_eq!(task_detail(&t, 2, now()), "Review of your comments on round 1 of 2.");
    let t = Task { review: Some(kind("rebase", None)), ..task("BP-1", "review") };
    assert_eq!(task_detail(&t, 2, now()), "Rebase check on round 1 of 2.");
    let t = Task { pr: Some("https://x/pull/4".into()), review: Some(kind("full", Some("approved"))), ..task("BP-1", "ready") };
    assert_eq!(task_detail(&t, 2, now()), "PR #4, approved by full review.");
    let t = Task { pr: Some("https://x/pull/4".into()), review: Some(kind("light", Some("approved"))), ..task("BP-1", "ready") };
    assert_eq!(task_detail(&t, 2, now()), "PR #4, approved by light review.");
}

#[test]
fn detail_ends_with_what_github_said() {
    let pr = Some("https://x/pull/4".to_string());
    let gh = |state: Option<&str>, checks: Option<&str>, new| {
        Some(Gh { state: state.map(String::from), checks: checks.map(String::from), new, sent: None })
    };
    let t = Task { pr: pr.clone(), gh: gh(None, Some("failing"), Some(1)), ..task("BP-1", "ready") };
    assert_eq!(task_detail(&t, 2, now()), "PR #4. Checks failing, 1 new comment.");
    let sent = Some(Gh { checks: Some("failing".into()), sent: Some(true), ..Default::default() });
    let t = Task { gh: sent, ..task("BP-1", "working") };
    assert_eq!(task_detail(&t, 2, now()), "Fixing what GitHub said. Checks failing.");
    let t = Task { pr, gh: gh(Some("merged"), None, None), ..task("BP-1", "ready") };
    assert_eq!(task_detail(&t, 2, now()), "PR #4. Merged on GitHub.");
}

#[test]
fn local_tasks_show_their_branch() {
    let t = Task {
        local: Some(true),
        branch: Some("bullpen/bp-2-x".into()),
        base: Some("main".into()),
        review: Some(Review { round: None, verdict: Some("approved".into()), kind: None, block: None }),
        ..task("BP-2", "ready")
    };
    assert_eq!(task_detail(&t, 2, now()), "Local branch bullpen/bp-2-x, approved by review.");
    assert_eq!(task_detail(&Task { review: None, ..t.clone() }, 2, now()), "Local branch bullpen/bp-2-x.");
    let rebased = Task { rebased: Some("main".into()), ..t.clone() };
    assert_eq!(task_detail(&rebased, 2, now()), "Local branch bullpen/bp-2-x, approved by review. Rebased onto main.");
    let done = Task { status: "done".into(), merged: Some("abc".into()), ..t.clone() };
    assert_eq!(task_detail(&done, 2, now()), "Accepted. Merged into main.");
    assert_eq!(confirm_text(Action::Accept, &t, &merge("squash")), "Accept and squash-merge into main?");
    assert_eq!(confirm_text(Action::Drop, &t, &merge("rebase")), "Drop it?");
}

#[test]
fn stat_shows_for_review_and_ready() {
    let t = Task { diffstat: Some("3 files, +120 −14".into()), ..task("BP-1", "ready") };
    assert_eq!(task_stat(&t), "3 files, +120 −14");
    assert_eq!(task_stat(&Task { status: "review".into(), ..t.clone() }), "3 files, +120 −14");
    assert_eq!(task_stat(&Task { status: "working".into(), ..t }), "");
    assert_eq!(task_stat(&task("BP-1", "ready")), "");
}

#[test]
fn triage_is_the_coordinators_not_yours() {
    let t = Task { why: Some("Failed: API error".into()), ..task("BP-1", "triage") };
    assert!(t.triage());
    assert!(!t.needs_you());
    assert!(!t.flagged()); // no amber bar
    assert_eq!(task_detail(&t, 2, now()), "Failed: API error");
    let ended = Task { why: Some("Ended its turn without saying ready".into()), ..task("BP-1", "triage") };
    assert_eq!(task_detail(&ended, 2, now()), "Ended its turn without ready.");
    assert_eq!(task_keys(Some(&t)), vec![("⏎", "open"), ("p", "peek"), ("t", "tell")]);
    let reviewer = Task { why: Some("Reviewer stopped without a verdict".into()), ..task("BP-1", "triage") };
    assert!(reviewer.on_reviewer());
    assert!(!t.on_reviewer());
    let asked = Task { why: Some("Reviewer needs you (permission)".into()), ..task("BP-1", "needs") };
    assert!(asked.on_reviewer() && asked.flagged());
    // escalated: it needs you, with its reason
    let late = Task { why: Some("Failed (coordinator didn't resolve it)".into()), ..task("BP-1", "needs") };
    assert!(late.needs_you() && late.flagged());
    assert_eq!(task_detail(&late, 2, now()), "Failed (coordinator didn't resolve it)");
}

#[test]
fn a_finished_task_shows_its_summary_and_closes_with_a() {
    let summary = Some("# Recorded the demo video\n\nSaved to ~/Movies/demo.mov".to_string());
    let t = Task { summary, worktree: Some("/w".into()), ..task("BP-3", "finished") };
    let tasks = vec![task("BP-1", "ready"), t.clone(), task("BP-2", "working")];
    let heads: Vec<&str> = board_groups(&tasks, false).iter().map(|(h, _)| *h).collect();
    assert_eq!(heads, vec!["Ready to accept", "Finished", "Working"]);
    assert_eq!(mark("finished"), "◆");
    assert!(!t.needs_you() && !t.flagged() && !t.closed());
    // the key row says a closes it: the detail doesn't spend its room on that
    assert_eq!(task_detail(&t, 2, now()), "Finished, nothing to merge: Recorded the demo video.");
    assert_eq!(task_detail(&task("BP-3", "finished"), 2, now()), "Finished, nothing to merge.");
    assert_eq!(task_keys(Some(&t))[0], ("a", "close"));
    assert!(task_keys(Some(&t)).contains(&("v", "diff")));
    assert_eq!(confirm_text(Action::Close, &t, &merge("rebase")), "Close it and remove its worktree?");
    assert_eq!(task_stat(&t), "");
    // once closed it is done, under Closed, and counts as done for what waits on it
    let closed = Task { status: "done".into(), closed: Some(true), worktree: None, ..t };
    assert_eq!(task_detail(&closed, 2, now()), "Closed, nothing to merge.");
    let mut ts = vec![closed, Task { after: Some(vec!["BP-3".into()]), ..task("BP-4", "todo") }];
    link_waits(&mut ts);
    assert!(ts[1].waits.is_empty());
    let t: Task = serde_json::from_str(r#"{"id":"BP-3","status":"finished","summary":"Notes in ~/n.md"}"#).unwrap();
    assert_eq!(task_detail(&t, 2, now()), "Finished, nothing to merge: Notes in ~/n.md.");
}

#[test]
fn a_task_its_coordinator_is_on_says_when_it_comes_to_you() {
    let since = |m| Task { since: Some(minutes_ago(m)), why: Some("Ended its turn without saying ready".into()), ..task("BP-1", "triage") };
    let cfg = Config::default_for_test(); // triage_minutes: 10
    assert_eq!(detail_with_auto(&since(3), &cfg, now()), "Ended its turn without ready. Yours in 7 min.");
    assert_eq!(yours_phrase(&since(0), 10, now()), "Yours in 10 min.");
    assert_eq!(yours_phrase(&since(10), 10, now()), "Yours any moment."); // bullpen moves it on its next look
    assert_eq!(yours_phrase(&since(30), 10, now()), "Yours any moment.");
    assert_eq!(yours_phrase(&since(0), 180, now()), "Yours in 3 h.");
    let at = now() - chrono::Duration::seconds(30);
    let half = Task { since: Some(at.to_rfc3339()), ..since(0) };
    assert_eq!(yours_phrase(&half, 1, now()), "Yours in 1 min."); // part of a minute left
    // the config's triage_minutes, else bullpen's 10
    assert_eq!(Config::from_value(&serde_json::json!({"triage_minutes": 4})).triage_minutes, 4);
    assert_eq!(Config::from_value(&serde_json::json!({})).triage_minutes, 10);
    // only a task waiting on its coordinator, and one with no time is left as it is
    let needs = Task { status: "needs".into(), ..since(3) };
    assert_eq!(yours_phrase(&needs, 10, now()), "");
    assert_eq!(yours_phrase(&task("BP-1", "triage"), 10, now()), "");
}

#[test]
fn a_working_task_says_who_sent_it_back() {
    let pr = Some("https://x/pull/4".to_string());
    let gh = Gh { state: Some("open".into()), checks: Some("passing".into()), new: Some(0), sent: None };
    // a reviewer's changes, after the watcher recorded the old push's checks
    let t = Task { pr: pr.clone(), review: changes(1), gh: Some(gh.clone()), ..task("BP-1", "working") };
    assert_eq!(task_detail(&t, 2, now()), "Fixing what review round 1 found.");
    // GitHub's, even when an earlier review round asked for changes
    let failing = Gh { checks: Some("failing".into()), new: Some(1), sent: Some(true), ..gh.clone() };
    let t = Task { pr: pr.clone(), review: changes(1), gh: Some(failing), ..task("BP-1", "working") };
    assert_eq!(task_detail(&t, 2, now()), "Fixing what GitHub said. Checks failing, 1 new comment.");
    // resumed or told by the person: nothing about the old push
    let t = Task { pr, gh: Some(gh.clone()), ..task("BP-1", "working") };
    assert_eq!(task_detail(&t, 2, now()), "");
    assert_eq!(pr_phrase(&t), "");
    assert_eq!(pr_phrase(&Task { status: "ready".into(), ..t }), "checks passing");
}

fn minutes_ago(m: i64) -> String {
    (now() - chrono::Duration::minutes(m)).to_rfc3339()
}

fn doing(state: &str, text: &str, secs_ago: i64) -> Option<Doing> {
    Some(Doing { state: state.into(), text: text.into(), at: now().timestamp() - secs_ago })
}

#[test]
fn a_working_task_says_what_its_agent_is_doing() {
    let t = Task { since: Some(minutes_ago(10)), doing: doing("working", "Running  the\n tests", 5), ..task("BP-1", "working") };
    assert_eq!(doing_now(&t, now()), "Running the tests");
    // said just before bullpen noted the status: still news
    let t = Task { since: Some(minutes_ago(0)), doing: doing("working", "Reading", 10), ..task("BP-1", "working") };
    assert_eq!(doing_now(&t, now()), "Reading");
    let fallback = |t: Task| assert_eq!(doing_now(&t, now()), "", "{t:?}");
    let base = Task { since: Some(minutes_ago(10)), ..task("BP-1", "working") };
    fallback(Task { doing: None, ..base.clone() });
    fallback(Task { doing: doing("working", "  ", 5), ..base.clone() });
    fallback(Task { doing: doing("idle", "Reading", 5), ..base.clone() });
    fallback(Task { doing: doing("working", "Reading", 11 * 60), ..base.clone() }); // before its status
    fallback(Task { since: Some(minutes_ago(60)), doing: doing("working", "Reading", 31 * 60), ..base.clone() });
    fallback(Task { stuck: Some(Stuck { who: "worker".into(), since: minutes_ago(25) }), doing: doing("working", "Reading", 5), ..base.clone() });
    fallback(Task { note: Some("Paused.".into()), doing: doing("working", "Reading", 5), ..base });
}

#[test]
fn the_board_reads_the_doing_of_the_agent_a_task_waits_on() {
    let state = std::env::temp_dir().join(format!("bullpen-doing-{}", std::process::id()));
    let (dir, doing) = (state.join("repos").join("r"), state.join("doing"));
    std::fs::create_dir_all(dir.join("tasks")).unwrap();
    std::fs::create_dir_all(&doing).unwrap();
    let review = r#"{"round": 1, "block": "block:rev"}"#;
    for (id, status) in [("BP-1", "working"), ("BP-2", "review"), ("BP-3", "ready")] {
        let n = &id[3..];
        let t = format!(r#"{{"id": "{id}", "status": "{status}", "block": "block:w{n}", "review": {review}}}"#);
        std::fs::write(dir.join("tasks").join(format!("{id}.json")), t).unwrap();
    }
    let say = |b: &str, text: &str| {
        std::fs::write(doing.join(format!("{b}.json")), format!(r#"{{"state": "working", "text": "{text}", "at": 1}}"#)).unwrap()
    };
    say("block_w1", "Worker one");
    say("block_w2", "Worker two");
    say("block_rev", "Reviewing");
    say("block_w3", "Worker three");
    let tasks = load_tasks(&dir);
    let text = |i: usize| tasks[i].doing.as_ref().map(|d| d.text.clone());
    assert_eq!(text(0).as_deref(), Some("Worker one"));
    assert_eq!(text(1).as_deref(), Some("Reviewing")); // in review: its reviewer's
    assert_eq!(text(2), None); // ready: nobody's working on it
    std::fs::remove_dir_all(&state).unwrap();
}

#[test]
fn a_stuck_task_says_so() {
    let stuck = |who: &str, m| Some(Stuck { who: who.into(), since: minutes_ago(m) });
    let t = Task { stuck: stuck("worker", 25), ..task("BP-1", "working") };
    assert!(t.is_stuck() && t.flagged());
    assert_eq!(task_detail(&t, 2, now()), "No progress for 25 min. p shows its screen.");
    let noted = Task { note: Some("Hung on npm install".into()), ..t.clone() };
    assert_eq!(task_detail(&noted, 2, now()), "Hung on npm install");
    let review = Some(Review { round: Some(1), verdict: None, kind: None, block: None });
    let t = Task { review: review.clone(), stuck: stuck("reviewer", 190), ..task("BP-1", "review") };
    assert_eq!(task_detail(&t, 2, now()), "Reviewer has shown no progress for 3 h. ⏎ opens it.");
    // a worker's quiet spell says nothing about a task that has moved on
    let moved = Task { review, stuck: stuck("worker", 25), ..task("BP-1", "review") };
    assert!(!moved.is_stuck() && !moved.flagged());
    assert_eq!(task_detail(&moved, 2, now()), "Reviewer on round 1 of 2.");
    assert!(!task("BP-1", "working").is_stuck());
}

fn local(d: u32, h: u32, m: u32) -> DateTime<FixedOffset> {
    use chrono::TimeZone;
    Local.with_ymd_and_hms(2026, 11, d, h, m, 0).unwrap().fixed_offset()
}

#[test]
fn a_limited_task_says_when_it_resets() {
    let at = local(11, 13, 0); // a Wednesday, 1 PM
    let lim = |who: &str, resets: Option<String>| Some(Limit { who: who.into(), resets });
    let three = Some(local(11, 15, 0).to_rfc3339());
    let t = Task { limit: lim("worker", three.clone()), ..task("BP-1", "working") };
    assert!(t.is_limited());
    assert_eq!(task_detail(&t, 2, at), "Usage limit, resets 3:00 PM. Continues by itself then.");
    let t = Task { limit: lim("worker", None), ..task("BP-1", "working") };
    assert_eq!(task_detail(&t, 2, at), "Usage limit, reset time unknown. t tells it to continue.");
    let t = Task { limit: lim("reviewer", three.clone()), ..task("BP-1", "review") };
    assert_eq!(task_detail(&t, 2, at),
               "Reviewer stopped at the usage limit, resets 3:00 PM. Continues by itself then.");
    let t = Task { limit: lim("reviewer", None), ..task("BP-1", "review") };
    assert_eq!(task_detail(&t, 2, at), "Reviewer stopped at the usage limit, reset time unknown. ⏎ opens it.");
    // a worker's limit says nothing about a task that has moved on
    assert!(!Task { limit: lim("worker", three), ..task("BP-1", "ready") }.is_limited());
}

#[test]
fn clock_says_the_day_when_it_isnt_today() {
    let at = local(11, 13, 0);
    assert_eq!(clock(local(11, 15, 0), at), "3:00 PM");
    assert_eq!(clock(local(16, 0, 0), at), "Mon 12:00 AM");
    assert_eq!(clock(local(30, 9, 5), at), "Nov 30 9:05 AM");
}

#[test]
fn a_queued_task_says_what_it_waits_on() {
    let after = |ids: &[&str]| Some(ids.iter().map(|s| s.to_string()).collect());
    let mut tasks: Vec<Task> = vec![
        task("BP-1", "done"),
        task("BP-2", "working"),
        task("BP-3", "dropped"),
        Task { after: after(&["BP-1", "BP-2"]), ..task("BP-4", "todo") },
        Task { after: after(&["BP-1"]), ..task("BP-5", "todo") },
        Task { after: after(&["BP-3", "BP-9"]), ..task("BP-6", "todo") },
        Task { after: after(&["BP-2"]), ..task("BP-7", "working") },
    ];
    link_waits(&mut tasks);
    let detail = |i: usize| task_detail(&tasks[i], 2, now());
    assert_eq!(detail(3), "Waits on BP-2.");
    assert_eq!(detail(4), "Not started. s starts it.");
    assert_eq!(detail(5), "Waits on BP-3 (dropped), BP-9 (missing).");
    assert_eq!(detail(6), ""); // started anyway (--force): it waits on nothing now
    // read from the task's JSON, as bullpen writes it
    let t: Task = serde_json::from_str(r#"{"id":"BP-4","status":"todo","after":["BP-1"]}"#).unwrap();
    assert_eq!(t.after, after(&["BP-1"]));
}
