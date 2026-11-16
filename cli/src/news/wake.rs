//! The backstop for news no coordinator reads, and `news --pause` / `--resume`.
//!
//! When news isn't typed (`config --news monitor` or `wake`), a coordinator hears it only
//! while a reader runs: its Monitor on `news --follow`, or the waiter of its Stop hook. A Monitor expires after 30 minutes, and
//! news that comes while none runs would sit unseen until the person speaks. So a line
//! worth a wake (`wakes`) touches `<state>/wake-due`, and while that exists the watcher
//! runs `_wake_check` every 3 s. For each live coordinator that isn't paused and has
//! such news past its cursor, a minute old, with no live reader (a `news --follow`, or
//! the waiter its Stop hook parks: waiter.rs), it types every line
//! since the cursor into the coordinator's pane, as typed news is, and moves the cursor:
//! each line is typed once. It waits behind a draft in the input, as typed news does;
//! after 10 minutes of that it tells the person once for the batch.
//!
//! A pause (`news-<cid>.wait`, `{"paused": <stamp>}`) quiets the backstop for that
//! coordinator until `news --resume` or its next `bullpen up`; news is still logged.

use super::read::{news_cursor, news_for, news_records, news_size, set_news_cursor};
use super::{has_draft, touch, LINE_BREAK, NEWS_MARK};
use crate::coordinators::{alive, coordinators};
use crate::rex;
use bullpen_core::json::{self, s, Obj};
use bullpen_core::paths::{safe, state};
use bullpen_core::timeutil::{iso_now, seconds_since};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// News of these events rides along with the next wake (or `bullpen news`) and doesn't
/// wake a coordinator by itself: bookkeeping, nothing to act on. Any other event wakes,
/// so a new event can't go unheard.
pub const RIDES_ALONG: &[&str] = &[
    "started",
    "rebased",
    "accepted",
    "closed",
    "queue",
    "pickup",
    "kept",
    "dropped",
    "later",
    "tour-sent",
    "keys",
];

/// Seconds wake-worthy news waits for a reader before the backstop types it.
pub const UNREAD_FOR: f64 = 60.0;
/// Seconds the backstop waits behind a draft before it tells the person.
pub const DRAFT_FOR: f64 = 600.0;

/// Is news of this event worth waking a coordinator for?
pub fn wakes(event: &str) -> bool {
    !RIDES_ALONG.contains(&event)
}

/// Is news typed into the coordinators' panes? (As news() decides it.) Then the backstop
/// has nothing to do.
pub fn typed(repo: &Repo) -> bool {
    typed_mode(repo.cfg_str("news"))
}

/// Is this value of the `news` setting one that types news? (`monitor` and `wake` don't.)
pub fn typed_mode(news: &str) -> bool {
    !matches!(news, "monitor" | "wake")
}

/// Does a waiter in the coordinator's Stop hook bring the news (`config --news wake`)?
pub fn by_waiter(repo: &Repo) -> bool {
    repo.cfg_str("news") == "wake"
}

/// Does a live waiter own coordinator cid's wait?
pub fn has_waiter(repo: &Repo, cid: &str) -> bool {
    super::waiter::live(&wait_file(repo, cid))
}

/// End the waiter that owns coordinator cid's wait: it goes at its next look.
pub fn end_waiter(repo: &Repo, cid: &str) -> Res<()> {
    update_wait(repo, cid, |w| {
        w.remove("waiter");
    })
}

pub fn wake_due_path() -> PathBuf {
    state().join("wake-due")
}

/// Before news() logs a line that isn't typed: a coordinator it is for that has never
/// read news starts reading here, so the line is new to it (old news stays old, as for
/// a first `bullpen news`).
pub fn before_logged(repo: &Repo, to: &[String]) -> Res<()> {
    if typed(repo) {
        return Ok(());
    }
    let all: Vec<String> = coordinators(repo).iter().map(|c| s(c, "id").to_string()).collect();
    let size = news_size(repo);
    for cid in if to.is_empty() { &all } else { to } {
        if news_cursor(repo, cid)?.is_none() {
            set_news_cursor(repo, size, cid)?;
        }
    }
    Ok(())
}

