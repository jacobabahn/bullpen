//! What the stepper shows and does with each key (stepper.py's Stepper), apart from
//! the terminal and the task: a header, the step's title, files, note, the person's
//! comments on it and its hunks; after the last step, a summary of the comments.

use crate::line_edit::LineEdit;
use crate::tours::step_files;
use bullpen_core::json::{py_str, s, Obj};
use bullpen_core::proc::run_input;
use bullpen_core::text::{char_width, is_space, len, split, strip, textwrap_indent};
use serde_json::Value;
use std::collections::HashMap;

const STEP_NEXT: &[&str] = &["n", "l", "\x1b[C", "\x1bOC"]; // n, l, →
const STEP_BACK: &[&str] = &["p", "h", "\x1b[D", "\x1bOD"]; // p, h, ←
const LINE_DOWN: &[&str] = &["j", "\x1b[B", "\x1bOB", "\x0e"]; // j, ↓, ctrl+n
const LINE_UP: &[&str] = &["k", "\x1b[A", "\x1bOA", "\x10"]; // k, ↑, ctrl+p
const PAGE_DOWN: &[&str] = &[" ", "f", "\x1b[6~", "\x04", "\x06"]; // space, f, page down, ctrl+d, ctrl+f
const PAGE_UP: &[&str] = &["b", "\x1b[5~", "\x15", "\x02"]; // b, page up, ctrl+u, ctrl+b
                                                            // the keys at the foot, longest first: the first that fits is shown
const STEP_KEYS: &[&str] = &[
    "n next · p back · j k scroll · c comment · t tell · v diff · ⏎ open · a accept · esc close",
    "n next · p back · c comment · t tell · v diff · ⏎ open · a accept · esc close",
    "n next · p back · c comment · t tell · esc close",
    "n p · c comment · t tell · esc close",
];
const COMMENT_KEYS: &[&str] = &[
    "⏎ saves · ctrl+j new line · esc cancels · mention a line to point at it",
    "⏎ saves · ctrl+j new line · esc cancels",
    "⏎ save · ctrl+j line · esc cancel",
];
const TELL_KEYS: &[&str] = &["⏎ sends it now · esc cancels"];
const END_KEYS: &[&str] = &[
    "s send them back · a accept · p back · t tell · v diff · ⏎ open · esc close",
    "s send · a accept · p back · t tell · esc close",
    "s send · a accept · p back · esc close",
];
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const OFF: &str = "\x1b[0m";

/// What a key asks of the stepper's caller, which the stepper can't do itself.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    Close,
    Send,
    Accept,
    /// close, then open the task's pane (the board's ⏎)
    Open,
    /// close, then open its diff (the board's v)
    Diff,
    /// a comment to save on the step shown
    Comment(String),
    /// a line to send the worker now
    Tell(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Read,
    Comment,
    Tell,
}

/// A step's lines of hunks: diff(step, width).
pub type DiffFn = Box<dyn Fn(usize, i64) -> Vec<String>>;

pub fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n != 1 { "s" } else { "" })
}

/// The stepper on one tour. at is the step shown (from 0); at == the number of steps
/// is the summary at the end.
pub struct Stepper {
    pub tid: String,
    pub title: String,
    pub steps: Vec<Obj>,
    pub commit: Value,
    pub comments: Vec<Obj>,
    pub diff: DiffFn,
    pub at: usize,
    pub top: i64,
    pub mode: Mode,
    pub edit: LineEdit,
    pub msg: String,
    sure: bool,
    pub cache: HashMap<(usize, i64), Vec<String>>,
}

