//! Helpers the app's tests share.

use super::*;
pub(crate) use crate::model::testing::{json, task};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use std::path::Path;
use std::process::{Command, Stdio};

impl App {
    /// A board on these tasks, with no state folder and no bullpen to run.
    pub(crate) fn for_test(tasks: Vec<Task>) -> App {
        let (tx, _rx) = std::sync::mpsc::channel();
        let dir = PathBuf::from("/nonexistent/bullpen-board-test");
        let args = Args { root: dir.clone(), bullpen: dir.join("no-bullpen"), dir, ..Default::default() };
        App { tasks, repo_name: "repo".into(), ..App::new(args, None, tx) }
    }
}

pub(crate) fn press(a: &mut App, c: char) {
    a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

pub(crate) fn owned(id: &str, status: &str, c: &str) -> Task {
    task(id, status).with(json!({"coordinator": c}))
}

pub(crate) fn needs(id: &str, ask: bool, kind: &str) -> Task {
    let t = task(id, "needs").with(json!({"stop": {"by": "worker", "kind": kind}}));
    if ask { t.with(json!({"ask": true})) } else { t }
}

pub(crate) fn prompt(options: &[(&str, &str)]) -> Prompt {
    Prompt {
        who: "worker".into(),
        question: Some("Do you want to proceed?".into()),
        options: options.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect(),
        id: Some("ab12".into()),
        screen: vec!["touch hello.txt".into()],
    }
}

/// A scratch folder of its own for a test, removed at the end.
pub(crate) struct Scratch(pub(crate) PathBuf);
impl Scratch {
    pub(crate) fn new(name: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!("bullpen-board-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn program(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A program just written can't run while another test's fork still holds it
    // open for writing (ETXTBSY): wait until it runs.
    for _ in 0..200 {
        let run = Command::new(path).arg("--ready").stdout(Stdio::null()).stderr(Stdio::null()).status();
        match run {
            Err(e) if e.raw_os_error() == Some(26) => std::thread::sleep(Duration::from_millis(10)),
            _ => break,
        }
    }
}

pub(crate) fn key(a: &mut App, code: KeyCode) {
    a.key(KeyEvent::new(code, KeyModifiers::NONE));
}
