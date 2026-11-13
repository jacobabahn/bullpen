use super::*;

#[test]
fn a_programs_subject_starts_a_sentence() {
    assert_eq!(BOARD.subject(), "The board");
    assert_eq!(CLI.subject(), "bullpen's command line");
    assert_eq!(names(&[&CLI, &BOARD]), "bullpen's command line and the board");
}

#[test]
fn the_source_hash_is_board_builds() {
    // the Python twin (accept's rebuild) and this one read each other's stamps
    let py = "import sys; sys.path.insert(0, sys.argv[1]); from bullpen import board_build as b; \
              print(b.CLI.source_hash(), b.BOARD_PROGRAM.source_hash())";
    let out = std::process::Command::new("python3")
        .args(["-S", "-c", py])
        .arg(home())
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let want: Vec<&str> = text.split_whitespace().collect();
    assert_eq!(
        want,
        [CLI.source_hash().unwrap(), BOARD.source_hash().unwrap()],
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
