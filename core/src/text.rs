//! Text as Python measures and cuts it: task_line's width, fit and wrap (textwrap),
//! and str's strip, split and splitlines, whose whitespace and line breaks are
//! Python's, not Rust's.

use crate::widths::{COMBINING, WIDE};

fn in_table(table: &[(u32, u32)], c: char) -> bool {
    let c = c as u32;
    table
        .binary_search_by(|&(a, b)| {
            if b < c {
                std::cmp::Ordering::Less
            } else if a > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// task_line.text_width of one character: 2 East Asian Wide or Full, 0 combining, else 1.
pub fn char_width(c: char) -> usize {
    if in_table(&WIDE, c) {
        2
    } else if in_table(&COMBINING, c) {
        0
    } else {
        1
    }
}

pub fn width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// Python's len(): code points.
pub fn len(text: &str) -> usize {
    text.chars().count()
}

/// Cut text to w columns, with an ellipsis when it doesn't fit.
pub fn fit(text: &str, w: i64) -> String {
    if width(text) as i64 <= w {
        return text.into();
    }
    let mut out = String::new();
    let mut used = 0i64;
    for c in text.chars() {
        let cw = char_width(c) as i64;
        if used + cw > w - 1 {
            break;
        }
        out.push(c);
        used += cw;
    }
    if w > 0 {
        out.push('…');
        out
    } else {
        String::new()
    }
}

/// `f"{text:<w}"`: padded with spaces to w code points.
pub fn ljust(text: &str, w: usize) -> String {
    format!("{text}{}", " ".repeat(w.saturating_sub(len(text))))
}

/// `f"{text:>w}"`.
pub fn rjust(text: &str, w: usize) -> String {
    format!("{}{text}", " ".repeat(w.saturating_sub(len(text))))
}

/// str.isspace() of one character.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

pub fn lstrip(s: &str) -> &str {
    s.trim_start_matches(is_space)
}

pub fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// str.split() with no separator: the words between runs of whitespace.
pub fn split(s: &str) -> Vec<&str> {
    s.split(is_space).filter(|w| !w.is_empty()).collect()
}

/// `" ".join(s.split())`.
pub fn squash(s: &str) -> String {
    split(s).join(" ")
}

/// str.splitlines(): lines without their ends, at Python's line breaks.
pub fn splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if matches!(
            c,
            '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}'
        ) {
            out.push(&s[start..i]);
            let mut end = i + c.len_utf8();
            if c == '\r' && it.peek().is_some_and(|&(_, n)| n == '\n') {
                it.next();
                end += 1;
            }
            start = end;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// textwrap's chunks: words and runs of whitespace, after tabs are expanded (to 8
/// columns) and each other ASCII whitespace becomes a space.
fn chunks(text: &str) -> Vec<Vec<char>> {
    let mut munged = Vec::new();
    let mut col = 0;
    for c in text.chars() {
        match c {
            '\t' => {
                let n = 8 - col % 8;
                munged.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            '\n' | '\r' => {
                munged.push(' ');
                col = 0;
            }
            '\x0b' | '\x0c' => {
                munged.push(' ');
                col += 1;
            }
            c => {
                munged.push(c);
                col += 1;
            }
        }
    }
    let mut out: Vec<Vec<char>> = Vec::new();
    for c in munged {
        let space = c == ' ';
        match out.last_mut() {
            Some(last) if (last[0] == ' ') == space => last.push(c),
            _ => out.push(vec![c]),
        }
    }
    out
}

fn blank(chunk: &[char]) -> bool {
    chunk.iter().all(|&c| is_space(c))
}

/// textwrap.wrap(text, width, break_on_hyphens=False): lines of at most width code
/// points (code points, as Python counts them), long words broken.
pub fn textwrap(text: &str, width: usize) -> Vec<String> {
    let mut chunks = chunks(text);
    chunks.reverse();
    let mut lines: Vec<String> = Vec::new();
    while !chunks.is_empty() {
        let mut cur: Vec<Vec<char>> = Vec::new();
        let mut cur_len = 0;
        if !lines.is_empty() && blank(chunks.last().unwrap()) {
            chunks.pop();
        }
        while let Some(c) = chunks.last() {
            if cur_len + c.len() <= width {
                cur_len += c.len();
                cur.push(chunks.pop().unwrap());
            } else {
                break;
            }
        }
        if chunks.last().is_some_and(|c| c.len() > width) {
            let left = if width < 1 { 1 } else { width - cur_len };
            if left > 0 {
                let chunk = chunks.last_mut().unwrap();
                let tail = chunk.split_off(left);
                cur.push(std::mem::replace(chunk, tail));
            } else if cur.is_empty() {
                cur.push(chunks.pop().unwrap());
            }
        }
        if cur.last().is_some_and(|c| blank(c)) {
            cur.pop();
        }
        if !cur.is_empty() {
            lines.push(cur.concat().into_iter().collect());
        }
    }
    lines
}

/// task_line.wrap: text in at most `lines` lines of w columns, the last ending in
/// " …" when cut.
pub fn wrap(text: &str, w: i64, lines: usize) -> Vec<String> {
    let mut out = textwrap(text, w.max(1) as usize);
    if out.is_empty() {
        out.push(String::new());
    }
    if out.len() > lines {
        out.truncate(lines);
        let last = out.pop().unwrap();
        out.push(fit(&format!("{last} …"), w));
    }
    out
}

/// Capitalize the first character (Python's `s[0].upper() + s[1:]`).
pub fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "test_text.rs"]
mod tests;