/// After news() logged a line that isn't typed: the watcher looks for unread news.
pub fn after_logged(repo: &Repo, event: &str) -> Res<()> {
    if !typed(repo) && wakes(event) {
        touch(&wake_due_path())?;
    }
    Ok(())
}

/// (stamp, task ID, event) of a news.log record's first line: `<stamp>[ [to]] <id> <event>: <line>`.
pub fn record_head(line: &str) -> Option<(&str, &str, &str)> {
    let (stamp, rest) = line.split_once(' ')?;
    let rest = match rest.strip_prefix('[') {
        Some(r) => r.split_once("] ")?.1,
        None => rest,
    };
    let (id, rest) = rest.split_once(' ')?;
    let (event, _) = rest.split_once(": ")?;
    (!stamp.is_empty() && !event.is_empty() && !event.contains([' ', ':'])).then_some((stamp, id, event))
}

/// A record past a coordinator's cursor: where it ends, and what typing it says (None:
/// not for this coordinator, skipped).
pub(super) struct Unread {
    pub end: u64,
    pub said: Option<String>,
    /// when it was logged, if it is worth a wake
    pub wakes_since: Option<String>,
    /// "<task> <event>", if it is this coordinator's
    pub what: String,
}

/// The whole records after offset, for coordinator cid or not.
pub(super) fn unread(repo: &Repo, offset: u64, cid: &str, listed: &[String]) -> Res<Vec<Unread>> {
    let Ok(mut f) = std::fs::File::open(repo.dir.join("news.log")) else {
        return Ok(Vec::new());
    };
    f.seek(SeekFrom::Start(offset))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;
    let mut out = Vec::new();
    let mut end = offset;
    for raws in news_records(&data) {
        end += raws.iter().map(|r| r.len() as u64).sum::<u64>();
        let first = String::from_utf8_lossy(raws[0]).trim_end_matches('\n').to_string();
        let mine = !first.starts_with('\t') && news_for(&first, cid, listed);
        if !mine {
            out.push(Unread {
                end,
                said: None,
                wakes_since: None,
                what: String::new(),
            });
            continue;
        }
        let at = first.find(NEWS_MARK).unwrap_or(0);
        let mut lines = vec![first[at..].to_string()];
        for raw in &raws[1..] {
            lines.push(
                String::from_utf8_lossy(raw)
                    .trim_end_matches('\n')
                    .chars()
                    .skip(1)
                    .collect(),
            );
        }
        let head = record_head(&first);
        let wakes_since = head
            .filter(|(_, _, event)| wakes(event))
            .map(|(stamp, _, _)| stamp.to_string());
        out.push(Unread {
            end,
            said: Some(lines.join(LINE_BREAK)),
            wakes_since,
            what: head.map_or(String::new(), |(_, id, event)| format!("{id} {event}")),
        });
    }
    Ok(out)
}

/// `news-<cid>.wait` holds what waking coordinator cid goes by: the pause (`paused`),
/// the backstop's notes (`held`, `resumed`), the waiter that owns the wait (`waiter`)
/// and the last wake (`woke`, `woke_for`: waiter.rs).
fn wait_path(repo: &Repo, cid: &str) -> PathBuf {
    repo.dir.join(format!("news-{}.wait", safe(cid)))
}

pub(super) fn wait_file(repo: &Repo, cid: &str) -> Obj {
    json::read_obj(&wait_path(repo, cid)).ok().flatten().unwrap_or_default()
}

/// Change `news-<cid>.wait`: read, change, written by rename, one writer at a time (the
/// waiter, the backstop and `--pause` all write it, so none loses another's keys).
pub(super) fn update_wait<T>(repo: &Repo, cid: &str, change: impl FnOnce(&mut Obj) -> T) -> Res<T> {
    let _held = bullpen_core::lock::lock(&repo.dir.join(format!("news-{}.wait.lock", safe(cid))))?;
    let path = wait_path(repo, cid);
    let mut w = wait_file(repo, cid);
    let before = w.clone();
    let out = change(&mut w);
    if w.is_empty() {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    } else if w != before {
        json::write_obj(&path, &w)?; // by rename
    }
    Ok(out)
}

