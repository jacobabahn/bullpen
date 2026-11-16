//! `bullpen news --recent [N]`: the last N lines of the news, each with its time (and,
//! with --all-repos, its repo), for the person and for a news panel. It only reads
//! news.log: it takes no reader's lock and moves no coordinator's cursor, so what a
//! coordinator's `news` or `news --follow` prints next is the same as without it.

use super::read::news_records;
use super::NEWS_MARK;
use bullpen_core::json::dumps;
use bullpen_core::repo::{known_repos, repo_root};
use bullpen_core::text::{len, ljust};
use bullpen_core::timeutil::{now, parse, Stamp};
use bullpen_core::{Repo, Res};
use chrono::Local;
use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// How many lines `--recent` shows without a number.
pub const RECENT: usize = 20;

/// One logged line of news: `<stamp>[ [to]] <task> <event>: bullpen · <text>`, and the
/// tab-led lines after it (a diff).
#[derive(Debug, PartialEq)]
pub struct Item {
    pub stamp: String,
    pub to: Vec<String>,
    pub task: String,
    pub event: String,
    /// what it says, without `bullpen · `
    pub text: String,
    pub more: Vec<String>,
}

/// `news --recent`: print the last n lines, of this repo or (all) of every repo.
pub fn print_recent(n: usize, all: bool, as_json: bool) -> Res<i32> {
    let repos = if all {
        known_repos()?
    } else {
        vec![Repo::new(repo_root()?)?]
    };
    let mut items: Vec<(Repo, Item)> = Vec::new();
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

fn item_json(r: &Repo, it: &Item) -> Value {
    json!({
        "repo": r.root_str(),
        "time": it.stamp,
        "task": it.task,
        "event": it.event,
        "to": it.to,
        "text": it.text,
        "more": it.more,
    })
}

/// The lines for a terminal: `<when>  [<repo>  ]<text>`, the times (and repos) in a
/// column. A diff after a line is only counted: `--json` has it.
pub fn text_lines(items: &[(Repo, Item)], all: bool, at: Stamp) -> Vec<String> {
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
        out.push(format!("{}  {repo}{}{more}", ljust(&w, ww), it.text));
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
pub fn last_items(log: &Path, n: usize) -> Res<Vec<Item>> {
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
        let mut recs = news_records(&data);
        if from > 0 && !recs.is_empty() {
            recs.remove(0); // it may start mid-line, or be a diff without its line
        }
        if recs.len() >= n || from == 0 {
            let start = recs.len().saturating_sub(n);
            return Ok(recs[start..].iter().filter_map(|r| item(r)).collect());
        }
        piece *= 4;
    }
}

/// A record's Item, or None for one that isn't a news line (a diff left without its line).
fn item(raws: &[&[u8]]) -> Option<Item> {
    let first = String::from_utf8_lossy(raws[0]).trim_end_matches('\n').to_string();
    let mut it = parse_line(&first)?;
    it.more = raws[1..]
        .iter()
        .map(|r| String::from_utf8_lossy(&r[1..]).trim_end_matches('\n').to_string())
        .collect();
    Some(it)
}

/// A news.log line's parts: `<stamp>[ [to]] <task> <event>: <said>`.
pub fn parse_line(line: &str) -> Option<Item> {
    if line.starts_with('\t') {
        return None;
    }
    let (stamp, rest) = line.split_once(' ')?;
    let (to, rest) = match rest.strip_prefix('[').and_then(|r| r.split_once("] ")) {
        Some((to, rest)) => (to.split(',').map(String::from).collect(), rest),
        None => (Vec::new(), rest),
    };
    let (task, rest) = rest.split_once(' ')?;
    let (event, said) = rest.split_once(": ")?;
    let text = match said.find(NEWS_MARK) {
        Some(i) => &said[i + NEWS_MARK.len()..],
        None => said,
    };
    Some(Item {
        stamp: stamp.into(),
        to,
        task: task.into(),
        event: event.into(),
        text: text.into(),
        more: Vec::new(),
    })
}

#[cfg(test)]
#[path = "test_recent.rs"]
mod tests;
