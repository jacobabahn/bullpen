//! The list's images for a frame, from its pieces (pieces.rs): the base image placed as
//! its crops, the selected block's image, the tail's and the cap's, the scroll marks, and a
//! placement of a mark's frames (sprite.rs) for each mark that moves. Each image is
//! drawn only when what it shows changed (its key); shown.rs writes what changed.

use super::font::Fonts;
use super::kitty::Place;
use super::pieces::{mark_y, rows_in, sprite_mark, Plan};
use super::render::{render, Canvas, Cut, Marks};
use super::shown::{Screen, Slot, Want};
use super::sprite::{self, Sprite};
use super::worker::Job;
use super::{meter, png, View};
use crate::model::TextSize;
use crate::ui::rows::{Mark, Row};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

/// What a frame's list images are drawn from and with.
pub struct Frame<'a> {
    pub view: &'a View,
    pub plan: &'a Plan,
    pub c: &'a Canvas,
    pub size: TextSize,
    /// is a row's mark left out of the images, for a sprite (pieces::bare)
    pub bare: &'a dyn Fn(&Row) -> bool,
    /// when a bare mark's animation began (ms since the board started), and the time now
    pub began: &'a dyn Fn(&Row) -> u128,
    pub ms: u128,
}

/// A key: what an image in this slot shows, with the cell size, the type size and the
/// colours it's drawn in.
pub fn key(slot: &Slot, f: &Frame, what: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (slot, f.c.grid, f.size, f.c.theme, what).hash(&mut h);
    h.finish()
}

/// The rows the base image draws: those with nothing selected, or the rows as they are;
/// of them, its window's.
fn base_rows<'a>(f: &Frame<'a>) -> &'a [Row] {
    let rows = if f.plan.whole { &f.view.rows } else { &f.view.base };
    &rows.rows[f.plan.win.clone()]
}

/// What the base image shows of the rows shown, but for which of its marks are left
/// out: when this changes, the image is drawn again anyway (marks done settling go
/// back in it).
pub fn base_shows(f: &Frame) -> u64 {
    let p = f.plan;
    let rows = if p.whole { &f.view.rows } else { &f.view.base };
    key(&Slot::List, f, (&rows.rows[p.top..p.end], p.laid.idw.to_bits()))
}

/// The base image as drawn: its key, its window (pixels from the list's top, and how
/// many), the layout of its rows (their spans, hashed, with the window and the rest of
/// what places them) and how each of them looks (hashed), from its first row.
#[derive(Debug, Clone, PartialEq)]
pub struct ListImage {
    pub key: u64,
    pub win: (u32, u32),
    layout: u64,
    first: usize,
    rows: Vec<u64>,
}

/// The base image a frame's plan would draw.
pub fn list_image(f: &Frame) -> ListImage {
    let p = f.plan;
    let one = |what: &dyn Fn(&mut std::collections::hash_map::DefaultHasher)| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        what(&mut h);
        h.finish()
    };
    let rows: Vec<u64> = base_rows(f).iter().map(|r| one(&|h| (r, (f.bare)(r)).hash(h))).collect();
    let spans: Vec<u32> = p.win.clone().map(|i| p.laid.heights[i]).collect();
    let layout = key(&Slot::List, f, (&p.win, p.win_at, p.win_y, p.win_h, spans, p.laid.idw.to_bits(), p.whole));
    let key = one(&|h| (layout, &rows).hash(h));
    ListImage { key, win: (p.win_at + p.win_y, p.win_h), layout, first: p.win.start, rows }
}

/// Does the base image `drawn` show what `now` would, of the rows shown (`shown`): the
/// same layout, and each of those rows the same? A row off screen that changed is drawn
/// when it comes on screen.
pub fn still(drawn: &ListImage, now: &ListImage, shown: std::ops::Range<usize>) -> bool {
    drawn.layout == now.layout && shown.into_iter().all(|i| drawn.rows.get(i - drawn.first) == now.rows.get(i - now.first))
}

/// The base image keyed `k` as the worker's thread draws it (worker.rs); its font is
/// the board's to say.
pub fn job(f: &Frame, k: u64) -> Job {
    let p = f.plan;
    let rs = if p.whole { &f.view.rows } else { &f.view.base };
    let bare = rs.rows.iter().map(f.bare).collect();
    let (rows, laid, range) = (rs.clone(), p.laid.clone(), p.win.clone());
    Job { key: k, rows, laid, range, at: p.win_at, y: p.win_y, h: p.win_h, bare, canvas: *f.c, font: None }
}

/// The slot's image drawn and made a PNG, or why it couldn't be drawn, as the message
/// says it.
pub fn encode(slot: &Slot, draw: impl FnOnce() -> Result<tiny_skia::Pixmap, String>) -> Result<Vec<u8>, String> {
    let drawn = meter::time(|| format!("draw {slot:?}"), draw).map_err(|e| format!("Drawing the list as text: {e}."))?;
    Ok(meter::time(|| format!("png {slot:?}"), || png::encode(&drawn)))
}

/// A placement in the list's block: on its cell row `row`, `rows` high, of the image's
/// pixels from `crop_y`, the list's whole width.
fn across(f: &Frame, p: u32, row: u16, rows: u16, crop_y: u32) -> Place {
    let (ch, w) = (f.c.grid.cell_h, f.c.grid.width());
    let v = f.view;
    Place { id: 0, p, col: v.col, row: v.row + row, crop_x: 0, crop_y, crop_w: w, crop_h: rows as u32 * ch, cols: v.cols, rows, z: 0 }
}

