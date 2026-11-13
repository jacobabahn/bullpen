//! `bullpen --help` and the one-line error for a command bullpen doesn't have, exactly as
//! the Python wrote them (cli.Parser.format_help, cli.unknown_command): Python's
//! textwrap (hyphens break too), the terminal's width as shutil reads it, and difflib's
//! close match.

use crate::commands::{Command, Owner, COMMANDS};

/// The help's groups, in order, and the label a command's help drops in its own group
/// (the title already says it).
pub const GROUPS: [(&str, &str); 4] = [
    ("You", "(the person's call) "),
    ("Coordinator", "(coordinator) "),
    ("Worker and reviewer", ""),
    ("Setup", ""),
];
pub const USAGE: &str = "usage: bullpen COMMAND [ARGS]\n";
const ABOUT: &str = "bullpen: a coordinator and its worker agents, each in its own Rex pane.";

/// shutil.get_terminal_size().columns: $COLUMNS, else stdout's terminal, else 80.
pub fn term_width() -> usize {
    if let Some(n) = std::env::var("COLUMNS").ok().and_then(|v| v.trim().parse::<i64>().ok()).filter(|n| *n > 0) {
        return n as usize;
    }
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } == 0;
    if ok && ws.ws_col > 0 {
        ws.ws_col as usize
    } else {
        80
    }
}

/// The commands `--help` lists in a group, in the table's order.
fn listed(group: &str) -> impl Iterator<Item = &'static Command> + '_ {
    COMMANDS.iter().filter(move |c| c.group == Some(group) && !matches!(c.owner, Owner::Alias(_)))
}

/// The top-level help for a terminal `cols` wide.
pub fn top_help(cols: usize) -> String {
    let width = (cols as i64 - 2).max(40) as usize;
    let name_w = GROUPS.iter().flat_map(|(g, _)| listed(g)).map(|c| c.name.chars().count()).max().unwrap_or(0) + 2;
    let mut out = format!("{USAGE}\n{}\n", wrap(ABOUT, width, "", ""));
    for (title, label) in GROUPS {
        out += &format!("\n{title}:\n");
        for c in listed(title) {
            let h = c.help.strip_prefix(label).unwrap_or(c.help);
            let first = format!("  {:<name_w$}", c.name);
            out += &wrap(h, width, &first, &" ".repeat(name_w + 2));
            out += "\n";
        }
    }
    out + "\n" + &wrap("`bullpen COMMAND --help` says more about one.", width, "", "") + "\n"
}

/// The one-line error for a first argument that is no command bullpen has.
pub fn unknown_command(name: &str) -> String {
    if name.starts_with('-') {
        return format!("bullpen: `{name}` goes after a command. `bullpen --help` lists them.");
    }
    let offered = COMMANDS.iter().filter(|c| !c.name.starts_with('_') && !matches!(c.owner, Owner::Alias(_)));
    let hint = close_match(name, offered.map(|c| c.name)).map(|m| format!(" Did you mean `{m}`?")).unwrap_or_default();
    format!("bullpen: no command `{name}`.{hint} `bullpen --help` lists them.")
}

// ---------- textwrap ----------

fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0b' | '\x0c' | '\r' | ' ')
}

/// Python's `\w`.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `[^\d\W]`: a word character that isn't a digit.
fn is_letter(c: char) -> bool {
    is_word(c) && !c.is_numeric()
}

fn is_word_punct(c: char) -> bool {
    is_word(c) || "!\"'&.,?".contains(c)
}

/// At i: two or more '-' and then a word character (`-{2,}\w`)?
fn dashes_then_word(t: &[char], i: usize) -> Option<usize> {
    let n = t[i..].iter().take_while(|&&c| c == '-').count();
    (n >= 2 && t.get(i + n).is_some_and(|&c| is_word(c))).then_some(n)
}

/// textwrap's chunks (TextWrapper.wordsep_re, break_on_hyphens on): runs of whitespace,
/// em-dashes between words, and words, split after a hyphen between letters.
fn chunks(t: &[char]) -> Vec<String> {
    let at = |i: usize| t.get(i).copied();
    let letter = |i: Option<usize>| i.and_then(at).is_some_and(is_letter);
    let mut out = Vec::new();
    let mut p = 0;
    while p < t.len() {
        let end = if is_ws(t[p]) {
            p + t[p..].iter().take_while(|&&c| is_ws(c)).count()
        } else if let Some(n) = (p > 0 && is_word_punct(t[p - 1])).then(|| dashes_then_word(t, p)).flatten() {
            p + n
        } else {
            let mut e = p + 1;
            loop {
                if at(e) == Some('-') {
                    let behind = (letter(e.checked_sub(2)) && letter(e.checked_sub(1)))
                        || (letter(e.checked_sub(3)) && at(e.wrapping_sub(2)) == Some('-') && letter(e.checked_sub(1)));
                    let ahead = letter(Some(e + 1))
                        && (letter(Some(e + 2)) || (at(e + 2) == Some('-') && letter(Some(e + 3))));
                    if e >= 1 && behind && ahead {
                        break e + 1;
                    }
                }
                if e >= t.len() || is_ws(t[e]) {
                    break e;
                }
                if is_word_punct(t[e - 1]) && dashes_then_word(t, e).is_some() {
                    break e;
                }
                e += 1;
            }
        };
        out.push(t[p..end].iter().collect());
        p = end;
    }
    out
}

