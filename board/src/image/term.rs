//! Asking the terminal: whether it draws kitty graphics, its colours (OSC 10, 11, 4)
//! and its cell size (CSI 16 t), then reading what it answers, up to its answer to a
//! primary device attributes query (CSI c), which every terminal sends, last.
//!
//! The board's key reader (crossterm's) would take the answers for keys, so it stops
//! while the board asks (`Gate`): the reader parks and says so, the board writes the
//! questions between frames, and the reader reads the answers itself, then goes back
//! to keys. A key typed meanwhile is read with the answers and passed on as a key
//! (`keys`): letters, ⏎, esc, backspace, tab, ctrl+letter, the arrows and paste.

use super::theme::{parse_color, Said};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long the board waits for the terminal's answers.
pub const WAIT: Duration = Duration::from_secs(2);

/// The questions: the graphics query (with `graphics`), the colours, the cell size,
/// and last the device attributes, whose answer ends the answers.
pub fn questions(graphics: Option<u32>) -> String {
    let mut q = graphics.map(super::kitty::query).unwrap_or_default();
    q.push_str("\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b]4;1;?;2;?;3;?;4;?\x1b\\\x1b[16t\x1b[c");
    q
}

/// What the terminal answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answers {
    /// it said OK to the graphics query
    pub graphics: bool,
    pub said: Said,
    /// a cell's width and height in pixels
    pub cell: Option<(u32, u32)>,
    /// its answer to the device attributes came: everything it would say has
    /// (none: it didn't answer in time)
    pub done: bool,
    /// what wasn't an answer: keys typed meanwhile
    pub rest: Vec<u8>,
}

/// Read the answers in `bytes` (to the graphics query with ID `graphics`). An escape
/// sequence cut off at the end is left for more bytes (`done` is false).
pub fn parse(bytes: &[u8], graphics: Option<u32>) -> Answers {
    let mut a = Answers::default();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != 0x1b || i + 1 >= bytes.len() {
            a.rest.push(bytes[i]);
            i += 1;
            continue;
        }
        let (kind, start) = (bytes[i + 1], i + 2);
        let end = match kind {
            b']' | b'_' | b'P' => string_end(bytes, start),
            b'[' => csi_end(bytes, start),
            _ => {
                a.rest.push(bytes[i]);
                i += 1;
                continue;
            }
        };
        let Some((body_end, next)) = end else {
            // cut off: the rest of it hasn't come
            a.rest.extend_from_slice(&bytes[i..]);
            break;
        };
        let body = String::from_utf8_lossy(&bytes[start..body_end]).into_owned();
        let known = match kind {
            b']' => osc(&body, &mut a.said),
            b'_' => {
                if let (Some(id), Some(rest)) = (graphics, body.strip_prefix('G')) {
                    let (control, msg) = rest.split_once(';').unwrap_or((rest, ""));
                    if control.split(',').any(|kv| kv == format!("i={id}")) && msg == "OK" {
                        a.graphics = true;
                    }
                }
                true
            }
            b'P' => true,
            _ => csi(&body, &mut a),
        };
        if !known {
            a.rest.extend_from_slice(&bytes[i..next]);
        }
        i = next;
    }
    a
}

/// An OSC or APC string's end: (where its body ends, where the next byte is), at BEL or ESC \.
fn string_end(b: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut j = from;
    while j < b.len() {
        match b[j] {
            0x07 => return Some((j, j + 1)),
            0x1b if j + 1 < b.len() && b[j + 1] == b'\\' => return Some((j, j + 2)),
            0x1b if j + 1 >= b.len() => return None,
            _ => j += 1,
        }
    }
    None
}

/// A CSI sequence's end: at its final byte (0x40 to 0x7e).
fn csi_end(b: &[u8], from: usize) -> Option<(usize, usize)> {
    (from..b.len()).find(|&j| (0x40..=0x7e).contains(&b[j])).map(|j| (j + 1, j + 1))
}

/// An OSC answer about a colour: 10 the foreground, 11 the background, 4;N a palette entry.
fn osc(body: &str, said: &mut Said) -> bool {
    let mut parts = body.split(';');
    match parts.next() {
        Some("10") => said.fg = parts.next().and_then(parse_color),
        Some("11") => said.bg = parts.next().and_then(parse_color),
        Some("4") => {
            let rest: Vec<&str> = parts.collect();
            for pair in rest.chunks(2) {
                if let [n, c] = pair {
                    if let (Ok(n), Some(c)) = (n.parse::<u8>(), parse_color(c)) {
                        said.palette.push((n, c));
                    }
                }
            }
        }
        _ => {}
    }
    true
}

/// A CSI answer: the device attributes (`?…c`, the last) or the cell size (`6;H;Wt`).
/// Anything else is a key.
fn csi(body: &str, a: &mut Answers) -> bool {
    if body.starts_with('?') && body.ends_with('c') {
        a.done = true;
        return true;
    }
    if let Some(size) = body.strip_prefix("6;").and_then(|s| s.strip_suffix('t')) {
        if let Some((h, w)) = size.split_once(';') {
            if let (Ok(h), Ok(w)) = (h.parse::<u32>(), w.parse::<u32>()) {
                if h > 0 && w > 0 {
                    a.cell = Some((w, h));
                }
            }
        }
        return true;
    }
    false
}

