//! `notify` on the state folder: every repo's tasks and settings, and the watcher's
//! doing/ notes, under one recursive watch, as mission control has it. Each change is
//! a `Heard::Changed` for the snapshot's reader (fleet.rs); its own socket and log
//! aren't changes. And on the folders of the transcripts someone has open (`Folders`),
//! and only those.

use crate::fleet::Heard;
use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Mutex;

/// Watch the state folder for as long as the program runs. Without a watch (notify
/// can't make one), the snapshot is still read every second.
pub fn state_folder(state: &Path, tx: Sender<Heard>) {
    let mut w = match notify::recommended_watcher(move |r: notify::Result<notify::Event>| {
        if r.is_ok_and(|e| e.paths.is_empty() || !e.paths.iter().all(|p| own(p))) {
            let _ = tx.send(Heard::Changed);
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            return eprintln!(
                "bullpen web: can't watch {} ({e}): reading it every second",
                state.display()
            )
        }
    };
    if let Err(e) = w.watch(state, RecursiveMode::Recursive) {
        return eprintln!(
            "bullpen web: can't watch {} ({e}): reading it every second",
            state.display()
        );
    }
    std::mem::forget(w); // kept for the program's life
}

/// The server's own files in the state folder.
fn own(p: &Path) -> bool {
    matches!(p.file_name().and_then(|n| n.to_str()), Some("web.sock" | "web.log"))
}

/// `notify` on the folders of the transcripts someone is watching (a worktree's holds
/// its worker's and its reviewer's), each watched while any of its transcripts is: a
/// change to a file in one is sent on as its path. Without a watch, the transcripts
/// are still read every second (transcripts.rs).
pub struct Folders {
    w: Option<Mutex<notify::RecommendedWatcher>>,
    count: Mutex<HashMap<PathBuf, usize>>,
}

impl Folders {
    pub fn new(tx: Sender<PathBuf>) -> Folders {
        let w = notify::recommended_watcher(move |r: notify::Result<notify::Event>| {
            // a write, a new file, one gone: never a read (the server's own would loop)
            let Some(e) = r.ok().filter(|e| !matches!(e.kind, EventKind::Access(_))) else {
                return;
            };
            for p in e.paths {
                let _ = tx.send(p);
            }
        });
        let w = w.map_err(|e| eprintln!("bullpen web: can't watch transcripts ({e}): reading them every second"));
        Folders {
            w: w.ok().map(Mutex::new),
            count: Mutex::default(),
        }
    }

    pub fn add(&self, dir: &Path) {
        let mut count = self.count.lock().unwrap();
        let n = count.entry(dir.to_path_buf()).or_insert(0);
        *n += 1;
        if let (1, Some(w)) = (*n, &self.w) {
            if let Err(e) = w.lock().unwrap().watch(dir, RecursiveMode::NonRecursive) {
                eprintln!(
                    "bullpen web: can't watch {} ({e}): reading it every second",
                    dir.display()
                );
            }
        }
    }

    /// How many folders are watched.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.count.lock().unwrap().len()
    }

    pub fn remove(&self, dir: &Path) {
        let mut count = self.count.lock().unwrap();
        let Some(n) = count.get_mut(dir) else {
            return;
        };
        *n -= 1;
        if *n == 0 {
            count.remove(dir);
            if let Some(w) = &self.w {
                let _ = w.lock().unwrap().unwatch(dir);
            }
        }
    }
}