impl Stepper {
    pub fn new(tid: &str, title: &str, tour: &Obj, comments: Vec<Obj>, at: i64, diff: DiffFn) -> Stepper {
        let steps: Vec<Obj> = tour
            .get("steps")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|v| v.as_object().cloned().unwrap_or_default())
            .collect();
        let at = at.clamp(0, steps.len() as i64) as usize;
        Stepper {
            tid: tid.into(),
            title: title.into(),
            steps,
            commit: tour.get("commit").cloned().unwrap_or(Value::Null),
            comments,
            diff,
            at,
            top: 0,
            mode: Mode::Read,
            edit: LineEdit::default(),
            msg: String::new(),
            sure: false,
            cache: HashMap::new(),
        }
    }

    pub fn at_end(&self) -> bool {
        self.at == self.steps.len()
    }

    pub fn go(&mut self, i: i64) {
        let i = i.clamp(0, self.steps.len() as i64) as usize;
        if i != self.at {
            self.at = i;
            self.top = 0;
        }
    }

    fn typing(&self) -> bool {
        self.mode != Mode::Read
    }

    /// Apply one key (rows: the body's height, for paging). Returns what the stepper
    /// can't do itself, or None.
    pub fn key(&mut self, k: &str, rows: i64) -> Option<Act> {
        if self.typing() {
            return self.comment_key(k);
        }
        let sure = self.sure;
        self.sure = false;
        self.msg.clear();
        match k {
            "\x1b" | "q" | "\x03" | "\x07" => return Some(Act::Close),
            "\r" | "\n" => return Some(Act::Open),
            "v" => return Some(Act::Diff),
            "t" => {
                self.mode = Mode::Tell;
                self.edit = LineEdit::default();
                return None;
            }
            "a" => return self.accept_key(sure),
            _ => {}
        }
        if self.at_end() {
            if k == "s" {
                if !self.comments.is_empty() {
                    return Some(Act::Send);
                }
                self.msg = "No comments to send: p goes back to the steps.".into();
                return None;
            }
        } else if k == "c" {
            self.mode = Mode::Comment;
            self.edit = LineEdit::new("", true);
            return None;
        }
        let page = (rows - 2).max(1);
        if STEP_NEXT.contains(&k) {
            self.go(self.at as i64 + 1);
        } else if STEP_BACK.contains(&k) {
            self.go(self.at as i64 - 1);
        } else if LINE_DOWN.contains(&k) {
            self.top += 1;
        } else if LINE_UP.contains(&k) {
            self.top = (self.top - 1).max(0);
        } else if PAGE_DOWN.contains(&k) {
            self.top += page;
        } else if PAGE_UP.contains(&k) {
            self.top = (self.top - page).max(0);
        } else if k == "g" {
            self.top = 0;
        } else if k == "G" {
            self.top = 1_000_000_000; // clamped when drawn
        }
        None
    }

    /// a: at the end, accept (a second a when comments aren't sent); before the end, a
    /// second a accepts all the same.
    fn accept_key(&mut self, sure: bool) -> Option<Act> {
        let unsent = if self.comments.is_empty() {
            String::new()
        } else {
            format!("{} not sent: ", plural(self.comments.len(), "comment"))
        };
        if sure || (self.at_end() && unsent.is_empty()) {
            return Some(Act::Accept);
        }
        self.sure = true;
        self.msg = if self.at_end() {
            format!("{unsent}a again accepts anyway, s sends them back instead.")
        } else {
            format!("{unsent}a again accepts {} before the end.", self.tid)
        };
        None
    }

    /// A key while a comment (lines), or a line to tell the worker, is typed.
    fn comment_key(&mut self, k: &str) -> Option<Act> {
        if self.edit.key(k) {
            return None;
        }
        if k == "\r" || k == "\n" {
            let (mode, text) = (self.mode, self.typed_text());
            self.mode = Mode::Read;
            if text.is_empty() {
                return None;
            }
            return Some(if mode == Mode::Tell {
                Act::Tell(text)
            } else {
                Act::Comment(text)
            });
        }
        if matches!(k, "\x1b" | "\x03" | "\x07") {
            self.msg = if self.mode == Mode::Comment {
                "Comment not saved."
            } else {
                "Not sent."
            }
            .into();
            self.mode = Mode::Read;
        }
        None
    }

    /// What's typed, to save or send: a tell on one line, a comment's lines without
    /// the spaces at their ends or blank lines around them.
    pub fn typed_text(&self) -> String {
        let text = self.edit.text();
        if self.mode == Mode::Tell {
            return split(&text).join(" ");
        }
        let lines: Vec<&str> = text.split('\n').map(bullpen_core::text::rstrip).collect();
        lines.join("\n").trim_matches('\n').to_string()
    }

    /// The drafts on step i of this tour.
    fn on_step(&self, i: usize) -> Vec<&Obj> {
        let n = Value::from(i as i64 + 1);
        self.comments
            .iter()
            .filter(|c| c.get("step") == Some(&n) && c.get("commit").unwrap_or(&Value::Null) == &self.commit)
            .collect()
    }

    /// The lines under the header: the step's title, files, note, comments and hunks;
    /// or, at the end, the comments to send.
    pub fn body(&mut self, width: i64) -> Vec<String> {
        let w = (width - 2).max(10);
        if self.at_end() {
            let mut out = vec![format!("{BOLD}End of the tour{OFF}"), String::new()];
            if self.comments.is_empty() {
                out.push("No comments. a accepts the task, esc closes, p goes back.".into());
                return out;
            }
            out.push(format!(
                "Your {}, not sent yet:",
                plural(self.comments.len(), "comment")
            ));
            for (n, c) in self.comments.iter().enumerate() {
                let field = |k: &str| c.get(k).map_or("None".into(), py_str);
                out.push(String::new());
                out.push(format!(
                    "{:>2}. {BOLD}step {}, {}{OFF}",
                    n + 1,
                    field("step"),
                    field("title")
                ));
                out.extend(wrap_text(s(c, "text"), w - 4, "    "));
            }
            out.push(String::new());
            out.push("s sends them back to the worker, who fixes them and runs ready again.".into());
            return out;
        }
        let st = &self.steps[self.at];
        let mut out = vec![
            format!("{BOLD}{}{OFF}", st.get("title").map_or("None".into(), py_str)),
            format!("{DIM}{}{OFF}", step_files(st)),
            String::new(),
        ];
        if bullpen_core::json::truthy(st.get("note")) {
            out.extend(wrap_text(&st.get("note").map(py_str).unwrap_or_default(), w, ""));
            out.push(String::new());
        }
        for c in self.on_step(self.at) {
            for line in wrap_text(s(c, "text"), w - 2, "▍ ") {
                out.push(format!("\x1b[33m{line}{OFF}"));
            }
            out.push(String::new());
        }
        let key = (self.at, width);
        if !self.cache.contains_key(&key) {
            let lines = (self.diff)(self.at, width);
            self.cache.insert(key, lines);
        }
        out.extend(self.cache[&key].iter().cloned());
        out
    }

    pub fn prompt(&self) -> String {
        if self.mode == Mode::Comment {
            format!("Comment on step {}: ", self.at + 1)
        } else {
            format!("Tell {}: ", self.tid)
        }
    }

    /// The rows of the box something is typed in, and the cursor's (row, column) in
    /// them from 0. A tell is one line after its prompt; a comment is its prompt, then
    /// its lines, up to half the body's rows.
    pub fn typed_box(&mut self, width: i64, height: i64) -> (Vec<String>, (i64, i64)) {
        let prompt = self.prompt();
        let plen = len(&prompt) as i64;
        if self.mode == Mode::Tell {
            let (shown, col) = self.edit.view(width - plen - 3);
            return (vec![format!(" {BOLD}{prompt}{OFF}{shown}")], (0, 1 + plen + col as i64));
        }
        let (lines, (row, col)) = self.edit.rows(width - 2, ((height - 5).div_euclid(2)).max(1));
        let mut rows = vec![format!(" {BOLD}{}{OFF}", prompt.trim_end_matches(is_space))];
        rows.extend(lines.iter().map(|l| format!(" {l}")));
        (rows, (row as i64 + 1, 1 + col as i64))
    }

    /// (row, column) of the cursor, from 1, while something is typed.
    pub fn cursor(&mut self, width: i64, height: i64) -> (i64, i64) {
        let (rows, (row, col)) = self.typed_box(width, height);
        (height - rows.len() as i64 + row, col + 1)
    }

    /// The keys while something is typed, the longest that fits width.
    pub fn typing_keys(&self, width: i64) -> &'static str {
        let keys = if self.mode == Mode::Comment {
            COMMENT_KEYS
        } else {
            TELL_KEYS
        };
        fitting(keys, width)
    }

    /// The stepper's lines: a header, the body from self.top, a message line and the keys.
    pub fn screen(&mut self, width: i64, height: i64) -> Vec<String> {
        let n = self.steps.len();
        let mut place = if self.at_end() {
            "end".to_string()
        } else {
            format!("step {} of {n}", self.at + 1)
        };
        if !self.comments.is_empty() {
            place += &format!(" · {}", plural(self.comments.len(), "comment"));
        }
        let mut head = format!("{}  {}", self.tid, self.title);
        let room = (width - len(&place) as i64 - 4).max(0);
        if len(&head) as i64 > room {
            head = py_head(&head, room - 1) + "…";
        }
        let gap = (width - 2 - len(&head) as i64 - len(&place) as i64).max(1) as usize;
        let mut out = vec![format!("{BOLD} {head}{}{place}{OFF}", " ".repeat(gap)), String::new()];
        let typing = if self.typing() {
            self.typed_box(width, height).0
        } else {
            vec![String::new()]
        };
        let rows = (height - 3 - typing.len() as i64).max(1);
        let body = self.body(width);
        self.top = self.top.min((body.len() as i64 - rows).max(0));
        let shown: Vec<&String> = body.iter().skip(self.top as usize).take(rows as usize).collect();
        let blank = rows as usize - shown.len();
        out.extend(shown.iter().map(|line| format!(" {line}{OFF}")));
        out.extend(std::iter::repeat_n(String::new(), blank));
        let mut out: Vec<String> = out.iter().map(|l| clip_line(l, width - 1)).collect();
        let more = body.len() as i64 - self.top - rows;
        if self.typing() {
            out.extend(typing);
            out.push(format!(" {DIM}{}{OFF}", self.typing_keys(width)));
        } else {
            let down = if more > 0 {
                format!("↓ {more} more")
            } else {
                String::new()
            };
            out.push(if self.msg.is_empty() {
                format!(" {DIM}{down}{OFF}")
            } else {
                format!(" {BOLD}{}{OFF}", self.msg)
            });
            let keys = if self.at_end() { END_KEYS } else { STEP_KEYS };
            out.push(format!(" {DIM}{}{OFF}", fitting(keys, width)));
        }
        out.iter().map(|l| clip_line(l, width - 1) + OFF).collect()
    }
}

