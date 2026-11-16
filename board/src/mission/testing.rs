//! Helpers mission control's tests share: a state folder of repos on disk, read as
//! mission control reads it.

use super::*;
pub(crate) use serde_json::{json, Value};

/// A scratch state folder, removed at the end.
pub(crate) struct State(pub(crate) PathBuf);

impl State {
    pub(crate) fn new(name: &str) -> State {
        let d = std::env::temp_dir().join(format!("bullpen-mission-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        State(d)
    }

    /// A repo with a checkout, these coordinators and these tasks; its state folder.
    pub(crate) fn repo(&self, name: &str, coordinators: Value, tasks: &[Value]) -> PathBuf {
        let root = self.0.join("checkouts").join(name);
        std::fs::create_dir_all(&root).unwrap();
        let dir = self.0.join("repos").join(name);
        std::fs::create_dir_all(dir.join("tasks")).unwrap();
        let cfg = json!({"root": root, "coordinators": coordinators, "prefix": "BP"});
        std::fs::write(dir.join("config.json"), cfg.to_string()).unwrap();
        for t in tasks {
            let id = t["id"].as_str().unwrap();
            std::fs::write(dir.join("tasks").join(format!("{id}.json")), t.to_string()).unwrap();
        }
        dir
    }

    /// Mission control on this state folder, read once, with no bullpen to run.
    pub(crate) fn mission(&self) -> Mission {
        let (tx, _rx) = mpsc::channel();
        let mut m = Mission::new(self.0.clone(), self.0.join("no-bullpen"), None, tx);
        m.session = None;
        m.reload();
        m
    }
}

impl Drop for State {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A task: its ID, status and title, waiting since `ago` minutes back.
pub(crate) fn t(id: &str, status: &str, title: &str, ago: i64) -> Value {
    let since = bullpen_core::timeutil::iso_seconds(&(bullpen_core::timeutil::now() - chrono::Duration::minutes(ago)));
    json!({"id": id, "status": status, "title": title, "since": since})
}

/// The selected task, as "repo BP-1".
pub(crate) fn selected(m: &Mission) -> String {
    let (app, t, _) = m.current_task().expect("a task selected");
    format!("{} {}", app.root.file_name().unwrap().to_string_lossy(), t.id)
}

pub(crate) fn press(m: &mut Mission, c: char) -> bool {
    m.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

pub(crate) fn key(m: &mut Mission, code: KeyCode) -> bool {
    m.key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// The screen as text, trailing spaces cut.
pub(crate) fn screen(m: &mut Mission, w: u16, h: u16) -> Vec<String> {
    use ratatui::backend::TestBackend;
    let mut term = ratatui::Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| super::draw::draw(f, m)).unwrap();
    let buf = term.backend().buffer();
    (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect()
}
