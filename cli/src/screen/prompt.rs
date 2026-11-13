//! The prompt Claude Code's screen ends with: its
//! question, its options and an ID for it. A numbered menu with its ❯ on an option is
//! answered by its number; a "(y/n)" question by y or n. The Python's regexes are matched
//! by hand (`\d` as an ASCII digit).
//!
//! Claude Code's menus: "❯ 1. Yes", "  2. No" (the ❯ marks the chosen one). Its old boxed
//! layout puts a │ at each edge.

use bullpen_core::text::{is_space, lstrip, rstrip, splitlines, strip};

/// Lines a menu can have below it: "Esc to cancel · Tab to amend", a rule.
const HINT_LINES: usize = 3;
/// At most this many lines above a question are its prompt's: the command it asks about.
const PROMPT_CONTEXT: usize = 8;

/// A prompt on the screen: its question, its options as (key, label), and its ID.
#[derive(Debug, PartialEq)]
pub struct Prompt {
    pub question: String,
    pub options: Vec<(String, String)>,
    pub id: String,
}

/// The line without a box's │ at either edge (`re.sub(r"^\s*│|│\s*$", "", line).rstrip()`).
pub fn unboxed(line: &str) -> String {
    let c: Vec<char> = line.chars().collect();
    let lead = c.iter().take_while(|&&ch| is_space(ch)).count();
    let start = if c.get(lead) == Some(&'│') { lead + 1 } else { 0 };
    let end = (start..c.len())
        .find(|&i| c[i] == '│' && c[i + 1..].iter().all(|&ch| is_space(ch)))
        .unwrap_or(c.len());
    let kept: String = c[start..end].iter().collect();
    rstrip(&kept).to_string()
}

/// RULE_RE.match: `^\s*[╭╰]?[─━╌┄-]{3,}[╮╯]?\s*$`.
pub fn is_rule(line: &str) -> bool {
    edge(line, &['╭', '╰'], &['─', '━', '╌', '┄', '-'], &['╮', '╯'])
}

/// TOP_RULE_RE.match: `^\s*╭?─{3,}╮?\s*$`, a prompt's top edge (a dashed ╌ rule is inside it).
pub fn is_top_rule(line: &str) -> bool {
    edge(line, &['╭'], &['─'], &['╮'])
}

/// `^\s*[open]?[dash]{3,}[close]?\s*$`.
fn edge(line: &str, open: &[char], dash: &[char], close: &[char]) -> bool {
    let mut it = lstrip(line).chars().peekable();
    it.next_if(|c| open.contains(c));
    let mut n = 0;
    while it.next_if(|c| dash.contains(c)).is_some() {
        n += 1;
    }
    it.next_if(|c| close.contains(c));
    n >= 3 && it.all(is_space)
}

/// YES_NO_RE.search: `[(\[]\s*y\s*/\s*n\s*[)\]]`, ignoring case.
pub fn is_yes_no(line: &str) -> bool {
    let c: Vec<char> = line.chars().collect();
    (0..c.len()).any(|i| {
        if !matches!(c[i], '(' | '[') {
            return false;
        }
        let mut j = i + 1;
        for want in ['y', '/', 'n'] {
            while j < c.len() && is_space(c[j]) {
                j += 1;
            }
            if j >= c.len() || c[j].to_ascii_lowercase() != want {
                return false;
            }
            j += 1;
        }
        while j < c.len() && is_space(c[j]) {
            j += 1;
        }
        j < c.len() && matches!(c[j], ')' | ']')
    })
}

/// OPTION_RE.match: `^(\s*)(❯|›|>)?\s*(\d{1,2})\.\s+(\S.*)$`, as (indent, marked, number,
/// label): the indent's length, whether it has a marker, its number and the rest.
pub fn option(line: &str) -> Option<(usize, bool, String, String)> {
    let c: Vec<char> = line.chars().collect();
    let indent = c.iter().take_while(|&&ch| is_space(ch)).count();
    let mut i = indent;
    let marked = matches!(c.get(i), Some('❯' | '›' | '>'));
    if marked {
        i += 1;
    }
    while i < c.len() && is_space(c[i]) {
        i += 1;
    }
    let digits = c[i..].iter().take_while(|ch| ch.is_ascii_digit()).count();
    if !(1..=2).contains(&digits) || c.get(i + digits) != Some(&'.') {
        return None;
    }
    let number: String = c[i..i + digits].iter().collect();
    let mut j = i + digits + 1;
    let gap = c[j..].iter().take_while(|&&ch| is_space(ch)).count();
    j += gap;
    if gap == 0 || j >= c.len() {
        return None;
    }
    Some((indent, marked, number, c[j..].iter().collect()))
}

