//! A risk label on a task that is ready (`risk`, read by core's risk.rs): low, medium
//! or high, with why. It labels; it changes nothing about what auto-accepts.
//!
//! Two parts, the higher wins (a tie says the reviewer's why):
//!
//! - **rules**, bullpen's own, from the diff, which the reviewer can't lower: a path in
//!   the repo's `risk_paths` or a new dependency in a `Cargo.toml` is high; docs or
//!   tests alone are low; a prompt is medium; other code goes by its size. They read
//!   the repo's settings, never the branch.
//! - **review**, the reviewer's judgement with its verdict (a `## Risk` part in its
//!   findings: `medium: ...`), from the round that approved it. A round that gives none,
//!   and a rebase check, keep the last one's. A light review that was set for a change
//!   that calls for a full one (by its size, or a prompt or the rules) is at least medium.
//!
//! It's worked out at every step that makes a task ready with a change, so it follows
//! the code: a kept approval (only the base moved) works the rules out again, which
//! say the same for the same lines, and keeps the reviewer's part.

use crate::review::kind_of_change;
use bullpen_core::json::{get, obj, s, Obj};
use bullpen_core::proc::{git, head};
use bullpen_core::risk::rank;
use bullpen_core::text::splitlines;
use bullpen_core::{Repo, Res};
use serde_json::{json, Value};

/// More changed lines of code than this is a large change: high.
pub const LARGE_LINES: i64 = 400;

/// What the reviewer said at the step that made the task ready.
pub enum Said<'a> {
    /// an approval, with its findings file's text
    Approval(&'a str),
    /// an approval kept (the code is as approved): the reviewer's part stays
    Kept,
    /// no approval (review off, or the rounds ran out): rules alone
    Nothing,
}

/// A level and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub level: &'static str,
    pub why: String,
}

fn part(level: &'static str, why: impl Into<String>) -> Part {
    Part { level, why: why.into() }
}

impl Part {
    fn to_obj(&self) -> Value {
        json!({"level": self.level, "why": self.why})
    }
}

/// Put the task's label on it, for its code as it is now.
pub fn assess(repo: &Repo, t: &mut Obj, base_ref: &str, said: Said) -> Res<()> {
    let wt = s(t, "worktree").to_string();
    if wt.is_empty() || !std::path::Path::new(&wt).is_dir() {
        t.insert("risk".into(), Value::Null);
        return Ok(());
    }
    let range = format!("{base_ref}...HEAD");
    let numstat = git(&wt, &["diff", "--numstat", "--no-renames", &range], true)?.stdout;
    let manifests = git(
        &wt,
        &["diff", "-U100000", "--no-renames", &range, "--", ":(glob)**/Cargo.toml"],
        true,
    )?
    .stdout;
    let small = repo.cfg.get("light_review_lines").and_then(Value::as_i64).unwrap_or(0);
    let rules = rules_of(
        &numstat,
        adds_dependency(&manifests),
        get(&repo.cfg, "risk_paths"),
        small,
    );
    let r = obj(t, "review");
    let before = obj(t, "risk").get("review").and_then(Value::as_object).cloned();
    let review = match said {
        Said::Nothing => None,
        Said::Kept => before,
        Said::Approval(_) if s(r, "kind") == "rebase" => before,
        Said::Approval(text) => match reviewer_of(text) {
            Some(p) => {
                let mut o = p.to_obj().as_object().cloned().unwrap_or_default();
                o.insert("round".into(), r.get("round").cloned().unwrap_or(Value::Null));
                Some(o)
            }
            None => before,
        },
    };
    let floor = match said {
        Said::Nothing => None,
        _ => light_floor(r, &numstat, small),
    };
    let mut out = Obj::new();
    let review_part = review.as_ref().and_then(part_of);
    let top = higher(&rules, review_part.as_ref(), floor.as_ref());
    out.insert("level".into(), json!(top.level));
    out.insert("why".into(), json!(top.why));
    out.insert("rules".into(), rules.to_obj());
    out.insert("review".into(), review.map_or(Value::Null, Value::Object));
    if let Some(f) = &floor {
        out.insert("floor".into(), f.to_obj());
    }
    out.insert("commit".into(), json!(head(&wt)?));
    t.insert("risk".into(), Value::Object(out));
    Ok(())
}

/// The highest of the parts; on a tie the reviewer's, then the floor's, then the rules'.
pub fn higher(rules: &Part, review: Option<&Part>, floor: Option<&Part>) -> Part {
    let mut top = rules.clone();
    for p in [floor, review].into_iter().flatten() {
        if rank(p.level) >= rank(top.level) {
            top = p.clone();
        }
    }
    top
}

