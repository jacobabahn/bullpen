//! The usage limit: is one in force and when it resets (for `start`
//! and the run queue), and reading one off an agent's turn or screen, what the task
//! and <state>/limit.json then hold until it resets (for the watcher's events).
//! In force: `limit_active`, `limit_phrase`; read off a turn or screen: `limit_hit`,
//! `limit_over`, `limit_reset`.
//!
//! Every agent shares one subscription. An agent that reaches its usage limit stops
//! with a line like "You've hit your 5-hour limit · resets 3pm": Claude Code reports
//! that turn as `error` (or just goes idle), with the line as the record's msg or only
//! on its screen. Nothing for the person to do: the task keeps its status, carries a
//! `limit` and is continued after the reset. <state>/limit.json holds the limit for
//! every repo while it lasts; `start` refuses meanwhile, and the queue starts nothing.
//!
//! Python's patterns (LIMIT_RE, RESET_RE and the older Unix-time form) are matched by
//! hand here, with re's own rules: (?i)'s case folds, Unicode `\s`, `\d` and `\w`.

use crate::agents::pane;
use crate::news::kind::Kind;
use bullpen_core::json::{self, obj, s, truthy, Obj};
use bullpen_core::paths::{is_word, rex, state};
use bullpen_core::proc::run;
use bullpen_core::repo::Repo;
use bullpen_core::task::{clip, is_limited, is_waiting, set_status};
use bullpen_core::text::{is_space, splitlines, squash, strip};
use bullpen_core::timeutil::{clock, iso_now, iso_seconds, now, parse, seconds_since, Stamp};
use bullpen_core::Res;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde_json::{json, Value};
use std::path::PathBuf;

/// How a turn ends that may be at the limit; a blocked agent waits on the person.
pub const LIMIT_STATES: [&str; 3] = ["error", "idle", "done"];
/// Seconds after the reset before continuing: Claude Code may do it itself.
pub const LIMIT_GRACE: f64 = 120.0;
/// Seconds a limit with no known reset time holds `start` back.
pub const LIMIT_FORGET: f64 = 5.0 * 3600.0;
/// What a limited agent is told once the limit has reset.
pub const LIMIT_CONTINUE: &str = "The usage limit has reset. Carry on where you stopped.";
const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// <state>/limit.json (LIMIT_FILE).
pub fn limit_file() -> PathBuf {
    state().join("limit.json")
}

/// The usage limit in force ({"at", "resets", "text"}), or None. It holds until its
/// reset time; one with no known reset time, for LIMIT_FORGET seconds.
pub fn limit_active(at: Option<Stamp>) -> Res<Option<Obj>> {
    limit_in(&limit_file(), at)
}

/// limit_active, for the limit noted in a file.
pub fn limit_in(file: &std::path::Path, at: Option<Stamp>) -> Res<Option<Obj>> {
    let lim = match json::read(file) {
        Ok(Some(Value::Object(o))) => o,
        Ok(_) => return Ok(None),
        Err(e) => return bullpen_core::fail(e),
    };
    if lim.is_empty() {
        return Ok(None);
    }
    let holds = if truthy(lim.get("resets")) {
        seconds_since(s(&lim, "resets"), at) < 0.0
    } else {
        seconds_since(s(&lim, "at"), at) < LIMIT_FORGET
    };
    Ok(holds.then_some(lim))
}

/// "resets 3:00 PM" for a stored reset time, "reset time unknown" for none.
pub fn limit_phrase(resets: Option<&Value>, at: Option<Stamp>) -> String {
    match resets.and_then(Value::as_str).filter(|r| !r.is_empty()).and_then(parse) {
        Some(when) => format!("resets {}", clock(when, at)),
        None => "reset time unknown".into(),
    }
}

/// Is this agent of the task (its "worker" or its "reviewer") stopped at the limit?
pub fn limited(t: &Obj, who: &str) -> bool {
    is_limited(t) && s(obj(t, "limit"), "who") == who
}

/// A limited task's why: "Usage limit, resets 3:00 PM".
pub fn limit_why(lim: &Obj, at: Option<Stamp>) -> String {
    format!("Usage limit, {}", limit_phrase(lim.get("resets"), at))
}

// ---------- reading a limit ----------

