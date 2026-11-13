//! Typing on a terminal (bullpen/line_edit.py): the keys a read gives, raw mode and
//! bracketed paste, and the line editor the picker (and, from T11b, the stepper's
//! comment and tell boxes) uses. Python keeps its own line_edit.py (a twin) for its stepper.

use bullpen_core::text::{char_width, printable};

pub const PASTE_START: &str = "\x1b[200~";
pub const PASTE_END: &str = "\x1b[201~";
/// Bracketed paste on and off: a paste arrives marked, so its line breaks don't press ⏎.
pub const PASTE_ON: &str = "\x1b[?2004h";
pub const PASTE_OFF: &str = "\x1b[?2004l";

const LEFT: &[&str] = &["\x1b[D", "\x1bOD", "\x02"]; // ←, ctrl+b
const RIGHT: &[&str] = &["\x1b[C", "\x1bOC", "\x06"]; // →, ctrl+f
const WORD_LEFT: &[&str] = &["\x1bb", "\x1b[1;3D", "\x1b[1;5D"]; // alt+b, alt+←, ctrl+←
const WORD_RIGHT: &[&str] = &["\x1bf", "\x1b[1;3C", "\x1b[1;5C"];
const HOME: &[&str] = &["\x1b[H", "\x1bOH", "\x1b[1~", "\x1b[7~", "\x01"]; // Home, ctrl+a
const END: &[&str] = &["\x1b[F", "\x1bOF", "\x1b[4~", "\x1b[8~", "\x05"]; // End, ctrl+e
const UP: &[&str] = &["\x1b[A", "\x1bOA"]; // with lines
const DOWN: &[&str] = &["\x1b[B", "\x1bOB"];
const NEW_LINE: &[&str] = &["\n", "\x1b\r"]; // ctrl+j, alt+⏎ (with lines)

fn is_space(c: char) -> bool {
    bullpen_core::text::is_space(c)
}

/// What's typed in the picker (and the tour stepper's comment and tell boxes), and the
/// cursor in it (a character index). The board's tell and answer line (board/src/edit.rs)
/// takes the same keys. On one line, a pasted line break is a space; with lines, a paste
/// keeps its breaks and ctrl+j or alt+⏎ starts a new line.
#[derive(Default)]
pub struct LineEdit {
    pub lines: bool,
    text: Vec<char>,
    pub at: usize,
    left: usize,
    #[allow(dead_code)] // rows' (the stepper's, from T11b)
    top: usize,
}

impl LineEdit {
    #[allow(dead_code)] // the stepper's boxes (T11b) start with text and lines
    pub fn new(text: &str, lines: bool) -> LineEdit {
        let mut e = LineEdit {
            lines,
            ..LineEdit::default()
        };
        e.paste(text);
        e
    }

    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    /// Apply one editing key; false when it isn't one (⏎, esc), for the caller.
    pub fn key(&mut self, k: &str) -> bool {
        let (n, at) = (self.text.len(), self.at);
        if let Some(rest) = k.strip_prefix(PASTE_START) {
            self.paste(rest.strip_suffix(PASTE_END).unwrap_or(rest));
        } else if LEFT.contains(&k) {
            self.at = at.saturating_sub(1);
        } else if RIGHT.contains(&k) {
            self.at = (at + 1).min(n);
        } else if WORD_LEFT.contains(&k) {
            self.at = self.word_start();
        } else if WORD_RIGHT.contains(&k) {
            self.at = self.word_end();
        } else if HOME.contains(&k) {
            self.at = self.line_start();
        } else if END.contains(&k) {
            self.at = self.line_end();
        } else if k == "\x7f" || k == "\x08" {
            self.cut(at.saturating_sub(1), at);
        } else if k == "\x1b[3~" {
            self.cut(at, (at + 1).min(n)); // delete
        } else if ["\x17", "\x1b\x7f", "\x1b\x08"].contains(&k) {
            self.cut(self.word_start(), at); // ctrl+w, alt+backspace
        } else if k == "\x1bd" {
            self.cut(at, self.word_end()); // alt+d
        } else if k == "\x15" {
            self.cut(self.line_start(), at); // ctrl+u: to the start of the line
        } else if k == "\x0b" {
            self.cut(at, self.line_end()); // ctrl+k: to its end
        } else if self.lines && NEW_LINE.contains(&k) {
            self.insert("\n");
        } else if self.lines && (UP.contains(&k) || DOWN.contains(&k)) {
            self.move_line(if UP.contains(&k) { -1 } else { 1 });
        } else if printable(k) && !k.starts_with('\x1b') {
            self.insert(k);
        } else {
            return false;
        }
        true
    }

