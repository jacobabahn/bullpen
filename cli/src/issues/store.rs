//! `issues.json`, beside `backlog.json`: every issue bullpen has read, by number, so
//! none is asked about twice. A record: `{"item": "L11", "decided": "yes" | "no" | null,
//! "at": <when it was read, then when it was decided>, "updated": <the issue's updatedAt
//! as last read>}`, and `closed` (when bullpen saw it gone from the open ones).

use bullpen_core::json::{self, s, Obj};
use bullpen_core::timeutil::iso_now;
use bullpen_core::{lock, Repo, Res};
use serde_json::{json, Value};
use std::path::PathBuf;

pub fn path(repo: &Repo) -> PathBuf {
    repo.dir.join("issues.json")
}

/// The records as they are now, by issue number (none when nothing was read yet).
pub fn load(repo: &Repo) -> Res<Obj> {
    Ok(json::read_obj(&path(repo))?.unwrap_or_default())
}

/// Load the records under their lock (`issues.lock`), change them with f and write them
/// back; f's answer. Nothing is written when f fails.
pub fn change<T>(repo: &Repo, f: impl FnOnce(&mut Obj) -> Res<T>) -> Res<T> {
    std::fs::create_dir_all(&repo.dir)?;
    let _held = lock::lock(&repo.dir.join("issues.lock"))?;
    let mut seen = load(repo)?;
    let out = f(&mut seen)?;
    json::write_obj(&path(repo), &seen)?;
    Ok(out)
}

/// A record for an issue just read: undecided.
pub fn record(item: &str, updated: &str) -> Value {
    json!({"item": item, "decided": null, "at": iso_now(), "updated": updated})
}

/// What the person decided about an issue, as its item's state says it: on the list or
/// a task is a yes, dropped is a no. None while it asks (or is done: a closed issue).
pub fn decision_of(item: &Obj) -> Option<&'static str> {
    match s(item, "state") {
        "kept" | "started" => Some("yes"),
        "dropped" => Some("no"),
        _ => None,
    }
}

/// Note the item's decision on its record, when it has one the record doesn't say yet.
/// Whether the record is decided now.
pub fn settle(rec: &mut Obj, item: &Obj) -> bool {
    if let Some(d) = decision_of(item).filter(|d| s(rec, "decided") != *d) {
        rec.insert("decided".into(), json!(d));
        rec.insert("at".into(), json!(iso_now()));
    }
    !s(rec, "decided").is_empty()
}

/// The record's item ID.
pub fn item_of(rec: &Value) -> &str {
    rec.get("item").and_then(Value::as_str).unwrap_or("")
}
