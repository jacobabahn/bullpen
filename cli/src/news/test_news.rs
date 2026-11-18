use super::*;

#[test]
fn faint_follows_sgr() {
    assert!(sgr_faint("2", false));
    assert!(!sgr_faint("", true)); // \x1b[m resets
    assert!(!sgr_faint("22", true));
    assert!(sgr_faint("1;2", false));
    assert!(sgr_faint("38;5;2", true)); // a colour's own 2 isn't faint, and doesn't reset it
    assert!(!sgr_faint("38;2;2;2;2", false));
    assert!(sgr_faint("38:5:2;2", false));
}

#[test]
fn a_line_in_pieces_by_faintness() {
    let mut faint = false;
    let pieces = styled_line("❯ \x1b[2mTry \"fix\"\x1b[0m x\x1b[3Cy\r", &mut faint);
    let want = [
        ("❯ ", false),
        ("Try \"fix\"", true),
        (" x", false),
        ("   ", false),
        ("y", false),
    ];
    assert_eq!(pieces, want.map(|(t, f)| (t.to_string(), f)));
    // faint carries on to the next line
    let mut faint = false;
    styled_line("\x1b[2ma", &mut faint);
    assert_eq!(styled_line("b", &mut faint), [("b".to_string(), true)]);
}

#[test]
fn a_draft_is_what_the_person_typed_at_the_prompt() {
    assert!(draft_in("output\n❯ half a thought\n"));
    assert!(!draft_in("output\n❯ \x1b[2mTry \"fix the bug\"\x1b[0m\n"));
    assert!(!draft_in("❯ \n\n"));
    assert!(!draft_in("no prompt here\n"));
    // the last line starting ❯ counts, of the last ten that aren't blank
    assert!(!draft_in("❯ old\n❯ \n"));
    let far = format!("❯ typed\n{}", "line\n".repeat(10));
    assert!(!draft_in(&far));
}

#[test]
fn escapes_as_vt_esc_matches_them() {
    assert!(matches!(vt_esc("\x1b[1;2m"), Some((6, Esc::Sgr(p))) if p == "1;2"));
    assert!(matches!(vt_esc("\x1b[C"), Some((3, Esc::Forward(d))) if d.is_empty()));
    assert!(matches!(vt_esc("\x1b[?25h"), Some((6, Esc::Other))));
    assert!(matches!(vt_esc("\x1b[ x"), Some((4, Esc::Other))));
    assert!(matches!(vt_esc("\x1b[\x01"), Some((2, Esc::Other)))); // ESC [ alone
    assert!(matches!(vt_esc("\x1bM"), Some((2, Esc::Other))));
    assert!(vt_esc("\x1b!").is_none());
    assert!(vt_esc("x").is_none());
}

#[test]
fn outboxes_are_found_in_order() {
    let dir = std::env::temp_dir().join(format!("bullpen-outboxes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in [
        "outbox.txt",
        "outbox-c2.txt",
        "outbox-c1.txt",
        "other.txt",
        "outbox.log",
    ] {
        std::fs::write(dir.join(name), "").unwrap();
    }
    let names: Vec<String> = outboxes(&dir)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["outbox-c1.txt", "outbox-c2.txt", "outbox.txt"]);
    std::fs::remove_dir_all(&dir).unwrap();
}
