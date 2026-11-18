//! `bullpen news --recent [N]`: the last N lines of the news, each with its time (and,
//! with --all-repos, its repo), for the person and for a news panel. It only reads
//! news.log: it takes no reader's lock and moves no coordinator's cursor, so what a
//! coordinator's `news` or `news --follow` prints next is the same as without it.

use super::record::{self, Record};
use bullpen_core::fleet;
use bullpen_core::json::dumps;
use bullpen_core::paths::state;
use bullpen_core::repo::repo_root;
use bullpen_core::text::{len, ljust};
use bullpen_core::timeutil::{now, parse, Stamp};
use bullpen_core::{Repo, Res};
use chrono::Local;
use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// How many lines `--recent` shows without a number.
pub const RECENT: usize = 20;

/// `news --recent`: print the last n lines, of this repo or (all) of every repo.
pub fn print_recent(n: usize, all: bool, as_json: bool) -> Res<i32> {
    let repos = if all {
        fleet::repos(&state())?
    } else {
        vec![Repo::new(repo_root()?)?]
    };
    let mut items: Vec<(Repo, Record)> = Vec::new();
    for r in repos {
        for it in last_items(&r.dir.join("news.log"), n)? {
            items.push((r.clone(), it));
        }
    }
    // oldest first, by time across repos (a stamp that doesn't parse goes first)
    items.sort_by_key(|(_, it)| parse(&it.stamp));
    let items = items.split_off(items.len().saturating_sub(n));
    let lines = if as_json {
        vec![dumps(&Value::Array(
            items.iter().map(|(r, it)| item_json(r, it)).collect(),
        ))]
    } else {
        text_lines(&items, all, now())
    };
    let mut out = std::io::stdout().lock();
    for l in lines {
        if writeln!(out, "{l}").is_err() {
            return Ok(0); // a reader that's gone (`| head`)
        }
    }
    Ok(0)
}

fn item_json(r: &Repo, it: &Record) -> Value {
    json!({
        "repo": r.root_str(),
        "time": it.stamp,
        "task": it.task,
        "event": it.event,
        "to": it.to,
        "text": it.text(),
        "more": it.more,
    })
}

/// The lines for a terminal: `<when>  [<repo>  ]<text>`, the times (and repos) in a
/// column. A diff after a line is only counted: `--json` has it.
pub fn text_lines(items: &[(Repo, Record)], all: bool, at: Stamp) -> Vec<String> {
    if items.is_empty() {
        return vec![if all {
            "no news in any repository yet"
        } else {
            "no news yet"
        }
        .into()];
    }
    let whens: Vec<String> = items.iter().map(|(_, it)| when(&it.stamp, at)).collect();
    let ww = whens.iter().map(|w| len(w)).max().unwrap_or(0);
    let rw = items.iter().map(|(r, _)| len(&r.name())).max().unwrap_or(0);
    let mut out = Vec::new();
    for ((r, it), w) in items.iter().zip(whens) {
        let repo = if all {
            format!("{}  ", ljust(&r.name(), rw))
        } else {
            String::new()
        };
        let more = match it.more.len() {
            0 => String::new(),
            n => format!(" ({n} more line{}: --json prints them)", if n == 1 { "" } else { "s" }),
        };
        out.push(format!("{}  {repo}{}{more}", ljust(&w, ww), it.text()));
    }
    out
}

/// When a line was logged, for a person: "3:04 PM" today, "Mon 3:04 PM" in the last
/// week, "Oct 9 3:04 PM" before; the stamp as it is when it doesn't parse.
pub fn when(stamp: &str, at: Stamp) -> String {
    let Some(t) = parse(stamp) else {
        return stamp.to_string();
    };
    let t = t.with_timezone(&Local);
    let text = t.format("%-I:%M %p").to_string();
    let days = (at.with_timezone(&Local).date_naive() - t.date_naive()).num_days();
    match days {
        0 => text,
        1..=6 => format!("{} {text}", t.format("%a")),
        _ => format!("{} {text}", t.format("%b %-d")),
    }
}

/// The last n lines of a news log, oldest first. Only its end is read: a bigger piece
/// each time until it holds n whole lines, or it's the whole file.
pub fn last_items(log: &Path, n: usize) -> Res<Vec<Record>> {
    let Ok(mut f) = std::fs::File::open(log) else {
        return Ok(Vec::new()); // no news yet
    };
    let size = f.metadata()?.len();
    let mut piece: u64 = 64 * 1024;
    loop {
        let from = size.saturating_sub(piece);
        f.seek(SeekFrom::Start(from))?;
        let mut data = Vec::new();
        f.read_to_end(&mut data)?;
        let mut recs = record::raw(&data);
        if from > 0 && !recs.is_empty() {
            recs.remove(0); // it may start mid-line, or be a diff without its line
        }
        if recs.len() >= n || from == 0 {
            let start = recs.len().saturating_sub(n);
            return Ok(recs[start..].iter().filter_map(|r| Record::read(r)).collect());
        }
        piece *= 4;
    }
}

#[cfg(test)]
#[path = "tests/test_recent.rs"]
mod tests;
