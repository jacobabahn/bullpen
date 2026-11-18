use super::*;
use crate::model::testing::*;
use chrono::{DateTime, FixedOffset, Local};

/// The task's sentence, by the default settings (review_rounds 2).
fn detail(t: &Task) -> String {
    task_detail(t, &Config::default_for_test(), now())
}

fn approved(kind: Option<&str>) -> Value {
    json!({"verdict": "approved", "kind": kind})
}

#[test]
fn auto_accept_says_when_a_task_differs_or_waits() {
    let cfg = |on: bool| Config::default_for_test().with(json!({"auto_accept": on}));
    let t = task("BP-1", "ready").with(json!({"pr": "https://github.com/me/repo/pull/4", "review": {"round": 1, "verdict": "approved"}}));
    assert_eq!(detail_with_auto(&t, &cfg(true), now()), "PR #4, approved by review.");
    let held = t.clone().with(json!({"auto_accept": false}));
    assert_eq!(detail_with_auto(&held, &cfg(true), now()), "PR #4, approved by review. Kept for you: no auto-accept.");
    assert_eq!(detail_with_auto(&held, &cfg(false), now()), "PR #4, approved by review.");
    let own = task("BP-1", "todo").with(json!({"auto_accept": true}));
    assert_eq!(detail_with_auto(&own, &cfg(false), now()), "Not started. s starts it. Auto-accepts once approved.");
    let waiting = t.clone().with(json!({"auto_wait": true}));
    assert_eq!(detail_with_auto(&waiting, &cfg(true), now()), "PR #4, approved by review. Auto-accepts once its checks pass.");
    assert_eq!(detail_with_auto(&held.with(json!({"status": "done"})), &cfg(true), now()), "Accepted. PR #4 merged.");
}

#[test]
fn a_task_says_its_model_when_it_isnt_the_configs() {
    let cfg = Config::default_for_test();
    let own = task("BP-1", "todo").with(json!({"worker_model": "haiku"}));
    assert_eq!(detail_with_auto(&own, &cfg, now()), "Not started. s starts it. Model: haiku.");
    assert_eq!(detail_with_auto(&task("BP-1", "todo"), &cfg, now()), "Not started. s starts it.");
    let haiku = cfg.with(json!({"worker_model": "haiku"}));
    assert_eq!(detail_with_auto(&own, &haiku, now()), "Not started. s starts it.");
    assert_eq!(detail_with_auto(&own.with(json!({"status": "done"})), &cfg, now()), "Accepted.");
}

#[test]
fn detail_says_what_a_task_needs() {
    let ended = "Ended its turn without saying ready";
    let t = task("BP-1", "needs").with(json!({"why": ended, "note": "Asks: python3 or python?"}));
    assert_eq!(detail(&t), "Asks: python3 or python?");
    let t = task("BP-1", "needs").with(json!({"why": ended}));
    assert!(detail(&t).contains("probably with a question"));
    let t = task("BP-1", "review").with(json!({"review": {"round": 2}}));
    assert_eq!(detail(&t), "Reviewer on round 2 of 2.");
    let t = task("BP-1", "ready").with(json!({"pr": "https://x/pull/4", "review": approved(None)}));
    assert_eq!(detail(&t), "PR #4, approved by review.");
    let t = task("BP-1", "working").with(json!({"review": changes(1)}));
    assert_eq!(detail(&t), "Fixing what review round 1 found.");
}

#[test]
fn detail_says_which_kind_of_review() {
    let review = |k: &str| task("BP-1", "review").with(json!({"review": {"round": 1, "kind": k}}));
    assert_eq!(detail(&review("light")), "Light review on round 1 of 2.");
    assert_eq!(detail(&review("followup")), "Follow-up review on round 1 of 2.");
    assert_eq!(detail(&review("tour")), "Review of your comments on round 1 of 2.");
    assert_eq!(detail(&review("rebase")), "Rebase check on round 1 of 2.");
    assert_eq!(detail(&review("rebased")), "Review after a rebase on round 1 of 2.");
    let ready = |k: &str| task("BP-1", "ready").with(json!({"pr": "https://x/pull/4", "review": approved(Some(k))}));
    assert_eq!(detail(&ready("full")), "PR #4, approved by full review.");
    assert_eq!(detail(&ready("light")), "PR #4, approved by light review.");
}

