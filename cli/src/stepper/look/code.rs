//! A step's code, drawn: its patch read into files, hunks and lines; each file a strip
//! with its name and +/− counts, each hunk under the lines it covers in the new file
//! and the function it's in, each line with its number in a gutter, added and removed
//! lines on a green or red wash with a bar at their edge. It's drawn on the terminal's
//! grid: a character a cell and a line a row, at the terminal's own size. The code's
//! colours are a small highlighter's (keywords, strings, comments, numbers and type
//! names, by the file's extension), in the terminal's colours.

use super::ink::{Face, Ink};
use bullpen_core::text::{fit, width};
use bullpen_draw::theme::{bright, faint, lit, solid, Rgb, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub kind: Kind,
    /// its number: in the new file, or in the old one for a removed line
    pub n: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hunk {
    /// the lines it covers in the new file
    pub from: u32,
    pub to: u32,
    /// what git says it's in (the function's line)
    pub within: String,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub name: String,
    pub hunks: Vec<Hunk>,
    pub added: u32,
    pub removed: u32,
    /// a binary file, or one with no hunks (a rename, a mode change)
    pub note: Option<String>,
}

/// A step's patch (git's, as `tour --step` gives it) as files.
pub fn parse(patch: &str) -> Vec<File> {
    let mut files: Vec<File> = Vec::new();
    let (mut old, mut new) = (0u32, 0u32);
    for l in patch.lines() {
        if let Some(rest) = l.strip_prefix("diff --git a/") {
            let name = rest.rsplit_once(" b/").map_or(rest, |(_, b)| b).to_string();
            files.push(File {
                name,
                hunks: vec![],
                added: 0,
                removed: 0,
                note: None,
            });
            continue;
        }
        let Some(file) = files.last_mut() else { continue };
        if l.starts_with("@@") {
            let mut nums = l.split_whitespace().skip(1);
            let (o, n) = (range(nums.next()), range(nums.next()));
            (old, new) = (o.0, n.0);
            let within = l.splitn(3, "@@").nth(2).unwrap_or("").trim().to_string();
            let to = n.0 + n.1.max(1) - 1;
            file.hunks.push(Hunk {
                from: n.0,
                to,
                within,
                lines: vec![],
            });
            continue;
        }
        let Some(hunk) = file.hunks.last_mut() else {
            if l.starts_with("Binary files") {
                file.note = Some("A binary file: no lines to show.".into());
            }
            continue;
        };
        let text = l.get(1..).unwrap_or("").replace('\t', "    ");
        let (kind, n) = match l.as_bytes().first() {
            Some(b'+') => (Kind::Added, &mut new),
            Some(b'-') => (Kind::Removed, &mut old),
            Some(b'\\') => continue,
            _ => (Kind::Context, &mut new),
        };
        hunk.lines.push(Line { kind, n: *n, text });
        *n += 1;
        match kind {
            Kind::Added => file.added += 1,
            Kind::Removed => file.removed += 1,
            Kind::Context => old += 1,
        }
    }
    for f in files.iter_mut().filter(|f| f.hunks.is_empty() && f.note.is_none()) {
        f.note = Some("No lines changed (a rename or a mode change).".into());
    }
    files
}

/// "-12,5" or "+3": its start and count.
fn range(part: Option<&str>) -> (u32, u32) {
    let p = part.unwrap_or("").trim_start_matches(['-', '+']);
    let mut it = p.split(',');
    let start = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (start, it.next().and_then(|s| s.parse().ok()).unwrap_or(1))
}

/// What a piece of code is, for its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
    Name,
}

/// The languages the highlighter knows, by how they comment and what their keywords are.
struct Lang {
    line_comment: &'static [&'static str],
    keywords: &'static [&'static str],
    quotes: &'static [char],
}

const RUST: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
    "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
];
const PYTHON: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "False", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or",
    "pass", "raise", "return", "True", "try", "while", "with", "yield", "self",
];
const LUA: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil",
    "not", "or", "repeat", "return", "then", "true", "until", "while",
];
const SHELL: &[&str] = &[
    "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function", "if", "in", "local", "return",
    "then", "until", "while",
];
const C_LIKE: &[&str] = &[
    "break",
    "case",
    "class",
    "const",
    "continue",
    "default",
    "else",
    "enum",
    "export",
    "false",
    "for",
    "func",
    "function",
    "go",
    "if",
    "import",
    "interface",
    "let",
    "new",
    "null",
    "package",
    "return",
    "static",
    "struct",
    "switch",
    "this",
    "true",
    "type",
    "var",
    "void",
    "while",
];

