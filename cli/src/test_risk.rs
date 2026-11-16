use super::*;
use serde_json::json;

fn rules(numstat: &str) -> Part {
    rules_of(numstat, false, None, 40)
}

#[test]
fn docs_or_tests_alone_are_low() {
    assert_eq!(rules("3\t1\tdocs/review.md\n"), part("low", "docs only"));
    assert_eq!(rules("3\t1\tREADME.md\n"), part("low", "docs only"));
    assert_eq!(
        rules("90\t0\ttests/test_review.py\n2\t0\tcli/src/test_risk.rs\n"),
        part("low", "tests only")
    );
    assert_eq!(
        rules("900\t0\ttests/test_x.py\n1\t1\tdocs/x.md\n"),
        part("low", "docs and tests only")
    );
}

#[test]
fn a_prompt_is_at_least_medium() {
    assert_eq!(rules("4\t0\tprompts/reviewer.md\n"), part("medium", "changes a prompt"));
    assert_eq!(
        rules("4\t0\tprompts/reviewer.md\n2\t0\tcli/src/x.rs\n"),
        part("medium", "changes a prompt")
    );
    assert_eq!(
        rules("4\t0\tprompts/reviewer.md\n200\t0\tcli/src/x.rs\n").level,
        "medium"
    );
}

#[test]
fn code_goes_by_its_size() {
    assert_eq!(
        rules("10\t2\tcli/src/x.rs\n"),
        part("low", "a small change (12 lines in 1 file)")
    );
    // docs beside the code don't count toward its size
    assert_eq!(rules("10\t2\tcli/src/x.rs\n300\t0\tdocs/x.md\n").level, "low");
    assert_eq!(
        rules("10\t2\tcli/src/x.rs\n1\t0\tcli/src/y.rs\n1\t0\tcli/src/z.rs\n"),
        part("medium", "14 lines in 3 files")
    );
    assert_eq!(
        rules("300\t101\tcli/src/x.rs\n"),
        part("high", "a large change (401 lines in 1 file)")
    );
    assert_eq!(rules("-\t-\tcli/logo.png\n"), part("medium", "changes a binary file"));
    assert_eq!(rules(""), part("low", "no change"));
}

#[test]
fn a_sensitive_path_is_high_whatever_its_size() {
    let paths = json!({"cli/src/accept/": "changes how accept merges", "bin/": "", "install.sh": "changes install.sh"});
    let r = rules_of("1\t0\tcli/src/accept/mod.rs\n", false, Some(&paths), 40);
    assert_eq!(r, part("high", "changes how accept merges"));
    let r = rules_of("1\t0\tbin/bullpen\n1\t0\tinstall.sh\n", false, Some(&paths), 40);
    assert_eq!(r, part("high", "changes bin/, and 1 more sensitive path"));
    // even in docs or tests
    let list = json!(["docs/secret.md"]);
    assert_eq!(
        rules_of("1\t0\tdocs/secret.md\n", false, Some(&list), 40),
        part("high", "changes docs/secret.md")
    );
}

#[test]
fn a_new_dependency_is_high() {
    assert_eq!(
        rules_of("1\t0\tCargo.toml\n", true, None, 40),
        part("high", "adds a dependency")
    );
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

const MANIFEST: &str = "diff --git a/cli/Cargo.toml b/cli/Cargo.toml\n--- a/cli/Cargo.toml\n+++ b/cli/Cargo.toml\n@@ -1,8 +1,9 @@\n [package]\n name = \"bullpen-cli\"\n-version = \"0.1.0\"\n+version = \"0.2.0\"\n \n [dependencies]\n";

#[test]
fn a_dependency_added_is_new_but_a_version_changed_isnt() {
    assert!(!adds_dependency(&format!(
        "{MANIFEST}-serde_json = \"1.0\"\n+serde_json = \"1.1\"\n chrono = \"0.4\"\n"
    )));
    assert!(adds_dependency(&format!(
        "{MANIFEST} serde_json = \"1.0\"\n+regex = \"1\"\n"
    )));
    assert!(!adds_dependency(MANIFEST)); // only the package's version
    let table = "diff --git a/Cargo.toml b/Cargo.toml\n@@ -1,2 +1,4 @@\n [workspace]\n+[workspace.dependencies.regex]\n+version = \"1\"\n";
    assert!(adds_dependency(table));
    let dev = "diff --git a/x/Cargo.toml b/x/Cargo.toml\n@@ -1,2 +1,3 @@\n [dev-dependencies]\n+tempfile = \"3\"\n";
    assert!(adds_dependency(dev));
    let removed = "diff --git a/x/Cargo.toml b/x/Cargo.toml\n@@ -1,3 +1,2 @@\n [dependencies]\n-tempfile = \"3\"\n";
    assert!(!adds_dependency(removed));
}

#[test]
fn the_reviewer_says_its_level_in_a_risk_part() {
    let text =
        "## Findings\n\nChecked it.\n\n## Risk\n\nmedium: changes when the queue starts a task.\n\n## Notes\n\n- x\n";
    assert_eq!(
        reviewer_of(text),
        Some(part("medium", "changes when the queue starts a task"))
    );
    let said = |body: &str| reviewer_of(&format!("## Findings\n\nok\n\n## Risk\n\n{body}\n"));
    assert_eq!(
        said("- `risk: high · couldn't run the GitHub path`"),
        Some(part("high", "couldn't run the GitHub path"))
    );
    assert_eq!(said("**Low** — a rename"), Some(part("low", "a rename")));
    assert_eq!(said("Med"), Some(part("medium", "")));
    assert_eq!(said("unsure"), None);
    assert_eq!(
        reviewer_of("## Risk: high, the summary's claim about rebase isn't checked\n"),
        Some(part("high", "the summary's claim about rebase isn't checked"))
    );
    assert_eq!(reviewer_of("## Findings\n\nok\n"), None);
}

#[test]
fn the_higher_wins_and_a_tie_says_the_reviewers_why() {
    let (low, med, high) = (
        part("low", "docs only"),
        part("medium", "subtle"),
        part("high", "unchecked"),
    );
    assert_eq!(higher(&high, Some(&low), None), high); // the reviewer can't lower the rules
    assert_eq!(higher(&low, Some(&high), None), high);
    assert_eq!(higher(&med, Some(&part("medium", "theirs")), None).why, "theirs");
    assert_eq!(higher(&low, None, Some(&med)), med);
}

#[test]
fn a_light_review_set_on_a_big_change_is_at_least_medium() {
    let r = |v: serde_json::Value| v.as_object().cloned().unwrap();
    let big = "100\t0\tcli/src/x.rs\n";
    let set = r(json!({"kind": "light", "why": "set for BP-3"}));
    assert_eq!(light_floor(&set, big, 40).map(|p| p.level), Some("medium"));
    assert_eq!(light_floor(&set, "3\t0\tcli/src/x.rs\n", 40), None); // its size calls for a light one anyway
    let prompt = light_floor(&set, "1\t0\tprompts/worker.md\n", 40).unwrap(); // small, but a prompt
    assert_eq!(
        (prompt.level, prompt.why.as_str()),
        ("medium", "a light review, though it changes a prompt")
    );
    assert_eq!(
        light_floor(&r(json!({"kind": "light", "why": "docs only"})), big, 40),
        None
    );
    assert_eq!(
        light_floor(&r(json!({"kind": "full", "why": "set for BP-3"})), big, 40),
        None
    );
}
