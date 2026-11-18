use super::*;

#[test]
fn the_line_names_the_first_file_and_counts_the_rest() {
    let files = vec!["a.rs".to_string(), "b.rs".into(), "c.rs".into()];
    assert_eq!(
        overlap_line("BP-2 BP-10", &files, false),
        "BP-2 and BP-10 both changed a.rs (and 2 more)"
    );
    assert_eq!(
        overlap_line("BP-2 BP-10", &files[..1], true),
        "BP-2 and BP-10 both changed a.rs too"
    );
}

#[test]
fn a_pair_is_in_number_order() {
    assert_eq!(pair_key("BP-10", "BP-9"), "BP-9 BP-10");
    assert_eq!(pair_key("BP-3", "BP-5"), "BP-3 BP-5");
}

#[test]
fn lock_files_and_changelogs_never_overlap() {
    assert!(ignored("Cargo.lock"));
    assert!(ignored("web/package-lock.json"));
    assert!(ignored("CHANGELOG.md"));
    assert!(!ignored("cli/src/accept/mod.rs"));
}
