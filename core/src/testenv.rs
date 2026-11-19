//! For tests: a home, state and config folder of their own. The environment is the
//! whole process's and tests run in threads, so one test at a time holds it.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

static ENV: Mutex<()> = Mutex::new(());
static NEXT: AtomicU32 = AtomicU32::new(0);
const VARS: [&str; 3] = ["HOME", "BULLPEN_STATE", "XDG_CONFIG_HOME"];

/// The environment, held without changing it: no Env is set meanwhile.
pub fn held() -> MutexGuard<'static, ()> {
    ENV.lock().unwrap_or_else(|e| e.into_inner())
}

/// A test's own HOME, BULLPEN_STATE and XDG_CONFIG_HOME, in a fresh folder that goes
/// when it is dropped, the environment as it was (isolated.rs's) put back.
pub struct Env {
    pub dir: PathBuf,
    was: Vec<Option<OsString>>,
    _held: MutexGuard<'static, ()>,
}

impl Env {
    pub fn new() -> Env {
        let held = held();
        let was = VARS.iter().map(std::env::var_os).collect();
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("bullpen-core-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::env::set_var("HOME", dir.join("home"));
        std::env::set_var("BULLPEN_STATE", dir.join("state"));
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        Env { dir, was, _held: held }
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
        for (k, v) in VARS.iter().zip(&self.was) {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}