/// The longest of keys that fits width (with a column each side), else the shortest.
fn fitting(keys: &[&'static str], width: i64) -> &'static str {
    keys.iter()
        .copied()
        .find(|k| len(k) as i64 <= width - 2)
        .unwrap_or(keys[keys.len() - 1])
}

/// Python's text[:n], n maybe negative (from the end).
fn py_head(text: &str, n: i64) -> String {
    let count = len(text) as i64;
    let n = if n < 0 { (count + n).max(0) } else { n.min(count) };
    text.chars().take(n as usize).collect()
}

/// Text wrapped to width, its own lines and blank lines kept: a list item's lines
/// after the first line up under its text.
pub fn wrap_text(text: &str, width: i64, indent: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        let lead = indent.to_string() + &" ".repeat(lead_len(line));
        let lines = textwrap_indent(line, width.max(10), indent, &lead);
        if lines.is_empty() {
            out.push(String::new());
        } else {
            out.extend(lines);
        }
    }
    out
}

/// The length of `\s*([-*] |\d+\. )?` at the start of line, in characters.
fn lead_len(line: &str) -> usize {
    let cs: Vec<char> = line.chars().collect();
    let mut i = cs.iter().take_while(|&&c| is_space(c)).count();
    if i + 1 < cs.len() && matches!(cs[i], '-' | '*') && cs[i + 1] == ' ' {
        i += 2;
    } else {
        let digits = cs[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && cs.get(i + digits) == Some(&'.') && cs.get(i + digits + 1) == Some(&' ') {
            i += digits + 2;
        }
    }
    i
}

/// The length (bytes) of a terminal escape sequence at the start of text: CSI, OSC
/// (ended by BEL or ESC \), or ESC and one character. 0 when text doesn't start with one.
fn escape_len(text: &str) -> usize {
    let Some(rest) = text.strip_prefix('\x1b') else {
        return 0;
    };
    let b = rest.as_bytes();
    if b.first() == Some(&b'[') {
        let mut j = 1;
        while j < b.len() && (0x30..=0x3f).contains(&b[j]) {
            j += 1;
        }
        while j < b.len() && (0x20..=0x2f).contains(&b[j]) {
            j += 1;
        }
        if j < b.len() && (0x40..=0x7e).contains(&b[j]) {
            return 1 + j + 1;
        }
    } else if b.first() == Some(&b']') {
        let mut j = 1;
        while j < b.len() && b[j] != 0x07 && b[j] != 0x1b {
            j += 1;
        }
        if b.get(j) == Some(&0x07) {
            return 1 + j + 1;
        }
        if b.get(j) == Some(&0x1b) && b.get(j + 1) == Some(&b'\\') {
            return 1 + j + 2;
        }
    }
    // ESC and any one character (Python's `.`, which isn't a line break)
    match rest.chars().next() {
        Some(c) if c != '\n' => 1 + c.len_utf8(),
        _ => 0,
    }
}

/// A line cut to width columns on screen, its colour codes kept (they take none).
pub fn clip_line(line: &str, width: i64) -> String {
    let (mut out, mut used, mut i) = (String::new(), 0i64, 0);
    while i < line.len() {
        let n = escape_len(&line[i..]);
        if n > 0 {
            out.push_str(&line[i..i + n]);
            i += n;
            continue;
        }
        let c = line[i..].chars().next().unwrap_or(' ');
        let w = char_width(c) as i64;
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
        i += c.len_utf8();
    }
    out
}

/// str.expandtabs(size): each tab to the next multiple of size columns (code points).
fn expand_tabs(line: &str, size: usize) -> String {
    let mut out = String::new();
    let mut col = 0;
    for c in line.chars() {
        match c {
            '\t' => {
                let n = size - col % size;
                out.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            '\n' | '\r' => {
                out.push(c);
                col = 0;
            }
            c => {
                out.push(c);
                col += 1;
            }
        }
    }
    out
}

/// A patch's lines coloured as git would, for a terminal without delta.
pub fn colour_diff(diff: &str) -> Vec<String> {
    const HEADS: &[&str] = &[
        "diff --git",
        "index ",
        "--- ",
        "+++ ",
        "new file",
        "deleted file",
        "rename ",
        "similarity",
    ];
    diff.trim_end_matches('\n')
        .split('\n')
        .map(|line| {
            let line = expand_tabs(line, 4);
            if HEADS.iter().any(|h| line.starts_with(h)) {
                format!("{BOLD}{line}{OFF}")
            } else if line.starts_with("@@") {
                format!("\x1b[36m{line}{OFF}")
            } else if line.starts_with('+') {
                format!("\x1b[32m{line}{OFF}")
            } else if line.starts_with('-') {
                format!("\x1b[31m{line}{OFF}")
            } else {
                line
            }
        })
        .collect()
}

/// A step's patch as the lines shown: through delta when installed, else coloured.
pub fn diff_lines(diff: &str, width: i64, delta: Option<&str>) -> Vec<String> {
    if strip(diff).is_empty() {
        return vec![format!("{DIM}(no code in this step){OFF}")];
    }
    if let Some(delta) = delta {
        let argv = [delta.to_string(), "--paging=never".into(), format!("--width={width}")];
        if let Ok(p) = run_input(&argv, diff, false) {
            if p.ok() && !strip(&p.stdout).is_empty() {
                return p.stdout.trim_end_matches('\n').split('\n').map(String::from).collect();
            }
        }
    }
    colour_diff(diff)
}

#[cfg(test)]
#[path = "tests/test_screen.rs"]
mod tests;