pub fn paused(repo: &Repo, cid: &str) -> bool {
    wait_file(repo, cid).contains_key("paused")
}

/// Pause coordinator cid's news. False if it was paused already.
pub fn pause(repo: &Repo, cid: &str) -> Res<bool> {
    update_wait(repo, cid, |w| {
        if w.contains_key("paused") {
            return false;
        }
        w.insert("paused".into(), json!(iso_now()));
        true
    })
}

/// End coordinator cid's pause. False if it wasn't paused. News logged during the pause
/// gets a minute from now for a reader to take it, before the backstop types it.
pub fn resume(repo: &Repo, cid: &str) -> Res<bool> {
    let was = update_wait(repo, cid, |w| {
        if w.remove("paused").is_none() {
            return false;
        }
        w.insert("resumed".into(), json!(iso_now()));
        true
    })?;
    if was {
        touch(&wake_due_path())?;
    }
    Ok(was)
}

/// Is a reader taking coordinator cid's news right now? A `news --follow` holds its lock;
/// a waiter (`news --wake`, parked by the coordinator's Stop hook) is named in its wait
/// file, and wakes the coordinator itself within its batch window.
fn live_reader(repo: &Repo, cid: &str, w: &Obj) -> Res<bool> {
    Ok(super::waiter::live(w) || bullpen_core::lock::try_lock(&lock_path(repo, cid))?.is_none())
}

pub(super) fn lock_path(repo: &Repo, cid: &str) -> PathBuf {
    repo.dir.join(format!("news-{}.lock", safe(cid)))
}

/// How the backstop left a coordinator.
#[derive(Debug, PartialEq)]
pub enum Left {
    /// nothing unread worth a wake (or typed now)
    Done,
    /// news waits: for its minute, a reader, or an empty input
    Waits,
}

/// The backstop for one live coordinator.
fn check(repo: &Repo, c: &Obj, listed: &[String]) -> Res<Left> {
    let cid = s(c, "id");
    if paused(repo, cid) {
        return Ok(Left::Done); // resume looks again
    }
    let Some(cursor) = news_cursor(repo, cid)? else {
        return Ok(Left::Done); // never read: before_logged starts it before a line that wakes
    };
    let cursor = cursor.min(news_size(repo));
    let records = unread(repo, cursor, cid, listed)?;
    let Some(first) = records.iter().find_map(|r| r.wakes_since.as_deref()) else {
        return Ok(Left::Done);
    };
    if !alive(Some(c))? {
        return Ok(Left::Done); // `up` looks again when it's back
    }
    let w = wait_file(repo, cid);
    let resumed = w.get("resumed").map(json::py_str).unwrap_or_default();
    let unread_for = seconds_since(first, None).min(if resumed.is_empty() {
        f64::INFINITY
    } else {
        seconds_since(&resumed, None)
    });
    if unread_for < UNREAD_FOR || live_reader(repo, cid, &w)? {
        return Ok(Left::Waits);
    }
    if has_draft(s(c, "session"), s(c, "block"))? {
        held_by_draft(repo, c, cursor)?;
        return Ok(Left::Waits);
    }
    let Some(_lock) = bullpen_core::lock::try_lock(&lock_path(repo, cid))? else {
        return Ok(Left::Waits); // a reader came
    };
    // again under the lock: a reader may have moved the cursor since
    let cursor = news_cursor(repo, cid)?.unwrap_or(cursor).min(news_size(repo));
    for r in unread(repo, cursor, cid, listed)? {
        if let Some(said) = &r.said {
            rex::rex_type(s(c, "session"), s(c, "block"), &said.replace(LINE_BREAK, "\n"))?;
        }
        set_news_cursor(repo, r.end, cid)?; // after typing: each line is typed once
    }
    update_wait(repo, cid, |w| {
        for key in ["held", "resumed"] {
            w.remove(key);
        }
    })?;
    Ok(Left::Done)
}

