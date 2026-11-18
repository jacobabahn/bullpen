use super::*;

const FILE: &str = "## Findings\n\nChecked the parser and ran its tests.\n\n## Notes\n\n- `docs/x.md` still says y.\n- A follow-up: z deserves a task.\n";

#[test]
fn notes_are_the_notes_part() {
    let notes = notes_of(FILE).unwrap();
    assert_eq!(notes, "- `docs/x.md` still says y.\n- A follow-up: z deserves a task.");
    assert_eq!(count(&notes), 2);
}

#[test]
fn no_notes_part_or_none_is_no_notes() {
    assert_eq!(notes_of("## Findings\n\nAll good."), None);
    assert_eq!(notes_of("Approved: checked a and b."), None);
    assert_eq!(notes_of("## Findings\n\nok\n\n## Notes\n\nNone.\n"), None);
    assert_eq!(notes_of("## Notes\n\n## Findings\n\nok"), None);
}

#[test]
fn notes_end_at_the_next_heading() {
    let text = "## Notes\n\nOne thing.\n\nAnother thing.\n\n## Findings\n\nThe bug.";
    let notes = notes_of(text).unwrap();
    assert_eq!(notes, "One thing.\n\nAnother thing.");
    assert_eq!(count(&notes), 2);
}

#[test]
fn numbered_items_count_and_nested_ones_dont() {
    assert_eq!(count("1. a\n   - detail\n2) b"), 2);
}

#[test]
fn without_notes_keeps_only_the_findings() {
    assert_eq!(
        without_notes(FILE),
        "## Findings\n\nChecked the parser and ran its tests."
    );
    let after = "## Notes\n\n- x\n\n## Findings\n\nThe bug.";
    assert_eq!(without_notes(after), "## Findings\n\nThe bug.");
    assert_eq!(without_notes("Just findings."), "Just findings.");
}

#[test]
fn with_notes_reads_the_last_findings_file() {
    let dir = std::env::temp_dir().join(format!("bullpen-notes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("BP-1.review-1.findings.md");
    std::fs::write(&f, FILE).unwrap();
    let t = serde_json::json!({"review": {"findings": f.display().to_string()}});
    assert_eq!(with_notes(t.as_object().unwrap()), " with 2 notes");
    std::fs::write(&f, "## Findings\n\nok\n\n## Notes\n\n- one\n").unwrap();
    assert_eq!(with_notes(t.as_object().unwrap()), " with 1 note");
    std::fs::write(&f, "## Findings\n\nok\n").unwrap();
    assert_eq!(with_notes(t.as_object().unwrap()), "");
    let none = serde_json::json!({"review": {"findings": null}});
    assert_eq!(with_notes(none.as_object().unwrap()), "");
    std::fs::remove_dir_all(&dir).unwrap();
}
