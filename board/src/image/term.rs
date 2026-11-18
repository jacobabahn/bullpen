//! Asking the terminal: whether it draws kitty graphics, its colours (OSC 10, 11, 4)
//! and its cell size (CSI 16 t), then reading what it answers, up to its answer to a
//! primary device attributes query (CSI c), which every terminal sends, last.
//!
//! The board's key reader (crossterm's) would take the answers for keys, so it stops
//! while the board asks (`Gate`): the reader parks and says so, the board writes the
//! questions between frames, and the reader reads the answers itself, then goes back
//! to keys. A key typed meanwhile is read with the answers and passed on as a key
//! (`keys`): letters, ⏎, esc, backspace, delete, tab, ctrl and alt with a key, the
//! arrows (with ctrl, alt or shift), Home, End, PgUp, PgDn and paste.
//!
//! An answer is never taken for keys. One cut off is held until the rest of it comes.
//! When the answers don't all come in `WAIT`, the board goes on without them, but the
//! reader keeps reading (`drain`), passing keys on as they come, until the device
//! attributes come: the terminal's last answer, however late, so no answer reaches
//! crossterm. An esc with nothing after it for `QUIET` is a key there (one typed while
//! the answers are late), as crossterm takes it. A terminal that never sends its device
//! attributes has the board's keys read here for the rest of the run, and isn't asked
//! again.

use super::theme::{parse_color, Said};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long the board waits for the terminal's answers.
pub const WAIT: Duration = Duration::from_secs(2);
/// How long an esc read while the answers are late waits for more bytes before it's a key.
pub const QUIET: Duration = Duration::from_millis(100);

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
    /// the bytes read whole: from here on is an escape sequence cut off at the end
    /// (an answer, or a key, whose rest hasn't come), in neither `rest` nor the answers
    pub whole: usize,
}