/// A draft holds the batch that starts at the cursor: after DRAFT_FOR, the person hears
/// once for it.
fn held_by_draft(repo: &Repo, c: &Obj, cursor: u64) -> Res<()> {
    let cid = s(c, "id");
    // noted as told first: told once, even if sending fails
    let tell = update_wait(repo, cid, |w| {
        let mut held = w.get("held").and_then(Value::as_object).cloned().unwrap_or_default();
        if held.get("batch").and_then(Value::as_u64) != Some(cursor) {
            w.insert(
                "held".into(),
                json!({"batch": cursor, "since": iso_now(), "told": false}),
            );
            return false;
        }
        let since = held.get("since").map(json::py_str).unwrap_or_default();
        if json::truthy(held.get("told")) || seconds_since(&since, None) < DRAFT_FOR {
            return false;
        }
        held.insert("told".into(), json!(true));
        w.insert("held".into(), Value::Object(held));
        true
    })?;
    if !tell {
        return Ok(());
    }
    let text = format!(
        "Your coordinator ({cid}) has news it hasn't seen: your draft in its input has held it for {} minutes.",
        (DRAFT_FOR / 60.0) as i64
    );
    crate::notifications::tell_person_from_coordinator(repo, c, &text);
    Ok(())
}

/// The backstop for every coordinator of a repo whose news isn't typed. Waits if news
/// waits for any of them.
pub fn check_repo(repo: &Repo) -> Res<Left> {
    if typed(repo) {
        return Ok(Left::Done);
    }
    let listed: Vec<String> = coordinators(repo).iter().map(|c| s(c, "id").to_string()).collect();
    let mut left = Left::Done;
    for c in coordinators(repo) {
        if check(repo, c, &listed)? == Left::Waits {
            left = Left::Waits;
        }
    }
    Ok(left)
}

/// `_wake_check` (the watcher, while `<state>/wake-due` exists): the backstop, for every
/// repo; wake-due stays only while something waits. It goes before the checks and is
/// made again after them: a line logged meanwhile makes it too, and that one stays.
pub fn cmd_wake_check(argv: &[String]) -> Res<i32> {
    crate::args::Args::parse("_wake_check", argv, &[]);
    remove_mark(&wake_due_path())?;
    let mut waits = false;
    for d in crate::notifications::folders_with(|n| n == "news.log") {
        let repo = crate::notifications::repo_at(&d)?;
        if check_repo(&repo)? == Left::Waits {
            waits = true;
        }
    }
    mark_wait();
    if waits {
        touch(&wake_due_path())?;
    }
    Ok(0)
}

/// Remove one of the watcher's marks (`<state>/wake-due`, `<state>/outbox`), there or not.
pub fn remove_mark(path: &std::path::Path) -> Res<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// For the tests: a pause after the watcher's round (`_wake_check`, `_flush`) has looked,
/// so a test can log a line right then (BULLPEN_MARK_WAIT, seconds).
pub fn mark_wait() {
    if let Some(wait) = std::env::var("BULLPEN_MARK_WAIT")
        .ok()
        .and_then(|w| w.parse::<f64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_secs_f64(wait.max(0.0)));
    }
}

/// `news --pause` / `--resume` for coordinator cid: what to say.
pub fn pause_line(repo: &Repo, cid: &str, on: bool) -> Res<String> {
    if on {
        let was = !pause(repo, cid)?;
        return Ok(format!(
            "{NEWS_MARK}news for {cid} is paused{}: nothing types it into your pane until `bullpen news --resume` \
             (or the next `bullpen up`). It is still logged, and `bullpen news` prints it.",
            if was { " already" } else { "" }
        ));
    }
    Ok(if resume(repo, cid)? {
        format!("{NEWS_MARK}news for {cid} is on again. What came during the pause: `bullpen news` prints it.")
    } else {
        format!("{NEWS_MARK}news for {cid} wasn't paused.")
    })
}

/// After `up`: a coordinator back may have news no reader took.
pub fn after_up(repo: &Repo) -> Res<()> {
    if !typed(repo) {
        touch(&wake_due_path())?;
    }
    Ok(())
}

/// `up` for coordinator cid ends its pause: the line saying so, if it was paused.
pub fn end_pause_at_up(repo: &Repo, cid: &str) -> Res<Option<String>> {
    Ok(resume(repo, cid)?
        .then(|| format!("News for {cid} was paused: `up` ended the pause (`bullpen news --pause` pauses it again).")))
}

#[cfg(test)]
#[path = "test_wake.rs"]
mod tests;
