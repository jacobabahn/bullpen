use super::*;
use crate::json::dumps_obj;

/// Each setting's name, flag, default_phrase() and full_help(), as core.py gives them
/// (python3 printed them), in the table's order.
const PYTHON: &str = r#"[
["prefix", "--prefix", "BP", "the letters new task IDs start with (2 to 5 capitals) (default BP)"],
["next", "--next", "1", " (default 1)"],
["base", "--base", "the repo's default branch", "the branch tasks start from and open PRs against (default the repo's default branch)"],
["max_running", "--max-running", "3", "tasks working at once (review doesn't count); the queue starts more as they hand off (default 3)"],
["merge", "--merge", "rebase", "how accept merges a PR (default rebase)"],
["rebase_others", "--rebase-others", "on", "accept rebases the other open tasks onto the new base (default on)"],
["push_remote", "--push-remote", "none", "local mode: push the base branch to this remote (a backup, not origin) after each accept, and when `up` finds it behind (\"\" turns it off) (default none)"],
["auto_accept", "--auto-accept", "off", "accept (merge) a task by itself once review approves it (default off)"],
["setup", "--setup", "none", "a command run in each new worktree before its worker starts, over .bullpen/setup.sh (\"\" clears it) (default none)"],
["teardown", "--teardown", "none", "a command run in a worktree before accept, close or drop removes it, over .bullpen/teardown.sh (\"\" clears it) (default none)"],
["gh", "--gh", "gh on PATH", "the gh program, if not the one on PATH; BULLPEN_GH wins (\"\" clears it) (default gh on PATH)"],
["worker", "--worker", "['claude', '--permission-mode', 'auto']", " (default ['claude', '--permission-mode', 'auto'])"],
["coordinator_cmd", "--coordinator-cmd", "['claude', '--permission-mode', 'auto']", " (default ['claude', '--permission-mode', 'auto'])"],
["reviewer", "--reviewer", "['claude', '--permission-mode', 'auto']", " (default ['claude', '--permission-mode', 'auto'])"],
["tour", "--tour", "['claude']", " (default ['claude'])"],
["workers", "--workers", "float", "new workers as capsules in the coordinator's tab (float), as capsules in one workers tab (tab), a tab each (tabs), or in the background: in no tab until `go` shows one (default float)"],
["reviewers", "--reviewers", "background", "new reviewers the same way (float, tab, tabs, background), or where workers go (same) (default background)"],
["worker_model", "--worker-model", "the worker command's own", "claude's --model for workers (\"default\": none, so Claude Code picks) (default the worker command's own)"],
["reviewer_model", "--reviewer-model", "the reviewer command's own", "the same, for reviewers (a full first review) (default the reviewer command's own)"],
["reviewer_followup_model", "--reviewer-followup-model", "sonnet", "the same, for later review rounds and light reviews (default sonnet)"],
["tour_model", "--tour-model", "sonnet", "claude's --model for the agent that writes guided review tours (default sonnet)"],
["review", "--review", "on", "a reviewer agent checks each task before it's ready (default on)"],
["review_rounds", "--review-rounds", "2", "reviews a task gets before it is ready anyway, findings listed (default 2)"],
["light_review_lines", "--light-review-lines", "40", "a change of at most this many lines in at most 2 files gets a light review (default 40)"],
["risk_large_lines", "--risk-large-lines", "400", "more changed lines than this in a medium risk path make a ready task's risk high (default 400)"],
["risk_huge_lines", "--risk-huge-lines", "1200", "more changed lines of code than this make a ready task's risk high whatever it touches (0: never) (default 1200)"],
["risk_paths", "--risk-paths", "none", "the paths whose change makes a ready task's risk high or medium, each with why: set with `config --risk-path` (docs/review.md) (default none)"],
["board_layout", "--board-layout", "grouped", "how the board lists tasks (default grouped)"],
["board_bottom_gap", "--board-bottom-gap", "1 while workers or reviewers float, else 0", "empty rows at the board's bottom, under Rex's minimized-pane capsules (0: none) (default 1 while workers or reviewers float, else 0)"],
["board_repos", "--board-repos", "on", "the board's top line counts the tasks that need you or are ready in other repositories (default on)"],
["news", "--news", "type", "how news reaches the coordinator: typed into its pane (type), by a Claude Code hook that wakes it, written to the repo's .claude/settings.local.json (wake), or read by its Monitor tool from `bullpen news --follow` (monitor) (default type)"],
["notify", "--notify", "needs", "native notifications: when a task needs you (needs), also when one is ready, finished or auto-accepted (all), or none (off) (default needs)"],
["stuck_after", "--stuck-after", "20", "news when a working agent shows no progress this long (0: never) (default 20)"],
["triage_minutes", "--triage-minutes", "10", "how long a task can wait on its coordinator before it needs the person (default 10)"],
["pickup", "--pickup", "off", "start backlog items by themselves when there's room (docs/backlog.md) (default off)"],
["pickup_running", "--pickup-running", "1", "self-started tasks working or in review at once (default 1)"],
["pickup_waiting", "--pickup-waiting", "2", "pickup pauses while this many self-started tasks wait for you (default 2)"],
["pickup_per_day", "--pickup-per-day", "4", "self-started tasks a day, a spending guard (default 4)"],
["pickup_model", "--pickup-model", "the worker model", "claude's --model for self-started tasks (default the worker model)"],
["pickup_auto_accept", "--pickup-auto-accept", "off", "self-started tasks accept by themselves once review approves them (default off)"],
["issues", "--issues", "off", "read this repository's GitHub issues into the backlog, each asking for a yes (docs/issues.md) (default off)"],
["issues_repo", "--issues-repo", "origin's, or in local mode the push remote's", "the GitHub repository issues are read from, as OWNER/REPO (\"\" clears it) (default origin's, or in local mode the push remote's)"],
["issues_every", "--issues-every", "30", "minutes between the watcher's reads of the GitHub issues (default 30)"],
["coordinators", "--coordinators", "[]", " (default [])"],
["next_coordinator", "--next-coordinator", "1", " (default 1)"]
]"#;

