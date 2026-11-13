use super::*;

#[test]
fn a_programs_subject_starts_a_sentence() {
    assert_eq!(BOARD.subject(), "The board");
    assert_eq!(CLI.subject(), "bullpen's command line");
    assert_eq!(names(&[&CLI, &BOARD]), "bullpen's command line and the board");
}

#[test]
fn the_source_hash_is_the_tests() {
    // tests/run.py writes the stamp of the command line it builds with its own copy
    let py = "import sys; sys.path.insert(0, sys.argv[1]); import support; print(support.cli_source_hash())";
    let out = std::process::Command::new("python3")
        .args(["-c", py])
        .arg(home().join("tests"))
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        text.trim(),
        CLI.source_hash().unwrap(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
