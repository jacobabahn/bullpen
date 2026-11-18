use super::*;
use serde_json::json;

const SIZES: Sizes = Sizes {
    small: 40,
    large: 400,
    huge: 1200,
};

/// The changed files of numstat rows written one to a line.
fn rows(numstat: &str) -> Vec<Changed> {
    crate::git_names::numstat_of(&numstat.replace('\n', "\0"))
}

fn rules(numstat: &str) -> Part {
    rules_of(&rows(numstat), false, None, SIZES)
}

fn with(paths: &serde_json::Value, numstat: &str) -> Part {
    rules_of(&rows(numstat), false, Some(paths), SIZES)
}

#[test]
fn docs_or_tests_alone_are_low() {
    assert_eq!(rules("3\t1\tdocs/review.md\n"), part("low", "docs only"));
    assert_eq!(rules("3\t1\tREADME.md\n"), part("low", "docs only"));
    assert_eq!(
        rules("90\t0\ttests/test_review.py\n2\t0\tcli/src/tests/test_risk.rs\n"),
        part("low", "tests only")
    );
    assert_eq!(
        rules("9000\t0\ttests/test_x.py\n1\t1\tdocs/x.md\n"),
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
fn size_alone_makes_a_change_medium_and_high_only_far_past_the_line() {
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
    // what was "a large change", high: medium now
    assert_eq!(rules("300\t101\tcli/src/x.rs\n"), part("medium", "401 lines in 1 file"));
    assert_eq!(rules("1200\t0\tcli/src/x.rs\n").level, "medium");
    assert_eq!(
        rules("1200\t1\tcli/src/x.rs\n"),
        part("high", "a very large change (1201 lines in 1 file)")
    );
    // risk_huge_lines 0: size alone never makes it high
    let never = Sizes { huge: 0, ..SIZES };
    assert_eq!(
        rules_of(&rows("9000\t0\tcli/src/x.rs\n"), false, None, never).level,
        "medium"
    );
    assert_eq!(rules("-\t-\tcli/logo.png\n"), part("medium", "changes a binary file"));
    assert_eq!(rules(""), part("low", "no change"));
}

#[test]
fn a_high_area_is_high_whatever_its_size_and_says_its_lines() {
    let paths = json!({"cli/src/accept/": "changes how accept merges", "bin/": "", "install.sh": "changes install.sh"});
    assert_eq!(
        with(&paths, "2\t1\tcli/src/accept/mod.rs\n"),
        part("high", "changes how accept merges (3 lines)")
    );
    // the lines in the area, not the change's
    assert_eq!(
        with(
            &paths,
            "200\t50\tcli/src/accept/mod.rs\n50\t0\tcli/src/accept/rebase.rs\n500\t0\tcli/src/x.rs\n"
        ),
        part("high", "changes how accept merges (300 lines)")
    );
    // several: the one most changed, and how many more
    assert_eq!(
        with(
            &paths,
            "1\t0\tbin/bullpen\n4\t0\tinstall.sh\n1\t0\tcli/src/accept/mod.rs\n"
        ),
        part("high", "changes install.sh (4 lines), and 2 more sensitive paths")
    );
    // a test beside the area's code isn't the area: tests alone are low
    assert_eq!(
        with(&paths, "40\t0\tcli/src/accept/tests/test_rebase.rs\n"),
        part("low", "tests only")
    );
    assert_eq!(
        with(
            &paths,
            "40\t0\tcli/src/accept/tests/test_rebase.rs\n2\t0\tcli/src/accept/rebase.rs\n"
        ),
        part("high", "changes how accept merges (2 lines)")
    );
    // unless the pattern names tests
    assert_eq!(
        with(
            &json!({"tests/fakes/": "changes the fakes"}),
            "3\t0\ttests/fakes/rex.rs\n"
        ),
        part("high", "changes the fakes (3 lines)")
    );
    // a folder named without its /, too
    for folder in ["tests", "cli/tests", "cli/tests/"] {
        let numstat = format!("3\t0\t{}/fakes/rex.rs\n", folder.trim_end_matches('/'));
        assert_eq!(
            with(&json!({folder: "changes the fakes"}), &numstat),
            part("high", "changes the fakes (3 lines)"),
            "{folder}"
        );
    }
    // docs count, and a bare list reads
    let list = json!(["docs/secret.md"]);
    assert_eq!(
        with(&list, "1\t0\tdocs/secret.md\n"),
        part("high", "changes docs/secret.md (1 line)")
    );
    assert_eq!(
        with(&list, "-\t-\tdocs/secret.md\n"),
        part("high", "changes docs/secret.md")
    );
}

#[test]
fn a_medium_area_goes_by_how_much_of_it_is_touched() {
    let paths = json!({
        "cli/src/review.rs": {"level": "medium", "why": "changes how review runs"},
        "cli/src/accept/": {"level": "high", "why": "changes how accept merges"},
    });
    // a tiny touch doesn't count: the change is what its size says
    assert_eq!(
        with(&paths, "8\t2\tcli/src/review.rs\n"),
        part("low", "a small change (10 lines in 1 file)")
    );
    assert_eq!(
        with(&paths, "8\t3\tcli/src/review.rs\n"),
        part("medium", "changes how review runs (11 lines)")
    );
    // over its size's medium: the area is the why
    assert_eq!(
        with(&paths, "100\t0\tcli/src/review.rs\n300\t0\tcli/src/x.rs\n"),
        part("medium", "changes how review runs (100 lines)")
    );
    // a large change there is a step up
    assert_eq!(with(&paths, "400\t0\tcli/src/review.rs\n").level, "medium");
    assert_eq!(
        with(&paths, "400\t1\tcli/src/review.rs\n"),
        part("high", "changes how review runs, and a lot of it (401 lines)")
    );
    // the most sensitive area is the why, whatever the other's size
    assert_eq!(
        with(&paths, "100\t0\tcli/src/review.rs\n1\t0\tcli/src/accept/mod.rs\n"),
        part("high", "changes how accept merges (1 line), and 1 more sensitive path")
    );
    // a tiny touch of the medium one isn't counted among them
    assert_eq!(
        with(&paths, "1\t0\tcli/src/review.rs\n1\t0\tcli/src/accept/mod.rs\n"),
        part("high", "changes how accept merges (1 line)")
    );
    // a very large change that touches a medium area: its size is higher
    assert_eq!(
        with(&paths, "20\t0\tcli/src/review.rs\n1300\t0\tcli/src/x.rs\n"),
        part("high", "a very large change (1320 lines in 2 files)")
    );
    // a binary file in a medium area counts
    assert_eq!(
        with(&paths, "-\t-\tcli/src/review.rs\n"),
        part("medium", "changes how review runs")
    );
}

#[test]
fn a_new_dependency_is_high() {
    assert_eq!(
        rules_of(&rows("1\t0\tCargo.toml\n"), true, None, SIZES),
        part("high", "adds a dependency")
    );
    // beside a medium area, it is the why
    let paths = json!({"Cargo.toml": {"level": "medium", "why": "changes the manifest"}});
    assert_eq!(
        rules_of(&rows("20\t0\tCargo.toml\n"), true, Some(&paths), SIZES),
        part("high", "adds a dependency")
    );
}

#[test]
fn the_sizes_come_from_the_settings() {
    let cfg = bullpen_core::settings::defaults();
    let s = Sizes::of(&cfg);
    assert_eq!((s.small, s.large, s.huge), (40, 400, 1200));
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
    let big = &rows("100\t0\tcli/src/x.rs\n");
    let set = r(json!({"kind": "light", "why": "set for BP-3"}));
    assert_eq!(light_floor(&set, big, 40).map(|p| p.level), Some("medium"));
    assert_eq!(light_floor(&set, &rows("3\t0\tcli/src/x.rs\n"), 40), None); // its size calls for a light one anyway
    let prompt = light_floor(&set, &rows("1\t0\tprompts/worker.md\n"), 40).unwrap(); // small, but a prompt
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