fn lang(file: &str) -> Option<Lang> {
    let ext = file.rsplit_once('.').map_or("", |(_, e)| e);
    let name = file.rsplit('/').next().unwrap_or(file);
    let l = |line_comment, keywords, quotes| {
        Some(Lang {
            line_comment,
            keywords,
            quotes,
        })
    };
    match ext {
        "rs" => l(&["//"], RUST, &['"']),
        "py" => l(&["#"], PYTHON, &['"', '\'']),
        "lua" => l(&["--"], LUA, &['"', '\'']),
        "sh" | "bash" | "zsh" => l(&["#"], SHELL, &['"', '\'']),
        "toml" | "yaml" | "yml" => l(&["#"], &["true", "false"], &['"', '\'']),
        "json" => l(&[], &["true", "false", "null"], &['"']),
        "js" | "ts" | "tsx" | "jsx" | "go" | "c" | "h" | "cpp" | "java" | "swift" | "kt" => {
            l(&["//"], C_LIKE, &['"', '\'', '`'])
        }
        "md" => l(&[], &[], &['`']),
        _ if name.starts_with("Makefile") || ext.is_empty() => l(&["#"], SHELL, &['"', '\'']),
        _ => None,
    }
}

/// A line of code in pieces, each with what it is.
pub fn tokens(file: &str, text: &str) -> Vec<(String, Tok)> {
    let Some(lang) = lang(file) else {
        return vec![(text.to_string(), Tok::Plain)];
    };
    if file.ends_with(".md") && text.starts_with('#') {
        return vec![(text.to_string(), Tok::Keyword)];
    }
    let cs: Vec<char> = text.chars().collect();
    let mut out: Vec<(String, Tok)> = Vec::new();
    let mut push = |s: String, t: Tok| match out.last_mut() {
        Some((last, lt)) if *lt == t => last.push_str(&s),
        _ => out.push((s, t)),
    };
    let mut i = 0;
    while i < cs.len() {
        let rest: String = cs[i..].iter().collect();
        let c = cs[i];
        if lang.line_comment.iter().any(|m| rest.starts_with(m)) {
            push(rest, Tok::Comment);
            break;
        }
        if lang.quotes.contains(&c) && !(c == '\'' && file.ends_with(".rs")) {
            let mut j = i + 1;
            while j < cs.len() && cs[j] != c {
                j += if cs[j] == '\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(cs.len());
            push(cs[i..end].iter().collect(), Tok::Str);
            i = end;
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            let word: String = cs[start..i].iter().collect();
            let tok = if c.is_ascii_digit() {
                Tok::Number
            } else if lang.keywords.contains(&word.as_str()) {
                Tok::Keyword
            } else if c.is_uppercase() && word.chars().any(|c| c.is_lowercase()) {
                Tok::Name
            } else {
                Tok::Plain
            };
            push(word, tok);
            continue;
        }
        push(c.to_string(), Tok::Plain);
        i += 1;
    }
    out
}

/// Two colours mixed: `a` with `t` of `b`.
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// The terminal's colours for code: on a dark pane its colours lifted toward the
/// foreground, so each reads on the washes; on a light one its colours as they are,
/// toward the foreground, dark enough to read on white.
pub fn colour(th: &Theme, tok: Tok) -> [u8; 4] {
    let light = luma(th.bg) > luma(th.fg);
    let toward = |c: Rgb, t: f32| solid(mix(if light { c } else { lit(c) }, th.fg, t));
    match tok {
        Tok::Plain => th.text(),
        Tok::Keyword => toward(th.blue, if light { 0.15 } else { 0.3 }),
        Tok::Str => toward(th.green, if light { 0.2 } else { 0.25 }),
        Tok::Comment => faint(th.fg, 0.7),
        Tok::Number => toward(th.amber(), if light { 0.55 } else { 0.2 }),
        Tok::Name => toward(th.between(), if light { 0.2 } else { 0.35 }),
    }
}

pub fn luma(c: Rgb) -> f32 {
    0.299 * c.0 as f32 + 0.587 * c.1 as f32 + 0.114 * c.2 as f32
}

/// A line's wash: green added, red removed, at their brightest and faint.
pub fn wash(th: &Theme, kind: Kind) -> Option<[u8; 4]> {
    match kind {
        Kind::Added => Some(faint(bright(th.green), 0.13)),
        Kind::Removed => Some(faint(bright(th.red), 0.11)),
        Kind::Context => None,
    }
}

/// The rows the files' code takes on the terminal's grid: a file's strip a row, a row
/// between files, a row for a file's note, a row over each hunk (its lines and the
/// function it's in) and a row each line.
pub fn rows(files: &[File]) -> usize {
    let mut n = 0;
    for (k, f) in files.iter().enumerate() {
        n += usize::from(k > 0) + 1 + usize::from(f.note.is_some());
        n += f.hunks.iter().map(|h| 1 + h.lines.len()).sum::<usize>();
    }
    n
}

/// How tall the files' code is, in rows `ch` high.
pub fn height(files: &[File], ch: f32) -> f32 {
    rows(files) as f32 * ch
}

/// The gutter's digits: the most a line number of the files has, two at least.
fn digits(files: &[File]) -> usize {
    let lines = files.iter().flat_map(|f| &f.hunks).flat_map(|h| &h.lines);
    lines.map(|l| l.n.to_string().len()).max().unwrap_or(0).max(2)
}

/// Draw the files' code from `top` (a row's top), `w` wide from x (a cell's edge): every
/// character in a cell of the terminal's grid, a line a row, as the terminal's own text.
/// A line's number is in the gutter, its code after it; a line too long is cut.
pub fn draw(ink: &mut Ink, files: &[File], x: f32, top: f32, w: f32) {
    let th = ink.th;
    let (cw, ch) = ink.sz.cell;
    let d = digits(files);
    let code_x = x + (d + 3) as f32 * cw;
    let cols = ((x + w - code_x) / cw).floor().max(1.0) as usize;
    let mut y = top;
    for (k, file) in files.iter().enumerate() {
        if k > 0 {
            y += ch;
        }
        if ink.shows(y, ch) {
            strip(ink, file, x, y, w);
        }
        y += ch;
        if let Some(note) = &file.note {
            ink.boxed(note, Face::Ui, ink.sz.detail, code_x, y, ch, th.dim());
            y += ch;
        }
        for (hk, hunk) in file.hunks.iter().enumerate() {
            if ink.shows(y, ch) {
                let label = format!("lines {}–{}", hunk.from, hunk.to);
                let used = ink.cells(&label, Face::Mono, code_x, y, cols, th.dim());
                let room = cols.saturating_sub(used + 2);
                if !hunk.within.is_empty() && room > 3 {
                    let within = fit(&hunk.within, room as i64);
                    ink.cells(&within, Face::Mono, code_x + (used + 2) as f32 * cw, y, room, th.dim());
                }
                if hk > 0 {
                    for k in 0..3 {
                        let cy = y + ch / 2.0 + (k as f32 - 1.0) * 0.22 * ch;
                        ink.disc(x + (d + 2) as f32 * cw / 2.0, cy, 1.8 * ink.s, faint(th.fg, 0.45));
                    }
                }
            }
            y += ch;
            for line in &hunk.lines {
                if ink.shows(y, ch) {
                    code_line(ink, &file.name, line, (x, y, w), d, cols);
                }
                y += ch;
            }
        }
    }
}

/// A file's strip, a row: its folder dim, its name, and its counts at the right, on the
/// grid as the code under it.
fn strip(ink: &mut Ink, file: &File, x: f32, y: f32, w: f32) {
    let (th, s) = (ink.th, ink.s);
    let (cw, ch) = ink.sz.cell;
    ink.rrect(x, y, w, ch, 8.0 * s, faint(th.fg, 0.075));
    let cells = (w / cw).floor() as usize;
    let (dir, base) = file
        .name
        .rsplit_once('/')
        .map_or(("", file.name.as_str()), |(d, b)| (d, b));
    let (plus, minus) = (format!("+{}", file.added), format!("−{}", file.removed));
    let at = cells.saturating_sub(1 + width(&plus) + 2 + width(&minus));
    let room = at.saturating_sub(3);
    let base = fit(base, room as i64);
    let dir_room = room.saturating_sub(width(&base));
    let mut pen = 1;
    if !dir.is_empty() && dir_room >= 4 {
        let dir = fit(&format!("{dir}/"), dir_room as i64);
        pen += ink.cells(&dir, Face::Mono, x + cw, y, dir_room, th.dim());
    }
    ink.cells(&base, Face::MonoBold, x + pen as f32 * cw, y, room, th.text());
    let ax = x + at as f32 * cw;
    let used = ink.cells(&plus, Face::Mono, ax, y, cells, colour(&th, Tok::Str));
    let red = solid(mix(lit(th.red), th.fg, 0.1));
    ink.cells(&minus, Face::Mono, ax + (used + 2) as f32 * cw, y, cells, red);
}

/// One line: its wash and bar, its number, its code in its colours, a character a cell
/// (cut at `cols`, "…" in the last).
fn code_line(ink: &mut Ink, file: &str, line: &Line, (x, y, w): (f32, f32, f32), d: usize, cols: usize) {
    let (th, s) = (ink.th, ink.s);
    let (cw, ch) = ink.sz.cell;
    if let Some(c) = wash(&th, line.kind) {
        ink.rect(x, y, w, ch, c);
        let bar = if line.kind == Kind::Added { th.green } else { th.red };
        ink.rect(x, y, (4.0 * s).max(2.0), ch, solid(bar));
    }
    let n = line.n.to_string();
    let ink_n = if line.kind == Kind::Context {
        faint(th.fg, 0.5)
    } else {
        faint(th.fg, 0.7)
    };
    let nx = x + (1 + d).saturating_sub(n.len()) as f32 * cw;
    ink.cells(&n, Face::Mono, nx, y, n.len(), ink_n);
    let code_x = x + (d + 3) as f32 * cw;
    let cut = width(&line.text) > cols;
    let left = if cut { cols.saturating_sub(1) } else { cols };
    let mut col = 0;
    for (text, tok) in tokens(file, &line.text) {
        if col >= left {
            break;
        }
        let used = ink.cells(
            &text,
            Face::Mono,
            code_x + col as f32 * cw,
            y,
            left - col,
            colour(&th, tok),
        );
        col += used;
        if used < width(&text) {
            break; // a wide character at the edge
        }
    }
    if cut {
        ink.cells("…", Face::Mono, code_x + col as f32 * cw, y, 1, th.dim());
    }
}

#[cfg(test)]
#[path = "tests/test_code.rs"]
mod tests;