#[test]
fn detail_ends_with_what_github_said() {
    let t = task("BP-1", "ready").with(json!({"pr": "https://x/pull/4", "gh": {"checks": "failing", "new": 1}}));
    assert_eq!(detail(&t), "PR #4. Checks failing, 1 new comment.");
    let t = task("BP-1", "working").with(json!({"gh": {"checks": "failing", "sent": true}}));
    assert_eq!(detail(&t), "Fixing what GitHub said. Checks failing.");
    let t = task("BP-1", "ready").with(json!({"pr": "https://x/pull/4", "gh": {"state": "merged"}}));
    assert_eq!(detail(&t), "PR #4. Merged on GitHub.");
}

#[test]
fn local_tasks_show_their_branch() {
    let t = task("BP-2", "ready")
        .with(json!({"local": true, "branch": "bullpen/bp-2-x", "base": "main", "review": approved(None)}));
    assert_eq!(detail(&t), "Local branch bullpen/bp-2-x, approved by review.");
    assert_eq!(detail(&t.clone().with(json!({"review": null}))), "Local branch bullpen/bp-2-x.");
    let rebased = t.clone().with(json!({"rebased": "main"}));
    assert_eq!(detail(&rebased), "Local branch bullpen/bp-2-x, approved by review. Rebased onto main.");
    let done = t.clone().with(json!({"status": "done", "merged": "abc"}));
    assert_eq!(detail(&done), "Accepted. Merged into main.");
    assert_eq!(confirm_text(Action::Accept, &t, &merge("squash")), "Accept and squash-merge into main?");
    assert_eq!(confirm_text(Action::Drop, &t, &merge("rebase")), "Drop it?");
}

#[test]
fn stat_shows_for_review_and_ready() {
    let t = task("BP-1", "ready").with(json!({"diffstat": "3 files, +120 −14"}));
    assert_eq!(task_stat(&t), "3 files, +120 −14");
    assert_eq!(task_stat(&t.clone().with(json!({"status": "review"}))), "3 files, +120 −14");
    assert_eq!(task_stat(&t.with(json!({"status": "working"}))), "");
    assert_eq!(task_stat(&task("BP-1", "ready")), "");
}

#[test]
fn triage_is_the_coordinators_not_yours() {
    let why = |status: &str, why: &str| task("BP-1", status).with(json!({"why": why}));
    let t = why("triage", "Failed: API error");
    assert!(t.triage());
    assert!(!t.needs_you());
    assert!(!t.flagged()); // no amber bar
    assert_eq!(detail(&t), "Failed: API error");
    assert_eq!(detail(&why("triage", "Ended its turn without saying ready")), "Ended its turn without ready.");
    assert_eq!(task_keys(Some(&t)), vec![("⏎", "open"), ("p", "peek"), ("t", "tell")]);
    let stop = |by: &str, kind: &str| json!({"stop": {"by": by, "kind": kind}});
    assert!(why("triage", "Reviewer stopped without a verdict").with(stop("reviewer", "ended")).on_reviewer());
    assert!(!t.with(stop("worker", "failed")).on_reviewer());
    let asked = why("needs", "Reviewer needs you (permission)").with(stop("reviewer", "prompt"));
    assert!(asked.on_reviewer() && asked.flagged());
    // escalated: it needs you, with its reason
    let late = why("needs", "Failed (coordinator didn't resolve it)");
    assert!(late.needs_you() && late.flagged());
    assert_eq!(detail(&late), "Failed (coordinator didn't resolve it)");
}

#[test]
fn a_finished_task_shows_its_summary_and_closes_with_a() {
    let summary = "# Recorded the demo video\n\nSaved to ~/Movies/demo.mov";
    let t = task("BP-3", "finished").with(json!({"summary": summary, "worktree": "/w"}));
    let tasks = vec![task("BP-1", "ready"), t.clone(), task("BP-2", "working")];
    let heads: Vec<&str> = board_groups(&tasks, false).iter().map(|(h, _)| *h).collect();
    assert_eq!(heads, vec!["Ready to accept", "Finished", "Working"]);
    assert_eq!(mark("finished"), "◆");
    assert!(!t.needs_you() && !t.flagged() && !t.closed());
    // the key row says a closes it: the detail doesn't spend its room on that (`ls` does)
    assert_eq!(detail(&t), "Finished, nothing to merge: Recorded the demo video.");
    assert_eq!(detail(&task("BP-3", "finished")), "Finished, nothing to merge.");
    assert_eq!(task_keys(Some(&t))[0], ("a", "close"));
    assert!(task_keys(Some(&t)).contains(&("v", "diff")));
    assert_eq!(confirm_text(Action::Close, &t, &merge("rebase")), "Close it and remove its worktree?");
    assert_eq!(task_stat(&t), "");
    // finished with `finish --keep-branch`: a keeps the branch
    let kept = t.clone().with(json!({"keep_branch": true}));
    assert_eq!(detail(&kept), "Finished, branch to be kept: Recorded the demo video.");
    assert_eq!(confirm_text(Action::Close, &kept, &merge("rebase")), "Close it and keep its branch?");
    // once closed it is done, under Closed, and counts as done for what waits on it
    let closed = t.with(json!({"status": "done", "closed": true, "worktree": null}));
    assert_eq!(detail(&closed), "Closed, nothing to merge.");
    let mut ts = vec![closed, task("BP-4", "todo").with(json!({"after": ["BP-3"]}))];
    link_waits(&mut ts);
    assert!(ts[1].waits.is_empty());
    let t = task("BP-3", "finished").with(json!({"summary": "Notes in ~/n.md"}));
    assert_eq!(detail(&t), "Finished, nothing to merge: Notes in ~/n.md.");
}

