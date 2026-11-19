//! The list as pieces, so moving the selection sends a small image, not the list. The
//! list's rows are drawn once with nothing selected (the base image, ui/rows.rs's
//! `unselected`), and the selected task's block (its card and its lines) is an image of
//! its own over the cells it's in. A block is laid out the same selected or not
//! (render.rs), so there's one layout, and nothing under the block moves: the base image
//! is placed as two crops, the cells above the block's and the cells under them. The
//! base image is a window of the list, kept while the rows shown are in it, so scrolling
//! moves the crops too: the page when the list changed where it's shown, and two pages
//! at a page's steps when it scrolls out. So the base image is sent only when what it
//! shows of the rows shown changes or the list scrolls out of the window, and a j or k
//! sends the selected block's image (or places one sent before) and moves the two
//! crops. Every piece draws its rows where they are in the whole list (render's
//! `Cut::at`), so they match it to the bit.
//!
//! The list shows only whole rows: the crops end at the cell where the last whole row
//! ends, that cell is an image of its own (the tail), and the cells after it are empty.
//! With rows scrolled off above, the first cell is one too (the cap), so nothing of
//! theirs shows. Everything here is worked out from the rows laid
//! out (render.rs) and a cell's height; nothing touches the terminal.

use super::render::{fitting, lay, mark_at, Canvas, Laid, Marks};
use super::sprite::turns;
use super::font::Fonts;
use super::View;
use crate::ui::list::scroll_by;
use crate::ui::rows::{Mark, Row, Rows};
use std::ops::Range;

/// The list's block of cells, as pieces.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// the rows shown, [top, end), and the tasks a page has
    pub top: usize,
    pub end: usize,
    pub page: usize,
    /// the rows laid out: the same with nothing selected
    pub laid: Laid,
    /// the base image draws the rows as they are: nothing selected is shown
    pub whole: bool,
    /// the base image: of the rows `win`, the first `win_at` pixels down the list, the
    /// pixels [win_y, win_y + win_h) from the first one's top (window)
    pub win: Range<usize>,
    pub win_at: u32,
    pub win_y: u32,
    pub win_h: u32,
    /// the base image's crops: the pixel of it they start at, the list's cell they're
    /// placed on, the cells
    pub crops: Vec<(u32, u16, u16)>,
    /// the selected block's cells (first, how many), the tail's, and the cap's, drawn
    /// from `laid`
    pub sel: Option<(u16, u16)>,
    pub tail: Option<(u16, u16)>,
    pub cap: Option<(u16, u16)>,
    /// where each row shown starts, in pixels from the list's top
    pub ys: Vec<u32>,
    pub scroll: Marks,
}

impl Plan {
    /// Where row i starts, in pixels from the list's top (a row shown).
    pub fn y(&self, i: usize) -> u32 {
        self.ys[i - self.top]
    }

    /// How far the first row shown is from the whole list's first (render's `Cut::at`).
    pub fn at(&self) -> u32 {
        self.laid.tall(0, self.top)
    }
}

/// The selected block in the rows, [first, past): its task's (or item's) row and the
/// lines under it. None when the rows with nothing selected aren't the same rows but
/// for the selection (then the list is drawn whole, as the rows are).
fn block(rows: &Rows, base: &Rows) -> Option<(usize, usize)> {
    let s0 = rows.rows.iter().position(Row::is_selected)?;
    let s1 = s0 + 1 + rows.rows[s0 + 1..].iter().take_while(|r| r.is_detail()).count();
    let same = |(a, b): (&Row, &Row)| match (a, b) {
        (Row::Task(a), Row::Task(b)) => (&a.title, a.yours, &a.age, &a.risk) == (&b.title, b.yours, &b.age, &b.risk),
        (Row::Item(a), Row::Item(b)) => a.title == b.title,
        (Row::Detail(a), Row::Detail(b)) => (&a.text, a.lines, a.lead, a.stale) == (&b.text, b.lines, b.lead, b.stale),
        (Row::ItemDetail(a, n, _), Row::ItemDetail(b, m, _)) => (a, n) == (b, m),
        _ => false,
    };
    let around = rows.rows.len() == base.rows.len() && rows.rows[..s0] == base.rows[..s0] && rows.rows[s1..] == base.rows[s1..];
    (around && rows.rows[s0..s1].iter().zip(&base.rows[s0..s1]).all(same)).then_some((s0, s1))
}

