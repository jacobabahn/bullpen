//! `bullpen news`: print the news logged for this
//! coordinator since the last time, once each, moving its cursor past it; `--follow`
//! keeps going, as Claude Code's Monitor tool runs it.
//!
//! Signals (the plan's hard spot 5): printing a record and moving the cursor past it go
//! together, so SIGTERM, SIGINT and SIGHUP are held while they happen. Ctrl-C between
//! records ends it quietly (exit 0). SIGPIPE is ignored here: a write to a reader that's
//! gone fails, and it stops with the cursor before the lost record, exit 0.
//! Locks (hard spot 4): one reader per coordinator, by flock on news-<cid>.lock, the
//! lock the Python took; a second follower waits, a one-off read doesn't.

use super::NEWS_MARK;
use crate::args::{flag, value, Args};
use crate::coordinators::{calling_coordinator, coordinator_at, coordinators, primary};
use bullpen_core::config::load_config;
use bullpen_core::json::s;
use bullpen_core::paths::safe;
use bullpen_core::repo::repo_root;
use bullpen_core::{fail, Repo, Res};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

const NOT_A_COORDINATOR: &str = "this pane isn't one of this repo's coordinators, so it doesn't read \
their news: don't start the monitor again (`bullpen up --here` here makes it one)";

const SPEC: &[crate::args::Arg] = &[
    flag("--follow", "keep running and print each line as it is logged"),
    value("--every", "EVERY", crate::args::SUPPRESS), // seconds between looks
];

/// Set by SIGINT: stop between records.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigint(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// How the reading ended: on its own, or the reader is gone (a closed pipe).
enum Out {
    Read,
    Gone,
}

pub fn cmd_news(argv: &[String]) -> Res<i32> {
    let a = Args::parse("news", argv, SPEC);
    let follow = a.flag("--follow");
    let every = match a.opt("--every") {
        None => 0.5,
        Some(e) => match bullpen_core::text::strip(e).parse::<f64>() {
            Ok(n) => n,
            Err(_) => crate::args::usage_error("news", SPEC, &format!("argument --every: invalid float value: '{e}'")),
        },
    };
    // SAFETY: plain handlers set once, before anything is printed. Not SA_RESTART: a
    // follower waiting for the lock stops at Ctrl-C, as the Python did.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        let mut act: libc::sigaction = std::mem::zeroed();
        act.sa_sigaction = on_sigint as *const () as libc::sighandler_t;
        libc::sigemptyset(&mut act.sa_mask);
        libc::sigaction(libc::SIGINT, &act, std::ptr::null_mut());
    }
    let mut repo = Repo::new(repo_root()?)?;
    std::fs::create_dir_all(&repo.dir)?;
    let cid = news_reader(&repo);
    if not_a_reader(&mut repo, &cid)? {
        if follow {
            // the Monitor tool shows what's printed: say it there
            say(&format!("{NEWS_MARK}{NOT_A_COORDINATOR}"));
            return Ok(0);
        }
        return fail(NOT_A_COORDINATOR);
    }
    let name = if cid.is_empty() {
        "news.lock".into()
    } else {
        format!("news-{}.lock", safe(&cid))
    };
    // One reader at a time, or lines would come twice. A second follower waits for the
    // first to go (a monitor being replaced); a one-off read doesn't.
    let lock = match bullpen_core::lock::take(&repo.dir.join(name), follow) {
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Ok(0), // Ctrl-C while waiting
        held => held?,
    };
    let Some(_lock) = lock else {
        return fail("news is being followed already (`bullpen news --follow` is running)");
    };
    let mut offset = match news_cursor(&repo, &cid)? {
        Some(n) => n,
        None => {
            // first time: old news stays old
            let size = news_size(&repo);
            set_news_cursor(&repo, size, &cid)?;
            size
        }
    };
    loop {
        if INTERRUPTED.load(Ordering::SeqCst) {
            return Ok(0);
        }
        if follow && !news_by_monitor(&repo)? {
            // Typed news is logged too: following it would give every line twice.
            say(&format!(
                "{NEWS_MARK}news is typed into your pane now (`bullpen config --news type`): \
                 don't start the monitor again"
            ));
            return Ok(0);
        }
        if follow && not_a_reader(&mut repo, &cid)? {
            // dropped while following
            say(&format!("{NEWS_MARK}{NOT_A_COORDINATOR}"));
            return Ok(0);
        }
        match print_news(&repo, offset, &cid)? {
            (_, Out::Gone) => return Ok(0), // the cursor stays before the lost line
            (at, Out::Read) => offset = at,
        }
        if !follow || INTERRUPTED.load(Ordering::SeqCst) {
            return Ok(0);
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(every.max(0.0)));
    }
}