/// Read the answers in `bytes` (to the graphics query with ID `graphics`). An escape
/// sequence cut off at the end, or a lone escape there, is left for more bytes (`whole`).
pub fn parse(bytes: &[u8], graphics: Option<u32>) -> Answers {
    let mut a = Answers::default();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 >= bytes.len() {
            break;
        }
        if bytes[i] != 0x1b {
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
            break; // cut off: the rest of it hasn't come
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
    a.whole = i;
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
/// or `WAIT` passes. With them, what's left unread: an answer cut off when the time
/// ran out, which `drain` reads to its end. Keys typed after the answers are in `rest`.
pub fn read_answers(input: &mut impl Input, graphics: Option<u32>) -> (Answers, Vec<u8>) {
    let mut buf = Vec::new();
    let mut a = read_until(input, &mut buf, graphics, Instant::now() + WAIT);
    let left = buf.split_off(a.whole);
    if a.done {
        // after the last answer, a cut-off sequence is a key's (a lone esc)
        a.rest.extend(left);
        return (a, Vec::new());
    }
    (a, left)
}

/// After answers that didn't all come in time: read on until the device attributes
/// come (the terminal's last answer), passing the keys typed meanwhile to `typed` and
/// dropping the answers. `left` is the cut-off answer read already. A lone esc with
/// nothing after it for `QUIET` is a key. `tick` is called each `QUIET` or so, so the
/// reader can look for what it can't read here (the window's size).
pub fn drain(
    input: &mut impl Input,
    left: Vec<u8>,
    graphics: Option<u32>,
    mut typed: impl FnMut(Typed),
    mut tick: impl FnMut(),
) {
    let mut buf = left;
    loop {
        let before = buf.len();
        let a = read_until(input, &mut buf, graphics, Instant::now() + QUIET);
        let quiet = buf.len() == before;
        keys(&a.rest).into_iter().for_each(&mut typed);
        let mut left = buf.split_off(a.whole);
        if a.done || (quiet && left == b"\x1b") {
            keys(&left).into_iter().for_each(&mut typed);
            left.clear();
        }
        if a.done {
            return;
        }
        tick();
        buf = left;
    }
}

/// Read input into buf until the device attributes come or `deadline`; the answers in it.
fn read_until(input: &mut impl Input, buf: &mut Vec<u8>, graphics: Option<u32>, deadline: Instant) -> Answers {
    loop {
        let a = parse(buf, graphics);
        let left = deadline.saturating_duration_since(Instant::now());
        if a.done || left.is_zero() {
            return a;
        }
        match input.next(left) {
            Some(bytes) => buf.extend(bytes),
            None => return a,
        }
    }
}

/// Where the answers are read from: what comes within `wait`, None when nothing does.
pub trait Input {
    fn next(&mut self, wait: Duration) -> Option<Vec<u8>>;
}

/// Standard input, which the key reader reads too.
pub struct Stdin;

impl Input for Stdin {
    fn next(&mut self, wait: Duration) -> Option<Vec<u8>> {
        loop {
            let mut fds = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
            // SAFETY: one pollfd, on standard input, which outlives the call
            let n = unsafe { libc::poll(&mut fds, 1, wait.as_millis().max(1) as libc::c_int) };
            if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if n <= 0 {
                return None;
            }
            let mut chunk = [0u8; 4096];
            // SAFETY: reads at most chunk's length into chunk
            let got = unsafe { libc::read(0, chunk.as_mut_ptr().cast(), chunk.len()) };
            return (got > 0).then(|| chunk[..got as usize].to_vec());
        }
    }
}

/// What was typed while the board read the answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    Key(KeyEvent),
    Paste(String),
}

/// The keys in bytes typed while the reader was away: letters (UTF-8), ⏎, tab,
/// backspace, esc, ctrl+letter, alt with any of these (esc first), the arrows, Home and
/// End (with shift, alt or ctrl), PgUp, PgDn, Delete, shift+tab, and a bracketed paste.
/// Anything else is dropped.
pub fn keys(bytes: &[u8]) -> Vec<Typed> {
    let mut out = Vec::new();
    let text = String::from_utf8_lossy(bytes);
    let mut rest: &str = &text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("\x1b[200~") {
            let (pasted, tail) = after.split_once("\x1b[201~").unwrap_or((after, ""));
            out.push(Typed::Paste(pasted.to_string()));
            rest = tail;
            continue;
        }
        let (key, len) = one_key(rest);
        out.extend(key.map(Typed::Key));
        rest = &rest[len..];
    }
    out
}

/// The key at the start of text, if it's one `keys` knows, and how many bytes it takes.
fn one_key(text: &str) -> (Option<KeyEvent>, usize) {
    let key = |code: KeyCode, m: KeyModifiers| Some(KeyEvent::new(code, m));
    let none = KeyModifiers::NONE;
    let c = text.chars().next().expect("text isn't empty");
    if c == '\x1b' {
        let after = &text[1..];
        if let Some(csi) = after.strip_prefix('[') {
            return csi_key(csi);
        }
        if let Some(ss3) = after.strip_prefix('O') {
            let code = ss3.chars().next().and_then(arrow);
            return (code.and_then(|code| key(code, none)), 2 + usize::from(code.is_some()));
        }
        // esc then a key: alt with that key, unless it's esc too (or nothing follows)
        if !after.is_empty() && !after.starts_with('\x1b') {
            let (k, len) = one_key(after);
            return (k.map(|mut k| {
                k.modifiers |= KeyModifiers::ALT;
                k
            }), 1 + len);
        }
        return (key(KeyCode::Esc, none), 1);
    }
    let k = match c {
        '\r' | '\n' => key(KeyCode::Enter, none),
        '\t' => key(KeyCode::Tab, none),
        '\x7f' | '\x08' => key(KeyCode::Backspace, none),
        '\x01'..='\x1a' => key(KeyCode::Char((c as u8 - 1 + b'a') as char), KeyModifiers::CONTROL),
        c if !c.is_control() => key(KeyCode::Char(c), if c.is_uppercase() { KeyModifiers::SHIFT } else { none }),
        _ => None,
    };
    (k, c.len_utf8())
}