fn part_of(o: &Obj) -> Option<Part> {
    let level = level_word(s(o, "level"))?;
    Some(part(level, s(o, "why")))
}

/// A light review set for the task (`--review light`) of a change that calls for a full
/// one, by its size or because it changes a prompt or the rules: it looked at less than the change needs, so the label is at least medium.
fn light_floor(r: &Obj, numstat: &str, small: i64) -> Option<Part> {
    let set = r
        .get("why")
        .and_then(Value::as_str)
        .is_some_and(|w| w.starts_with("set for"));
    if s(r, "kind") != "light" || !set {
        return None;
    }
    let (kind, why) = kind_of_change(numstat, small);
    (kind == "full").then(|| match why {
        Some(why) => part("medium", &format!("a light review, though it {why}")),
        None => part("medium", "a light review where its size calls for a full one"),
    })
}

// ---------- the rules ----------

/// What a changed path is, for the rules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Area {
    Docs,
    Tests,
    Prompt,
    Code,
}

/// Docs (under `docs/`, or a `.md` outside `prompts/`), tests (under `tests/`, or a
/// `test_` or `_test.` file), a prompt (under `prompts/`), or code.
pub fn area(path: &str) -> Area {
    let name = path.rsplit('/').next().unwrap_or(path);
    if path.starts_with("prompts/") {
        Area::Prompt
    } else if path.starts_with("docs/") || path.ends_with(".md") {
        Area::Docs
    } else if path.starts_with("tests/")
        || path.contains("/tests/")
        || name.starts_with("test_")
        || name.contains("_test.")
    {
        Area::Tests
    } else {
        Area::Code
    }
}

