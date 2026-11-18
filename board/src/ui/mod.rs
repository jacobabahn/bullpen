//! Drawing the board. It sits in a narrow pane, so every line is cut to fit.
//!
//! `draw` picks what the mode shows, sizes the board once (`Sizes`), then draws the
//! list (`list`, its Later group `later`), the bottom rows (`bottom`), a full-screen view (`overlay`) or the
//! `?` list (`help`).

pub(crate) mod bottom;
mod help;
mod later;
mod list;
pub(crate) mod overlay;
#[cfg(test)]
#[path = "tests/testing.rs"]
pub(crate) mod testing;

use crate::model::Layout;
use crate::{App, Mode};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(crate) fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}
pub(crate) fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}
pub(crate) fn rev() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}
pub(crate) const AMBER: Color = Color::Yellow;

/// A risk level's colour: green, amber, red.
pub(crate) fn risk_style(level: &str) -> Style {
    match level {
        "high" => bold().fg(Color::Red),
        "medium" => Style::new().fg(AMBER),
        _ => Style::new().fg(Color::Green),
    }
}
/// Under the selected task's rows: a tint from the terminal's own palette, so status
/// colours still show on it. Some themes make it the background colour, so the ID
/// in reverse video is what says which task is selected in every theme.
pub(crate) const TINT: Color = Color::DarkGray;

pub(crate) fn mark_style(status: &str) -> Style {
    match status {
        "needs" => bold().fg(AMBER),
        "stopped" => bold().fg(Color::Red),
        "ready" | "finished" => bold().fg(Color::Green),
        "review" => bold().fg(Color::Blue),
        _ => dim(),
    }
}

fn frame(app: &App, frames: &[&'static str], ms: u128) -> &'static str {
    frame_at(app.started, frames, ms)
}

/// The frame of an animation that began at `started`, each frame held `ms`.
pub(crate) fn frame_at(started: std::time::Instant, frames: &[&'static str], ms: u128) -> &'static str {
    frames[(started.elapsed().as_millis() / ms) as usize % frames.len()]
}

/// A task with a command running (accept, drop, start, resume): a ring with a gap going round.
fn spinner(app: &App) -> &'static str {
    frame(app, SPINNER, 100)
}

/// A working task: the six-dot snake, at five frames a second. It says "nothing for you
/// here", so it moves slower than the command spinner, which is feedback.
fn working_mark(app: &App) -> &'static str {
    frame(app, WORKING, MARK_MS)
}

/// A task in review: a quarter arc turning, as slowly as the working mark.
fn review_mark(app: &App) -> &'static str {
    frame(app, REVIEW, MARK_MS)
}

/// How long the working and review marks hold each frame.
pub const MARK_MS: u128 = 200;

pub(crate) const SPINNER: &[&str] = &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];

pub(crate) const WORKING: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub(crate) const REVIEW: &[&str] = &["◜", "◝", "◞", "◟"];

/// Write a line at (x, y), never into the last column.
pub(crate) fn put(buf: &mut Buffer, area: Rect, y: usize, x: usize, line: impl Into<Line<'static>>) {
    let (w, h) = (area.width as usize, area.height as usize);
    if y < h && x + 1 < w {
        buf.set_line(area.x + x as u16, area.y + y as u16, &line.into(), (w - 1 - x) as u16);
    }
}

pub(crate) fn span(text: impl Into<String>, style: Style) -> Span<'static> {
    Span::styled(text.into(), style)
}

/// The empty rows to leave at the bottom (the config's board_bottom_gap): Rex draws
/// minimized panes' capsules over the bottom right of the window, so the board's
/// last lines would sit under them. A board too short for its other lines and
/// `need` more drops the gap first.
fn bottom_gap(app: &App, h: usize, need: usize) -> usize {
    let gap = app.cfg.bottom_gap;
    if h >= need + gap { gap } else { 0 }
}

/// A board with fewer rows than this is short: the list gets the rows of space.
const SHORT: usize = 12;
/// A board with fewer columns than this is narrow: the titles get the room.
const NARROW: usize = 36;

