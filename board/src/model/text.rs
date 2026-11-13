//! Fitting text to columns: width, fit, pad, wrap. The width is bullpen-core's, which
//! `ls` cuts by too.

use bullpen_core::text::char_width;
pub use bullpen_core::text::width;

/// The first of the texts that fits the width, else the last one cut.
pub fn first_fit(texts: &[String], w: usize) -> String {
    match texts.iter().find(|t| width(t) <= w) {
        Some(t) => t.clone(),
        None => fit(texts.last().map(String::as_str).unwrap_or(""), w),
    }
}

/// Cut text to width, with an ellipsis when it doesn't fit.
pub fn fit(text: &str, w: usize) -> String {
    bullpen_core::text::fit(text, w as i64)
}

/// Pad text with spaces to width columns.
pub fn pad(text: &str, w: usize) -> String {
    format!("{text}{}", " ".repeat(w.saturating_sub(width(text))))
}

/// Wrap text into at most `lines` lines of width columns, ending in " …" when cut.
pub fn wrap(text: &str, w: usize, lines: usize) -> Vec<String> {
    let w = w.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        // a word longer than a line is broken across lines
        while width(&word) > w {
            let room = if cur.is_empty() { w } else { w.saturating_sub(width(&cur) + 1) };
            if room == 0 {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            let (head, tail) = split_at_width(&word, room);
            if cur.is_empty() {
                cur = head;
            } else {
                cur = format!("{cur} {head}");
            }
            out.push(std::mem::take(&mut cur));
            word = tail;
        }
        if cur.is_empty() {
            cur = word;
        } else if width(&cur) + 1 + width(&word) <= w {
            cur = format!("{cur} {word}");
        } else {
            out.push(std::mem::replace(&mut cur, word));
        }
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    if out.len() > lines.max(1) {
        out.truncate(lines.max(1));
        let last = out.last_mut().unwrap();
        *last = if width(last) + 2 > w { fit(&format!("{last} …"), w) } else { format!("{last} …") };
    }
    out
}

pub fn split_at_width(s: &str, w: usize) -> (String, String) {
    let mut used = 0;
    for (i, c) in s.char_indices() {
        let cw = char_width(c);
        if used + cw > w {
            return (s[..i].into(), s[i..].into());
        }
        used += cw;
    }
    (s.into(), String::new())
}

#[cfg(test)]
#[path = "test_text.rs"]
mod tests;
