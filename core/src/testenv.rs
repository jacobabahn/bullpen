//! For tests: a home, state and config folder of their own. The environment is the
//! whole process's and tests run in threads, so one test at a time holds it.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::sync::atomic::{AtomicU32, Ordering};

static ENV: Mutex<()> = Mutex::new(());
static NEXT: AtomicU32 = AtomicU32::new(0);

/// A test's own HOME, BULLPEN_STATE and XDG_CONFIG_HOME, in a fresh folder that goes
/// when it is dropped (and the environment with it).
pub struct Env {
    pub dir: PathBuf,
    _held: MutexGuard<'static, ()>,
}

impl Env {
    pub fn new() -> Env {
        let held = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("bullpen-core-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::env::set_var("HOME", dir.join("home"));
        std::env::set_var("BULLPEN_STATE", dir.join("state"));
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        Env { dir, _held: held }
    }

    pub fn state(&self) -> PathBuf {
        self.dir.join("state")
    }

    /// Write the global config.json.
    pub fn global(&self, text: &str) {
        let path = self.dir.join("config/bullpen/config.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
