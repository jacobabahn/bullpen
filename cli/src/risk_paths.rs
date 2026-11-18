//! A repository's `risk_paths`: its sensitive areas, each with a level (high or medium)
//! and why (docs/review.md). risk.rs labels a change by the areas it touches;
//! `config --risk-path` adds or changes one and `--remove-risk-path` drops one, so
//! nobody edits config.json by hand; doctor says in one line when none are set.
//!
//! In config.json: `{"pattern": {"level": "medium", "why": "changes how review runs"}}`.
//! The older forms still read, as high: `{"pattern": "why"}` and a list of patterns.
//!
//! Each form read into a level and a why: `entries`; the area a path is in: `area_of`,
//! `path_matches`; `config`'s flags: `changed`; doctor's `risk` line: `check_risk`.

use crate::args::Args;
use bullpen_core::json::get;
use bullpen_core::{fail, Obj, Repo, Res};
use serde_json::{json, Value};

/// One sensitive area.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub pattern: String,
    pub level: &'static str,
    pub why: String,
}

fn entry(pattern: &str, level: Option<&str>, why: Option<&str>) -> Entry {
    Entry {
        pattern: pattern.into(),
        // a level nobody knows is read as the careful one
        level: if level == Some("medium") { "medium" } else { "high" },
        why: match why.map(str::trim) {
            Some(w) if !w.is_empty() => w.into(),
            _ => format!("changes {pattern}"),
        },
    }
}

/// The areas a `risk_paths` value names, in its order.
pub fn entries(risk_paths: Option<&Value>) -> Vec<Entry> {
    match risk_paths {
        Some(Value::Object(o)) => o
            .iter()
            .map(|(p, v)| match v {
                Value::Object(e) => entry(
                    p,
                    e.get("level").and_then(Value::as_str),
                    e.get("why").and_then(Value::as_str),
                ),
                other => entry(p, None, other.as_str()),
            })
            .collect(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(|p| entry(p, None, None))
            .collect(),
        _ => Vec::new(),
    }
}

/// The area a path is in: of those whose pattern matches, the most sensitive, then the
/// first. Its place in `entries`.
pub fn area_of(entries: &[Entry], path: &str) -> Option<usize> {
    let hit = |level: &str| {
        entries
            .iter()
            .position(|e| e.level == level && path_matches(&e.pattern, path))
    };
    hit("high").or_else(|| hit("medium"))
}

/// A risk_paths pattern: a folder (ending in `/`, or a plain name that is a folder of
/// the path), a file, or a glob where `*` is any run of characters.
pub fn path_matches(pattern: &str, path: &str) -> bool {
    if pattern.contains('*') {
        return glob(pattern.as_bytes(), path.as_bytes());
    }
    if let Some(dir) = pattern.strip_suffix('/') {
        return path.starts_with(&format!("{dir}/"));
    }
    path == pattern || path.starts_with(&format!("{pattern}/"))
}

fn glob(pat: &[u8], text: &[u8]) -> bool {
    match pat.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|i| glob(rest, &text[i..])),
        Some((c, rest)) => text.first() == Some(c) && glob(rest, &text[1..]),
    }
}

// ---------- config's flags ----------

pub const ADD: &str = "--risk-path";
pub const LEVEL: &str = "--level";
pub const WHY: &str = "--why";
pub const REMOVE: &str = "--remove-risk-path";
pub const LEVELS: &[&str] = &["high", "medium"];

/// Whether config was given any of the risk paths' flags.
pub fn asked(a: &Args) -> bool {
    [ADD, LEVEL, WHY, REMOVE].iter().any(|f| a.opt(f).is_some())
}

/// The flags given, for `config --global`'s refusal.
pub fn flags_given(a: &Args) -> Vec<String> {
    [ADD, LEVEL, WHY, REMOVE]
        .iter()
        .filter(|f| a.opt(f).is_some())
        .map(|f| f.to_string())
        .collect()
}

/// The repo's risk_paths with the changes config's flags ask for, in the form with a
/// level each; None when they ask for none. Entries it doesn't name keep their place.
pub fn changed(repo: &Repo, a: &Args) -> Res<Option<Value>> {
    if !asked(a) {
        return Ok(None);
    }
    let add = a.all(ADD);
    if add.len() > 1 {
        return fail(format!("{ADD} takes one pattern a run, with its {LEVEL} and {WHY}"));
    }
    let Some(&pattern) = add.first() else {
        if a.opt(LEVEL).is_some() || a.opt(WHY).is_some() {
            return fail(format!("{LEVEL} and {WHY} go with {ADD} PATTERN"));
        }
        return removed(repo, &a.all(REMOVE)).map(Some);
    };
    // git's paths are relative to the repository's top, with no ./ before them
    let mut pattern = pattern.trim();
    while let Some(rest) = pattern.strip_prefix("./") {
        pattern = rest;
    }
    if pattern.is_empty() || pattern == "." || pattern.starts_with('/') || pattern.starts_with("../") {
        return fail(format!(
            "{ADD} takes a path in the repository, from its top: a file, a folder/ or a glob with *"
        ));
    }
    let mut paths = as_obj(&removed(repo, &a.all(REMOVE))?);
    let level = a.opt(LEVEL).unwrap_or("high");
    let why = entry(pattern, Some(level), a.opt(WHY)).why;
    paths.insert(pattern.into(), json!({"level": level, "why": why}));
    Ok(Some(Value::Object(paths)))
}

/// The repo's risk_paths, each with its level, without the patterns named.
fn removed(repo: &Repo, patterns: &[&str]) -> Res<Value> {
    let mut paths: Obj = entries(get(&repo.cfg, "risk_paths"))
        .into_iter()
        .map(|e| (e.pattern, json!({"level": e.level, "why": e.why})))
        .collect();
    for p in patterns {
        if paths.shift_remove(p.trim()).is_none() {
            let has: Vec<&str> = paths.keys().map(String::as_str).collect();
            return fail(match has.is_empty() {
                true => format!("no risk path {p}: this repository has none"),
                false => format!("no risk path {p}: this repository has {}", has.join(", ")),
            });
        }
    }
    Ok(Value::Object(paths))
}

fn as_obj(v: &Value) -> Obj {
    v.as_object().cloned().unwrap_or_default()
}

// ---------- doctor's line ----------

/// doctor's `risk` line: how many areas the repo has, or that it has none and what the
/// label goes by then. Never a problem.
pub fn check_risk(repo: Option<&Repo>, why_not: Option<&'static str>) -> Res<(&'static str, String)> {
    let Some(repo) = repo else {
        return Ok(("skip", why_not.unwrap_or("not in a git repository").into()));
    };
    let all = entries(get(&repo.cfg, "risk_paths"));
    if all.is_empty() {
        return Ok((
            "ok",
            "no risk paths set, so a ready task's risk goes by its size alone: name the sensitive ones with \
             `bullpen config --risk-path PATTERN --level high|medium --why \"...\"`"
                .into(),
        ));
    }
    let high = all.iter().filter(|e| e.level == "high").count();
    let s = if all.len() == 1 { "" } else { "s" };
    Ok((
        "ok",
        format!("{} risk path{s} ({high} high, {} medium)", all.len(), all.len() - high),
    ))
}

#[cfg(test)]
#[path = "tests/test_risk_paths.rs"]
mod tests;
