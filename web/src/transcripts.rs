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
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// How long after a change is heard a transcript is read: a burst of writes is one read.
const SETTLE: Duration = Duration::from_millis(30);
/// The longest a read waits for writes to stop coming.
const SETTLE_AT_MOST: Duration = Duration::from_millis(100);
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
    /// the offset its first read (or its read after it was written anew) went to:
    /// what changed before it isn't known apart
    known_from: u64,
}

/// What a page read to an offset has missed.
#[derive(Debug, PartialEq)]
pub enum Missed {
    /// nothing
    Nothing,
    /// these items, `{items, cursor}`
    Items(Value),
    /// more than can be told apart (the offset is from before this server read the
    /// file, or isn't one of its offsets): the chat is to be sent whole
    Whole,
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
            let mut swept = Instant::now();
            while let Some(all) = weak.upgrade() {
                if !all.read_changed(&rx, &mut swept) {
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
                    known_from: 0,
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

    /// Wait for a change in a watched folder, and read on each watched transcript that
    /// changed; and every transcript watched once a second since the last time (swept),
    /// whatever was heard, in case a change was missed. False when the watch is gone.
    fn read_changed(&self, rx: &Receiver<PathBuf>, swept: &mut Instant) -> bool {
        let mut heard = vec![];
        match rx.recv_timeout(EVERY.saturating_sub(swept.elapsed())) {
            Ok(p) => heard.push(p),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false,
        }
        if !heard.is_empty() {
            let until = Instant::now() + SETTLE_AT_MOST;
            while let Ok(p) = rx.recv_timeout(SETTLE.min(until.saturating_duration_since(Instant::now()))) {
                heard.push(p);
            }
        }
        let sweep = swept.elapsed() >= EVERY;
        if sweep {
            *swept = Instant::now();
        }
        let watched: Vec<PathBuf> = self.watched.lock().unwrap().keys().cloned().collect();
        let due = watched.iter().filter(|w| sweep || heard.contains(w));
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
        self.read_locked(&mut guard);
    }

    fn read_locked(&self, r: &mut Read) {
        let was = r.t.offset;
        let Ok(changed) = r.t.read() else {
            return; // gone, or can't be read now: the next read tries again
        };
        let offset = r.t.offset;
        let first = !std::mem::replace(&mut r.once, true);
        if first || offset < was {
            r.known_from = offset;
        }
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

    /// Read on, and start hearing its updates; with a page's `cursor`, also what that
    /// page has missed. Done in one hold of the transcript, so nothing the read made is
    /// both missed and heard, and nothing read after is neither.
    pub fn join(&self, cursor: Option<u64>) -> (broadcast::Receiver<Arc<Update>>, Missed) {
        let mut guard = self.read.lock().unwrap();
        self.read_locked(&mut guard);
        let rx = self.tx.subscribe();
        let missed = cursor.map_or(Missed::Nothing, |c| missed(&guard, c));
        (rx, missed)
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

    /// What a page read to the offset `cursor` has missed.
    pub fn since(&self, cursor: u64) -> Missed {
        missed(&self.read.lock().unwrap(), cursor)
    }
}

/// The items changed since the offset `cursor` was read to, oldest first, and the
/// offset now; the chat whole when the cursor is from before what this read knows (a
/// page from before the server started again) or past where it has read to.
fn missed(r: &Read, cursor: u64) -> Missed {
    if cursor < r.known_from || cursor > r.t.offset {
        return Missed::Whole;
    }
    let all = r.t.chat.all().iter();
    let missed = all.filter(|i| r.changed.get(&i.id).is_some_and(|&at| at > cursor));
    let items: Vec<Value> = missed.map(chat::item).collect();
    match items.is_empty() {
        true => Missed::Nothing,
        false => Missed::Items(json!({"items": items, "cursor": r.t.offset})),
    }
}

#[cfg(test)]
#[path = "tests/test_transcripts.rs"]
mod tests;