/// Lay the list out as pieces in its `rows_n` cells, scrolled from `view.top` to keep
/// the selection in view; the base image's window is `prev` (pixels from the list's
/// top, and how many), the one on the terminal, while the rows shown are in it.
pub fn plan(view: &View, c: &Canvas, fonts: &Fonts, prev: Option<(u32, u32)>) -> Plan {
    let ch = c.grid.cell_h;
    let rows_n = view.rows_n;
    let room = rows_n as u32 * ch;
    let laid = lay(&view.rows, c, fonts);
    let top = scroll_by(&view.rows.rows, view.top, &laid.heights, room);
    let end = fitting(&laid, top, room);
    let page = view.rows.rows[top..end].iter().filter(|r| r.is_entry()).count().max(1);
    let ys = (top..end).scan(0, |y, i| Some(std::mem::replace(y, *y + laid.heights[i]))).collect();
    let scroll = Marks { up: top > 0, down: end < view.rows.rows.len() };
    let in_view = block(&view.rows, &view.base).filter(|(s0, _)| (top..end).contains(s0));
    let (win, win_y, win_h) = window(&laid, top, room, prev);
    let win_at = laid.tall(0, win.start);
    let at0 = laid.tall(0, top) - win_at - win_y;
    let mut p = Plan { top, end, page, laid, whole: in_view.is_none(), win, win_at, win_y, win_h, crops: vec![], sel: None, tail: None, cap: None, ys, scroll };
    // the cell where the last whole row ends: what's past it mustn't show, and the rest
    // of that cell is an image of its own (the tail)
    let last = (p.laid.tall(top, end) / ch).min(rows_n as u32) as u16;
    let tail = |p: &mut Plan, past: u16| {
        if last >= past && last < rows_n {
            p.tail = Some((last, 1));
        }
    };
    let Some((s0, s1)) = in_view else {
        // nothing selected in view: the base image is the list
        if last > 0 {
            p.crops.push((at0, 0, last));
        }
        tail(&mut p, 0);
        cap(&mut p, ch);
        return p;
    };
    // the cells the block is in (its card ends where its last row does): the base
    // image's are above them and under them, where they were
    let y_s = p.y(s0);
    let first = (y_s / ch) as u16;
    let past = (y_s + p.laid.tall(s0, s1)).div_ceil(ch).min(rows_n as u32) as u16;
    if first > 0 {
        p.crops.push((at0, 0, first));
    }
    p.sel = Some((first, past - first));
    if last > past {
        p.crops.push((at0 + past as u32 * ch, past, last - past));
    }
    tail(&mut p, past);
    cap(&mut p, ch);
    p
}

/// With rows scrolled off above the list, its first cell is an image of its own (the
/// cap), drawn from the rows shown: the base image has the rows above too, and what of
/// theirs reaches down (a wash, a bar) mustn't show.
fn cap(p: &mut Plan, ch: u32) {
    if p.top == 0 {
        return;
    }
    if let Some(i) = p.crops.iter().position(|&(_, at, _)| at == 0) {
        let (from, _, n) = p.crops.remove(i);
        if n > 1 {
            p.crops.insert(i, (from + ch, 1, n - 1));
        }
        p.cap = Some((0, 1));
    }
}