/// A short ID for a prompt from its lines: the same prompt on screen again, not just
/// the same question ("Do you want to proceed?" asks about every command).
pub fn prompt_id<S: AsRef<str>>(lines: &[S]) -> String {
    let text: Vec<&str> = lines
        .iter()
        .map(AsRef::as_ref)
        .filter(|l| !strip(l).is_empty() && !is_rule(l))
        .map(strip)
        .collect();
    bullpen_core::sha1::hex(text.join("\n").as_bytes())[..12].to_string()
}

/// Where the prompt whose question is line i starts: below its top edge.
pub fn prompt_top<S: AsRef<str>>(lines: &[S], i: usize) -> usize {
    let top = i.saturating_sub(PROMPT_CONTEXT);
    (top..i)
        .rev()
        .find(|&j| is_top_rule(lines[j].as_ref()))
        .map_or(top, |j| j + 1)
}

/// The question Claude Code's screen ends with, or None when it doesn't end with one. A
/// menu's "Type something." is left out: it wants typing in the pane. Its id: prompt_id
/// of the question, what's above it and its options.
pub fn read_prompt(text: &str) -> Option<Prompt> {
    let mut lines: Vec<String> = splitlines(text).into_iter().map(unboxed).collect();
    while lines.last().is_some_and(|l| strip(l).is_empty()) {
        lines.pop();
    }
    let filled: Vec<usize> = (0..lines.len())
        .filter(|&i| !strip(&lines[i]).is_empty() && !is_rule(&lines[i]))
        .collect();
    for &i in filled[filled.len().saturating_sub(HINT_LINES)..].iter().rev() {
        if is_yes_no(&lines[i]) {
            let options = vec![("y".into(), "Yes".into()), ("n".into(), "No".into())];
            let id = prompt_id(&lines[prompt_top(&lines, i)..=i]);
            return Some(Prompt {
                question: strip(&lines[i]).into(),
                options,
                id,
            });
        }
    }
    // the last option, with at most a few hint lines under it, and no input box
    let mut last = None;
    for (k, &i) in filled.iter().enumerate().rev() {
        if option(&lines[i]).is_some() {
            last = Some(i);
            break;
        }
        if lstrip(&lines[i]).starts_with(['❯', '›']) {
            return None; // the agent's own input box: it isn't asking
        }
        if filled.len() - k > HINT_LINES {
            return None;
        }
    }
    let last = last?;
    let (mut options, mut marked, mut question, mut top) = (Vec::new(), false, None, 0);
    let mut indent = option(&lines[last]).map_or(0, |o| o.0);
    for i in (0..=last).rev() {
        let line = &lines[i];
        if let Some((ind, mark, number, label)) = option(line) {
            options.push((number, strip(&label).to_string()));
            marked = marked || mark;
            indent = indent.min(ind);
        } else if strip(line).is_empty()
            || is_rule(line)
            || line.chars().count() - lstrip(line).chars().count() > indent + 1
        {
            continue; // a gap, a rule, or the option above's description or wrapped label
        } else {
            question = Some(strip(line).to_string());
            top = prompt_top(&lines, i);
            break;
        }
    }
    options.reverse();
    if !marked || options.iter().enumerate().any(|(n, (k, _))| *k != (n + 1).to_string()) {
        return None; // a numbered list in what the agent wrote, not a menu
    }
    options.retain(|(_, label)| !label.starts_with("Type something"));
    let id = prompt_id(&lines[top..=last]);
    Some(Prompt {
        question: question.unwrap_or_default(),
        options,
        id,
    })
}

#[cfg(test)]
#[path = "test_prompt.rs"]
mod tests;
