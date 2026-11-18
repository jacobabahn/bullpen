//! The list as pieces, so moving the selection sends a small image, not the list. The
//! list's rows are drawn once with nothing selected (the base image, ui/rows.rs's
//! `unselected`), and the selected task's block (its card, its whole title and all its
//! lines) is an image of its own over the cells it takes. The base image is placed
//! twice, as crops: the cells above the selected block, and the cells under it, moved
//! down by the whole cells the block grew by (the rows under it start that many cells
//! lower; the space left over is under the block). So the base image is sent only when
//! what it shows changes or the list scrolls, and a j or k sends the selected block's
//! image and moves the two crops.
//!
//! The list shows only whole rows: when the block pushes rows under it past the list's
//! end, the cell where the last whole row ends is an image of its own too (the tail),
//! and the cells after it are empty. Everything here is worked out from the rows laid
//! out (render.rs) and a cell's height; nothing touches the terminal.

use super::render::{card_below, fitting, lay, mark_at, Canvas, Laid, Marks};
use super::sprite::turns;
use super::font::Fonts;
use super::View;
use crate::ui::list::scroll_by;
use crate::ui::rows::{Mark, Row, Rows};

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
    /// the rows with nothing selected, laid out; the base image draws [top, base_end)
    pub base: Laid,
    pub base_end: usize,
    /// the base image draws the rows as they are (`laid`): nothing selected is shown
    pub whole: bool,
    /// the base image's crops: its first cell, the list's cell it's placed on, the cells
    pub crops: Vec<(u16, u16, u16)>,
    /// the selected block's cells (first, how many), and the tail's, drawn from `laid`
    pub sel: Option<(u16, u16)>,
    pub tail: Option<(u16, u16)>,
    /// where each row shown starts, in pixels from the list's top
    pub ys: Vec<u32>,
    pub scroll: Marks,
}

impl Plan {
    /// Where row i starts, in pixels from the list's top (a row shown).
    pub fn y(&self, i: usize) -> u32 {
        self.ys[i - self.top]
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
/// the selection in view.
pub fn plan(view: &View, c: &Canvas, fonts: &Fonts) -> Plan {
    let ch = c.grid.cell_h;
    let rows_n = view.rows_n;
    let room = rows_n as u32 * ch;
    let mut laid = lay(&view.rows, c, fonts);
    let base = lay(&view.base, c, fonts);
    let block = block(&view.rows, &view.base);
    // the block grows by whole cells, with room under it for its card's edge
    let mut grew = 0;
    if let Some((s0, s1, b1)) = block {
        let (hs, hb) = (laid.tall(s0, s1) + card_below(c), base.tall(s0, b1));
        grew = hs.saturating_sub(hb).div_ceil(ch);
        laid.after[s1 - 1] = hb + grew * ch - laid.tall(s0, s1);
    }
    let top = scroll_by(&view.rows.rows, view.top, &laid.spans(), room);
    let end = fitting(&laid, top, room);
    let page = view.rows.rows[top..end].iter().filter(|r| r.is_entry()).count().max(1);
    let ys = (top..end).scan(0, |y, i| Some(std::mem::replace(y, *y + laid.span(i)))).collect();
    let scroll = Marks { up: top > 0, down: end < view.rows.rows.len() };
    let mut p = Plan { top, end, page, base_end: top, whole: false, laid, base, crops: vec![], sel: None, tail: None, ys, scroll };
    let Some((s0, _, b1)) = block.filter(|(s0, ..)| (top..end).contains(s0)) else {
        // nothing selected in view: the base image is the list
        p.base = p.laid.clone();
        (p.base_end, p.whole) = (end, true);
        p.crops.push((0, 0, rows_n));
        return p;
    };
    p.base_end = fitting(&p.base, top, room);
    let y_s = p.y(s0);
    let first = y_s / ch;
    let past = ((y_s + p.base.tall(s0, b1)).div_ceil(ch) + grew).min(rows_n as u32);
    if first > 0 {
        p.crops.push((0, 0, first as u16));
    }
    p.sel = Some((first as u16, (past - first) as u16));
    let shift = grew as u16;
    let past = past as u16;
    if grew == 0 {
        // the rows under it are where the base image has them, and end as it does
        if past < rows_n {
            p.crops.push((past, past, rows_n - past));
        }
        return p;
    }
    // the cell where the last whole row ends: the rows past it are the base image's
    // next ones, which mustn't show
    let last = (p.laid.tall(top, end) / ch).min(rows_n as u32) as u16;
    if last > past {
        p.crops.push((past - shift, past, last - past));
    }
    if last >= past && last < rows_n {
        p.tail = Some((last, 1));
    }
    p
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