/// Print a line and flush, as print(..., flush=True); a reader that's gone is no error.
fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}").and_then(|_| out.flush());
}

/// Whose news it prints: the coordinator in whose pane it runs, else the primary one;
/// "" (all news) when the repo has none.
fn news_reader(repo: &Repo) -> String {
    calling_coordinator(repo)
        .or_else(|| primary(repo))
        .map_or(String::new(), |c| s(c, "id").to_string())
}

/// Is `bullpen news` running in a Rex pane other than coordinator cid's? Each line is
/// printed once, so a reader there (an agent that used to be the coordinator, a shell)
/// would take lines from the coordinator's own monitor. Outside Rex there's no telling:
/// it reads the primary's news, as before.
fn not_a_reader(repo: &mut Repo, cid: &str) -> Res<bool> {
    let block = std::env::var("REX_BLOCK").unwrap_or_default();
    if block.is_empty() || cid.is_empty() {
        return Ok(false);
    }
    repo.cfg = load_config(&repo.dir)?; // read again: it can change while we follow
    Ok(coordinator_at(repo, &block, false).is_none_or(|me| s(me, "id") != cid))
}

/// Is the setting `monitor` right now? Read again: it can change while we follow.
fn news_by_monitor(repo: &Repo) -> Res<bool> {
    Ok(s(&load_config(&repo.dir)?, "news") == "monitor")
}

pub fn cursor_path(repo: &Repo, cid: &str) -> PathBuf {
    repo.dir.join(if cid.is_empty() {
        "news.cursor".into()
    } else {
        format!("news-{}.cursor", safe(cid))
    })
}

/// How far into news.log (in bytes) coordinator cid has read with `bullpen news`, or
/// None: never. (news.cursor: the one cursor from before several coordinators, or the
/// reader when there's no coordinator; c1 carries it on.)
fn news_cursor(repo: &Repo, cid: &str) -> Res<Option<u64>> {
    let mut paths = vec![cursor_path(repo, cid)];
    if cid == "c1" {
        paths.push(cursor_path(repo, ""));
    }
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(n) = py_int(&text) {
            return match u64::try_from(n) {
                Ok(n) => Ok(Some(n)),
                Err(_) => fail(format!("negative seek position {n}")), // Python's seek fails on it too
            };
        }
    }
    Ok(None)
}

/// Python's int() of a str, for the plain cases: spaces around, a sign, digits with
/// single underscores between them. None where int() fails.
pub fn py_int(text: &str) -> Option<i64> {
    let t = bullpen_core::text::strip(text);
    let (neg, body) = match t.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if body.is_empty() || body.starts_with('_') || body.ends_with('_') || body.contains("__") {
        return None;
    }
    if !body.bytes().all(|b| b.is_ascii_digit() || b == b'_') {
        return None;
    }
    let n: i64 = body.replace('_', "").parse().ok()?;
    Some(if neg { -n } else { n })
}

pub fn set_news_cursor(repo: &Repo, offset: u64, cid: &str) -> Res<()> {
    std::fs::create_dir_all(&repo.dir)?;
    let path = cursor_path(repo, cid);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&tmp, format!("{offset}\n"))?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn news_size(repo: &Repo) -> u64 {
    std::fs::metadata(repo.dir.join("news.log")).map_or(0, |m| m.len())
}