/// The rules' part, from `git diff --numstat`'s rows: a sensitive path (risk_paths) or
/// a new dependency is high; docs and tests alone are low; a prompt is at least
/// medium; other code is low when small (at most `small` lines in at most 2 files),
/// high past LARGE_LINES, else medium.
pub fn rules_of(numstat: &str, adds_dep: bool, risk_paths: Option<&Value>, small: i64) -> Part {
    let rows: Vec<(Option<i64>, String)> = splitlines(numstat)
        .into_iter()
        .filter_map(|row| {
            let mut p = row.splitn(3, '\t');
            let (added, deleted, path) = (p.next()?, p.next()?, p.next()?);
            let lines = added
                .parse::<i64>()
                .ok()
                .zip(deleted.parse::<i64>().ok())
                .map(|(a, d)| a + d);
            Some((lines, path.to_string()))
        })
        .collect();
    let sensitive: Vec<String> = rows.iter().filter_map(|(_, p)| sensitive_why(risk_paths, p)).collect();
    if let Some(first) = sensitive.first() {
        let others = sensitive.iter().filter(|w| *w != first).count();
        return match others {
            0 => part("high", first.clone()),
            n => part("high", format!("{first}, and {n} more sensitive path{}", plural(n))),
        };
    }
    if adds_dep {
        return part("high", "adds a dependency");
    }
    if rows.is_empty() {
        return part("low", "no change");
    }
    let areas: Vec<Area> = rows.iter().map(|(_, p)| area(p)).collect();
    let (docs, tests) = (areas.contains(&Area::Docs), areas.contains(&Area::Tests));
    let prompt = areas.contains(&Area::Prompt);
    let code: Vec<&(Option<i64>, String)> = rows
        .iter()
        .filter(|(_, p)| matches!(area(p), Area::Code | Area::Prompt))
        .collect();
    if code.is_empty() {
        return part(
            "low",
            match (docs, tests) {
                (true, true) => "docs and tests only",
                (true, false) => "docs only",
                _ => "tests only",
            },
        );
    }
    if !areas.contains(&Area::Code) {
        return part("medium", "changes a prompt");
    }
    let files = code.len() as i64;
    let lines: Option<i64> = code.iter().map(|(n, _)| *n).sum();
    let Some(n) = lines else {
        return part("medium", "changes a binary file");
    };
    let size = format!(
        "{n} line{} in {files} file{}",
        plural(n as usize),
        plural(files as usize)
    );
    let by_size = if n > LARGE_LINES {
        part("high", format!("a large change ({size})"))
    } else if n <= small && files <= 2 {
        part("low", format!("a small change ({size})"))
    } else {
        part("medium", size)
    };
    if prompt && by_size.level == "low" {
        return part("medium", "changes a prompt");
    }
    by_size
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Why a path is sensitive, by the repo's risk_paths ({pattern: why}, or a list of
/// patterns), or None.
pub fn sensitive_why(risk_paths: Option<&Value>, path: &str) -> Option<String> {
    let said = |pat: &str, why: Option<&Value>| match why.and_then(Value::as_str).map(str::trim) {
        Some(w) if !w.is_empty() => w.to_string(),
        _ => format!("changes {pat}"),
    };
    match risk_paths? {
        Value::Object(o) => o
            .iter()
            .find(|(p, _)| path_matches(p, path))
            .map(|(p, w)| said(p, Some(w))),
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .find(|p| path_matches(p, path))
            .map(|p| said(p, None)),
        _ => None,
    }
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

/// Whether a diff of `Cargo.toml` files (with all their lines as context) adds a
/// dependency: a name under a dependencies table (`[dependencies]`,
/// `[dev-dependencies]`, `[workspace.dependencies]`, `[target.'...'.dependencies]`)
/// that the diff doesn't also remove (a changed version is no new dependency), or a
/// new table of one (`[dependencies.serde]`).
pub fn adds_dependency(diff: &str) -> bool {
    let is_deps = |table: &str| {
        table
            .split('.')
            .any(|p| p.trim_matches(['\'', '"']).ends_with("dependencies"))
    };
    let (mut added, mut removed) = (Vec::new(), Vec::new());
    let mut table = String::new();
    let mut in_hunk = false;
    for line in diff.split('\n') {
        if line.starts_with("diff --git ") {
            table.clear();
            in_hunk = false;
            continue;
        }
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }
        if !in_hunk || line.is_empty() {
            continue;
        }
        let (sign, text) = line.split_at(1);
        let text = text.trim();
        if text.starts_with('[') {
            let name = text.trim_matches(|c| c == '[' || c == ']').trim().to_string();
            // [dependencies.serde]: the table is the dependency
            if let Some((t, dep)) = name.rsplit_once('.').filter(|(t, _)| is_deps(t)) {
                let key = format!("{t}:{dep}");
                match sign {
                    "+" => added.push(key),
                    "-" => removed.push(key),
                    _ => {}
                }
            }
            if sign != "-" {
                table = name;
            }
            continue;
        }
        if !is_deps(&table) || text.is_empty() || text.starts_with('#') {
            continue;
        }
        let Some((key, _)) = text.split_once('=') else { continue };
        let key = format!("{table}:{}", key.trim().trim_matches(['\'', '"']));
        match sign {
            "+" => added.push(key),
            "-" => removed.push(key),
            _ => {}
        }
    }
    added.iter().any(|k| !removed.contains(k))
}

// ---------- the reviewer's part ----------

/// The reviewer's level and why from its findings' `## Risk` part: its first line,
/// like `medium: changes when the queue starts a task` (`risk: high · ...`, a list
/// item, or `med` read too). None when it has none or names no level.
pub fn reviewer_of(text: &str) -> Option<Part> {
    let mut lines = text.lines().skip_while(|l| !is_risk_heading(l));
    let heading = lines.next()?;
    // `## Risk: medium ...` on the heading itself
    let on_heading = heading.split_once(':').map(|(_, r)| r.trim()).filter(|r| !r.is_empty());
    let first = match on_heading {
        Some(r) => r.to_string(),
        None => lines
            .take_while(|l| !(l.starts_with("# ") || l.starts_with("## ")))
            .map(str::trim)
            .find(|l| !l.is_empty())?
            .to_string(),
    };
    let first = first.trim_start_matches(['-', '*', ' ']).replace(['`', '*'], "");
    let lower = first.to_lowercase();
    let rest = lower
        .strip_prefix("risk")
        .map_or(lower.as_str(), |r| r.trim_start_matches([':', ' ']));
    let word: String = rest.chars().take_while(char::is_ascii_alphabetic).collect();
    let level = level_word(&word)?;
    let skip = lower.len() - rest.len() + word.len();
    let why = first
        .get(skip..)
        .unwrap_or("")
        .trim_start_matches([':', '·', '-', '—', '–', ',', ' '])
        .trim();
    let why = why.trim_end_matches('.').trim();
    Some(part(level, why))
}

fn level_word(word: &str) -> Option<&'static str> {
    match word.to_lowercase().as_str() {
        "low" => Some("low"),
        "medium" | "med" => Some("medium"),
        "high" => Some("high"),
        _ => None,
    }
}

fn is_risk_heading(line: &str) -> bool {
    let rest = line.strip_prefix("## ").unwrap_or("").trim().to_lowercase();
    rest == "risk" || rest.starts_with("risk:") || rest.starts_with("risk ")
}

#[cfg(test)]
#[path = "test_risk.rs"]
mod tests;