#[test]
fn a_task_its_coordinator_is_on_says_when_it_comes_to_you() {
    let since = |m| task("BP-1", "triage").with(json!({"since": minutes_ago(m), "why": "Ended its turn without saying ready"}));
    let cfg = Config::default_for_test(); // triage_minutes: 10
    assert_eq!(detail_with_auto(&since(3), &cfg, now()), "Ended its turn without ready. Yours in 7 min.");
    assert_eq!(yours_phrase(&since(0), 10, now()), "Yours in 10 min.");
    assert_eq!(yours_phrase(&since(10), 10, now()), "Yours any moment."); // bullpen moves it on its next look
    assert_eq!(yours_phrase(&since(30), 10, now()), "Yours any moment.");
    assert_eq!(yours_phrase(&since(0), 180, now()), "Yours in 3 h.");
    let at = now() - chrono::Duration::seconds(30);
    let half = since(0).with(json!({"since": at.to_rfc3339()}));
    assert_eq!(yours_phrase(&half, 1, now()), "Yours in 1 min."); // part of a minute left
    // the config's triage_minutes, else bullpen's 10
    assert_eq!(cfg.with(json!({"triage_minutes": 4})).triage_minutes, 4);
    assert_eq!(cfg.triage_minutes, 10);
    // only a task waiting on its coordinator, and one with no time is left as it is
    assert_eq!(yours_phrase(&since(3).with(json!({"status": "needs"})), 10, now()), "");
    assert_eq!(yours_phrase(&task("BP-1", "triage"), 10, now()), "");
}

#[test]
fn a_working_task_says_who_sent_it_back() {
    let gh = json!({"state": "open", "checks": "passing", "new": 0});
    let working = task("BP-1", "working").with(json!({"pr": "https://x/pull/4", "gh": gh}));
    // a reviewer's changes, after the watcher recorded the old push's checks
    let t = working.clone().with(json!({"review": changes(1)}));
    assert_eq!(detail(&t), "Fixing what review round 1 found.");
    // GitHub's, even when an earlier review round asked for changes
    let failing = json!({"state": "open", "checks": "failing", "new": 1, "sent": true});
    let t = working.clone().with(json!({"review": changes(1), "gh": failing}));
    assert_eq!(detail(&t), "Fixing what GitHub said. Checks failing, 1 new comment.");
    // resumed or told by the person: nothing about the old push
    assert_eq!(detail(&working), "");
    assert_eq!(pr_phrase(&working), "");
    assert_eq!(pr_phrase(&working.with(json!({"status": "ready"}))), "checks passing");
}

fn minutes_ago(m: i64) -> String {
    (now() - chrono::Duration::minutes(m)).to_rfc3339()
}

fn doing(state: &str, text: &str, secs_ago: i64) -> Option<bullpen_core::Obj> {
    json!({"state": state, "text": text, "at": now().timestamp() - secs_ago}).as_object().cloned()
}

