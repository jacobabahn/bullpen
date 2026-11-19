//! The list as pieces, so moving the selection sends a small image, not the list. The
//! list's rows are drawn once with nothing selected (the base image, ui/rows.rs's
//! `unselected`), and the selected task's block (its card, its whole title and all its
//! lines) is an image of its own over the cells it takes. The base image is placed
//! twice, as crops: the cells above the selected block, and the cells under it, moved
//! down by the whole cells the block grew by (the rows under it start that many cells
//! lower; the space left over is under the block). The base image is a window of the
//! list, kept while the rows shown are in it, so scrolling moves the crops too: the
//! page when the list changed where it's shown, and two pages at a page's steps when it
//! scrolls out. So the base image is sent only when what it shows of
//! the rows shown changes or the list scrolls out of the window, and a j or k sends the
//! selected block's image (or places one sent before) and moves the two crops. Every piece draws its rows
//! where they are in the whole list (render's `Cut::at`), so they match it to the bit.
//!
//! The list shows only whole rows: the crops end at the cell where the last whole row
//! ends, that cell is an image of its own (the tail), and the cells after it are empty.
//! With rows scrolled off above, the first cell is one too (the cap), so nothing of
//! theirs shows. Everything here is worked out from the rows laid
//! out (render.rs) and a cell's height; nothing touches the terminal.

use super::render::{card_below, fitting, lay, mark_at, Canvas, Laid, Marks};
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
    /// the rows laid out, the selected block with the space after it that makes the rows
    /// under it whole cells lower than in the base image
    pub laid: Laid,
    /// the rows with nothing selected, laid out
    pub base: Laid,
    /// the base image draws the rows as they are (`laid`): nothing selected is shown
    pub whole: bool,
    /// the base image: of the rows `win`, the first `win_at` pixels down the list, the
    /// pixels [win_y, win_y + win_h) from the first one's top (window)
    pub win: Range<usize>,
    /// the base image's rows the list shows (of `base`'s)
    pub shown: Range<usize>,
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

/// The selected block in the rows and in the base rows: its first row (the same in
/// both), and the row after it in each.
fn block(rows: &Rows, base: &Rows) -> Option<(usize, usize, usize)> {
    let s0 = rows.rows.iter().position(Row::is_selected)?;
    let after = |r: &Rows| s0 + 1 + r.rows[s0 + 1..].iter().take_while(|r| r.is_detail()).count();
    let (s1, b1) = (after(rows), after(base));
    // the rows around it are the same in both (else draw it all as the selected rows)
    (base.rows.len() >= b1 && rows.rows[s1..] == base.rows[b1..] && rows.rows[..s0] == base.rows[..s0]).then_some((s0, s1, b1))
}

/// Lay the list out as pieces in its `rows_n` cells, scrolled from `view.top` to keep
/// the selection in view; the base image's window is `prev` (pixels from the list's
/// top, and how many), the one on the terminal, while the rows shown are in it.
pub fn plan(view: &View, c: &Canvas, fonts: &Fonts, prev: Option<(u32, u32)>) -> Plan {
    let ch = c.grid.cell_h;
    let rows_n = view.rows_n;
    let room = rows_n as u32 * ch;
    let mut laid = lay(&view.rows, c, fonts);
    let base = lay(&view.base, c, fonts);
    let block = block(&view.rows, &view.base);
    // the block grows by whole cells, with room under it for its card's edge
    let mut grew = 0;
    if let Some((s0, s1, b1)) = block {
        // (and as much again clear under the card, so it isn't pressed on the next task)
        let (hs, hb) = (laid.tall(s0, s1) + 2 * card_below(c), base.tall(s0, b1));
        grew = hs.saturating_sub(hb).div_ceil(ch);
        laid.after[s1 - 1] = hb + grew * ch - laid.tall(s0, s1);
    }
    let top = scroll_by(&view.rows.rows, view.top, &laid.spans(), room);
    let end = fitting(&laid, top, room);
    let page = view.rows.rows[top..end].iter().filter(|r| r.is_entry()).count().max(1);
    let ys = (top..end).scan(0, |y, i| Some(std::mem::replace(y, *y + laid.span(i)))).collect();
    let scroll = Marks { up: top > 0, down: end < view.rows.rows.len() };
    let in_view = block.filter(|(s0, ..)| (top..end).contains(s0));
    let base = if in_view.is_some() { base } else { laid.clone() };
    let (win, win_y, win_h) = window(&base, top, room, prev);
    let win_at = base.tall(0, win.start);
    let at0 = base.tall(0, top) - win_at - win_y;
    let shown = top..fitting(&base, top, room);
    let mut p = Plan { top, end, page, laid, base, whole: in_view.is_none(), win, shown, win_at, win_y, win_h, crops: vec![], sel: None, tail: None, cap: None, ys, scroll };
    // the cell where the last whole row ends: what's past it mustn't show, and the rest
    // of that cell is an image of its own (the tail)
    let last = (p.laid.tall(top, end) / ch).min(rows_n as u32) as u16;
    let tail = |p: &mut Plan, past: u16| {
        if last >= past && last < rows_n {
            p.tail = Some((last, 1));
        }
    };
    let Some((s0, _, b1)) = in_view else {
        // nothing selected in view: the base image is the list
        if last > 0 {
            p.crops.push((at0, 0, last));
        }
        tail(&mut p, 0);
        cap(&mut p, ch);
        return p;
    };
    let y_s = p.y(s0);
    let first = y_s / ch;
    // (a task that needs you has its wash in the base image, which reaches under its
    // block as the card does: the cells it reaches are the block's image's too)
    let washed = matches!(&view.base.rows[s0], Row::Task(t) if t.flag.is_some());
    let under = if washed { card_below(c) } else { 0 };
    let past = ((y_s + p.base.tall(s0, b1) + under).div_ceil(ch) + grew).min(rows_n as u32);
    if first > 0 {
        p.crops.push((at0, 0, first as u16));
    }
    p.sel = Some((first as u16, (past - first) as u16));
    let (shift, past) = (grew as u16, past as u16);
    // the rows under it are the base image's, as many whole cells lower as it grew
    if last > past {
        p.crops.push((at0 + (past - shift) as u32 * ch, past, last - past));
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
    let starts: Vec<u32> = (0..n).scan(0, |at, i| Some(std::mem::replace(at, *at + base.span(i)))).collect();
    let first = (0..n).find(|&i| starts[i] + base.span(i) > y).unwrap_or(n).saturating_sub(1);
    let past = (0..n).find(|&i| starts[i] >= y + h).map_or(n, |i| (i + 1).min(n));
    let from = starts.get(first).copied().unwrap_or(0);
    (first..past, y - from, h)
}

impl Plan {
    /// The same plan with the base image's window [y, y + h) (what drawing ahead sends
    /// before the list scrolls into it; the crops are this plan's).
    pub fn with_window(&self, (y, h): (u32, u32)) -> Plan {
        let (win, win_y, win_h) = rows_of(&self.base, y, h);
        let win_at = self.base.tall(0, win.start);
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
    // a little past a row's end: the selected block's card reaches into the next row
    let reach = c.m.pad as i64 + 1;
    (p.top..p.end)
        .map(|i| (i, p.y(i) as i64))
        .filter(|&(i, y)| y < y1 && y + p.laid.span(i) as i64 + reach > y0)
        .map(|(i, y)| (i, y - y0))
        .collect()
}

#[cfg(test)]
#[path = "tests/test_pieces.rs"]
mod tests;
