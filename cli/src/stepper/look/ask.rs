//! Asking the terminal, before the tour is drawn as images: whether it draws kitty
//! graphics, its colours and its cell size (bullpen-draw's questions and answers, as
//! the board asks), read from standard input in raw mode until its device attributes,
//! its last answer, come or `WAIT` passes. Keys typed meanwhile are kept for the
//! stepper. A cell's size is the window's size in pixels over its cells when the
//! terminal says them, else its answer.

use bullpen_draw::answers::{parse, questions, Answers};
use std::io::Write;
use std::time::{Duration, Instant};

/// How long the tour waits for the answers.
const WAIT: Duration = Duration::from_secs(2);

/// Ask, and read the answers on fd: them, and the bytes typed meanwhile.
pub fn ask(fd: i32, out: &mut impl Write) -> (Answers, Vec<u8>) {
    let _ = out.write_all(questions(Some(super::FIRST - 1)).as_bytes());
    let _ = out.flush();
    let deadline = Instant::now() + WAIT;
    let mut buf = Vec::new();
    loop {
        let a = parse(&buf, Some(super::FIRST - 1));
        let left = deadline.saturating_duration_since(Instant::now());
        if a.done || left.is_zero() || !crate::line_edit::ready(fd, left.as_millis().max(1) as i32) {
            let typed = a.rest.clone();
            return (a, typed);
        }
        let mut chunk = [0u8; 4096];
        // SAFETY: reads at most chunk's length into chunk
        let got = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if got <= 0 {
            return (a, Vec::new());
        }
        buf.extend_from_slice(&chunk[..got as usize]);
    }
}

/// A cell's size from the window's size in pixels and in cells, when it says both.
pub fn window_cell(fd: i32) -> Option<(u32, u32)> {
    // SAFETY: TIOCGWINSZ fills a winsize
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } != 0 {
        return None;
    }
    let (w, h, c, r) = (
        ws.ws_xpixel as u32,
        ws.ws_ypixel as u32,
        ws.ws_col as u32,
        ws.ws_row as u32,
    );
    (w > 0 && h > 0 && c > 0 && r > 0).then(|| (w / c, h / r))
}
