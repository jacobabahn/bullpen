//! The event stream (`GET /events`), one per page: the snapshot as a `fleet` event on
//! connecting and whenever it changes (its id is the snapshot's number), and a `ping`
//! every 15 seconds, which keeps proxies and Safari from closing a quiet stream. The
//! chat, prompt, screen and done events come with the routes that need them.

use crate::fleet::Latest;
use axum::response::sse::{Event, Sse};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::{IntervalStream, WatchStream};
use tokio_stream::{Stream, StreamExt};

const PING: Duration = Duration::from_secs(15);

pub fn stream(latest: Latest) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let fleet =
        WatchStream::new(latest).map(|snap| Event::default().event("fleet").id(snap.n.to_string()).data(&snap.json));
    let start = tokio::time::Instant::now() + PING;
    let ping =
        IntervalStream::new(tokio::time::interval_at(start, PING)).map(|_| Event::default().event("ping").data(""));
    Sse::new(fleet.merge(ping).map(Ok))
}
