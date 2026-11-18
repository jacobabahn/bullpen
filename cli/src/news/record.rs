//! The one reader of news.log. A record is a line `<stamp>[ [to]] <task> <event>:
//! <said>` (news() writes `said` as `bullpen · <text>`) and the tab-led lines after it
//! (a diff). The backstop (wake.rs), `news`, `news --recent`, the last line about a
//! task (ready_news) and the tests' news check (`_news_log`) all read it through here.

use super::kind::Kind;
use super::NEWS_MARK;
use bullpen_core::json::dumps;
use bullpen_core::Res;
use serde_json::{json, Value};

/// One logged line of news, and the lines after it.
#[derive(Debug, PartialEq)]
pub struct Record {
    pub stamp: String,
    /// the coordinators it is for (none named: everyone)
    pub to: Vec<String>,
    pub task: String,
    /// its kind's word (kind.rs)
    pub event: String,
    /// all after `<event>: `
    pub said: String,
    /// the tab-led lines after it, without their tab
    pub more: Vec<String>,
}

impl Record {
    /// A record's first line in its parts, or None for one that isn't a news line (a
    /// diff line, or junk).
    pub fn parse(line: &str) -> Option<Record> {
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
        if stamp.is_empty() || event.is_empty() || event.contains([' ', ':']) || said.contains('\n') {
            return None;
        }
        Some(Record {
            stamp: stamp.into(),
            to,
            task: task.into(),
            event: event.into(),
            said: said.into(),
            more: Vec::new(),
        })
    }

    /// A record of raw lines (as `raw` gives them), or None when its first isn't a news line.
    pub fn read(raws: &[&[u8]]) -> Option<Record> {
        let mut r = Record::parse(&decoded(raws[0]))?;
        r.more = raws[1..].iter().map(|l| more_line(l)).collect();
        Some(r)
    }

    /// What it says, without `bullpen · `.
    pub fn text(&self) -> &str {
        match self.said.find(NEWS_MARK) {
            Some(i) => &self.said[i + NEWS_MARK.len()..],
            None => &self.said,
        }
    }

    /// Its kind, if its event names one (an old log's may not).
    pub fn kind(&self) -> Option<Kind> {
        Kind::of_event(&self.event)
    }

    /// Is it worth waking a coordinator for? Its kind says; an event no kind has wakes.
    pub fn wakes(&self) -> bool {
        self.kind().is_none_or(Kind::wakes)
    }

    /// Is it for coordinator cid? One that names none, or only coordinators no longer
    /// listed, is for everyone; with no coordinator (cid empty), all news is.
    pub fn is_for(&self, cid: &str, listed: &[String]) -> bool {
        self.to.is_empty()
            || cid.is_empty()
            || self.to.iter().any(|c| c == cid)
            || !self.to.iter().any(|c| listed.contains(c))
    }

    pub fn to_json(&self) -> Value {
        json!({
            "time": self.stamp,
            "to": self.to,
            "task": self.task,
            "event": self.event,
            "said": self.said,
            "text": self.text(),
            "more": self.more,
        })
    }
}

/// The whole records in data, each a news line with the tab-led lines after it, as raw
/// lines. One still being written isn't given, nor the record it would end.
pub fn raw(data: &[u8]) -> Vec<Vec<&[u8]>> {
    let mut out = Vec::new();
    let mut rec: Vec<&[u8]> = Vec::new();
    for line in data.split_inclusive(|b| *b == b'\n') {
        if !line.ends_with(b"\n") {
            return out;
        }
        if !rec.is_empty() && !line.starts_with(b"\t") {
            out.push(std::mem::take(&mut rec));
        }
        rec.push(line);
    }
    if !rec.is_empty() {
        out.push(rec);
    }
    out
}

/// What a reader shows of raw lines, if they are for coordinator cid: the first from
/// `bullpen · ` on, and the lines after it. A first line that isn't a news line is for
/// everyone, shown whole; a tab-led one with no line before it (a cursor left between
/// them) is for no one.
pub fn shown(raws: &[&[u8]], cid: &str, listed: &[String]) -> Option<Vec<String>> {
    let first = decoded(raws[0]);
    if first.starts_with('\t') || Record::parse(&first).is_some_and(|r| !r.is_for(cid, listed)) {
        return None;
    }
    let at = first.find(NEWS_MARK).unwrap_or(0);
    let mut lines = vec![first[at..].to_string()];
    lines.extend(raws[1..].iter().map(|l| more_line(l)));
    Some(lines)
}

/// A raw line as Python decodes it (errors="replace"), without its newline.
fn decoded(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).trim_end_matches('\n').to_string()
}

/// A tab-led line without its tab.
fn more_line(raw: &[u8]) -> String {
    decoded(raw).chars().skip(1).collect()
}

/// `_news_log LOG...`: the records of these logs as JSON, one array per log, for the
/// tests' news check (support.py), which reads the log through here rather than parse it.
pub fn cmd_news_log(argv: &[String]) -> Res<i32> {
    use crate::args::{pos, Args, Nargs};
    let a = Args::parse("_news_log", argv, &[pos("LOG", Nargs::Any, "")]);
    let mut logs = Vec::new();
    for path in a.args("LOG") {
        let data = std::fs::read(path)?;
        let records: Vec<Value> = raw(&data)
            .iter()
            .filter_map(|r| Record::read(r))
            .map(|r| r.to_json())
            .collect();
        logs.push(json!({"log": path, "records": records}));
    }
    println!("{}", dumps(&Value::Array(logs)));
    Ok(0)
}

#[cfg(test)]
#[path = "test_record.rs"]
mod tests;