/// Read the terminal's answers from standard input until its device attributes come,
/// or `WAIT` passes.
pub fn read_answers(graphics: Option<u32>) -> Answers {
    let deadline = Instant::now() + WAIT;
    let mut buf = Vec::new();
    loop {
        let a = parse(&buf, graphics);
        let left = deadline.saturating_duration_since(Instant::now());
        if a.done || left.is_zero() {
            return a;
        }
        let mut fds = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        // SAFETY: one pollfd, on standard input, which outlives the call
        let n = unsafe { libc::poll(&mut fds, 1, left.as_millis().max(1) as libc::c_int) };
        if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if n <= 0 {
            return parse(&buf, graphics);
        }
        let mut chunk = [0u8; 4096];
        // SAFETY: reads at most chunk's length into chunk
        let got = unsafe { libc::read(0, chunk.as_mut_ptr().cast(), chunk.len()) };
        if got <= 0 {
            return parse(&buf, graphics);
        }
        buf.extend_from_slice(&chunk[..got as usize]);
    }
}

/// What was typed while the board read the answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    Key(KeyEvent),
    Paste(String),
}

/// The keys in bytes typed while the reader was away: letters (UTF-8), ⏎, tab,
/// backspace, esc, ctrl+letter, the arrows, Home, End, PgUp, PgDn, and a bracketed
/// paste. Anything else is dropped.
pub fn keys(bytes: &[u8]) -> Vec<Typed> {
    let key = |code: KeyCode, m: KeyModifiers| Typed::Key(KeyEvent::new(code, m));
    let none = KeyModifiers::NONE;
    let mut out = Vec::new();
    let text = String::from_utf8_lossy(bytes);
    let mut rest: &str = &text;
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("\x1b[200~") {
            let (pasted, tail) = after.split_once("\x1b[201~").unwrap_or((after, ""));
            out.push(Typed::Paste(pasted.to_string()));
            rest = tail;
            continue;
        }
        let seq = [
            ("\x1b[A", KeyCode::Up), ("\x1b[B", KeyCode::Down), ("\x1b[C", KeyCode::Right), ("\x1b[D", KeyCode::Left),
            ("\x1bOA", KeyCode::Up), ("\x1bOB", KeyCode::Down), ("\x1bOC", KeyCode::Right), ("\x1bOD", KeyCode::Left),
            ("\x1b[H", KeyCode::Home), ("\x1b[F", KeyCode::End), ("\x1b[1~", KeyCode::Home), ("\x1b[4~", KeyCode::End),
            ("\x1b[5~", KeyCode::PageUp), ("\x1b[6~", KeyCode::PageDown),
        ]
        .into_iter()
        .find(|(s, _)| rest.starts_with(s));
        if let Some((s, code)) = seq {
            out.push(key(code, none));
            rest = &rest[s.len()..];
            continue;
        }
        if c == '\x1b' && rest.len() > 1 && rest.as_bytes()[1] == b'[' {
            // another sequence: skip it whole
            let end = rest[2..].find(|ch: char| ('\x40'..='\x7e').contains(&ch)).map_or(rest.len(), |j| j + 3);
            rest = &rest[end..];
            continue;
        }
        let typed = match c {
            '\r' | '\n' => key(KeyCode::Enter, none),
            '\t' => key(KeyCode::Tab, none),
            '\x7f' | '\x08' => key(KeyCode::Backspace, none),
            '\x1b' => key(KeyCode::Esc, none),
            '\x01'..='\x1a' => key(KeyCode::Char((c as u8 - 1 + b'a') as char), KeyModifiers::CONTROL),
            c if !c.is_control() => key(KeyCode::Char(c), if c.is_uppercase() { KeyModifiers::SHIFT } else { none }),
            _ => {
                rest = &rest[c.len_utf8()..];
                continue;
            }
        };
        out.push(typed);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Where the key reader is, while the board asks the terminal something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// reading keys
    Keys,
    /// the board wants to ask: the reader is to stop at its next look
    Wanted,
    /// the reader has stopped; the board writes the questions
    Parked,
    /// the questions are written: the reader reads the answers
    Asked,
}

/// Hands standard input from the key reader to the answers and back.
pub struct Gate {
    stage: Mutex<Stage>,
    moved: Condvar,
}

impl Default for Gate {
    fn default() -> Gate {
        Gate { stage: Mutex::new(Stage::Keys), moved: Condvar::new() }
    }
}

impl Gate {
    fn set(&self, s: Stage) {
        *self.stage.lock().unwrap_or_else(|e| e.into_inner()) = s;
        self.moved.notify_all();
    }

    fn get(&self) -> Stage {
        *self.stage.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The board wants to ask: false when it's asking already.
    pub fn want(&self) -> bool {
        let mut s = self.stage.lock().unwrap_or_else(|e| e.into_inner());
        if *s != Stage::Keys {
            return false;
        }
        *s = Stage::Wanted;
        true
    }

    /// The reader, before it looks for a key: should it stop for the board?
    pub fn wanted(&self) -> bool {
        self.get() == Stage::Wanted
    }

    /// The reader stops (the board is told by a message), and waits until the
    /// questions are written.
    pub fn park(&self) {
        self.set(Stage::Parked);
        let mut s = self.stage.lock().unwrap_or_else(|e| e.into_inner());
        while *s == Stage::Parked {
            s = self.moved.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// The board has written the questions: the reader reads the answers.
    pub fn asked(&self) {
        self.set(Stage::Asked);
    }

    /// The answers are read: back to keys.
    pub fn done(&self) {
        self.set(Stage::Keys);
    }
}

#[cfg(test)]
#[path = "tests/test_term.rs"]
mod tests;
