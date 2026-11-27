//! `bullpen news --wake`: the waiter a coordinator's Stop hook parks (Claude Code, with
//! `asyncRewake`). It costs no turn while nothing happens: it looks at `news.log`'s size
//! once a second. When news worth a wake (wake.rs) is logged for its coordinator, it
//! lets the batch settle, prints every line since the cursor to stderr, moves the
//! cursor and exits 2, which starts a turn in the idle session with those lines.
//! `Waiter::step` is one look, given its time, so its tests bring their own clock.
//!
//! - Who: the coordinator whose pane runs it (`REX_BLOCK`, by the pane index). In any
//!   other pane, unless news is `wake` (hook.rs installs it then), or while paused, it
//!   exits 0 at once.
//! - Newest wins: Claude Code starts one hook per turn end and stops none, so each
//!   waiter writes itself into `news-<cid>.wait` and goes quietly once the file names
//!   another.
//! - Bounds, kept in the wait file so they hold across waiters: one wake a minute; five
//!   minutes after a turn that ended in an API error (StopFailure), and no wake before
//!   a known usage-limit reset; a repeat of triage's line about a task doesn't wake for
//!   15 minutes.
//! - A wake whose turn ended in an API error is said again by the StopFailure waiter.
//! - A session killed outright signals nobody: the waiter goes once nobody reads its
//!   stderr, and moves the cursor only after the lines got out.
//! - It ends a minute before the hook's timeout (a hook killed there loses its exit
//!   2), delivering what waits, and on TERM, HUP and INT without moving the cursor.
//! - It starts no child process: one would outlive the session.
//! - It keeps its coordinator's conversation (session and transcript) in the wait file.

use super::read::{news_cursor, news_size, set_news_cursor};
use super::wake::{by_waiter, lock_path, unread, update_wait, wait_file, Unread};
use super::{LINE_BREAK, NEWS_MARK};
use crate::coordinators::{coordinator_at, coordinators};
use bullpen_core::config::load_config;
use bullpen_core::json::{self, s, Obj};
use bullpen_core::timeutil::{iso_micro, iso_seconds, now, seconds_since, Stamp};
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Seconds without a new line before a batch is delivered.
pub const SETTLE: f64 = 3.0;
/// Seconds a batch waits to settle at most.
pub const SETTLE_AT_MOST: f64 = 20.0;
/// Seconds between two wakes of a coordinator, at least.
pub const WAKE_GAP: f64 = 60.0;
/// The same, after a turn that ended in an API error, or at a usage limit with no known reset.
pub const FAILED_GAP: f64 = 300.0;
/// Seconds a repeat of the same task's line (REPEATS) doesn't wake again.
pub const REPEAT_FOR: f64 = 900.0;
/// The hook's timeout this is made for, in seconds (`--timeout` gives another).
pub const TIMEOUT: f64 = 86400.0;
/// Seconds before its timeout that a waiter ends by itself.
pub const EXPIRE_BEFORE: f64 = 60.0;
/// Events whose line is said again and again about one task while nothing changes
/// (triage's "ended its turn without `ready` 22 / 25 / 28 min ago").
pub const REPEATS: &[&str] = &["quiet"];

/// What a waiter does next.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// look again in a second
    Wait,
    /// end quietly, exit 0
    Quit,
    /// exit 2: this was printed to stderr, and the cursor is past it
    Wake(String),
}

pub struct Waiter {
    repo: Repo,
    cid: String,
    block: String,
    /// how the wait file names it: its pid and when it started
    pid: u32,
    started_as: String,
    started: Stamp,
    /// its turn ended in an API error (StopFailure)
    failed: bool,
    /// seconds it waits before it expires
    lasts: f64,
    /// news.log's size at the last look
    seen: Option<u64>,
    /// when the size last changed
    changed: Stamp,
    /// since when a batch worth a wake has waited
    batch: Option<Stamp>,
    /// where a usage limit is noted (<state>/limit.json)
    limit_file: PathBuf,
    /// hands the lines to the session (stderr); false if nobody is there to read them
    say: fn(&str) -> bool,
}

