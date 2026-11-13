//! `bullpen news`, one-off (bullpen/news.py): print the news logged for this
//! coordinator since the last time, once each, moving its cursor past it. `--follow`
//! keeps running, and stays with Python.

use crate::args::Args;
use crate::coordinators::{calling_coordinator, coordinator_at, primary};
use crate::core::{fail, load_config, repo_root, safe, Fail, Repo, Res};
use crate::json::s;
use std::fs::{File, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

const NEWS_MARK: &str = "bullpen · ";
const NOT_A_COORDINATOR: &str = "this pane isn't one of this repo's coordinators, so it doesn't read \
their news: don't start the monitor again (`bullpen up --here` here makes it one)";

pub fn cmd_news(a: &Args) -> Res<()> {
    if a.flag("--follow") || a.opt("--every").is_some_and(|e| e.parse::<f64>().is_err()) {
        return Err(Fail::Python);
    }
    let repo = Repo::new(&repo_root()?)?;
    std::fs::create_dir_all(&repo.dir)?;
    let cid = news_reader(&repo);
    if not_a_reader(&repo, &cid) {
        return fail(NOT_A_COORDINATOR);
    }
    let _lock = lock(&repo, &cid)?; // held until we're done
    let offset = match news_cursor(&repo, &cid)? {
        Some(n) => n,
        None => {
            // first time: old news stays old
            let size = news_size(&repo);
            set_news_cursor(&repo, size, &cid)?;
            size
        }
    };
    print_news(&repo, offset, &cid)
}

/// Whose news it prints: the coordinator in whose pane it runs, else the primary one;
/// "" (all news) when the repo has none.
fn news_reader(repo: &Repo) -> String {
    calling_coordinator(repo).or_else(|| primary(repo)).map_or(String::new(), |c| s(c, "id").to_string())
}

/// Is it running in a Rex pane other than coordinator cid's? (Outside Rex there's no telling.)
fn not_a_reader(repo: &Repo, cid: &str) -> bool {
    let block = std::env::var("REX_BLOCK").unwrap_or_default();
    if block.is_empty() || cid.is_empty() {
        return false;
    }
    coordinator_at(repo, &block, false).is_none_or(|me| s(me, "id") != cid)
}

/// One reader at a time: take the reader's lock without waiting.
fn lock(repo: &Repo, cid: &str) -> Res<File> {
    let name = if cid.is_empty() { "news.lock".into() } else { format!("news-{}.lock", safe(cid)) };
    let f = File::create(repo.dir.join(name))?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(TryLockError::WouldBlock) => fail("news is being followed already (`bullpen news --follow` is running)"),
        Err(TryLockError::Error(e)) => Err(e.into()),
    }
}

fn cursor_path(repo: &Repo, cid: &str) -> PathBuf {
    repo.dir.join(if cid.is_empty() { "news.cursor".into() } else { format!("news-{}.cursor", safe(cid)) })
}

/// How far into news.log (in bytes) coordinator cid has read, or None: never. (c1
/// carries on news.cursor, the one cursor from before several coordinators.) A negative
/// one goes to Python, which fails on it.
fn news_cursor(repo: &Repo, cid: &str) -> Res<Option<u64>> {
    let mut paths = vec![cursor_path(repo, cid)];
    if cid == "c1" {
        paths.push(cursor_path(repo, ""));
    }
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if let Some(n) = py_int(&text) {
            return u64::try_from(n).map(Some).map_err(|_| Fail::Python);
        }
    }
    Ok(None)
}

/// Python's int() of a str, for the plain cases: spaces around, a sign, digits with
/// single underscores between them. None where int() fails (or might not: other digits).
fn py_int(text: &str) -> Option<i64> {
    let t = crate::text::strip(text);
    let (neg, body) = match t.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if body.is_empty() || body.starts_with('_') || body.ends_with('_') || body.contains("__") {
        return None;
    }
    let n: i64 = body.replace('_', "").parse().ok()?;
    Some(if neg { -n } else { n })
}