/// The list's images this frame, the ones `screen` doesn't show yet drawn; the base
/// image is the one keyed `k` (list_image's, or one drawn before that still does). With
/// `jobs`, a base image to draw is left to the worker's thread (worker.rs) instead.
pub fn wants(f: &Frame, k: u64, screen: &Screen, fonts: &mut Fonts, jobs: Option<&mut Vec<Job>>) -> Result<Vec<Want>, String> {
    let (p, c, v) = (f.plan, f.c, f.view);
    let ch = c.grid.cell_h;
    let mut wants = Vec::new();

    // the base image, as its crops
    let png = if screen.has(&Slot::List, k) {
        None
    } else {
        let rs = if p.whole { &v.rows } else { &v.base };
        match jobs {
            // drawn on the worker's thread (drawing ahead)
            Some(jobs) => {
                jobs.push(job(f, k));
                None
            }
            None => {
                let bare = |i: usize| (f.bare)(&rs.rows[i]);
                let cut = Cut { range: p.win.clone(), at: p.win_at, y: p.win_y, h: p.win_h, marks: Marks::default(), bare: &bare };
                Some(encode(&Slot::List, || render(rs, &p.laid, cut, c, fonts).map(|s| s.pixmap))?)
            }
        }
    };
    let places = p.crops.iter().enumerate().map(|(k, &(from, at, n))| across(f, k as u32 + 1, at, n, from)).collect();
    wants.push(Want { slot: Slot::List, key: k, png, places });

    // the selected block's cells, the tail's and the cap's, drawn from the rows as they are
    for (slot, cells) in [(Slot::Sel, p.sel), (Slot::Tail, p.tail), (Slot::Cap, p.cap)] {
        let Some((first, n)) = cells.filter(|(_, n)| *n > 0) else { continue };
        let what: Vec<_> = rows_in(p, c, first, n).into_iter().map(|(i, y)| (&v.rows.rows[i], y, (f.bare)(&v.rows.rows[i]))).collect();
        let k = key(&slot, f, (what, p.laid.idw.to_bits(), n));
        let png = if screen.has(&slot, k) {
            None
        } else {
            let bare = |i: usize| (f.bare)(&v.rows.rows[i]);
            let cut = Cut { range: p.top..p.end, at: p.at(), y: first as u32 * ch, h: n as u32 * ch, marks: Marks::default(), bare: &bare };
            Some(encode(&slot, || render(&v.rows, &p.laid, cut, c, fonts).map(|s| s.pixmap))?)
        };
        wants.push(Want { slot, key: k, png, places: vec![across(f, 1, first, n, 0)] });
    }

    // the scroll marks: ↑ in the list's first cell of its last column, ↓ in its last
    if p.scroll.up || p.scroll.down {
        let slot = Slot::Sprite(Sprite::Chevrons);
        let at = |n: u32, row: u16| Place {
            id: 0,
            p: n,
            col: v.col + v.cols.saturating_sub(1),
            row: v.row + row,
            crop_x: 0,
            crop_y: (n - 1) * ch,
            crop_w: c.grid.cell_w,
            crop_h: ch,
            cols: 1,
            rows: 1,
            z: 1,
        };
        let places = [(p.scroll.up, at(1, 0)), (p.scroll.down, at(2, v.rows_n.saturating_sub(1)))];
        wants.push(sprite_want(f, slot, places.into_iter().filter_map(|(on, at)| on.then_some(at)).collect(), screen, fonts)?);
    }

    // a placement of a mark's frames for each mark left out of the images
    let mut marks: BTreeMap<Mark, Vec<Place>> = BTreeMap::new();
    for i in p.top..p.end {
        let row = &v.rows.rows[i];
        let Some(mark) = sprite_mark(row).filter(|_| (f.bare)(row)) else { continue };
        let frame = sprite::frame(&mark, f.ms.saturating_sub((f.began)(row)));
        let Some(at) = sprite::at(c, mark_y(p, i, c), frame, v.rows_n) else { continue };
        let places = marks.entry(mark).or_default();
        let cols = sprite::cols(c).min(v.cols);
        places.push(Place {
            id: 0,
            p: places.len() as u32 + 1,
            col: v.col,
            row: v.row + at.row,
            crop_x: 0,
            crop_y: at.crop_y,
            crop_w: cols as u32 * c.grid.cell_w,
            crop_h: at.crop_h,
            cols,
            rows: at.rows,
            z: 1,
        });
    }
    for (mark, places) in marks {
        wants.push(sprite_want(f, Slot::Sprite(Sprite::Mark(mark)), places, screen, fonts)?);
    }
    Ok(wants)
}

/// A small image placed over the list: drawn once for the cell size and colours.
fn sprite_want(f: &Frame, slot: Slot, places: Vec<Place>, screen: &Screen, fonts: &mut Fonts) -> Result<Want, String> {
    let k = key(&slot, f, ());
    let png = match (&slot, screen.has(&slot, k)) {
        (Slot::Sprite(s), false) => Some(encode(&slot, || sprite::draw(s, f.c, fonts))?),
        _ => None,
    };
    Ok(Want { slot, key: k, png, places })
}
