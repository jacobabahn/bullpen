//! `notify` on the state folder: every repo's tasks and settings, and the watcher's
//! doing/ notes, under one recursive watch, as mission control has it. Each change is
//! a `Heard::Changed` for the snapshot's reader (fleet.rs); its own socket and log
//! aren't changes.

use crate::fleet::Heard;
use notify::{RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::Sender;

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