fn set_news_cursor(repo: &Repo, offset: u64, cid: &str) -> Res<()> {
    std::fs::create_dir_all(&repo.dir)?;
    let path = cursor_path(repo, cid);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&tmp, format!("{offset}\n"))?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn news_size(repo: &Repo) -> u64 {
    std::fs::metadata(repo.dir.join("news.log")).map_or(0, |m| m.len())
}

/// Is a logged line for coordinator cid? `[c1,c2]` before the task ID names whom it's
/// for; lines without, or for coordinators no longer listed, are everyone's.
fn news_for(text: &str, cid: &str, listed: &[String]) -> bool {
    let head = text.split(NEWS_MARK).next().unwrap_or("");
    let to: Option<Vec<&str>> = crate::text::split(head)
        .into_iter()
        .find(|w| w.len() >= 2 && w.starts_with('[') && w.ends_with(']'))
        .map(|w| w[1..w.len() - 1].split(',').collect());
    let Some(to) = to else { return true };
    cid.is_empty() || to.contains(&cid) || !to.iter().any(|x| listed.iter().any(|l| l == x))
}

/// The whole news lines in data, each with the tab-led lines after it, as lists of raw
/// lines. A line still being written isn't given, nor the record it would end.
fn news_records(data: &[u8]) -> Vec<Vec<&[u8]>> {
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
/// once it is out (or skipped: another coordinator's).
fn print_news(repo: &Repo, mut offset: u64, cid: &str) -> Res<()> {
    let size = news_size(repo);
    if offset > size {
        // the log was removed or cut short: what's there now is new
        offset = 0;
        set_news_cursor(repo, 0, cid)?;
    }
    if offset == size {
        return Ok(());
    }
    let cfg = load_config(&repo.dir)?;
    let listed: Vec<String> = cfg
        .get("coordinators")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|c| c.as_object().map(|c| s(c, "id").to_string()))
        .collect();
    let mut f = File::open(repo.dir.join("news.log"))?;
    f.seek(SeekFrom::Start(offset))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;
    for raws in news_records(&data) {
        let text = decoded(raws[0]);
        let held = block_signals();
        if !text.starts_with('\t') && news_for(&text, cid, &listed) {
            let at = text.find(NEWS_MARK).unwrap_or(0);
            let mut lines = vec![text[at..].to_string()];
            lines.extend(raws[1..].iter().map(|r| decoded(r).chars().skip(1).collect::<String>()));
            let mut out = std::io::stdout().lock();
            writeln!(out, "{}", lines.join("\n"))?;
            out.flush()?;
        }
        offset += raws.iter().map(|r| r.len() as u64).sum::<u64>();
        set_news_cursor(repo, offset, cid)?; // after printing: if nobody was reading, it comes again
        restore_signals(&held);
    }
    Ok(())
}

/// Hold SIGTERM, SIGINT and SIGHUP: printing a line and moving the cursor go together.
fn block_signals() -> libc::sigset_t {
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
    unsafe {
        libc::pthread_sigmask(libc::SIG_SETMASK, old, std::ptr::null_mut());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_as_python_reads_them() {
        assert_eq!(py_int(" 12\n"), Some(12));
        assert_eq!(py_int("+1_000"), Some(1000));
        assert_eq!(py_int("-5"), Some(-5));
        for bad in ["junk", "", "1__0", "_1", "1.5"] {
            assert_eq!(py_int(bad), None, "{bad}");
        }
    }

    #[test]
    fn whose_news() {
        let listed = vec!["c1".to_string(), "c2".to_string()];
        assert!(news_for("t [c1,c2] BP-1 x: bullpen · hi", "c2", &listed));
        assert!(!news_for("t [c1] BP-1 x: bullpen · hi", "c2", &listed));
        assert!(news_for("t [c7] BP-1 x: bullpen · hi", "c2", &listed));
        assert!(news_for("t BP-1 x: bullpen · [c1]", "c2", &listed));
    }

    #[test]
    fn records_wait_for_their_end() {
        let recs = news_records(b"a\n\tb\nc\n\td\npart");
        assert_eq!(recs, vec![vec![&b"a\n"[..], b"\tb\n"]]);
    }
}