/// The limit message an agent's turn ended with, or None. It is the record's msg, or
/// (for a turn that failed or went idle, not one that finished) among the last lines
/// of its screen. A screen still showing the message we've already acted on (`seen`)
/// says nothing new.
pub fn limit_text(session: &str, block: &str, state: &str, msg: &str, seen: Option<&str>) -> Res<Option<String>> {
    if !LIMIT_STATES.contains(&state) {
        return Ok(None);
    }
    if says_limit(msg) {
        return Ok(Some(msg.to_string()));
    }
    if state == "done" {
        return Ok(None);
    }
    let mut argv = vec![rex(), "capture".into()];
    if !session.is_empty() {
        argv.extend(["-s".into(), session.into()]);
    }
    argv.extend(["-b".into(), block.into()]);
    let p = run(&argv, false)?;
    let lines: Vec<String> = splitlines(&p.stdout)
        .into_iter()
        .filter(|l| !strip(l).is_empty())
        .map(squash)
        .collect();
    let lines = &lines[lines.len().saturating_sub(15)..];
    for i in (0..lines.len()).rev() {
        if says_limit(&lines[i]) {
            // with the next line: it may have wrapped
            let text = clip(&lines[i..(i + 2).min(lines.len())].join(" "), 200);
            return Ok((Some(text.as_str()) != seen).then_some(text));
        }
    }
    Ok(None)
}

/// Did this event (its block, state and msg) say the agent stopped at the usage limit?
/// Then note it on the task and for `start`, with one line of news, and return true.
pub fn limit_hit(repo: &Repo, t: &mut Obj, who: &str, block: &str, state: &str, msg: &str) -> Res<bool> {
    let (session, _) = pane(t, who);
    let msg = strip(msg);
    let seen = t.get("limit_seen").and_then(Value::as_str).map(String::from);
    let Some(text) = limit_text(&session, block, state, msg, seen.as_deref())? else {
        return Ok(false);
    };
    let text = clip(&text, 200);
    let before = limit_active(None)?;
    let resets = match limit_reset(&text, None) {
        Some(when) => json!(iso_seconds(&when)),
        None => before
            .as_ref()
            .and_then(|b| b.get("resets"))
            .cloned()
            .unwrap_or(Value::Null),
    };
    let lim = json!({"at": iso_now(), "resets": resets, "text": text});
    json::write(&limit_file(), &lim)?;
    let id = t.get("id").map_or(String::new(), json::py_str);
    let name = if who == "reviewer" {
        format!("{id}'s reviewer")
    } else {
        id.clone()
    };
    let status = s(t, "status");
    let waits = if who == "reviewer" {
        status == "review" || is_waiting(status)
    } else {
        status == "working" || is_waiting(status) || status == "stopped"
    };
    if waits {
        // else the task doesn't wait on this agent: nothing to continue later
        let lim = json!({"who": who, "at": iso_now(), "resets": resets, "text": text});
        let lim = lim.as_object().expect("an object").clone();
        let why = limit_why(&lim, None);
        set_status(
            t,
            if who == "reviewer" { "review" } else { "working" },
            Some(&why),
            None,
        );
        t.insert("limit".into(), Value::Object(lim));
        let last = if who == "worker" {
            json!(format!("{state}|{msg}"))
        } else {
            t.get("last").cloned().unwrap_or(Value::Null)
        };
        t.insert("last".into(), last);
        crate::saves::save(repo, t)?;
    }
    let mut line = if truthy(Some(&resets)) {
        let then = if waits { ", it is continued then" } else { "" };
        format!(
            "{name} stopped at the usage limit, {}: nothing to do{then}",
            limit_phrase(Some(&resets), None)
        )
    } else if who == "reviewer" {
        format!(
            "{name} stopped at the usage limit, and its reset time couldn't be read, so it won't \
             be continued by itself: tell the person (`bullpen go {id} --review` shows it)"
        )
    } else {
        format!(
            "{name} stopped at the usage limit, and its reset time couldn't be read, so it won't \
             be continued by itself: tell the person, and `bullpen tell {id} continue` when they say the limit has \
             reset"
        )
    };
    if before.is_none() {
        line += ". `bullpen start` refuses meanwhile (`--force` overrides, only if the person asks)";
    }
    crate::news::say(repo, t, Kind::Limit, &line)?;
    Ok(true)
}

