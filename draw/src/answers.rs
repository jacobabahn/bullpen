//! The terminal's answers to the questions an image look asks before it draws: whether
//! it draws kitty graphics, its colours (OSC 10, 11, 4) and its cell size (CSI 16 t),
//! then its primary device attributes (CSI c), which every terminal sends, last, so
//! they end the answers. Reading them is the caller's: the board's key reader
//! (board/src/image/term.rs) and the tour's (cli/src/stepper/look/).

use crate::theme::{parse_color, Said};

/// The questions: the graphics query (with `graphics`), the colours, the cell size,
/// and last the device attributes, whose answer ends the answers.
pub fn questions(graphics: Option<u32>) -> String {
    let mut q = graphics.map(crate::kitty::query).unwrap_or_default();
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