/// The wait file's `waiter` or `woke`, as an object.
fn part<'a>(w: &'a Obj, key: &str) -> &'a Obj {
    json::obj(w, key)
}

fn age(stamp: &str, at: Stamp) -> f64 {
    seconds_since(stamp, Some(at))
}

/// Does a live waiter own coordinator's wait (w: its wait file) at time `at`? Its
/// process runs and it hasn't expired.
pub fn live(w: &Obj, at: Stamp) -> bool {
    let waiter = part(w, "waiter");
    let Some(pid) = waiter.get("pid").and_then(Value::as_i64).filter(|p| *p > 0) else {
        return false;
    };
    let expired = seconds_since(s(waiter, "until"), Some(at)) > 0.0;
    !expired && running(pid as libc::pid_t)
}

fn running(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only asks whether the process is there
    unsafe { libc::kill(pid, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM) }
}

impl Waiter {
    /// The waiter for the coordinator in pane block, started by hook event (`Stop`,
    /// `StopFailure`) of a session; it takes the wait over. None: nothing to wait for
    /// here (not a coordinator's pane, news that isn't `wake`, a pause).
    pub fn start(block: &str, event: &str, session: &str, timeout: f64, at: Stamp) -> Res<Option<Waiter>> {
        let entry = bullpen_core::repo::block_entry(block);
        if s(&entry, "role") != "coordinator" || s(&entry, "repo").is_empty() {
            return Ok(None);
        }
        let repo = Repo::new(s(&entry, "repo"))?;
        Waiter::take(repo, block, event, session, timeout, at, crate::limit::limit_file())
    }

    /// start, in a repo.
    pub fn take(
        repo: Repo,
        block: &str,
        event: &str,
        session: &str,
        timeout: f64,
        at: Stamp,
        limit_file: PathBuf,
    ) -> Res<Option<Waiter>> {
        let Some(cid) = coordinator_at(&repo, block, false).map(|c| s(c, "id").to_string()) else {
            return Ok(None);
        };
        if !by_waiter(&repo) {
            return Ok(None);
        }
        let lasts = (timeout - EXPIRE_BEFORE).max(0.0);
        let failed = event == "StopFailure";
        let me = Waiter {
            repo,
            cid,
            block: block.into(),
            pid: std::process::id(),
            started_as: iso_micro(&at),
            started: at,
            failed,
            lasts,
            seen: None,
            changed: at,
            batch: None,
            limit_file,
            say: say_on_stderr,
        };
        let until = at + chrono::Duration::milliseconds((lasts * 1000.0) as i64);
        let took = update_wait(&me.repo, &me.cid, |w| {
            if w.contains_key("paused") {
                return false;
            }
            let waiter = json!({"pid": me.pid, "started": me.started_as, "until": iso_seconds(&until),
                                "event": event, "session": session});
            w.insert("waiter".into(), waiter);
            if !failed {
                // the turn ended well: the last wake was heard
                if let Some(woke) = w.get_mut("woke").and_then(Value::as_object_mut) {
                    woke.remove("from");
                }
            }
            true
        })?;
        Ok(took.then_some(me))
    }

    fn named_in(&self, w: &Obj) -> bool {
        let waiter = part(w, "waiter");
        waiter.get("pid").and_then(Value::as_u64) == Some(self.pid.into()) && s(waiter, "started") == self.started_as
    }

    /// Give the wait up, if it is still this waiter's.
    pub fn leave(&self) -> Res<()> {
        update_wait(&self.repo, &self.cid, |w| {
            if self.named_in(w) {
                w.remove("waiter");
            }
        })
    }