/// The agent the task waits on is working again: forget its limit, and let `start`
/// through. True if it was limited. limit.json stays, as a limit that has reset: other
/// tasks may still wait to be continued (`_limit` removes it).
pub fn limit_over(t: &mut Obj) -> Res<bool> {
    if !is_limited(t) {
        return Ok(false);
    }
    let lim = t.shift_remove("limit").unwrap_or(Value::Null);
    t.insert("limit_seen".into(), lim.get("text").cloned().unwrap_or(Value::Null));
    let path = limit_file();
    match json::read(&path) {
        Ok(Some(Value::Object(mut lim))) if !lim.is_empty() => {
            lim.insert("resets".into(), json!(iso_now()));
            json::write_obj(&path, &lim)?;
        }
        Ok(_) => {}
        Err(e) => return bullpen_core::fail(e),
    }
    Ok(true)
}

// ---------- when it resets ----------

/// When a limit message says the limit resets, as a local time, or None if it doesn't
/// say so in a way we can read. Reads "resets 3pm", "resets 3:45pm", "resets Mon
/// 12:00am", "resets Oct 9, 3pm", "reset at 3pm (America/New_York)" and "resets
/// 15:00"; a time with no date is the next time the clock shows it.
///
/// As Python's datetimes do it: the clock is at's (its fixed offset), or the named
/// zone's, and the arithmetic and comparisons are on that wall clock.
pub fn limit_reset(text: &str, at: Option<Stamp>) -> Option<Stamp> {
    let at = at.unwrap_or_else(now);
    let t: Vec<char> = text.chars().collect();
    if let Some(secs) = unix_time(&t) {
        // older Claude Code: a Unix time
        return Local.timestamp_opt(secs, 0).single().map(|w| w.fixed_offset());
    }
    let r = (0..t.len()).find_map(|i| reset_at(&t, i))?;
    if r.ap.is_none() && r.m.is_none() {
        return None;
    }
    let (mut hour, minute) = (r.h, r.m.unwrap_or(0));
    if let Some(ap) = r.ap {
        if !(1..=12).contains(&hour) {
            return None;
        }
        hour = hour % 12 + if ci(ap, 'p') { 12 } else { 0 };
    }
    if hour > 23 || minute > 59 {
        return None;
    }
    // fold: the second time the zone's clock shows it (fall back); replace keeps it,
    // adding a timedelta clears it
    let (here, mut fold) = match &r.tz {
        None => (at.naive_local(), false),
        Some(tz) => {
            if !zone_exists(tz) {
                return None;
            }
            let (here, fold) = in_zone(tz, || {
                let here = wall(at.timestamp());
                (here, instant(here, false) != at.timestamp())
            });
            (here + Duration::nanoseconds(at.timestamp_subsec_nanos().into()), fold)
        }
    };
    let mut when = here.date().and_hms_opt(hour, minute, 0)?;
    let slack = Duration::minutes(5); // a time just gone is now, not tomorrow
    if let Some(month) = r.month {
        let date = r.date.unwrap_or(0);
        when = NaiveDate::from_ymd_opt(when.year(), month as u32 + 1, date)?.and_time(when.time());
        if when < here - Duration::days(1) {
            // Python raises here (Feb 29 into a year without one): read as no time
            when = NaiveDate::from_ymd_opt(when.year() + 1, when.month(), when.day())?.and_time(when.time());
        }
    } else if let Some(day) = r.day {
        let ahead = (day as i64 - when.weekday().num_days_from_monday() as i64).rem_euclid(7);
        when += Duration::days(ahead);
        fold = false;
        if when <= here - slack {
            when += Duration::days(7);
        }
    } else if when <= here - slack {
        when += Duration::days(1);
        fold = false;
    }
    let secs = match &r.tz {
        None => at.offset().from_local_datetime(&when).single()?.timestamp(),
        Some(tz) => in_zone(tz, || instant(when, fold)),
    };
    Local.timestamp_opt(secs, 0).single().map(|w| w.fixed_offset())
}

/// What RESET_RE's groups caught.
#[derive(Debug, Default, PartialEq)]
struct Reset {
    day: Option<usize>,
    month: Option<usize>,
    date: Option<u32>,
    h: u32,
    m: Option<u32>,
    ap: Option<char>,
    tz: Option<String>,
}

