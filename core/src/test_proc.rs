use super::*;

#[test]
fn output_reads_as_text_mode_does() {
    let d = run(&["printf", "a\\r\\nb\\rc\\n"], true).unwrap();
    assert_eq!(d.stdout, "a\nb\nc\n");
    assert_eq!(d.code, 0);
    assert_eq!(text(b"x\xffy"), "x\u{fffd}y");
}

#[test]
fn stdin_is_empty_unless_input_is_given() {
    assert_eq!(run(&["cat"], true).unwrap().stdout, "");
    assert_eq!(run_input(&["cat"], "hello\n", true).unwrap().stdout, "hello\n");
}

#[test]
fn a_failure_quotes_the_command_and_what_it_said() {
    let err = run(&["sh", "-c", "echo out; echo ' boom ' >&2; exit 3"], true).unwrap_err();
    assert_eq!(err.0, "`sh -c echo out; echo ' boom ' >&2; exit 3` failed: boom");
    let err = run(&["sh", "-c", "echo only out; exit 1"], true).unwrap_err();
    assert_eq!(err.0, "`sh -c echo only out; exit 1` failed: only out");
    let d = run(&["sh", "-c", "exit 4"], false).unwrap();
    assert_eq!((d.code, d.ok()), (4, false));
    let d = run(&["sh", "-c", "kill -TERM $$"], false).unwrap();
    assert_eq!(d.code, -15);
}

#[test]
fn a_missing_program_says_so() {
    let err = run(&["no-such-program-here"], false).unwrap_err();
    assert_eq!(err.0, "[Errno 2] No such file or directory: 'no-such-program-here'");
}

#[test]
fn run_in_a_folder() {
    let d = run_in(&["pwd"], Path::new("/"), true).unwrap();
    assert_eq!(d.stdout, "/\n");
}

#[test]
fn git_finds_this_checkout() {
    let here = env!("CARGO_MANIFEST_DIR");
    assert_eq!(head(here).unwrap().len(), 40);
    assert_eq!(merge_base(here, "no-such-ref-at-all").unwrap(), None);
}

#[test]
fn which_finds_programs_on_path() {
    let sh = which("sh").unwrap();
    assert!(sh.ends_with("/sh") && sh.starts_with('/'), "{sh}");
    assert_eq!(which("no-such-program-here"), None);
    assert_eq!(which("/bin/sh"), Some("/bin/sh".into()));
    assert_eq!(which("/no/such/sh"), None);
    assert_eq!(which("/"), None);
    let cmd = resolve(&["sh".into(), "-c".into()]);
    assert_eq!(cmd, vec![sh, "-c".into()]);
    assert_eq!(resolve(&["no-such-program-here".into()]), vec!["no-such-program-here".to_string()]);
}

#[test]
fn file_hash_is_sha256_hex() {
    let path = std::env::temp_dir().join(format!("bullpen-core-hash-{}", std::process::id()));
    std::fs::write(&path, "abc").unwrap();
    assert_eq!(file_hash(&path).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    let _ = std::fs::remove_file(&path);
}