/// The board's sizes, worked out once per frame: the list, the bottom rows and the
/// compact panel all draw from them.
struct Sizes {
    /// the board's width
    w: usize,
    /// its height above the bottom gap: everything at the bottom (the detail panel,
    /// message line, keys, scroll mark) sits on top of it
    h: usize,
    /// a short board gives the list the rows a tall one spends on space: no row of its
    /// own for the scroll mark, no blank line above the keys, and a message on the key row
    short: bool,
    /// a narrow one puts one space after the IDs, and (grouped) a task's age on its detail line
    narrow: bool,
    /// the rows under the list: the compact panel, the message and the keys
    footer: usize,
    /// the list's rows
    avail: usize,
    /// a short board's message goes on the key row
    msg_on_keys: bool,
}

impl Sizes {
    /// The sizes of a board in `area`, and the message rows above its keys.
    fn of(app: &App, area: Rect) -> (Sizes, Vec<Span<'static>>) {
        let (w, h) = (area.width as usize, area.height as usize);
        let short = h < SHORT;
        let narrow = w < NARROW;
        let footer = match (app.cfg.layout, short) {
            (Layout::Grouped, false) => 3,
            (Layout::Grouped, true) => 1,
            (Layout::Compact, false) => 7,
            (Layout::Compact, true) => 6,
        };
        // the rows above the list: the top line and a blank one, and on a tall board one
        // under it for the scroll mark
        let chrome = if short { 2 } else { 3 };
        let h = h - bottom_gap(app, h, 4 + footer);
        // the message (or the y/n question) wraps; its rows past the first come off the
        // list (while one row of it is left). On a short board it's one row, on the key
        // row unless something else is asked there
        let msg = bottom::message(app, w, if short { 1 } else { h.saturating_sub(chrome + 1 + footer) + 1 });
        let msg_on_keys = short && !msg.is_empty() && app.pending.is_none() && matches!(app.mode, Mode::Normal);
        let footer = match (short, msg_on_keys) {
            (true, true) => footer,
            (true, false) => footer + msg.len(),
            (false, _) => footer + msg.len().saturating_sub(1),
        };
        let avail = h.saturating_sub(chrome + footer).max(1);
        (Sizes { w, h, short, narrow, footer, avail, msg_on_keys }, msg)
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let full = f.area();
    let buf = f.buffer_mut();

    if matches!(app.mode, Mode::Overlay { .. } | Mode::Answer { .. } | Mode::Help { .. } | Mode::Tell { reply: true, .. }) {
        // the title, a blank line, at least one line and the foot
        let h = full.height as usize;
        app.height = (h - bottom_gap(app, h, 4)) as u16;
    }
    if let Mode::Overlay { title, lines, peek } = &app.mode {
        overlay::draw_overlay(buf, full, app.height as usize, title, lines, peek.is_some(), app.peek_failed);
        return;
    }
    if let Mode::Help { top } = &mut app.mode {
        *top = help::draw_help(buf, full, app.height as usize, *top);
        return;
    }
    if let Mode::Answer { title, prompt, .. } = &app.mode {
        overlay::draw_answer(buf, full, app.height as usize, title, prompt.as_ref());
        return;
    }
    if let Mode::Tell { id, text, reply: true } = &app.mode {
        let t = app.tasks.iter().find(|t| t.id == *id);
        let title = t.map_or(id.clone(), |t| format!("{}  {}", t.id, t.title));
        let question = t.and_then(|t| t.why.clone()).filter(|q| !q.is_empty()).unwrap_or("Waiting for you.".into());
        let (x, y) = overlay::draw_ask(buf, full, app.height as usize, &title, &question, text);
        f.set_cursor_position(Position::new(full.x + x as u16, full.y + y as u16));
        return;
    }

    let area = full;
    let (s, msg) = Sizes::of(app, area);
    app.height = s.h as u16;
    let now = crate::model::now();
    list::draw_list(buf, area, app, &s, now);

    // bottom: the compact layout's detail panel, a message line, the keys
    let cur = app.current();
    if let (Layout::Compact, Some(cur)) = (app.cfg.layout, cur) {
        list::draw_panel(buf, area, app, &s, cur, now);
    }
    if let Some(x) = bottom::draw_bottom(buf, area, app, &s, msg, cur) {
        f.set_cursor_position(Position::new(area.x + x as u16, area.y + s.h.saturating_sub(1) as u16));
    }
}

#[cfg(test)]
#[path = "tests/test_ui.rs"]
mod tests;
