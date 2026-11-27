//! The event stream (`GET /events?watch={repo}/{id}[/{who}]`), one per page: the
//! snapshot as a `fleet` event on connecting and whenever it changes (its id is the
//! snapshot's number), a `ping` every 15 seconds, which keeps proxies and Safari from
//! closing a quiet stream, and the watched chat's `chat` events: what each read of its
//! transcript made or changed. The prompt, screen and done events come with the routes
//! that need them.

use crate::chat::{self, Target, Where};
use crate::fleet::Latest;
use crate::transcripts::{Missed, Open, Transcripts, Update, Watching};
use axum::response::sse::{Event, Sse};
use bullpen_core::chat::Source;
use serde_json::Value;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::{IntervalStream, ReceiverStream, WatchStream};
use tokio_stream::{Stream, StreamExt};

const PING: Duration = Duration::from_secs(15);
/// How often a watched chat with no transcript yet is looked for again.
const LOOK_EVERY: Duration = Duration::from_secs(1);

/// The chat a stream watches: whose, and the offset the page read it to (`cursor`).
pub struct Watch {
    pub target: Target,
    pub cursor: Option<u64>,
}

pub fn stream(
    latest: Latest,
    all: Arc<Transcripts>,
    watch: Option<Watch>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel(16);
    if let Some(w) = watch {
        tokio::spawn(follow(latest.clone(), all, w, tx));
    }
    let fleet =
        WatchStream::new(latest).map(|snap| Event::default().event("fleet").id(snap.n.to_string()).data(&snap.json));
    let start = tokio::time::Instant::now() + PING;
    let ping =
        IntervalStream::new(tokio::time::interval_at(start, PING)).map(|_| Event::default().event("ping").data(""));
    Sse::new(fleet.merge(ping).merge(ReceiverStream::new(rx)).map(Ok))
}

/// The transcript a stream follows: its file, its updates, and its watch.
struct On {
    path: PathBuf,
    source: Source,
    open: Arc<Open>,
    rx: broadcast::Receiver<Arc<Update>>,
    _watching: Watching,
}

/// Send the watched chat's `chat` events until the stream goes: on connecting, what
/// changed since the page's `cursor` (if it gave one); then each update. When the chat
/// moves to another transcript (its agent's Stop hook moved its `chat`, or it had none
/// and now has one), or its file is written anew, or the stream fell behind, its chat
/// is sent whole (`reset`), as `/api/chat` has it.
async fn follow(mut latest: Latest, all: Arc<Transcripts>, w: Watch, tx: mpsc::Sender<Event>) {
    let mut place = locate(&latest, &w.target);
    let mut on = None;
    if let Some(p) = &place {
        if let Some((mut o, missed)) = start(&all, p, w.cursor).await {
            match missed {
                Missed::Nothing => {}
                Missed::Items(m) => send(&tx, &m).await,
                Missed::Whole => whole(&tx, &mut o, p).await,
            }
            on = Some(o);
        }
    }
    let mut look = tokio::time::interval(LOOK_EVERY);
    loop {
        let then = tokio::select! {
            _ = tx.closed() => return,
            changed = latest.changed() => {
                if changed.is_err() {
                    return;
                }
                let now = locate(&latest, &w.target);
                let moved = now.is_some() && now != place;
                place = now.or(place);
                if moved { Then::Look } else { Then::Wait }
            }
            _ = look.tick(), if on.is_none() => Then::Look,
            update = next(&mut on) => match update {
                Ok(u) => match &*u {
                    Update::Items(v) => {
                        send(&tx, v).await;
                        Then::Wait
                    }
                    Update::Anew => Then::Whole,
                },
                Err(broadcast::error::RecvError::Lagged(_)) => Then::Whole,
                Err(broadcast::error::RecvError::Closed) => return,
            },
        };
        match (then, &place) {
            (Then::Wait, _) | (_, None) => continue,
            (Then::Look, Some(p)) => match start(&all, p, None).await {
                Some((f, _)) if !on.as_ref().is_some_and(|o| o.path == f.path) => on = Some(f),
                _ => continue, // none found, or the same: nothing to send
            },
            (Then::Whole, _) => {}
        }
        if let (Some(o), Some(p)) = (&mut on, &place) {
            whole(&tx, o, p).await;
        }
    }
}

/// Send the chat whole, as `/api/chat` has it, with `reset: true`.
async fn whole(tx: &mpsc::Sender<Event>, o: &mut On, place: &Where) {
    o.rx = o.rx.resubscribe(); // what's queued is in the chat sent now
    let (open, place, source) = (o.open.clone(), place.clone(), o.source);
    let whole = tokio::task::spawn_blocking(move || {
        let mut v = chat::answer(&place, Some(source), open.page(None));
        v["reset"] = Value::Bool(true);
        v
    });
    if let Ok(v) = whole.await {
        send(tx, &v).await;
    }
}

/// What a stream does after what it heard: nothing, look for its transcript again, or
/// send its chat whole.
enum Then {
    Wait,
    Look,
    Whole,
}

/// Where the chat is in the snapshot now; None when it isn't there.
fn locate(latest: &Latest, t: &Target) -> Option<Where> {
    let snapshot: Value = serde_json::from_str(&latest.borrow().json).ok()?;
    chat::locate(&snapshot, t).ok()
}

/// Find its transcript, read it, and start hearing its updates, with what a page read
/// to `cursor` has missed; None when there's none yet.
async fn start(all: &Arc<Transcripts>, place: &Where, cursor: Option<u64>) -> Option<(On, Missed)> {
    let (all, place) = (all.clone(), place.clone());
    let found = tokio::task::spawn_blocking(move || {
        let (path, source) = place.find()?;
        let watching = all.watch(&path);
        let open = all.open(&path, source);
        let (rx, missed) = open.join(cursor);
        Some((path, source, open, rx, watching, missed))
    });
    let (path, source, open, rx, watching, missed) = found.await.ok()??;
    let on = On {
        path,
        source,
        open,
        rx,
        _watching: watching,
    };
    Some((on, missed))
}

async fn next(on: &mut Option<On>) -> Result<Arc<Update>, broadcast::error::RecvError> {
    match on {
        Some(o) => o.rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn send(tx: &mpsc::Sender<Event>, data: &Value) {
    let event = Event::default()
        .event("chat")
        .data(bullpen_core::json::dumps_line(data));
    let _ = tx.send(event).await;
}
