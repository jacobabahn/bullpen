//! Tasks from before timelines were recorded: what the news log says of them. Its
//! lines ("<stamp> [c1] BP-3 ready: bullpen · ...") are shown as the task's timeline,
//! but they don't say every change of status, so its phases stay unknown.

use crate::news::NEWS_MARK;
use bullpen_core::json::Obj;
use bullpen_core::task::clip;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

/// Each task's news, oldest first, as timeline entries (event "news": kind, text).
pub fn news_by_task(dir: &Path) -> HashMap<String, Vec<Obj>> {
    let mut out: HashMap<String, Vec<Obj>> = HashMap::new();
    let text = std::fs::read_to_string(dir.join("news.log")).unwrap_or_default();
    for line in text.lines().filter(|l| !l.starts_with('\t')) {
        if let Some((id, e)) = news_entry(line) {
            out.entry(id).or_default().push(e);
        }
    }
    out
}

/// A news record's first line as (task ID, entry), or None for one that isn't a task's.
pub fn news_entry(line: &str) -> Option<(String, Obj)> {
    let (at, rest) = line.split_once(' ')?;
    let rest = match rest.strip_prefix('[') {
        Some(r) => r.split_once("] ")?.1, // the coordinators it is for
        None => rest,
    };
    let (id, rest) = rest.split_once(' ')?;
    let (kind, text) = rest.split_once(": ")?;
    if !id.contains('-') || kind.contains(' ') || bullpen_core::timeutil::parse(at).is_none() {
        return None;
    }
    let text = text.strip_prefix(NEWS_MARK).unwrap_or(text);
    let mut e = Obj::new();
    e.insert("at".into(), json!(at));
    e.insert("event".into(), json!("news"));
    e.insert("kind".into(), json!(kind));
    e.insert("text".into(), json!(clip(text, 160)));
    Some((id.to_string(), e))
}

#[cfg(test)]
#[path = "test_old.rs"]
mod tests;
