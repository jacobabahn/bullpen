//! A reviewer's notes: the `## Notes` part of a findings file, the things it saw that
//! aren't wrong in the change itself. They go to the coordinator (the ready and auto-accepted
//! news say how many, `show` prints them), never back to the worker as findings.

use bullpen_core::json::{obj, py_str, truthy, Obj};

/// The body of the `## Notes` part of a findings file (up to the next heading of its
/// level or above), or None when it has none or says only "none".
pub fn notes_of(text: &str) -> Option<String> {
    let mut lines = text.lines().skip_while(|l| !is_notes_heading(l));
    lines.next()?;
    let body: Vec<&str> = lines.take_while(|l| !is_heading(l)).collect();
    let body = body.join("\n").trim().to_string();
    let bare = body.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    if bare.is_empty() || bare == "none" {
        return None;
    }
    Some(body)
}

/// The findings file's text without its `## Notes` part: what a worker, or the next
/// round's reviewer, gets as findings.
pub fn without_notes(text: &str) -> String {
    let mut out = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if is_notes_heading(line) {
            skipping = true;
        } else if is_heading(line) {
            skipping = false;
        }
        if !skipping {
            out.push(line);
        }
    }
    out.join("\n").trim_end().to_string()
}

/// How many notes: one per top-level list item, or else one per paragraph.
pub fn count(notes: &str) -> usize {
    let items = notes.lines().filter(|l| is_item(l)).count();
    if items > 0 {
        return items;
    }
    notes.split("\n\n").filter(|p| !p.trim().is_empty()).count()
}

/// The notes in a task's last findings file, or None.
pub fn review_notes(t: &Obj) -> Option<String> {
    let file = obj(t, "review")
        .get("findings")
        .filter(|v| truthy(Some(v)))
        .map(py_str)?;
    notes_of(&std::fs::read_to_string(file).ok()?)
}

/// " with 2 notes" for a task whose last findings file has notes, else "": for the
/// news that says it was approved.
pub fn with_notes(t: &Obj) -> String {
    match review_notes(t).map(|n| count(&n)) {
        Some(1) => " with 1 note".into(),
        Some(n) => format!(" with {n} notes"),
        None => String::new(),
    }
}

fn is_heading(line: &str) -> bool {
    line.starts_with("# ") || line.starts_with("## ")
}

fn is_notes_heading(line: &str) -> bool {
    let rest = line.strip_prefix("## ").unwrap_or("").trim().to_lowercase();
    rest == "notes" || rest.starts_with("notes ") || rest.starts_with("notes:") || rest.starts_with("notes (")
}

fn is_item(line: &str) -> bool {
    if line.starts_with("- ") || line.starts_with("* ") {
        return true;
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && (line[digits..].starts_with(". ") || line[digits..].starts_with(") "))
}

#[cfg(test)]
#[path = "tests/test_notes.rs"]
mod tests;
