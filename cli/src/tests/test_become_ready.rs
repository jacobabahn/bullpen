use std::path::{Path, PathBuf};

/// The Rust files under dir, tests (each folder's `tests/`) left out.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).expect("a source folder").flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if name != "tests" {
                sources(&p, out);
            }
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

/// The files that set a status of "ready" by name: a `set_status(` whose status is
/// "ready", or the status written by hand.
fn setting_ready(file: &str) -> bool {
    let text: String = file.chars().filter(|c| !c.is_whitespace()).collect();
    let by_set_status = text.match_indices("set_status(").any(|(at, _)| {
        text[at..]
            .split(';')
            .next()
            .is_some_and(|call| call.contains("\"ready\""))
    });
    by_set_status || text.contains("json!(\"ready\")")
}

#[test]
fn only_the_step_sets_a_task_ready() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for crate_dir in ["cli/src", "core/src", "board/src"] {
        sources(&root.join(crate_dir), &mut files);
    }
    let found: Vec<String> = files
        .iter()
        .filter(|p| setting_ready(&std::fs::read_to_string(p).expect("a source file")))
        .map(|p| p.strip_prefix(&root).unwrap_or(p).display().to_string())
        .collect();
    assert_eq!(
        found,
        vec!["cli/src/become_ready.rs"],
        "a task becomes ready only through make_ready"
    );
}

#[test]
fn a_status_set_by_name_is_found() {
    assert!(setting_ready("set_status(t,\n    \"ready\", None, None);"));
    assert!(setting_ready("t.insert(\"status\".into(), json!(\"ready\"));"));
    assert!(!setting_ready(
        "set_status(t, \"working\", None, None); if s == \"ready\" {}"
    ));
}