#[test]
fn a_working_task_says_what_its_agent_is_doing() {
    let at = |m: i64, d: Option<bullpen_core::Obj>| {
        let mut t = task("BP-1", "working").with(json!({"since": minutes_ago(m)}));
        t.doing = d;
        t
    };
    assert_eq!(doing_now(&at(10, doing("working", "Running  the\n tests", 5)), now()), "Running the tests");
    // said just before bullpen noted the status: still news
    assert_eq!(doing_now(&at(0, doing("working", "Reading", 10)), now()), "Reading");
    let fallback = |t: Task| assert_eq!(doing_now(&t, now()), "", "{t:?}");
    fallback(at(10, None));
    fallback(at(10, doing("working", "  ", 5)));
    fallback(at(10, doing("idle", "Reading", 5)));
    fallback(at(10, doing("working", "Reading", 11 * 60))); // before its status
    fallback(at(60, doing("working", "Reading", 31 * 60)));
    let reading = at(10, doing("working", "Reading", 5));
    fallback(reading.clone().with(json!({"stuck": {"who": "worker", "since": minutes_ago(25)}})));
    fallback(reading.with(json!({"note": "Paused."})));
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
    let tasks = load_tasks(&mut Default::default(), &dir);
    let text = |i: usize| tasks[i].doing.as_ref().map(|d| d["text"].as_str().unwrap_or("").to_string());
    assert_eq!(text(0).as_deref(), Some("Worker one"));
    assert_eq!(text(1).as_deref(), Some("Reviewing")); // in review: its reviewer's
    assert_eq!(text(2), None); // ready: nobody's working on it
    std::fs::remove_dir_all(&state).unwrap();
}

#[test]
fn a_stuck_task_says_so() {
    let stuck = |who: &str, m| json!({"stuck": {"who": who, "since": minutes_ago(m)}});
    let t = task("BP-1", "working").with(stuck("worker", 25));
    assert!(t.is_stuck() && t.flagged());
    assert_eq!(detail(&t), "No progress for 25 min. p shows its screen.");
    assert_eq!(detail(&t.with(json!({"note": "Hung on npm install"}))), "Hung on npm install");
    let review = task("BP-1", "review").with(json!({"review": {"round": 1}}));
    let t = review.clone().with(stuck("reviewer", 190));
    assert_eq!(detail(&t), "Reviewer has shown no progress for 3 h. ⏎ opens it.");
    // a worker's quiet spell says nothing about a task that has moved on
    let moved = review.with(stuck("worker", 25));
    assert!(!moved.is_stuck() && !moved.flagged());
    assert_eq!(detail(&moved), "Reviewer on round 1 of 2.");
    assert!(!task("BP-1", "working").is_stuck());
}

fn local(d: u32, h: u32, m: u32) -> DateTime<FixedOffset> {
    use chrono::TimeZone;
    Local.with_ymd_and_hms(2026, 11, d, h, m, 0).unwrap().fixed_offset()
}

#[test]
fn a_limited_task_says_when_it_resets() {
    let at = local(11, 13, 0); // a Wednesday, 1 PM
    let cfg = Config::default_for_test();
    let lim = |status: &str, who: &str, resets: Option<String>| {
        task("BP-1", status).with(json!({"limit": {"who": who, "resets": resets}}))
    };
    let three = Some(local(11, 15, 0).to_rfc3339());
    let t = lim("working", "worker", three.clone());
    assert!(t.is_limited());
    assert_eq!(task_detail(&t, &cfg, at), "Usage limit, resets 3:00 PM. Continues by itself then.");
    let t = lim("working", "worker", None);
    assert_eq!(task_detail(&t, &cfg, at), "Usage limit, reset time unknown. t tells it to continue.");
    let t = lim("review", "reviewer", three.clone());
    assert_eq!(task_detail(&t, &cfg, at), "Reviewer stopped at the usage limit, resets 3:00 PM. Continues by itself then.");
    let t = lim("review", "reviewer", None);
    assert_eq!(task_detail(&t, &cfg, at), "Reviewer stopped at the usage limit, reset time unknown. ⏎ opens it.");
    // a worker's limit says nothing about a task that has moved on
    assert!(!lim("ready", "worker", three).is_limited());
}

#[test]
fn a_queued_task_says_what_it_waits_on() {
    let after = |id: &str, status: &str, ids: &[&str]| task(id, status).with(json!({"after": ids}));
    let mut tasks: Vec<Task> = vec![
        task("BP-1", "done"),
        task("BP-2", "working"),
        task("BP-3", "dropped"),
        after("BP-4", "todo", &["BP-1", "BP-2"]),
        after("BP-5", "todo", &["BP-1"]),
        after("BP-6", "todo", &["BP-3", "BP-9"]),
        after("BP-7", "working", &["BP-2"]),
    ];
    link_waits(&mut tasks);
    let detail = |i: usize| detail(&tasks[i]);
    assert_eq!(detail(3), "Waits on BP-2.");
    assert_eq!(detail(4), "Not started. s starts it.");
    assert_eq!(detail(5), "Waits on BP-3 (dropped), BP-9 (missing).");
    assert_eq!(detail(6), ""); // started anyway (--force): it waits on nothing now
}
