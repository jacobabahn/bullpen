//! A step's code, drawn: its patch read into files, hunks and lines; each file a strip
//! with its name and +/− counts, each hunk under the lines it covers in the new file
//! and the function it's in, each line with its number in a gutter, added and removed
//! lines on a green or red wash with a bar at their edge. The code's colours are a
//! small highlighter's (keywords, strings, comments, numbers and type names, by the
//! file's extension), in the terminal's colours.

use super::ink::{Face, Ink};
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

/// The sizes code is drawn at, at a 2× cell.
const CODE: f32 = 21.0;
const LINE: f32 = 31.0;
const STRIP: f32 = 44.0;
const HUNK: f32 = 34.0;
const FILE_GAP: f32 = 22.0;

/// How tall the files' code is, drawn `s` big.
pub fn height(files: &[File], s: f32) -> f32 {
    let mut h = 0.0;
    for (k, f) in files.iter().enumerate() {
        h += if k > 0 { FILE_GAP } else { 0.0 } + STRIP + 6.0;
        if f.note.is_some() {
            h += LINE;
        }
        for hunk in &f.hunks {
            h += HUNK + LINE * hunk.lines.len() as f32;
        }
    }
    (h * s).round()
}

/// Draw the files' code from `top`, `w` wide from x.
pub fn draw(ink: &mut Ink, files: &[File], x: f32, top: f32, w: f32) {
    let th = ink.th;
    let s = ink.s;
    let (code, lh) = (CODE * s, (LINE * s).round());
    let gutter = ink.ty.width("00000", Face::Mono, code) + 22.0 * s;
    let mut y = top;
    for (k, file) in files.iter().enumerate() {
        if k > 0 {
            y += FILE_GAP * s;
        }
        strip(ink, file, x, y, w);
        y += (STRIP + 6.0) * s;
        if let Some(note) = &file.note {
            ink.boxed(note, Face::Ui, 19.0 * s, x + gutter, y, lh, th.dim());
            y += lh;
        }
        for (hk, hunk) in file.hunks.iter().enumerate() {
            let hh = HUNK * s;
            let label = format!("lines {}–{}", hunk.from, hunk.to);
            let end = ink.boxed(&label, Face::Ui, 19.0 * s, x + gutter, y, hh, th.dim());
            if !hunk.within.is_empty() {
                let room = x + w - end - 30.0 * s;
                let t = ink.ty.fit(&hunk.within, Face::Mono, 18.0 * s, room);
                ink.boxed(&t, Face::Mono, 18.0 * s, end + 18.0 * s, y, hh, faint(th.fg, 0.55));
            }
            if hk > 0 {
                for d in 0..3 {
                    ink.disc(
                        x + gutter / 2.0,
                        y + (9.0 + 8.0 * d as f32) * s,
                        1.8 * s,
                        faint(th.fg, 0.45),
                    );
                }
            }
            y += hh;
            for line in &hunk.lines {
                code_line(ink, &file.name, line, (x, y, w, lh), gutter);
                y += lh;
            }
        }
    }
}

/// A file's strip: its folder dim, its name, and its counts at the right.
fn strip(ink: &mut Ink, file: &File, x: f32, y: f32, w: f32) {
    let (th, s) = (ink.th, ink.s);
    let (code, sh) = (CODE * s, STRIP * s);
    ink.rrect(x, y, w, sh, 10.0 * s, faint(th.fg, 0.075));
    let (dir, base) = file
        .name
        .rsplit_once('/')
        .map_or(("", file.name.as_str()), |(d, b)| (d, b));
    let small = 0.9 * code;
    let plus = format!("+{}", file.added);
    let minus = format!("  −{}", file.removed);
    let counts = ink.ty.width(&plus, Face::Mono, small) + ink.ty.width(&minus, Face::Mono, small);
    let cx = x + w - 16.0 * s - counts;
    let mut pen = x + 16.0 * s;
    if !dir.is_empty() {
        let room = cx - pen - 24.0 * s - ink.ty.width(base, Face::MonoBold, code);
        let dir = ink.ty.fit(&format!("{dir}/"), Face::Mono, code, room.max(0.0));
        pen = ink.boxed(&dir, Face::Mono, code, pen, y, sh, th.dim());
    }
    ink.boxed(base, Face::MonoBold, code, pen, y, sh, th.text());
    let e = ink.boxed(&plus, Face::Mono, small, cx, y, sh, colour(&th, Tok::Str));
    ink.boxed(&minus, Face::Mono, small, e, y, sh, solid(mix(lit(th.red), th.fg, 0.1)));
}

/// One line: its wash and bar, its number, its code in its colours (cut at the edge).
fn code_line(ink: &mut Ink, file: &str, line: &Line, (x, y, w, lh): (f32, f32, f32, f32), gutter: f32) {
    let (th, s) = (ink.th, ink.s);
    let code = CODE * s;
    if let Some(c) = wash(&th, line.kind) {
        ink.rect(x, y, w, lh, c);
        let bar = if line.kind == Kind::Added { th.green } else { th.red };
        ink.rect(x, y, (4.0 * s).max(2.0), lh, solid(bar));
    }
    let n = line.n.to_string();
    let nw = ink.ty.width(&n, Face::Mono, 0.86 * code);
    let ink_n = if line.kind == Kind::Context {
        faint(th.fg, 0.5)
    } else {
        faint(th.fg, 0.7)
    };
    ink.boxed(&n, Face::Mono, 0.86 * code, x + gutter - 14.0 * s - nw, y, lh, ink_n);
    let room = w - gutter - 12.0 * s;
    let advance = ink.ty.width("0", Face::Mono, code).max(1.0);
    let fits = (room / advance).floor() as usize;
    let cut = line.text.chars().count() > fits;
    let mut left = if cut { fits.saturating_sub(1) } else { fits };
    let mut pen = x + gutter;
    for (text, tok) in tokens(file, &line.text) {
        if left == 0 {
            break;
        }
        let piece: String = text.chars().take(left).collect();
        left -= piece.chars().count();
        pen = ink.boxed(&piece, Face::Mono, code, pen, y, lh, colour(&th, tok));
    }
    if cut {
        ink.boxed("…", Face::Mono, code, pen, y, lh, th.dim());
    }
}

#[cfg(test)]
#[path = "tests/test_code.rs"]
mod tests;
