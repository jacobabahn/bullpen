use super::checks::{json_error, repo_scope, rex_version_key, shebang_program, shlex_split, REX_MIN};
use super::{problems_phrase, summary};

#[test]
fn test_rex_versions_compare_previews_before_releases() {
    let key = |t: &str| rex_version_key(t);
    assert!(key("0.1.0-preview.12").unwrap() < REX_MIN);
    assert_eq!(key("0.1.0-preview.13"), Some(REX_MIN));
    assert!(key("0.1.0").unwrap() > key("0.1.0-preview.99").unwrap());
    assert!(key("v0.2.0-preview.1").unwrap() > REX_MIN);
    assert_eq!(key("dev"), None);
}

#[test]
fn rex_versions_that_arent_ones() {
    for t in [
        "",
        "0.1",
        "0.1.0.2",
        "0.1.0-preview.",
        "0.1.0-beta.1",
        "x0.1.0",
        "0.1.0-preview.1x",
        "vv0.1.0",
    ] {
        assert_eq!(rex_version_key(t), None, "{t}");
    }
    assert_eq!(rex_version_key("  v1.2.3\n"), Some((1, 2, 3, u64::MAX)));
}

#[test]
fn test_a_shebangs_program_skips_envs_options() {
    let prog = |argv: &[&str]| shebang_program(argv);
    assert_eq!(prog(&["/bin/bash", "-e"]).as_deref(), Some("/bin/bash"));
    assert_eq!(prog(&["/usr/bin/env", "bash"]).as_deref(), Some("bash"));
    assert_eq!(prog(&["/usr/bin/env", "-S", "bash", "-e"]).as_deref(), Some("bash"));
    assert_eq!(prog(&["/usr/bin/env", "-Sbash", "-e"]).as_deref(), Some("bash"));
    assert_eq!(
        prog(&["/usr/bin/env", "-i", "FOO=1", "-u", "BAR", "python3"]).as_deref(),
        Some("python3")
    );
    assert_eq!(prog(&["/usr/bin/env", "-S"]), None);
}

fn words(text: &str) -> Option<Vec<String>> {
    shlex_split(text)
}

fn are(list: &[&str]) -> Option<Vec<String>> {
    Some(list.iter().map(|s| s.to_string()).collect())
}

#[test]
fn shlex_splits_words_as_python_does() {
    assert_eq!(words("cd . && echo hi"), are(&["cd", ".", "&&", "echo", "hi"]));
    assert_eq!(words("  a\tb\n"), are(&["a", "b"]));
    assert_eq!(words(""), are(&[]));
    assert_eq!(words("'' a"), are(&["", "a"]));
    assert_eq!(words("'my prog' x"), are(&["my prog", "x"]));
    assert_eq!(words("a'b c'd"), are(&["ab cd"]));
    assert_eq!(words(r#""a \"b\" \\ \x""#), are(&[r#"a "b" \ \x"#]));
    assert_eq!(words(r"a\ b c"), are(&["a b", "c"]));
    assert_eq!(words("FOO=1 make"), are(&["FOO=1", "make"]));
    assert_eq!(words("a;b|c"), are(&["a;b|c"]));
}

#[test]
fn shlex_fails_where_python_raises() {
    assert_eq!(words("echo 'hi"), None);
    assert_eq!(words("echo \"hi"), None);
    assert_eq!(words("echo hi\\"), None);
    assert_eq!(words("ok 'later unclosed"), None);
    assert_eq!(words("\"a\\"), None);
}

#[test]
fn problems_are_counted_in_words() {
    assert_eq!(problems_phrase(1), "1 problem");
    assert_eq!(problems_phrase(2), "2 problems");
    assert_eq!(problems_phrase(0), "0 problems");
}

fn decode_error(text: &str) -> String {
    let e = serde_json::from_str::<serde_json::Value>(text).unwrap_err();
    json_error(text, &e)
}

#[test]
fn json_errors_read_as_pythons() {
    // what Python's json.loads says of each
    let cases = [
        ("", "Expecting value: line 1 column 1 (char 0)"),
        ("{} x", "Extra data: line 1 column 4 (char 3)"),
        ("{\n  \"a\" 1}", "Expecting ':' delimiter: line 2 column 7 (char 8)"),
        (
            "{",
            "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)",
        ),
        ("{\"a\":1", "Expecting ',' delimiter: line 1 column 7 (char 6)"),
        ("[1,2", "Expecting ',' delimiter: line 1 column 5 (char 4)"),
        (
            "{\"a\":1,}",
            "Illegal trailing comma before end of object: line 1 column 7 (char 6)",
        ),
        (
            "[1,]",
            "Illegal trailing comma before end of array: line 1 column 3 (char 2)",
        ),
        ("{\"a", "Unterminated string starting at: line 1 column 2 (char 1)"),
        ("nul", "Expecting value: line 1 column 1 (char 0)"),
        (
            "{1:2}",
            "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)",
        ),
        ("{\"a\":", "Expecting value: line 1 column 6 (char 5)"),
        (
            "{\"a\":\"x\\\"y",
            "Unterminated string starting at: line 1 column 6 (char 5)",
        ),
        (
            "{\n\"a\":1,\n}",
            "Illegal trailing comma before end of object: line 2 column 6 (char 7)",
        ),
        (
            "\u{feff}{}",
            "Unexpected UTF-8 BOM (decode using utf-8-sig): line 1 column 1 (char 0)",
        ),
        ("{\"a\":1}\n{}", "Extra data: line 2 column 1 (char 8)"),
    ];
    for (text, said) in cases {
        assert_eq!(decode_error(text), said, "{text:?}");
    }
}

#[test]
fn the_last_line_names_the_problems() {
    assert_eq!(summary(&[]), "All good.");
    let failed = ["keys".to_string(), "shop/coordinators".to_string()];
    assert_eq!(summary(&failed), "2 problems: keys, shop/coordinators.");
}

#[test]
fn gh_says_whether_the_token_in_use_has_the_repo_scope() {
    let said = |scopes: &str| {
        format!("github.com\n  ✓ Logged in to github.com account me (keyring)\n  - Active account: true\n  - Token scopes: {scopes}\n")
    };
    assert_eq!(repo_scope(&said("'gist', 'read:org', 'repo', 'workflow'")), Some(true));
    assert_eq!(repo_scope(&said("gist, read:org, repo")), Some(true)); // an older gh: no quotes
    assert_eq!(repo_scope(&said("'gist', 'public_repo'")), Some(false));
    // can't tell: no scopes named (a fine-grained token, a token from the environment)
    assert_eq!(repo_scope(&said("none")), None);
    assert_eq!(
        repo_scope("github.com\n  ✓ Logged in to github.com account me (GH_TOKEN)\n"),
        None
    );
    // a second account that isn't the one in use doesn't count, either way
    let other =
        "  ✓ Logged in to github.com account other (keyring)\n  - Active account: false\n  - Token scopes: 'repo'\n";
    assert_eq!(repo_scope(&format!("{}{other}", said("'gist'"))), Some(false));
    assert_eq!(repo_scope(&format!("{}{other}", said("'repo'"))), Some(true));
}