/// The base image's window of the rows laid out, for a list scrolled to `top` with
/// `room` pixels: its rows, and the pixels from the first one's top it shows. `prev`
/// (the window on the terminal) while the rows shown are in it. Scrolled out of it, two
/// pages at a page's steps (`grid`), so scrolling on sends the image again a page on,
/// not at each row, and the window a page wants is the same however it got there. With
/// no window (the list changed where it's shown), the page alone, so a change sends what
/// it did when the image was the page.
fn window(base: &Laid, top: usize, room: u32, prev: Option<(u32, u32)>) -> (Range<usize>, u32, u32) {
    let y0 = base.tall(0, top);
    let total = base.tall(0, base.heights.len());
    let fits = |&(y, h): &(u32, u32)| y <= y0 && (y0 + room).min(total) <= y + h;
    let (y, h) = match prev {
        Some(w) if fits(&w) => w,
        Some(_) => grid(y0, room),
        None => (y0, room),
    };
    rows_of(base, y, h)
}

/// The window of two pages, at a page's steps, that a page starting `y0` pixels down the
/// list is in.
pub fn grid(y0: u32, room: u32) -> (u32, u32) {
    let room = room.max(1);
    (y0 / room * room, 2 * room)
}

/// The window [y, y + h) of the rows laid out, cut to the list's end: its rows (those it
/// shows, and one more each way for what reaches in), the pixels from the first one's
/// top it starts at, and its height.
fn rows_of(base: &Laid, y: u32, h: u32) -> (Range<usize>, u32, u32) {
    let n = base.heights.len();
    let total = base.tall(0, n).max(1);
    let y = y.min(total - 1);
    let h = h.min(total - y).max(1);
    let starts: Vec<u32> = (0..n).scan(0, |at, i| Some(std::mem::replace(at, *at + base.heights[i]))).collect();
    let first = (0..n).find(|&i| starts[i] + base.heights[i] > y).unwrap_or(n).saturating_sub(1);
    let past = (0..n).find(|&i| starts[i] >= y + h).map_or(n, |i| (i + 1).min(n));
    let from = starts.get(first).copied().unwrap_or(0);
    (first..past, y - from, h)
}

impl Plan {
    /// The same plan with the base image's window [y, y + h) (what drawing ahead sends
    /// before the list scrolls into it; the crops are this plan's).
    pub fn with_window(&self, (y, h): (u32, u32)) -> Plan {
        let (win, win_y, win_h) = rows_of(&self.laid, y, h);
        let win_at = self.laid.tall(0, win.start);
        Plan { win, win_at, win_y, win_h, ..self.clone() }
    }
}

/// Is a row's mark left out of the list's images, for a sprite to place: one that turns
/// (with motion), or a task's settling in (`settling`, its ID).
pub fn bare(row: &Row, motion: bool, settling: &dyn Fn(&str) -> bool) -> bool {
    motion
        && match row {
            Row::Task(t) => turns(&t.mark) || settling(&t.id),
            Row::Item(i) => i.job,
            _ => false,
        }
}

/// The mark a sprite draws for a row whose mark is bare.
pub fn sprite_mark(row: &Row) -> Option<Mark> {
    match row {
        Row::Task(t) => Some(t.mark.clone()),
        Row::Item(i) if i.job => Some(Mark::Job),
        _ => None,
    }
}

/// Where row i's mark's centre is, in whole pixels from the list's top.
pub fn mark_y(p: &Plan, i: usize, c: &Canvas) -> u32 {
    p.y(i) + mark_at(c).1.floor() as u32
}

/// The rows of `p.laid` an image of the list's cells [first, first + n) shows, each with
/// where it starts in it: what that image's key hashes.
pub fn rows_in(p: &Plan, c: &Canvas, first: u16, n: u16) -> Vec<(usize, i64)> {
    let ch = c.grid.cell_h as i64;
    let (y0, y1) = (first as i64 * ch, (first + n) as i64 * ch);
    (p.top..p.end)
        .map(|i| (i, p.y(i) as i64))
        .filter(|&(i, y)| y < y1 && y + p.laid.heights[i] as i64 > y0)
        .map(|(i, y)| (i, y - y0))
        .collect()
}

#[cfg(test)]
#[path = "tests/test_pieces.rs"]
mod tests;