    /// Pasted text at the cursor: a tab is a space, a line break is one unless this has
    /// lines, and other control characters go.
    pub fn paste(&mut self, text: &str) {
        let mut text = text.replace("\r\n", "\n").replace('\r', "\n").replace('\t', " ");
        if !self.lines {
            text = text.replace('\n', " ");
        }
        let kept: String = text
            .chars()
            .filter(|&c| c == '\n' || bullpen_core::text::is_printable(c))
            .collect();
        self.insert(&kept);
    }

    fn insert(&mut self, text: &str) {
        let add: Vec<char> = text.chars().collect();
        let n = add.len();
        self.text.splice(self.at..self.at, add);
        self.at += n;
    }

    fn cut(&mut self, a: usize, b: usize) {
        self.text.drain(a..b.max(a));
        self.at = a;
    }

    /// Where the word before the cursor starts (the spaces after it go with it).
    fn word_start(&self) -> usize {
        let mut i = self.at;
        while i > 0 && is_space(self.text[i - 1]) {
            i -= 1;
        }
        while i > 0 && !is_space(self.text[i - 1]) {
            i -= 1;
        }
        i
    }

    fn word_end(&self) -> usize {
        let mut i = self.at;
        while i < self.text.len() && is_space(self.text[i]) {
            i += 1;
        }
        while i < self.text.len() && !is_space(self.text[i]) {
            i += 1;
        }
        i
    }

    fn line_start(&self) -> usize {
        self.text[..self.at]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |i| i + 1)
    }

    fn line_end(&self) -> usize {
        self.text[self.at..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(self.text.len(), |i| self.at + i)
    }

    /// Up or down a line, to the same column or the line's end.
    fn move_line(&mut self, by: i32) {
        let start = self.line_start();
        let col = self.at - start;
        if by < 0 && start > 0 {
            let prev = self.text[..start - 1]
                .iter()
                .rposition(|&c| c == '\n')
                .map_or(0, |i| i + 1);
            self.at = (prev + col).min(start - 1);
        } else if by > 0 && self.line_end() < self.text.len() {
            let next = self.line_end() + 1;
            let end = self.text[next..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(self.text.len(), |i| next + i);
            self.at = (next + col).min(end);
        }
    }

    fn cols(&self, a: usize, b: usize) -> usize {
        self.text[a..b].iter().map(|&c| char_width(c)).sum()
    }

    /// On one line: what's shown in room columns, and the cursor's column in it. The
    /// shown part moves only when the cursor would leave it, as a shell's line does.
    pub fn view(&mut self, room: i64) -> (String, usize) {
        let room = room.max(1) as usize; // the cursor takes a column of its own at the end
        self.left = self.left.min(self.at);
        while self.left < self.at && self.cols(self.left, self.at) >= room {
            self.left += 1;
        }
        let mut to = self.left;
        while to < self.text.len() && self.cols(self.left, to + 1) <= room {
            to += 1;
        }
        (self.text[self.left..to].iter().collect(), self.cols(self.left, self.at))
    }

    /// With lines: the text wrapped to width, at most `most` rows of it around the
    /// cursor, and the cursor's (row, column) in them.
    #[allow(dead_code)] // the stepper's comment box (T11b)
    pub fn rows(&mut self, width: i64, most: i64) -> (Vec<String>, (usize, usize)) {
        let width = width.max(2) as usize;
        let (mut out, mut cur, mut i) = (Vec::new(), (0, 0), 0);
        let text = self.text();
        for line in text.split('\n') {
            let (mut row, mut used) = (String::new(), 0);
            for c in line.chars() {
                if used + char_width(c) > width - 1 {
                    out.push(std::mem::take(&mut row)); // a column for the cursor at the end
                    used = 0;
                }
                if i == self.at {
                    cur = (out.len(), used);
                }
                row.push(c);
                used += char_width(c);
                i += 1;
            }
            if i == self.at {
                cur = (out.len(), used);
            }
            out.push(row);
            i += 1; // the line break
        }
        let most = most.max(1) as usize;
        self.top = self.top.max((cur.0 + 1).saturating_sub(most)).min(cur.0);
        let shown = out.iter().skip(self.top).take(most).cloned().collect();
        (shown, (cur.0 - self.top, cur.1))
    }
}

/// The length of the key at the start of text (in bytes): an escape sequence (an
/// arrow), an alt+key or a whole paste is one key.
fn key_len(text: &str) -> usize {
    let one = text.chars().next().map_or(1, char::len_utf8);
    if let Some(rest) = text.strip_prefix(PASTE_START) {
        if let Some(end) = rest.find(PASTE_END) {
            return PASTE_START.len() + end + PASTE_END.len();
        }
    }
    let Some(rest) = text.strip_prefix('\x1b') else {
        return one;
    };
    let mut cs = rest.chars();
    match cs.next() {
        Some('[') => {
            // CSI: parameters 0x30–0x3F, intermediates 0x20–0x2F, a final 0x40–0x7E
            let b = rest.as_bytes();
            let mut j = 1;
            while j < b.len() && (0x30..=0x3f).contains(&b[j]) {
                j += 1;
            }
            while j < b.len() && (0x20..=0x2f).contains(&b[j]) {
                j += 1;
            }
            if j < b.len() && (0x40..=0x7e).contains(&b[j]) {
                1 + j + 1
            } else {
                one
            }
        }
        Some('O') => cs.next().map_or(one, |c| 2 + c.len_utf8()),
        Some('\x1b') | None => one,
        Some(c) => 1 + c.len_utf8(),
    }
}

/// The keys in what the terminal sent.
pub fn split_keys(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let n = key_len(&text[i..]);
        out.push(text[i..i + n].to_string());
        i += n;
    }
    out
}