    /// Is this pane still a coordinator that waits for its news? (Settings read again.)
    fn still_waits(&mut self) -> Res<bool> {
        self.repo.cfg = load_config(&self.repo.dir)?;
        let me = coordinator_at(&self.repo, &self.block, false);
        Ok(by_waiter(&self.repo) && me.is_some_and(|c| s(c, "id") == self.cid))
    }

    fn listed(&self) -> Vec<String> {
        coordinators(&self.repo)
            .iter()
            .map(|c| s(c, "id").to_string())
            .collect()
    }

    /// The records past the cursor (which starts at the end of the log for a
    /// coordinator that never read news: old news stays old).
    fn unread(&self) -> Res<(u64, Vec<Unread>)> {
        let size = news_size(&self.repo);
        let cursor = match news_cursor(&self.repo, &self.cid)? {
            Some(at) => at.min(size),
            None => {
                set_news_cursor(&self.repo, size, &self.cid)?;
                size
            }
        };
        Ok((cursor, unread(&self.repo, cursor, &self.cid, &self.listed())?))
    }

    /// One look, at time `at`.
    pub fn step(&mut self, at: Stamp) -> Res<Step> {
        let w = wait_file(&self.repo, &self.cid);
        if w.contains_key("paused") || !self.named_in(&w) {
            return Ok(Step::Quit);
        }
        let size = news_size(&self.repo);
        if self.seen != Some(size) {
            self.seen = Some(size);
            self.changed = at;
            if !self.still_waits()? {
                return Ok(Step::Quit);
            }
            let worth = self.unread()?.1.iter().any(|r| worth_a_wake(r, &w, at));
            self.batch = if worth { self.batch.or(Some(at)) } else { None };
        }
        let again = self.failed && part(&w, "woke").contains_key("from");
        let expiring = (at - self.started).num_milliseconds() as f64 / 1000.0 >= self.lasts;
        if self.batch.is_none() && !again {
            return Ok(if expiring { Step::Quit } else { Step::Wait });
        }
        if !expiring && !(self.settled(at) && self.may_wake(&w, at)?) {
            return Ok(Step::Wait);
        }
        self.deliver(at, again)
    }

    /// Has the batch settled: SETTLE seconds with no new line, or SETTLE_AT_MOST in all?
    fn settled(&self, at: Stamp) -> bool {
        let since = |t: Stamp| (at - t).num_milliseconds() as f64 / 1000.0;
        self.batch
            .is_none_or(|b| since(self.changed) >= SETTLE || since(b) >= SETTLE_AT_MOST)
    }

    /// Do the bounds allow a wake now?
    fn may_wake(&self, w: &Obj, at: Stamp) -> Res<bool> {
        let mut gap = if self.failed { FAILED_GAP } else { WAKE_GAP };
        if let Some(limit) = crate::limit::limit_in(&self.limit_file, Some(at))? {
            if json::truthy(limit.get("resets")) {
                return Ok(false); // the turn would only fail: after the reset
            }
            gap = FAILED_GAP;
        }
        let last = s(part(w, "woke"), "at");
        Ok(last.is_empty() || age(last, at) >= gap)
    }

