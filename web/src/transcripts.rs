//! The transcripts the server has read, kept while it runs: each read whole the first
//! time anyone asks for it, then read on from its offset (bullpen_core::chat's
//! Transcript), whoever asks (the chat route, or the watch when the file changes).
//! What a read makes or changes goes out to every stream watching that transcript, as
//! one update. Only the transcripts someone watches are watched (watch.rs's folders,
//! and a read every second in case a change was missed).

use crate::chat;
use bullpen_core::chat::{Source, Transcript};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

/// How long after a change is heard a transcript is read: a burst of writes is one read.
const SETTLE: Duration = Duration::from_millis(30);
/// How often a watched transcript is read with no change heard.
const EVERY: Duration = Duration::from_secs(1);
/// How many updates a slow stream may fall behind before it's sent its chat anew.
const BEHIND: usize = 64;

/// What a read of a transcript sends its watchers.
#[derive(Clone, Debug)]
pub enum Update {
    /// the items made or changed, oldest first (as the page gets them), and the offset read to
    Items(Value),
    /// the file was written anew: its chat starts again
    Anew,
}

/// One transcript: its chat so far, and its watchers.
pub struct Open {
    read: Mutex<Read>,
    tx: broadcast::Sender<Arc<Update>>,
}

struct Read {
    t: Transcript,
    /// each item's ID, and the offset read to when it last changed
    changed: HashMap<String, u64>,
    /// whether it has been read: the first read is the whole file, which nobody can
    /// have had, so it isn't sent as an update
    once: bool,
}

/// Every transcript read, by its path, and which are watched (by how many streams).
pub struct Transcripts {
    open: Mutex<HashMap<PathBuf, Arc<Open>>>,
    watched: Mutex<HashMap<PathBuf, usize>>,
    folders: crate::watch::Folders,
}

/// A stream's watch on a transcript: it stops when this goes.
pub struct Watching {
    all: Arc<Transcripts>,
    path: PathBuf,
}

impl Drop for Watching {
    fn drop(&mut self) {
        self.all.unwatch(&self.path);
    }
}

impl Transcripts {
    /// Start: the watch on the folders of watched transcripts, and the thread that
    /// reads them when they change.
    pub fn start() -> Arc<Transcripts> {
        let (tx, rx) = channel();
        let all = Arc::new(Transcripts {
            open: Mutex::default(),
            watched: Mutex::default(),
            folders: crate::watch::Folders::new(tx),
        });
        let weak = Arc::downgrade(&all);
        std::thread::spawn(move || {
            while let Some(all) = weak.upgrade() {
                if !all.read_changed(&rx) {
                    return;
                }
            }
        });
        all
    }

    /// The transcript at path, as far as it has been read (nothing, the first time).
    pub fn open(&self, path: &Path, source: Source) -> Arc<Open> {
        let mut all = self.open.lock().unwrap();
        let o = all.entry(path.to_path_buf()).or_insert_with(|| {
            Arc::new(Open {
                read: Mutex::new(Read {
                    t: Transcript::new(path.to_path_buf(), source),
                    changed: HashMap::new(),
                    once: false,
                }),
                tx: broadcast::channel(BEHIND).0,
            })
        });
        o.clone()
    }

    /// The transcript at path, read on to its end.
    pub fn read(&self, path: &Path, source: Source) -> Arc<Open> {
        let open = self.open(path, source);
        open.read_on();
        open
    }

    /// Watch the transcript at path (its folder, under notify) until what this returns goes.
    pub fn watch(self: &Arc<Self>, path: &Path) -> Watching {
        let mut watched = self.watched.lock().unwrap();
        let n = watched.entry(path.to_path_buf()).or_insert(0);
        *n += 1;
        if *n == 1 {
            if let Some(dir) = path.parent() {
                self.folders.add(dir);
            }
        }
        Watching {
            all: self.clone(),
            path: path.to_path_buf(),
        }
    }

    fn unwatch(&self, path: &Path) {
        let mut watched = self.watched.lock().unwrap();
        let Some(n) = watched.get_mut(path) else {
            return;
        };
        *n -= 1;
        if *n == 0 {
            watched.remove(path);
            if let Some(dir) = path.parent() {
                self.folders.remove(dir);
            }
        }
    }

    /// Wait for a change in a watched folder (or a second), then read on each watched
    /// transcript that changed (each, when it's the second). False when the watch is gone.
    fn read_changed(&self, rx: &Receiver<PathBuf>) -> bool {
        let mut heard = vec![];
        match rx.recv_timeout(EVERY) {
            Ok(p) => heard.push(p),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false,
        }
        if !heard.is_empty() {
            while let Ok(p) = rx.recv_timeout(SETTLE) {
                heard.push(p);
            }
        }
        let watched: Vec<PathBuf> = self.watched.lock().unwrap().keys().cloned().collect();
        let due = watched.iter().filter(|w| heard.is_empty() || heard.contains(w));
        let open: Vec<Arc<Open>> = {
            let all = self.open.lock().unwrap();
            due.filter_map(|w| all.get(w).cloned()).collect()
        };
        for o in open {
            o.read_on();
        }
        true
    }
}

impl Open {
    /// Read on from the offset, and send what changed to its watchers.
    pub fn read_on(&self) {
        let mut guard = self.read.lock().unwrap();
        let r = &mut *guard;
        let was = r.t.offset;
        let Ok(changed) = r.t.read() else {
            return; // gone, or can't be read now: the next read tries again
        };
        let offset = r.t.offset;
        let first = !std::mem::replace(&mut r.once, true);
        if offset < was {
            r.changed.clear();
            for i in r.t.chat.items() {
                r.changed.insert(i.id.clone(), offset);
            }
            let _ = self.tx.send(Arc::new(Update::Anew));
            return;
        }
        for i in &changed {
            r.changed.insert(i.id.clone(), offset);
        }
        if changed.is_empty() || first {
            return;
        }
        let items: Vec<Value> = changed.iter().map(chat::item).collect();
        let _ = self
            .tx
            .send(Arc::new(Update::Items(json!({"items": items, "cursor": offset}))));
    }

    /// Hear each update from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Update>> {
        self.tx.subscribe()
    }

    /// A page of the chat: the last items (or those before one), whether there are
    /// earlier ones, its title and permission mode, and the offset read to. None when
    /// `before` isn't one of its items.
    pub fn page(&self, before: Option<&str>) -> Option<Value> {
        let r = self.read.lock().unwrap();
        let items = r.t.chat.items();
        let (items, more) = chat::page(&items, before)?;
        let items: Vec<Value> = items.into_iter().map(chat::item).collect();
        Some(json!({
            "title": r.t.chat.title,
            "mode": r.t.chat.mode,
            "items": items,
            "cursor": r.t.offset,
            "more": more,
        }))
    }

    /// The items changed since the offset `cursor` was read to, oldest first, and the
    /// offset now: what a page read then has missed. None when nothing has.
    pub fn since(&self, cursor: u64) -> Option<Value> {
        let r = self.read.lock().unwrap();
        let missed =
            r.t.chat
                .all()
                .iter()
                .filter(|i| r.changed.get(&i.id).is_some_and(|&at| at > cursor));
        let items: Vec<Value> = missed.map(chat::item).collect();
        (!items.is_empty()).then(|| json!({"items": items, "cursor": r.t.offset}))
    }
}

#[cfg(test)]
#[path = "tests/test_transcripts.rs"]
mod tests;
