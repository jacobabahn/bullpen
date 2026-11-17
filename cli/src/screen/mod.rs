//! What Claude Code's screen in a pane says: a background shell of its own still running, compacting, what it is
//! doing (a spinner, a question, its prompt), and the prompt it is at (prompt.rs). The
//! Python's regexes are matched by hand.

pub mod prompt;

use crate::rex::screen;
use bullpen_core::paths::is_word;
use bullpen_core::text::{is_space, splitlines, strip};
use bullpen_core::Res;

const COMPACTING: [&str; 3] = [
    "Compacting conversation",
    "Running PreCompact hooks",
    "Running Pre-Compact hooks",
];

/// Does c have word (lowercase ASCII) at i, ignoring case?
fn word_at(c: &[char], i: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    i + w.len() <= c.len()
        && c[i..i + w.len()]
            .iter()
            .zip(&w)
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

/// Is there a word boundary at i, after a word (nothing, or no word character, there)?
fn ends_word(c: &[char], i: usize) -> bool {
    i >= c.len() || !is_word(c[i])
}

/// What the footer counts that a turn waits on: a background shell, and a Monitor
/// (Claude Code 2.1.326: "1 shell", "2 monitors", "1 monitor still running").
const JOBS: [&str; 2] = ["shell", "monitor"];

/// `<noun>s?\b` at j, ignoring case.
fn noun_at(c: &[char], j: usize, noun: &str) -> bool {
    if !word_at(c, j, noun) {
        return false;
    }
    let k = j + noun.len();
    (k < c.len() && c[k].eq_ignore_ascii_case(&'s') && ends_word(c, k + 1)) || ends_word(c, k)
}

/// SHELL_RE.search, for each of JOBS: `\b\d+ (?:background )?shells?\b|shells? still
/// running`, ignoring case.
pub fn shell_re_search(line: &str) -> bool {
    let c: Vec<char> = line.chars().collect();
    for i in 0..c.len() {
        if JOBS
            .iter()
            .any(|n| word_at(&c, i, &format!("{n} still running")) || word_at(&c, i, &format!("{n}s still running")))
        {
            return true;
        }
        if !c[i].is_ascii_digit() || (i > 0 && is_word(c[i - 1])) {
            continue;
        }
        let mut j = i;
        while j < c.len() && c[j].is_ascii_digit() {
            j += 1;
        }
        if j >= c.len() || c[j] != ' ' {
            continue;
        }
        j += 1;
        let at = |j: usize| JOBS.iter().any(|n| noun_at(&c, j, n));
        if (word_at(&c, j, "background ") && at(j + 11)) || at(j) {
            return true;
        }
    }
    false
}

/// Does Claude Code's footer say a background shell of its own still runs?
pub fn shell_running(text: &str) -> bool {
    let lines = splitlines(text);
    lines[lines.len().saturating_sub(6)..]
        .iter()
        .any(|l| shell_re_search(l))
}

/// COMPACT_RE.match: `^\W{0,2}\s*(?:Compacting conversation|Running Pre-?Compact hooks)`.
pub fn compact_re_match(line: &str) -> bool {
    let c: Vec<char> = line.chars().collect();
    for k in 0..=2.min(c.len()) {
        if k > 0 && is_word(c[k - 1]) {
            break;
        }
        let rest: String = c[k..].iter().collect();
        let rest = rest.trim_start_matches(is_space);
        if COMPACTING.iter().any(|w| rest.starts_with(w)) {
            return true;
        }
    }
    false
}

/// The screen's last n lines that aren't blank, stripped.
pub fn stripped_lines(text: &str, n: usize) -> Vec<&str> {
    let lines: Vec<&str> = splitlines(text)
        .into_iter()
        .map(strip)
        .filter(|l| !l.is_empty())
        .collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

/// Does the status message, or else the screen, say it is compacting?
pub fn compacting_in(msg: &str, text: &str) -> bool {
    COMPACTING.iter().any(|w| msg.starts_with(w)) || stripped_lines(text, 12).iter().any(|l| compact_re_match(l))
}

/// Is Claude Code compacting its conversation (auto-compaction)? Its status record says
/// only that it's working, so this goes by its message, else its spinner line: "✽
/// Compacting conversation…" (or the PreCompact hooks it runs first).
pub fn compacting(session: &str, block: &str, msg: &str) -> Res<bool> {
    if COMPACTING.iter().any(|w| msg.starts_with(w)) {
        return Ok(true);
    }
    Ok(compacting_in("", &screen(session, block)?))
}

/// What Claude Code's screen shows: working (a spinner), blocked (a question), idle (its
/// prompt), or None if it's none of those.
pub fn screen_state(text: &str) -> Option<&'static str> {
    let lines = stripped_lines(text, 12);
    if lines.iter().any(|l| l.contains("esc to interrupt")) {
        Some("working")
    } else if lines.iter().any(|l| l.starts_with("Do you want to")) {
        Some("blocked")
    } else if lines.iter().any(|l| l.starts_with('❯')) {
        Some("idle")
    } else {
        None
    }
}

#[cfg(test)]
#[path = "test_screen.rs"]
mod tests;