    /// Hand over what waits: under the news lock, every record since the cursor (and,
    /// again, the last wake's when its turn failed).
    fn deliver(&mut self, at: Stamp, again: bool) -> Res<Step> {
        if !self.still_waits()? {
            return Ok(Step::Quit);
        }
        let Some(_lock) = bullpen_core::lock::try_lock(&lock_path(&self.repo, &self.cid))? else {
            // a `news --follow` (or the backstop) is reading: look again once it has
            self.seen = None;
            return Ok(Step::Wait);
        };
        let w = wait_file(&self.repo, &self.cid);
        if w.contains_key("paused") || !self.named_in(&w) {
            return Ok(Step::Quit);
        }
        let (cursor, new) = self.unread()?;
        let woke = part(&w, "woke");
        let from = woke.get("from").and_then(Value::as_u64).filter(|_| again);
        let woken: Vec<String> = new
            .iter()
            .filter(|r| worth_a_wake(r, &w, at))
            .map(|r| r.what.clone())
            .collect();
        if woken.is_empty() && from.is_none() {
            self.batch = None; // a reader took it since
            return Ok(Step::Wait);
        }
        let mut lines = Vec::new();
        if let Some(from) = from.filter(|f| *f < cursor) {
            lines.push(format!(
                "{NEWS_MARK}your last turn ended in an error, so here is its news again, in case it wasn't acted on:"
            ));
            let old = unread(&self.repo, from, &self.cid, &self.listed())?;
            lines.extend(old.into_iter().filter(|r| r.end <= cursor).filter_map(|r| r.said));
        }
        lines.extend(new.iter().filter_map(|r| r.said.clone()));
        if lines.is_empty() {
            self.batch = None; // nothing of the failed wake is left to say (the log was cut)
            update_wait(&self.repo, &self.cid, |w| {
                if let Some(woke) = w.get_mut("woke").and_then(Value::as_object_mut) {
                    woke.remove("from");
                }
            })?;
            return Ok(Step::Wait);
        }
        let end = new.last().map_or(cursor, |r| r.end);
        // From here to the exit nothing stops it: printing and moving the cursor go together.
        hold_signals();
        if STOPPED.load(Ordering::SeqCst) {
            return Ok(Step::Quit); // the session is going: the next reader has the lines
        }
        let said = lines.join("\n").replace(LINE_BREAK, "\n");
        if !(self.say)(&said) {
            return Ok(Step::Quit); // nobody reads them (the session died): the cursor stays
        }
        set_news_cursor(&self.repo, end, &self.cid)?;
        update_wait(&self.repo, &self.cid, |w| {
            let mut told = part(w, "woke_for").clone();
            told.retain(|_, when| age(&json::py_str(when), at) < REPEAT_FOR);
            for what in woken.iter().filter(|what| repeats(what)) {
                told.insert(what.clone(), json!(iso_seconds(&at)));
            }
            let from = from.unwrap_or(cursor).min(cursor);
            w.insert("woke".into(), json!({"at": iso_seconds(&at), "from": from}));
            w.insert("woke_for".into(), Value::Object(told));
            for key in ["waiter", "held", "resumed"] {
                w.remove(key);
            }
            if part(w, "woke_for").is_empty() {
                w.remove("woke_for");
            }
        })?;
        Ok(Step::Wake(said))
    }
}

/// Is "<task> <event>" one of the lines that repeat?
fn repeats(what: &str) -> bool {
    what.rsplit(' ').next().is_some_and(|event| REPEATS.contains(&event))
}

/// Does this unread record wake the coordinator? Its event does (wake.rs), and it isn't
/// a repeat of a line about the same task that woke it within REPEAT_FOR.
fn worth_a_wake(r: &Unread, w: &Obj, at: Stamp) -> bool {
    if r.wakes_since.is_none() {
        return false;
    }
    let told = part(w, "woke_for").get(&r.what).map(json::py_str);
    !(repeats(&r.what) && told.is_some_and(|when| age(&when, at) < REPEAT_FOR))
}

/// Set by SIGTERM, SIGHUP and SIGINT: the session is going.
static STOPPED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_stop(_: libc::c_int) {
    STOPPED.store(true, Ordering::SeqCst);
}

/// SIGTERM, SIGHUP and SIGINT end the wait at its next look (they cut its sleep short).
fn catch_signals() {
    // SAFETY: plain handlers, set once before the loop; not SA_RESTART, so a sleep ends
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        let mut act: libc::sigaction = std::mem::zeroed();
        act.sa_sigaction = on_stop as *const () as libc::sighandler_t;
        libc::sigemptyset(&mut act.sa_mask);
        for sig in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
            libc::sigaction(sig, &act, std::ptr::null_mut());
        }
    }
}