/// Is a logged line for coordinator cid? Lines name the coordinators they're for as
/// `[c1,c2]` before the task ID; lines without, or for coordinators no longer listed,
/// are for everyone.
pub fn news_for(text: &str, cid: &str, listed: &[String]) -> bool {
    let head = text.split(NEWS_MARK).next().unwrap_or("");
    let to: Option<Vec<&str>> = bullpen_core::text::split(head)
        .into_iter()
        .find(|w| w.len() >= 2 && w.starts_with('[') && w.ends_with(']'))
        .map(|w| w[1..w.len() - 1].split(',').collect());
    let Some(to) = to else { return true };
    cid.is_empty() || to.contains(&cid) || !to.iter().any(|x| listed.iter().any(|l| l == x))
}

/// The whole news lines in data, each with the tab-led lines after it, as lists of raw
/// lines. One still being written isn't given, nor the record it would end.
pub fn news_records(data: &[u8]) -> Vec<Vec<&[u8]>> {
    let mut out = Vec::new();
    let mut rec: Vec<&[u8]> = Vec::new();
    for raw in data.split_inclusive(|b| *b == b'\n') {
        if !raw.ends_with(b"\n") {
            return out;
        }
        if !rec.is_empty() && !raw.starts_with(b"\t") {
            out.push(std::mem::take(&mut rec));
        }
        rec.push(raw);
    }
    if !rec.is_empty() {
        out.push(rec);
    }
    out
}

/// A raw line as Python decodes it (errors="replace"), without its newline.
fn decoded(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).trim_end_matches('\n').to_string()
}

/// Print coordinator cid's news logged after offset, moving its cursor past each record
/// once it is out (or skipped: another coordinator's). A record's own lines after it
/// (each starting with a tab: a diff) go with it. Returns the new offset. A line still
/// being written waits.
fn print_news(repo: &Repo, mut offset: u64, cid: &str) -> Res<(u64, Out)> {
    let size = news_size(repo);
    if offset > size {
        // the log was removed or cut short: what's there now is new
        offset = 0;
        set_news_cursor(repo, 0, cid)?;
    }
    if offset == size {
        return Ok((offset, Out::Read));
    }
    let mut fresh = repo.clone();
    fresh.cfg = load_config(&repo.dir)?;
    let listed: Vec<String> = coordinators(&fresh).iter().map(|c| s(c, "id").to_string()).collect();
    let mut f = std::fs::File::open(repo.dir.join("news.log"))?;
    f.seek(SeekFrom::Start(offset))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;
    for raws in news_records(&data) {
        if INTERRUPTED.load(Ordering::SeqCst) {
            break;
        }
        let text = decoded(raws[0]);
        // Printing and moving the cursor go together: being stopped in between (a
        // monitor expiring) would print the line again next time.
        let held = block_signals();
        // a tab-led line with no line before it (a cursor left between them) is skipped
        if !text.starts_with('\t') && news_for(&text, cid, &listed) {
            let at = text.find(NEWS_MARK).unwrap_or(0);
            let mut lines = vec![text[at..].to_string()];
            lines.extend(raws[1..].iter().map(|r| decoded(r).chars().skip(1).collect::<String>()));
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{}", lines.join("\n")).and_then(|_| out.flush()).is_err() {
                restore_signals(&held);
                return Ok((offset, Out::Gone));
            }
        }
        offset += raws.iter().map(|r| r.len() as u64).sum::<u64>();
        let moved = set_news_cursor(repo, offset, cid); // after printing: if nobody was reading, it comes again
        restore_signals(&held);
        moved?;
    }
    Ok((offset, Out::Read))
}

/// Hold SIGTERM, SIGINT and SIGHUP; returns the mask to put back.
fn block_signals() -> libc::sigset_t {
    // SAFETY: sigset_t is plain data, filled by sigemptyset/pthread_sigmask
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        let mut old: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::sigaddset(&mut set, sig);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut old);
        old
    }
}

fn restore_signals(old: &libc::sigset_t) {
    // SAFETY: old came from pthread_sigmask
    unsafe {
        libc::pthread_sigmask(libc::SIG_SETMASK, old, std::ptr::null_mut());
    }
}

#[cfg(test)]
#[path = "test_read.rs"]
mod tests;
