//! What an agent's screen says (bullpen/events.py): a background shell still running,
//! and compacting. Its two regexes are matched by hand (no regex crate for two).

use crate::core::{is_word, Res};
use crate::rex::screen;
use crate::text::{is_space, splitlines, strip};

const COMPACTING: [&str; 3] = ["Compacting conversation", "Running PreCompact hooks", "Running Pre-Compact hooks"];

/// Does c have word (lowercase ASCII) at i, ignoring case?
fn word_at(c: &[char], i: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    i + w.len() <= c.len() && c[i..i + w.len()].iter().zip(&w).all(|(a, b)| a.to_ascii_lowercase() == *b)
}

/// Is there a word boundary just before i (the word before it having ended)?
fn ends_word(c: &[char], i: usize) -> bool {
    i >= c.len() || !is_word(c[i])
}

/// `shells?\b` at j, ignoring case.
fn shells_at(c: &[char], j: usize) -> bool {
    if !word_at(c, j, "shell") {
        return false;
    }
    let k = j + 5;
    (k < c.len() && c[k].eq_ignore_ascii_case(&'s') && ends_word(c, k + 1)) || ends_word(c, k)
}

/// SHELL_RE.search: `\b\d+ (?:background )?shells?\b|shells? still running`, ignoring case.
fn shell_re_search(line: &str) -> bool {
    let c: Vec<char> = line.chars().collect();
    for i in 0..c.len() {
        if word_at(&c, i, "shell still running") || word_at(&c, i, "shells still running") {
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
        if (word_at(&c, j, "background ") && shells_at(&c, j + 11)) || shells_at(&c, j) {
            return true;
        }
    }
    false
}

/// Does Claude Code's footer say a background shell of its own still runs?
pub fn shell_running(text: &str) -> bool {
    let lines = splitlines(text);
    lines[lines.len().saturating_sub(6)..].iter().any(|l| shell_re_search(l))
}

/// COMPACT_RE.match: `^\W{0,2}\s*(?:Compacting conversation|Running Pre-?Compact hooks)`.
fn compact_re_match(line: &str) -> bool {
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
    let lines: Vec<&str> = splitlines(text).into_iter().map(strip).filter(|l| !l.is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

/// Is Claude Code compacting its conversation? By its status message, else its spinner line.
pub fn compacting(session: &str, block: &str, msg: &str) -> Res<bool> {
    if COMPACTING.iter().any(|w| msg.starts_with(w)) {
        return Ok(true);
    }
    Ok(stripped_lines(&screen(session, block)?, 12).iter().any(|l| compact_re_match(l)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shells_as_the_regex_finds_them() {
        for yes in ["  2 background shells · ↓ to manage", "1 shell still running", "3 Background Shells", "x 1 shell"] {
            assert!(shell_re_search(yes), "{yes}");
        }
        for no in ["a1 shell", "2 shellfish", "12shells", "no shells here"] {
            assert!(!shell_re_search(no), "{no}");
        }
    }

    #[test]
    fn compacting_as_the_regex_matches_it() {
        assert!(compact_re_match("✽ Compacting conversation… (esc to interrupt)"));
        assert!(compact_re_match("Running Pre-Compact hooks"));
        assert!(compact_re_match("·· \tRunning PreCompact hooks"));
        assert!(!compact_re_match("a Compacting conversation"));
        assert!(!compact_re_match("··· Compacting conversation"));
    }
}