/// LIMIT_RE: does the text say an agent is at its limit? `(?i)you['’]ve hit your
/// [\w' -]*limit|usage limit reached|limit will reset|out of usage credits`.
fn says_limit(text: &str) -> bool {
    let t: Vec<char> = text.chars().collect();
    (0..t.len()).any(|i| {
        let hit = || {
            let j = lit(&t, i, "you")?;
            let j = (j < t.len() && matches!(t[j], '\'' | '’')).then_some(j + 1)?;
            let mut k = lit(&t, j, "ve hit your ")?;
            loop {
                if lit(&t, k, "limit").is_some() {
                    return Some(());
                }
                if k < t.len() && (is_word(t[k]) || matches!(t[k], '\'' | ' ' | '-')) {
                    k += 1;
                } else {
                    return None;
                }
            }
        };
        hit().is_some()
            || ["usage limit reached", "limit will reset", "out of usage credits"]
                .iter()
                .any(|w| lit(&t, i, w).is_some())
    })
}

/// `limit reached\|(\d{10})\b`, as a number.
fn unix_time(t: &[char]) -> Option<i64> {
    let at = |i: usize| {
        let j = i + "limit reached|".len();
        let same = t.get(i..j)?.iter().copied().eq("limit reached|".chars());
        let digits: Vec<u32> = (j..j + 10)
            .map(|k| t.get(k).copied().and_then(digit))
            .collect::<Option<_>>()?;
        let boundary = t.get(j + 10).is_none_or(|&c| !is_word(c));
        (same && boundary).then(|| digits.iter().fold(0i64, |n, &d| n * 10 + d as i64))
    };
    (0..t.len()).find_map(at)
}

/// RESET_RE matched at i (re.search tries each i in turn):
///   (?i)resets?(?:\s+at)?\s+
///   (?:(?P<day>mon|tue|...)[a-z]*,?\s+)?
///   (?:(?P<month>jan|...)[a-z]*\.?\s+(?P<date>\d{1,2})(?:,|\s+at)?\s+)?
///   (?P<h>\d{1,2})(?::(?P<m>\d{2}))?\s*(?P<ap>[ap])?(?(ap)\.?m\b\.?)
///   (?:\s*\((?P<tz>[A-Za-z_]+(?:/[A-Za-z0-9_+-]+)*)\))?
/// Each choice is tried in re's order, the greedy one first.
fn reset_at(t: &[char], i: usize) -> Option<Reset> {
    let mut j = lit(t, i, "reset")?;
    if t.get(j).is_some_and(|&c| ci(c, 's')) {
        j += 1;
    }
    let with_at = spaces1(t, j).and_then(|k| lit(t, k, "at")).and_then(|k| spaces1(t, k));
    [with_at, spaces1(t, j)].into_iter().flatten().find_map(|k| {
        let day = day_at(t, k).map(|(d, e)| (Some(d), e));
        [day, Some((None, k))].into_iter().flatten().find_map(|(day, k)| {
            let mut r = dated_time(t, k)?;
            r.day = day;
            Some(r)
        })
    })
}

/// `(?P<day>mon|...)[a-z]*,?\s+`: the day and where the group ends.
fn day_at(t: &[char], i: usize) -> Option<(usize, usize)> {
    let (d, mut j) = DAYS.iter().enumerate().find_map(|(d, w)| Some((d, lit(t, i, w)?)))?;
    while t.get(j).is_some_and(|&c| is_az(c)) {
        j += 1;
    }
    if t.get(j) == Some(&',') {
        j += 1;
    }
    Some((d, spaces1(t, j)?))
}

/// The month group, if it's there, then the time.
fn dated_time(t: &[char], i: usize) -> Option<Reset> {
    if let Some(r) = month_at(t, i) {
        return Some(r);
    }
    time_at(t, i)
}