/// json.dumps(core.DEFAULT_CONFIG, indent=2).
const DEFAULT_CONFIG: &str = r#"{
  "prefix": "BP",
  "next": 1,
  "base": null,
  "max_running": 3,
  "merge": "rebase",
  "rebase_others": true,
  "push_remote": null,
  "auto_accept": false,
  "setup": null,
  "teardown": null,
  "gh": null,
  "worker": [
    "claude",
    "--permission-mode",
    "auto"
  ],
  "coordinator_cmd": [
    "claude",
    "--permission-mode",
    "auto"
  ],
  "reviewer": [
    "claude",
    "--permission-mode",
    "auto"
  ],
  "tour": [
    "claude"
  ],
  "workers": "float",
  "reviewers": "background",
  "worker_model": null,
  "reviewer_model": null,
  "reviewer_followup_model": "sonnet",
  "tour_model": "sonnet",
  "review": true,
  "review_rounds": 2,
  "light_review_lines": 40,
  "risk_large_lines": 400,
  "risk_huge_lines": 1200,
  "risk_paths": null,
  "board_layout": "grouped",
  "board_bottom_gap": null,
  "board_repos": true,
  "news": "type",
  "notify": "needs",
  "stuck_after": 20,
  "triage_minutes": 10,
  "pickup": false,
  "pickup_running": 1,
  "pickup_waiting": 2,
  "pickup_per_day": 4,
  "pickup_model": null,
  "pickup_auto_accept": false,
  "issues": false,
  "issues_repo": null,
  "issues_every": 30,
  "coordinators": [],
  "next_coordinator": 1
}"#;

#[test]
fn the_table_is_core_pys() {
    let want: Vec<(String, String, Option<String>, String)> = serde_json::from_str(PYTHON).unwrap();
    assert_eq!(want.len(), SETTINGS.len());
    for (s, (name, flag, phrase, help)) in SETTINGS.iter().zip(want) {
        assert_eq!(s.name, name);
        assert_eq!(s.flag(), flag, "{name}");
        assert_eq!(s.default_phrase(), phrase, "{name}");
        assert_eq!(s.full_help(), help, "{name}");
    }
}

#[test]
fn the_defaults_are_core_pys() {
    assert_eq!(dumps_obj(&defaults()), DEFAULT_CONFIG);
}

#[test]
fn repo_only_is_the_tables() {
    let from_table: Vec<&str> = SETTINGS.iter().filter(|s| s.repo_only).map(|s| s.name).collect();
    assert_eq!(from_table, REPO_ONLY);
    assert!(is_repo_only("prefix") && !is_repo_only("merge"));
    assert!(is_internal("layered") && !is_internal("prefix"));
}

#[test]
fn settings_by_name_and_kind() {
    let s = setting("max_running").unwrap();
    assert_eq!(
        (s.kind, s.kind.as_str(), s.least, s.metavar),
        (Kind::Number, "number", Some(1), Some("N"))
    );
    assert_eq!(
        setting("reviewers").unwrap().choices.unwrap(),
        ["same", "float", "tab", "tabs", "background"]
    );
    assert_eq!(setting("rebase_others").unwrap().kind.as_str(), "on/off");
    assert!(setting("nope").is_none());
}

#[test]
fn the_prefix_pattern_matches_as_re_fullmatch() {
    let p = setting("prefix").unwrap();
    for ok in ["BP", "ABCDE", "XYZ"] {
        assert!(p.matches(ok), "{ok}");
    }
    for bad in ["B", "ABCDEF", "bp", "B1", "", "BP\n", "ÉT"] {
        assert!(!p.matches(bad), "{bad}");
    }
    assert!(setting("setup").unwrap().matches("anything"));
    // a new pattern needs its own matcher in pattern_matches
    let patterns: Vec<&str> = SETTINGS.iter().filter_map(|s| s.pattern).collect();
    assert_eq!(patterns, ["[A-Z]{2,5}"]);
}