fn arrow(c: char) -> Option<KeyCode> {
    match c {
        'A' => Some(KeyCode::Up),
        'B' => Some(KeyCode::Down),
        'C' => Some(KeyCode::Right),
        'D' => Some(KeyCode::Left),
        'H' => Some(KeyCode::Home),
        'F' => Some(KeyCode::End),
        _ => None,
    }
}

/// A key sent as CSI (after `ESC [`): `A`…`D`, `H`, `F` (`1;3D` with alt, …), `N~`
/// (Home, Delete, End, PgUp, PgDn), `Z` (shift+tab). Its length counts the `ESC [`.
fn csi_key(csi: &str) -> (Option<KeyEvent>, usize) {
    let Some(end) = csi.find(|ch: char| ('\x40'..='\x7e').contains(&ch)) else {
        return (None, 2 + csi.len()); // cut off: dropped
    };
    let (params, last) = (&csi[..end], csi.as_bytes()[end] as char);
    let len = 2 + end + 1;
    let mut nums = params.split(';').map(|n| n.parse::<u8>().unwrap_or(1));
    let first = nums.next().unwrap_or(1);
    // xterm's modifier: 1 + shift 1, alt 2, ctrl 4
    let m = nums.next().unwrap_or(1).saturating_sub(1);
    let mut mods = KeyModifiers::NONE;
    for (bit, flag) in [(1, KeyModifiers::SHIFT), (2, KeyModifiers::ALT), (4, KeyModifiers::CONTROL)] {
        if m & bit != 0 {
            mods |= flag;
        }
    }
    let code = match last {
        '~' => match first {
            1 | 7 => Some(KeyCode::Home),
            3 => Some(KeyCode::Delete),
            4 | 8 => Some(KeyCode::End),
            5 => Some(KeyCode::PageUp),
            6 => Some(KeyCode::PageDown),
            _ => None,
        },
        'Z' => Some(KeyCode::BackTab),
        c => arrow(c),
    };
    (code.map(|code| KeyEvent::new(code, mods)), len)
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

    /// Is the reader reading keys (not stopped for the board, nor reading answers)?
    pub fn open(&self) -> bool {
        self.get() == Stage::Keys
    }

    /// The reader, before it looks for a key: should it stop for the board?
    pub fn wanted(&self) -> bool {
        self.get() == Stage::Wanted
    }

    /// The reader stops for the board: it's parked first, then the board is told
    /// (`tell`, false when the board is gone), then it waits until the questions are
    /// written. Parked before the board hears of it, so the board's `asked` can't come
    /// first and be lost. The gate opens to keys again when what this returns is
    /// dropped, however the reader goes on (None: at once).
    pub fn park(&self, tell: impl FnOnce() -> bool) -> Option<Open<'_>> {
        self.set(Stage::Parked);
        let open = Open(self);
        if !tell() {
            return None;
        }
        let mut s = self.stage.lock().unwrap_or_else(|e| e.into_inner());
        while *s == Stage::Parked {
            s = self.moved.wait(s).unwrap_or_else(|e| e.into_inner());
        }
        Some(open)
    }

    /// The board has written the questions: the reader reads the answers.
    pub fn asked(&self) {
        let mut s = self.stage.lock().unwrap_or_else(|e| e.into_inner());
        if *s == Stage::Parked {
            *s = Stage::Asked;
            self.moved.notify_all();
        }
    }

    /// The answers are read: back to keys.
    pub fn done(&self) {
        self.set(Stage::Keys);
    }
}

/// The reader is away from keys: when this is dropped, on every way out of reading
/// the answers (an error, a panic, the board gone), the gate opens to keys again.
pub struct Open<'a>(&'a Gate);

impl Drop for Open<'_> {
    fn drop(&mut self) {
        self.0.done();
    }
}

#[cfg(test)]
#[path = "tests/test_term.rs"]
mod tests;