/// `(?P<month>jan|...)[a-z]*\.?\s+(?P<date>\d{1,2})(?:,|\s+at)?\s+`, then the time.
fn month_at(t: &[char], i: usize) -> Option<Reset> {
    let (month, mut j) = MONTHS.iter().enumerate().find_map(|(m, w)| Some((m, lit(t, i, w)?)))?;
    while t.get(j).is_some_and(|&c| is_az(c)) {
        j += 1;
    }
    if t.get(j) == Some(&'.') {
        j += 1;
    }
    let j = spaces1(t, j)?;
    let (date, ends) = digits_at(t, j)?;
    ends.into_iter().find_map(|k| {
        let comma = (t.get(k) == Some(&',')).then_some(k + 1);
        let at = spaces1(t, k).and_then(|e| lit(t, e, "at"));
        [comma, at, Some(k)].into_iter().flatten().find_map(|e| {
            let mut r = time_at(t, spaces1(t, e)?)?;
            r.month = Some(month);
            r.date = Some(date(k));
            Some(r)
        })
    })
}

/// `\d{1,2}` at i: its value for an end, and the ends to try, the longer first.
fn digits_at(t: &[char], i: usize) -> Option<(impl Fn(usize) -> u32 + '_, Vec<usize>)> {
    let first = t.get(i).copied().and_then(digit)?;
    let mut ends = vec![i + 1];
    if t.get(i + 1).copied().and_then(digit).is_some() {
        ends.insert(0, i + 2);
    }
    let value = move |end: usize| {
        if end == i + 2 {
            first * 10 + t.get(i + 1).copied().and_then(digit).unwrap_or(0)
        } else {
            first
        }
    };
    Some((value, ends))
}

/// `(?P<h>\d{1,2})(?::(?P<m>\d{2}))?\s*(?P<ap>[ap])?(?(ap)\.?m\b\.?)` and the zone.
/// Everything after h may be left out, so the longest h always matches.
fn time_at(t: &[char], i: usize) -> Option<Reset> {
    let (h, ends) = digits_at(t, i)?;
    let mut j = ends[0];
    let mut r = Reset {
        h: h(j),
        ..Reset::default()
    };
    if t.get(j) == Some(&':') {
        let d = (
            t.get(j + 1).copied().and_then(digit),
            t.get(j + 2).copied().and_then(digit),
        );
        if let (Some(a), Some(b)) = d {
            r.m = Some(a * 10 + b);
            j += 3;
        }
    }
    j = spaces(t, j);
    if let Some(&ap) = t.get(j).filter(|&&c| matches!(c, 'a' | 'A' | 'p' | 'P')) {
        let mut k = j + 1;
        if t.get(k) == Some(&'.') {
            k += 1;
        }
        if t.get(k).is_some_and(|&c| ci(c, 'm')) && t.get(k + 1).is_none_or(|&c| !is_word(c)) {
            k += 1;
            if t.get(k) == Some(&'.') {
                k += 1;
            }
            r.ap = Some(ap);
            j = k;
        }
    }
    r.tz = zone_at(t, j);
    Some(r)
}

/// `\s*\((?P<tz>[A-Za-z_]+(?:/[A-Za-z0-9_+-]+)*)\)` at i: the zone's name.
fn zone_at(t: &[char], i: usize) -> Option<String> {
    let mut j = spaces(t, i);
    (t.get(j) == Some(&'(')).then_some(())?;
    j += 1;
    let start = j;
    let first = |c: char| is_az(c) || c == '_';
    let rest = |c: char| first(c) || c.is_ascii_digit() || c == '+' || c == '-';
    while t.get(j).is_some_and(|&c| first(c)) {
        j += 1;
    }
    (j > start).then_some(())?;
    while t.get(j) == Some(&'/') && t.get(j + 1).is_some_and(|&c| rest(c)) {
        j += 1;
        while t.get(j).is_some_and(|&c| rest(c)) {
            j += 1;
        }
    }
    (t.get(j) == Some(&')')).then(|| t[start..j].iter().collect())
}

// ---------- matching as re does ----------

/// One character of the text against one of a (?i) pattern: re folds case simply,
/// and also takes İ and ı for i, ſ for s and the Kelvin sign for k.
fn ci(c: char, p: char) -> bool {
    c.eq_ignore_ascii_case(&p)
        || match p.to_ascii_lowercase() {
            'i' => matches!(c, '\u{130}' | '\u{131}'),
            's' => c == '\u{17f}',
            'k' => c == '\u{212a}',
            _ => false,
        }
}