/// textwrap.wrap(text, width, initial_indent, subsequent_indent): Python's defaults
/// otherwise (tabs to 8 columns, whitespace as spaces, long words broken at a hyphen if
/// they can be). Widths count code points, as Python does.
pub fn wrap_with(text: &str, width: usize, first: &str, rest: &str) -> Vec<String> {
    let mut col = 0;
    let mut munged = Vec::new();
    for c in text.chars() {
        if c == '\t' {
            let n = 8 - col % 8;
            munged.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else {
            munged.push(if is_ws(c) { ' ' } else { c });
            col = if c == '\n' || c == '\r' { 0 } else { col + 1 };
        }
    }
    let len = |s: &str| s.chars().count();
    let blank = |s: &str| s.chars().all(|c| c == ' ');
    let mut chunks = chunks(&munged);
    chunks.reverse();
    let mut lines = Vec::new();
    while !chunks.is_empty() {
        let indent = if lines.is_empty() { first } else { rest };
        let room = width as i64 - len(indent) as i64;
        if !lines.is_empty() && blank(chunks.last().unwrap()) {
            chunks.pop();
        }
        let mut cur: Vec<String> = Vec::new();
        let mut cur_len = 0i64;
        while let Some(c) = chunks.last() {
            let l = len(c) as i64;
            if cur_len + l <= room {
                cur_len += l;
                cur.push(chunks.pop().unwrap());
            } else {
                break;
            }
        }
        if chunks.last().is_some_and(|c| len(c) as i64 > room) {
            long_word(&mut chunks, &mut cur, cur_len, room);
        }
        if cur.last().is_some_and(|c| blank(c)) {
            cur.pop();
        }
        if !cur.is_empty() {
            lines.push(format!("{indent}{}", cur.concat()));
        }
    }
    lines
}

/// TextWrapper._handle_long_word: break a word too long for a line, after a hyphen
/// when one falls in the room left.
fn long_word(chunks: &mut [String], cur: &mut Vec<String>, cur_len: i64, room: i64) {
    let left = if room < 1 { 1 } else { (room - cur_len).max(0) as usize };
    let chunk: Vec<char> = chunks.last().unwrap().chars().collect();
    let mut end = left.min(chunk.len());
    if chunk.len() > left {
        if let Some(h) = chunk[..left].iter().rposition(|&c| c == '-') {
            if h > 0 && chunk[..h].iter().any(|&c| c != '-') {
                end = h + 1;
            }
        }
    }
    cur.push(chunk[..end].iter().collect());
    *chunks.last_mut().unwrap() = chunk[end..].iter().collect();
}

/// textwrap.fill.
pub fn wrap(text: &str, width: usize, first: &str, rest: &str) -> String {
    wrap_with(text, width, first, rest).join("\n")
}

/// textwrap.wrap's lines with no indent; at least one line.
pub fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    let lines = wrap_with(text, width, "", "");
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

// ---------- difflib ----------

/// SequenceMatcher(None, a, b).ratio(): 2 * matched / total, matched summing the
/// longest matching blocks found recursively (no junk: bullpen's names are short).
fn ratio(a: &[char], b: &[char]) -> f64 {
    fn matched(a: &[char], b: &[char]) -> usize {
        let (mut bi, mut bj, mut size) = (0, 0, 0);
        let mut prev = vec![0usize; b.len() + 1];
        for i in 0..a.len() {
            let mut cur = vec![0usize; b.len() + 1];
            for j in 0..b.len() {
                if a[i] == b[j] {
                    let k = prev[j] + 1;
                    cur[j + 1] = k;
                    if k > size {
                        (bi, bj, size) = (i + 1 - k, j + 1 - k, k);
                    }
                }
            }
            prev = cur;
        }
        if size == 0 {
            return 0;
        }
        size + matched(&a[..bi], &b[..bj]) + matched(&a[bi + size..], &b[bj + size..])
    }
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }
    2.0 * matched(a, b) as f64 / total as f64
}

/// difflib.get_close_matches(word, names, n=1): the best name with a ratio of at least
/// 0.6; of equals, the one that sorts last.
pub fn close_match<'a>(word: &str, names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let w: Vec<char> = word.chars().collect();
    let mut best: Option<(f64, &str)> = None;
    for n in names {
        let r = ratio(&n.chars().collect::<Vec<_>>(), &w);
        if r >= 0.6 && best.is_none_or(|(br, bn)| r > br || (r == br && n > bn)) {
            best = Some((r, n));
        }
    }
    best.map(|(_, n)| n)
}

#[cfg(test)]
#[path = "test_help.rs"]
mod tests;