/// codecs' incremental UTF-8 decoder with "replace": bytes split across reads wait for
/// the rest of their character; bad ones become U+FFFD.
#[derive(Default)]
pub struct Decoder {
    pending: Vec<u8>,
}

impl Decoder {
    pub fn decode(&mut self, data: &[u8]) -> String {
        self.pending.extend_from_slice(data);
        let mut out = String::new();
        let mut rest: &[u8] = &self.pending;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    out.push_str(s);
                    rest = &[];
                    break;
                }
                Err(e) => {
                    let (good, bad) = rest.split_at(e.valid_up_to());
                    out.push_str(std::str::from_utf8(good).unwrap_or_default());
                    match e.error_len() {
                        Some(n) => {
                            out.push('\u{fffd}');
                            rest = &bad[n..];
                        }
                        None => {
                            rest = bad; // a character the next read finishes
                            break;
                        }
                    }
                }
            }
        }
        self.pending = rest.to_vec();
        out
    }
}

/// Is there something to read on fd within ms milliseconds?
fn ready(fd: i32, ms: i32) -> bool {
    let mut p = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut p, 1, ms) > 0 }
}

fn read_fd(fd: i32, n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    let got = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), n) };
    buf.truncate(got.max(0) as usize);
    buf
}

/// The keys one read of the terminal gives.
pub fn read_keys(fd: i32, decoder: &mut Decoder) -> Vec<String> {
    let mut data = read_fd(fd, 1024);
    if data.is_empty() {
        return vec!["\x03".into()]; // the terminal went away
    }
    if data == b"\x1b" && ready(fd, 30) {
        data.extend(read_fd(fd, 1024)); // the rest of an arrow, not esc
    }
    let mut text = decoder.decode(&data);
    // a paste is one key however many reads it takes (unended after a second: its keys)
    while let Some(at) = text.rfind(PASTE_START) {
        if text[at..].contains(PASTE_END) {
            break;
        }
        let more = if ready(fd, 1000) {
            read_fd(fd, 65536)
        } else {
            Vec::new()
        };
        if more.is_empty() {
            break;
        }
        text += &decoder.decode(&more);
    }
    split_keys(&text)
}

/// The terminal on fd in raw mode until this is dropped (tty.setraw, then the old
/// settings back with TCSADRAIN).
pub struct Raw {
    fd: i32,
    old: libc::termios,
}

impl Raw {
    pub fn new(fd: i32) -> std::io::Result<Raw> {
        let mut old: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut old) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut t = old;
        t.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        t.c_oflag &= !libc::OPOST;
        t.c_cflag &= !(libc::CSIZE | libc::PARENB);
        t.c_cflag |= libc::CS8;
        t.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
        t.c_cc[libc::VMIN] = 1;
        t.c_cc[libc::VTIME] = 0;
        unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &t) };
        Ok(Raw { fd, old })
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(self.fd, libc::TCSADRAIN, &self.old) };
    }
}

/// shutil.get_terminal_size(): $COLUMNS and $LINES, else stdout's terminal, else 80×24.
pub fn term_size() -> (i64, i64) {
    let env = |k: &str| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0)
    };
    let (mut cols, mut rows) = (env("COLUMNS"), env("LINES"));
    if cols <= 0 || rows <= 0 {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        let (c, r) = if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } == 0 {
            (ws.ws_col as i64, ws.ws_row as i64)
        } else {
            (80, 24)
        };
        if cols <= 0 {
            cols = if c > 0 { c } else { 80 };
        }
        if rows <= 0 {
            rows = if r > 0 { r } else { 24 };
        }
    }
    (cols, rows)
}

#[cfg(test)]
#[path = "test_line_edit.rs"]
mod tests;