fn hold_signals() {
    // SAFETY: sigset_t is plain data, filled by sigemptyset
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for sig in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
            libc::sigaddset(&mut set, sig);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// Print the lines to stderr, where Claude Code reads a hook's news. False if they
/// didn't get out (the pipe's reader is gone).
fn say_on_stderr(lines: &str) -> bool {
    let mut err = std::io::stderr().lock();
    writeln!(err, "{lines}").and_then(|_| err.flush()).is_ok()
}

/// Is whoever reads stderr gone? A session killed outright (kill -9, a crash) sends its
/// hooks no signal: its end of the pipe closing is all that shows. A waiter left behind
/// would hold the backstop off, and print its news to nobody.
fn reader_gone() -> bool {
    let mut fd = libc::pollfd {
        fd: 2,
        events: 0,
        revents: 0,
    };
    // SAFETY: one valid pollfd, no wait
    let n = unsafe { libc::poll(&mut fd, 1, 0) };
    n > 0 && fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
}

/// Sleep, or until a signal comes.
fn nap(seconds: f64) {
    let seconds = seconds.max(0.0);
    let ts = libc::timespec {
        tv_sec: seconds.trunc() as libc::time_t,
        tv_nsec: (seconds.fract() * 1e9) as libc::c_long,
    };
    // SAFETY: ts is a valid timespec; the time left isn't asked for
    unsafe {
        libc::nanosleep(&ts, std::ptr::null_mut());
    }
}

/// What Claude Code gives a hook on stdin, as an object ({} when run by hand).
fn hook_input() -> Obj {
    // SAFETY: isatty only looks at the descriptor
    if unsafe { libc::isatty(0) } == 1 {
        return Obj::new();
    }
    let mut text = String::new();
    let _ = std::io::stdin().read_to_string(&mut text);
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Keep the conversation of the coordinator in pane block, whose turn just ended, as
/// the wait file's `chat`: its session and transcript (the newest, after a `/clear`).
fn keep_chat(block: &str, session: &str, transcript: &str) -> Res<()> {
    let entry = bullpen_core::repo::block_entry(block);
    if s(&entry, "role") != "coordinator" || s(&entry, "repo").is_empty() || session.is_empty() {
        return Ok(());
    }
    let repo = Repo::new(s(&entry, "repo"))?;
    let Some(cid) = coordinator_at(&repo, block, false).map(|c| s(c, "id").to_string()) else {
        return Ok(());
    };
    update_wait(&repo, &cid, |w| {
        w.insert("chat".into(), json!({"session": session, "transcript": transcript}));
    })
}

/// `news --wake`: wait for this coordinator's news. Exit 2 with the lines on stderr
/// wakes its session; exit 0 is quiet.
pub fn cmd_wake(every: f64, timeout: f64) -> Res<i32> {
    let block = std::env::var("REX_BLOCK").unwrap_or_default();
    if bullpen_core::repo::block_entry(&block).is_empty() {
        return Ok(0); // not a pane of bullpen's: every other Claude session's turn end
    }
    let input = hook_input();
    let session = match s(&input, "session_id") {
        "" => std::env::var("CLAUDE_CODE_SESSION_ID").unwrap_or_default(),
        id => id.to_string(),
    };
    let _ = keep_chat(&block, &session, s(&input, "transcript_path"));
    catch_signals();
    let Some(mut waiter) = Waiter::start(&block, s(&input, "hook_event_name"), &session, timeout, now())? else {
        return Ok(0);
    };
    loop {
        if STOPPED.load(Ordering::SeqCst) || reader_gone() {
            waiter.leave()?;
            return Ok(0);
        }
        match waiter.step(now())? {
            Step::Wait => nap(every),
            Step::Quit => {
                waiter.leave()?;
                return Ok(0);
            }
            Step::Wake(_) => return Ok(2),
        }
    }
}

#[cfg(test)]
#[path = "tests/test_waiter.rs"]
mod tests;