/// `(?i)[a-z]`.
fn is_az(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '\u{130}' | '\u{131}' | '\u{17f}' | '\u{212a}')
}

/// The (?i) word at i: where it ends.
fn lit(t: &[char], i: usize, word: &str) -> Option<usize> {
    let n = word.chars().count();
    let here = t.get(i..i + n)?;
    here.iter().zip(word.chars()).all(|(&c, p)| ci(c, p)).then_some(i + n)
}

/// `\s*`: where the run of whitespace at i ends.
fn spaces(t: &[char], mut i: usize) -> usize {
    while t.get(i).is_some_and(|&c| is_space(c)) {
        i += 1;
    }
    i
}

/// `\s+`.
fn spaces1(t: &[char], i: usize) -> Option<usize> {
    let j = spaces(t, i);
    (j > i).then_some(j)
}

/// The first digit (0) of each run of Unicode decimal digits (Nd), as `\d` and int()
/// read them.
const ZEROS: [u32; 76] = [
    0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66, 0xde6, 0xe50, 0xed0,
    0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90, 0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620,
    0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0, 0xff10, 0x104a0, 0x10d30, 0x10d40, 0x11066, 0x110f0, 0x11136,
    0x111d0, 0x112f0, 0x11450, 0x114d0, 0x11650, 0x116c0, 0x116d0, 0x116da, 0x11730, 0x118e0, 0x11950, 0x11bf0,
    0x11c50, 0x11d50, 0x11da0, 0x11f50, 0x16130, 0x16a60, 0x16ac0, 0x16b50, 0x16d70, 0x1ccf0, 0x1d7ce, 0x1d7d8,
    0x1d7e2, 0x1d7ec, 0x1d7f6, 0x1e140, 0x1e2f0, 0x1e4f0, 0x1e5f1, 0x1e950, 0x1fbf0,
];

/// `\d`: the digit's value.
fn digit(c: char) -> Option<u32> {
    let c = c as u32;
    let zero = ZEROS[..ZEROS.partition_point(|&z| z <= c)].last()?;
    (c - zero < 10).then_some(c - zero)
}

// ---------- a named zone's clock, through libc ----------

/// Is there a zone of this name? As zoneinfo: a TZif file under the zoneinfo folder.
fn zone_exists(name: &str) -> bool {
    let dir = std::env::var_os("TZDIR")
        .filter(|d| !d.is_empty())
        .map_or_else(|| PathBuf::from("/usr/share/zoneinfo"), PathBuf::from);
    let mut magic = [0u8; 4];
    std::fs::File::open(dir.join(name))
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut magic))
        .is_ok()
        && &magic == b"TZif"
}

extern "C" {
    fn tzset();
}

/// f, run with the zone as the C library's local time; TZ is put back after.
fn in_zone<R>(name: &str, f: impl FnOnce() -> R) -> R {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let old = std::env::var_os("TZ");
    std::env::set_var("TZ", name);
    unsafe { tzset() };
    let r = f();
    match old {
        Some(v) => std::env::set_var("TZ", v),
        None => std::env::remove_var("TZ"),
    }
    unsafe { tzset() };
    r
}

/// The zone's UTC offset at a Unix time, in seconds.
fn offset_at(secs: i64) -> i64 {
    let t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return 0;
    }
    tm.tm_gmtoff as i64
}

/// The zone's clock at a Unix time.
fn wall(secs: i64) -> NaiveDateTime {
    let off = offset_at(secs);
    chrono::DateTime::from_timestamp(secs + off, 0).map_or(NaiveDateTime::MIN, |d| d.naive_utc())
}

/// The Unix time the zone's clock shows `when` at, as zoneinfo does: of two, the
/// earlier (the later with fold); in a gap, by the offset before it (after it, with
/// fold).
fn instant(when: NaiveDateTime, fold: bool) -> i64 {
    let n = when.and_utc().timestamp();
    let before = offset_at(n - 86400);
    let after = offset_at(n + 86400);
    let found = [n - before, n - after].into_iter().filter(|&u| offset_at(u) == n - u);
    let found = if fold { found.max() } else { found.min() };
    found.unwrap_or(if fold { n - after } else { n - before })
}

#[cfg(test)]
#[path = "tests/test_limit.rs"]
mod tests;
