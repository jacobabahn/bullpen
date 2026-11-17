use super::*;
use crate::json::dumps_obj;

// Facts about the table, not a copy of it: a new setting needs no edit here.

#[test]
fn each_setting_has_one_name_a_flag_and_help() {
    let names: Vec<&str> = SETTINGS.iter().map(|s| s.name).collect();
    for (i, s) in SETTINGS.iter().enumerate() {
        assert!(!names[..i].contains(&s.name), "{} twice", s.name);
        assert!(
            s.name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "{}",
            s.name
        );
        assert_eq!(s.flag(), format!("--{}", s.name.replace('_', "-")));
        // a setting with a flag says what it does; one without is set by hand, or bullpen's own
        assert!(s.kind == Kind::File || !s.help.is_empty(), "{} has no help", s.name);
        assert!(
            s.default_phrase().is_some(),
            "{}: a null default needs its none",
            s.name
        );
        assert_eq!(s.choices.is_some(), s.kind == Kind::Choice, "{}", s.name);
    }
}

#[test]
fn the_help_and_defaults_read_as_core_pys() {
    // as core.py gave them, one setting of each kind of default
    let help = |n: &str| setting(n).unwrap().full_help();
    assert_eq!(help("merge"), "how accept merges a PR (default rebase)");
    assert_eq!(
        help("max_running"),
        "tasks working at once (review doesn't count); the queue starts more as they hand off (default 3)"
    );
    assert_eq!(
        help("auto_accept"),
        "accept (merge) a task by itself once review approves it (default off)"
    );
    assert_eq!(
        help("worker_model"),
        "claude's --model for workers (\"default\": none, so Claude Code picks) (default the worker command's own)"
    );
    assert_eq!(help("worker"), " (default ['claude', '--permission-mode', 'auto'])");
    assert_eq!(help("coordinators"), " (default [])");
}

#[test]
fn the_defaults_are_the_tables_in_its_order() {
    let d = defaults();
    let keys: Vec<&str> = d.keys().map(String::as_str).collect();
    let names: Vec<&str> = SETTINGS.iter().map(|s| s.name).collect();
    assert_eq!(keys, names);
    for s in SETTINGS.iter() {
        assert_eq!(d[s.name], s.default_value(), "{}", s.name);
    }
    // json.dumps(core.DEFAULT_CONFIG, indent=2) writes them as Python did
    let text = dumps_obj(&d);
    assert!(text.starts_with("{\n  \"prefix\": \"BP\",\n  \"next\": 1,\n  \"base\": null,\n"));
    assert!(text.contains("\"worker\": [\n    \"claude\",\n    \"--permission-mode\",\n    \"auto\"\n  ],\n"));
    assert!(text.contains("\"review\": true,\n") && text.contains("\"coordinators\": [],\n"));
}

#[test]
fn repo_only_is_the_tables() {
    for s in SETTINGS.iter() {
        assert_eq!(is_repo_only(s.name), s.repo_only, "{}", s.name);
    }
    assert!(is_repo_only("prefix") && is_repo_only("coordinators") && !is_repo_only("merge"));
    assert!(!is_repo_only("layered") && !is_repo_only("nope"));
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
