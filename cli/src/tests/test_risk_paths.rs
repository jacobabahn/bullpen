use super::*;

fn e(pattern: &str, level: &'static str, why: &str) -> Entry {
    Entry {
        pattern: pattern.into(),
        level,
        why: why.into(),
    }
}

#[test]
fn each_form_of_risk_paths_reads() {
    let levels = json!({
        "cli/src/accept/": {"level": "high", "why": "changes how accept merges"},
        "cli/src/review.rs": {"level": "medium", "why": " changes how review runs "},
        "lua/": {"level": "medium"},
        "bin/": {"why": "changes what everything runs"},
        "odd": {"level": "extreme", "why": ""},
    });
    assert_eq!(
        entries(Some(&levels)),
        vec![
            e("cli/src/accept/", "high", "changes how accept merges"),
            e("cli/src/review.rs", "medium", "changes how review runs"),
            e("lua/", "medium", "changes lua/"),
            e("bin/", "high", "changes what everything runs"), // no level: high
            e("odd", "high", "changes odd"),                   // nor one bullpen knows
        ]
    );
    // the forms from before levels: a why, or a bare list, are high
    let whys = json!({"cli/src/accept/": "changes how accept merges", "bin/": ""});
    assert_eq!(
        entries(Some(&whys)),
        vec![
            e("cli/src/accept/", "high", "changes how accept merges"),
            e("bin/", "high", "changes bin/")
        ]
    );
    assert_eq!(
        entries(Some(&json!(["install.sh"]))),
        vec![e("install.sh", "high", "changes install.sh")]
    );
    assert_eq!(entries(Some(&Value::Null)), vec![]);
    assert_eq!(entries(None), vec![]);
}

#[test]
fn a_path_is_in_its_most_sensitive_area() {
    let all = vec![
        e("cli/", "medium", "the command line"),
        e("cli/src/accept/", "high", "accept"),
        e("cli/src/", "high", "the source"),
    ];
    assert_eq!(area_of(&all, "cli/src/accept/mod.rs"), Some(1)); // the first of the high ones
    assert_eq!(area_of(&all, "cli/src/show.rs"), Some(2));
    assert_eq!(area_of(&all, "cli/Cargo.toml"), Some(0));
    assert_eq!(area_of(&all, "core/src/lib.rs"), None);
}

#[test]
fn patterns() {
    assert!(path_matches("cli/src/accept/", "cli/src/accept/mod.rs"));
    assert!(!path_matches("cli/src/accept/", "cli/src/accept.rs"));
    assert!(path_matches("bin", "bin/bullpen"));
    assert!(path_matches("install.sh", "install.sh"));
    assert!(!path_matches("install.sh", "tests/install.sh"));
    assert!(path_matches("*Cargo.lock", "Cargo.lock"));
    assert!(path_matches("cli/src/*accept*.rs", "cli/src/auto_accept.rs"));
    assert!(!path_matches("cli/src/*.rs", "core/src/x.rs"));
}
