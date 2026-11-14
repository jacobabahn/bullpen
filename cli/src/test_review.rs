use super::*;

#[test]
fn own_lines_hash_each_files_lines_without_context() {
    let diff = "diff --git a/a.py b/a.py\nindex 1..2 100644\n--- a/a.py\n+++ b/a.py\n@@ -0,0 +1 @@\n+x = 1\n\
                diff --git a/b.md b/b.md\nnew file mode 100644\nindex 0..3\n--- /dev/null\n+++ b/b.md\n@@ -0,0 +1 @@\n+hi\n";
    let own = own_lines_of(diff);
    // what review.py gives (its hashlib.sha1), in the diff's order; the last file ends with the blank line after the diff
    let want = json!({"a.py": "9affda4cff351d59a27ec0d606e8e3a4ba09608e",
                      "b.md": "f9abf8bd34122392e1d6fb3881b0c526aa0b2221"});
    assert_eq!(own.keys().collect::<Vec<_>>(), ["a.py", "b.md"]);
    assert_eq!(Value::Object(own), want);
}

#[test]
fn own_lines_of_no_diff_is_empty() {
    assert!(own_lines_of("").is_empty());
}

#[test]
fn change_size_counts_files_and_lines() {
    assert_eq!(change_size_of("3\t1\tsrc/a.rs\n2\t0\tdocs/x.md\n"), (2, Some(6), false));
    assert_eq!(
        change_size_of("3\t1\tdocs/a.md\n1\t1\tprompts/p.txt\n"),
        (2, Some(6), true)
    );
    assert_eq!(change_size_of("-\t-\timg.png\n1\t1\ta.rs\n"), (2, None, false));
    assert_eq!(change_size_of(""), (0, Some(0), false));
}

#[test]
fn first_review_is_light_for_docs_or_a_small_change() {
    assert_eq!(
        kind_by_size(1, Some(400), true, 20),
        ("light".into(), Some("docs only".into()))
    );
    assert_eq!(
        kind_by_size(1, Some(1), false, 20),
        ("light".into(), Some("1 line in 1 file".into()))
    );
    assert_eq!(
        kind_by_size(2, Some(20), false, 20),
        ("light".into(), Some("20 lines in 2 files".into()))
    );
    assert_eq!(kind_by_size(3, Some(5), false, 20), ("full".into(), None));
    assert_eq!(kind_by_size(1, Some(21), false, 20), ("full".into(), None));
    assert_eq!(kind_by_size(1, None, false, 20), ("full".into(), None));
}

#[test]
fn review_kind_in_words() {
    let r = |v: Value| v.as_object().unwrap().clone();
    assert_eq!(review_kind(&r(json!({}))), "review");
    assert_eq!(
        review_kind(&r(json!({"kind": "light", "why": "docs only"}))),
        "light review (docs only)"
    );
    assert_eq!(
        review_kind(&r(json!({"kind": "followup", "why": null}))),
        "follow-up review"
    );
    assert_eq!(
        review_kind(&r(json!({"kind": "tour"}))),
        "review of the person's comments"
    );
    assert_eq!(review_kind(&r(json!({"kind": "rebase"}))), "rebase check");
    assert_eq!(review_kind(&r(json!({"kind": "rebased"}))), "review after a rebase");
}

#[test]
fn the_header_says_of_how_many_rounds_only_for_a_counted_round_within_the_limit() {
    assert_eq!(of_rounds(2, 2, "followup"), " of 2");
    assert_eq!(of_rounds(3, 2, "rebased"), "");
    assert_eq!(of_rounds(1, 2, "rebase"), "");
    assert_eq!(of_rounds(1, 2, "tour"), "");
}

#[test]
fn shortstat_in_a_few_words() {
    let line = " 3 files changed, 120 insertions(+), 14 deletions(-)\n";
    assert_eq!(shortstat_of(line).as_deref(), Some("3 files, +120 −14"));
    assert_eq!(
        shortstat_of(" 1 file changed, 2 insertions(+)\n").as_deref(),
        Some("1 file, +2 −0")
    );
    assert_eq!(
        shortstat_of(" 1 file changed, 1 deletion(-)\n").as_deref(),
        Some("1 file, +0 −1")
    );
    assert_eq!(shortstat_of(""), None);
}

#[test]
fn a_clipped_diff_says_what_to_run_when_too_long() {
    assert_eq!(clipped_diff("  \n", "git diff"), "(no changes)");
    assert_eq!(clipped_diff("a\nb\n", "git diff"), "a\nb");
    let long = "x\n".repeat(MAX_DIFF_LINES + 1);
    assert_eq!(
        clipped_diff(&long, "git diff"),
        "(801 lines, too long to include: run `git diff`)"
    );
}

#[test]
fn docs_are_docs_prompts_and_markdown() {
    assert!(is_doc("docs/a.txt") && is_doc("prompts/x") && is_doc("README.md"));
    assert!(!is_doc("src/main.rs"));
}
