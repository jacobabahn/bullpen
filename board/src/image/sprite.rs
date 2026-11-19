//! The image look's small images placed over the list, each sent once and then only
//! placed: the marks that move, and the scroll marks. A mark that turns (working, in
//! review, a command running, a tour being written) is a strip of its frames, at the
//! text board's pace; a mark that settles in when a task's status changes is a strip of
//! its sizes. Each mark on screen is a placement of its strip, and a tick changes only
//! which frame the placement crops: a few dozen bytes, never the list again. The list's
//! images leave those marks out (render.rs's `bare`).
//!
//! A strip's frames are tiles a few cells high, the mark at the same height in each,
//! with a cell's room above and below: a placement crops the cells the mark covers from
//! wherever its centre falls in a cell, so a mark that isn't on a cell's line (rows
//! needn't be whole cells) still lands where the list put it.

use super::font::Fonts;
use super::render::{mark_at, mark_reach, Canvas, Draw, Grid, Marks};
use crate::ui::rows::Mark;
use crate::ui::{self, MARK_MS, SPINNER_MS};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

/// A mark's tiles take the list's first columns, where its marks are: as many as reach
/// past its right edge at its largest (three at least).
pub fn cols(c: &Canvas) -> u16 {
    let right = mark_at(c).0 + mark_reach(c, POP_MAX);
    ((right / c.grid.cell_w as f32).ceil() as u16).max(3)
}

/// How long a mark settling in holds each of its sizes: 160 ms in all, growing past its
/// size and back.
pub const POP_MS: u128 = 40;
const POP: [f32; 4] = [0.55, 0.85, 1.12, 1.0];
const POP_MAX: f32 = 1.12;

/// What a small image shows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sprite {
    /// a mark, frame by frame: one that turns, or one settling in
    Mark(Mark),
    /// the scroll marks: ↑ in its top cell, ↓ in the one under
    Chevrons,
}

/// Does the mark turn (as on the text board)?
pub fn turns(mark: &Mark) -> bool {
    matches!(mark, Mark::Working | Mark::Review | Mark::Job | Mark::Tour(_))
}

/// A mark's frames, and how long each is held: the text board's (ui/mod.rs) for one that
/// turns, POP's for one settling in.
pub fn frames(mark: &Mark) -> (usize, u128) {
    match mark {
        Mark::Working => (ui::WORKING.len(), MARK_MS),
        Mark::Review => (ui::REVIEW.len(), MARK_MS),
        Mark::Job | Mark::Tour(_) => (ui::SPINNER.len(), SPINNER_MS),
        Mark::Stuck(_) | Mark::Still(_) => (POP.len(), POP_MS),
    }
}

/// How long a mark takes to settle in.
pub fn settling_ms() -> u128 {
    POP.len() as u128 * POP_MS
}

/// The frame of a mark `ms` after its animation began: a turning one's goes round, a
/// settling one's stops at its last.
pub fn frame(mark: &Mark, ms: u128) -> usize {
    let (n, each) = frames(mark);
    if turns(mark) {
        ui::frame_index(ms, n, each)
    } else {
        ((ms / each) as usize).min(n - 1)
    }
}

/// A tile's shape: its height, the row of pixels the mark's centre is on, how far the
/// mark reaches, and the cells a mark covers at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tile {
    h: u32,
    centre: u32,
    reach: u32,
}

fn tile(c: &Canvas) -> Tile {
    let ch = c.grid.cell_h;
    let reach = mark_reach(c, POP_MAX).ceil() as u32;
    // the cells from the mark's top to its bottom, wherever it starts in a cell
    let most = (2 * reach + 1).div_ceil(ch) + 1;
    Tile { h: (most + 1) * ch, centre: ch + reach, reach }
}

/// Draw a sprite: a mark's frames one under the other, each a tile `cols` cells wide; or
/// the two scroll marks, a cell each.
pub fn draw(sprite: &Sprite, c: &Canvas, fonts: &mut Fonts) -> Result<Pixmap, String> {
    let (cw, ch) = (c.grid.cell_w, c.grid.cell_h);
    match sprite {
        Sprite::Mark(mark) => {
            let t = tile(c);
            let (n, _) = frames(mark);
            let (x, y) = mark_at(c);
            // the mark's centre is as far into its pixel as the list's
            let frac = y.fract();
            let w = cols(c) as u32 * cw;
            let mut pm = Pixmap::new(w, n as u32 * t.h).ok_or("can't make a mark's image")?;
            let mut d = Draw::new(&mut pm, fonts, c, 0.0);
            for f in 0..n {
                let cy = (f as u32 * t.h + t.centre) as f32 + frac;
                if turns(mark) {
                    d.mark_frame(mark, x, cy, f as f32 / n as f32, 1.0);
                } else {
                    d.mark_frame(mark, x, cy, 0.0, POP[f]);
                }
            }
            Ok(pm)
        }
        Sprite::Chevrons => {
            // drawn as the list's last column draws them, in an image a column wide
            let one = Canvas { grid: Grid { cols: 1, ..c.grid }, ..*c };
            let mut pm = Pixmap::new(cw, 2 * ch).ok_or("can't make the scroll marks' image")?;
            let mut up = Pixmap::new(cw, ch).ok_or("can't make the scroll marks' image")?;
            Draw::new(&mut up, fonts, &one, 0.0).scroll_marks(Marks { up: true, down: false }, ch as f32);
            let mut down = Pixmap::new(cw, ch).ok_or("can't make the scroll marks' image")?;
            Draw::new(&mut down, fonts, &one, 0.0).scroll_marks(Marks { up: false, down: true }, ch as f32);
            let paint = PixmapPaint::default();
            pm.draw_pixmap(0, 0, up.as_ref(), &paint, Transform::identity(), None);
            pm.draw_pixmap(0, ch as i32, down.as_ref(), &paint, Transform::identity(), None);
            Ok(pm)
        }
    }
}

/// Where a mark's frame goes: the list's cell row it starts on, how many cells, and the
/// crop of the strip (its top and height in pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct At {
    pub row: u16,
    pub rows: u16,
    pub crop_y: u32,
    pub crop_h: u32,
}

/// Where frame `f` of a mark whose centre is `cy` pixels (whole) down the list goes, in
/// a list `rows_n` cells high; None when none of it is in the list.
pub fn at(c: &Canvas, cy: u32, f: usize, rows_n: u16) -> Option<At> {
    let ch = c.grid.cell_h;
    let t = tile(c);
    let first = cy.saturating_sub(t.reach) / ch;
    let last = (cy + t.reach + 1).div_ceil(ch).min(rows_n as u32);
    if first >= last {
        return None;
    }
    // the tile's pixel row at the top of the first cell
    let off = t.centre + first * ch - cy;
    Some(At { row: first as u16, rows: (last - first) as u16, crop_y: f as u32 * t.h + off, crop_h: (last - first) * ch })
}

#[cfg(test)]
#[path = "tests/test_sprite.rs"]
mod tests;
