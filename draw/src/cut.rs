//! Cutting an image into pieces under the terminal's limit. Rex's client draws a kitty
//! graphics image of at most 10,000,000 bytes decoded (width × height × 4) and drops or
//! blurs a larger one (Rex's bug report "Kitty graphics: inline images of 10 MB or more
//! are silently dropped by the client", 21 Oct 2026), so no image the board or the tour
//! sends is over `MAX_BYTES`: one that would be is sent as pieces, each an image of its
//! own, and a placement of the whole is placed as the pieces' placements, each a run of
//! whole cells, so nothing moves on screen (`bin/kitty-limit-check` shows the limit).
//! The terminal also keeps only so many bytes of images in a pane (`QUOTA`), so the
//! board and the tour each keep theirs under `BUDGET`.
//!
//! An image is cut into strips its whole width, as tall as fit; only when a strip two
//! cells high is over the limit is it cut into pieces side by side too. Pieces overlap
//! by a cell, so every cell of a crop is inside one piece even when the crop doesn't
//! start on a cell's edge (the board's list, scrolled by rows that aren't whole cells).

use crate::kitty::Place;
use crate::png;
use tiny_skia::{IntRect, Pixmap};

/// The most bytes (decoded RGBA) an image sent is: a margin under Rex's 10,000,000.
pub const MAX_BYTES: usize = 9_500_000;

/// The bytes (decoded) of images Rex's terminal keeps in a pane: past it, an upload
/// deletes the oldest images not on screen, without a word to a program that sent with
/// q=2 (`bin/kitty-quota-check`: 14 images of 9,496,000 bytes kept, a 15th deleting the
/// first).
pub const QUOTA: usize = 128 << 20;

/// The most bytes (decoded) of images the board or the tour keeps on the terminal,
/// shown and hidden, about three quarters of `QUOTA`: each deletes what it keeps past
/// this itself before it sends more, so the terminal never deletes one for it.
pub const BUDGET: usize = 96_000_000;

/// A piece of an image: its pixels from (x, y), w by h.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Piece {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Is an image w by h pixels under the limit?
pub fn fits(w: u32, h: u32) -> bool {
    w as usize * h as usize * 4 <= MAX_BYTES
}

/// The rows of cells `ch` high a strip `w` pixels wide can have under the limit (0 when
/// not even one).
pub fn rows_fitting(w: u32, ch: u32) -> u32 {
    (MAX_BYTES / (4 * w.max(1) as usize * ch.max(1) as usize)) as u32
}

/// [from, to) of a length, `step` apart, each `step + over` long, the last to the end.
fn spans(len: u32, step: u32, over: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut at = 0;
    loop {
        if at + step + over >= len {
            out.push((at, len - at));
            return out;
        }
        out.push((at, step + over));
        at += step;
    }
}

/// The pieces an image `w` by `h` pixels is sent as, for a cell `(cw, ch)`: the image
/// itself when it's under the limit. None when even a piece two cells square is over.
pub fn pieces(w: u32, h: u32, (cw, ch): (u32, u32)) -> Option<Vec<Piece>> {
    if fits(w, h) {
        return Some(vec![Piece { x: 0, y: 0, w, h }]);
    }
    let (cw, ch) = (cw.max(1), ch.max(1));
    let (xs, ys) = if fits(w, 2 * ch) {
        // strips its whole width, as tall as fit, each a cell over the next
        let tall = (MAX_BYTES / (4 * w as usize)) as u32;
        (vec![(0, w)], spans(h, tall - ch, ch))
    } else {
        // two cells high, as wide as fit, each a cell over the next
        let wide = (MAX_BYTES / (4 * 2 * ch as usize)) as u32;
        if wide < 2 * cw {
            return None;
        }
        (spans(w, wide - cw, cw), spans(h, ch, ch))
    };
    Some(ys.iter().flat_map(|&(y, h)| xs.iter().map(move |&(x, w)| Piece { x, y, w, h })).collect())
}

/// An image ready to send: the PNG of each of its pieces, and the cell it was cut for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pngs {
    pub cell: (u32, u32),
    pub pieces: Vec<(Piece, Vec<u8>)>,
}

impl Pngs {
    /// A placement of the image as its pieces' placements (`split`).
    pub fn split(&self, p: &Place) -> Vec<(usize, Place)> {
        let cut: Vec<Piece> = self.pieces.iter().map(|(piece, _)| *piece).collect();
        split(p, &cut, self.cell)
    }
}

/// An image as the PNGs of its pieces for a cell `cell` pixels, or why it can't be cut.
pub fn encode(pm: &Pixmap, cell: (u32, u32)) -> Result<Pngs, String> {
    let (w, h) = (pm.width(), pm.height());
    let cut = pieces(w, h, cell).ok_or_else(|| format!("can't cut an image {w}×{h} pixels under the terminal's limit"))?;
    if cut.len() == 1 {
        return Ok(Pngs { cell, pieces: vec![(cut[0], png::encode(pm))] });
    }
    let pieces = cut
        .into_iter()
        .map(|p| {
            let rect = IntRect::from_xywh(p.x as i32, p.y as i32, p.w, p.h).ok_or("a piece out of the image")?;
            let part = pm.clone_rect(rect).ok_or("a piece out of the image")?;
            Ok((p, png::encode(&part)))
        })
        .collect::<Result<_, String>>()?;
    Ok(Pngs { cell, pieces })
}

/// The runs of a crop's cells (from `start` pixels, `n` cells `size` apart) in the same
/// span: (the span's start, the first cell, how many).
fn runs(starts: &[u32], start: u32, n: u16, size: u32) -> Vec<(u32, u16, u16)> {
    let mut out: Vec<(u32, u16, u16)> = Vec::new();
    for k in 0..n {
        let at = start + k as u32 * size;
        let span = starts.iter().copied().filter(|s| *s <= at).max().unwrap_or(0);
        match out.last_mut() {
            Some((s, _, m)) if *s == span => *m += 1,
            _ => out.push((span, k, 1)),
        }
    }
    out
}

/// A placement of the whole image as placements of its pieces: each with the piece's
/// index. The crop is pixel for pixel (`cols` cells of `cw`, `rows` of `ch`); a cell is
/// in the last piece that starts at or above it (and left of it), and the cells next to
/// each other in one piece are one placement. An image in one piece is placed as it is.
pub fn split(p: &Place, cut: &[Piece], (cw, ch): (u32, u32)) -> Vec<(usize, Place)> {
    if cut.len() <= 1 {
        return vec![(0, *p)];
    }
    let mut xs: Vec<u32> = cut.iter().map(|c| c.x).collect();
    let mut ys: Vec<u32> = cut.iter().map(|c| c.y).collect();
    xs.sort();
    xs.dedup();
    ys.sort();
    ys.dedup();
    let mut out = Vec::new();
    for (y, r0, rows) in runs(&ys, p.crop_y, p.rows, ch) {
        for (x, c0, cols) in runs(&xs, p.crop_x, p.cols, cw) {
            let Some(i) = cut.iter().position(|c| (c.x, c.y) == (x, y)) else { continue };
            let at = Place {
                col: p.col + c0,
                row: p.row + r0,
                crop_x: p.crop_x + c0 as u32 * cw - x,
                crop_y: p.crop_y + r0 as u32 * ch - y,
                crop_w: cols as u32 * cw,
                crop_h: rows as u32 * ch,
                cols,
                rows,
                ..*p
            };
            out.push((i, at));
        }
    }
    out
}

/// The width and height of each image an output uploads, from its PNG's header: what a
/// test checks against the limit.
pub fn uploaded(out: &str) -> Vec<(u32, u32)> {
    out.split("\x1b_Ga=t,f=100,")
        .skip(1)
        .filter_map(|cmd| {
            let data = cmd.split_once(';')?.1;
            let head = unbase64(data.get(..32)?)?;
            let n = |at: usize| u32::from_be_bytes(head[at..at + 4].try_into().expect("4 bytes"));
            Some((n(16), n(20)))
        })
        .collect()
}

fn unbase64(text: &str) -> Option<Vec<u8>> {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for quad in text.as_bytes().chunks(4) {
        let mut n = 0u32;
        for &c in quad {
            n = n << 6 | ABC.iter().position(|a| *a == c)? as u32;
        }
        n <<= 6 * (4 - quad.len()) as u32;
        out.extend_from_slice(&n.to_be_bytes()[1..1 + quad.len() * 3 / 4]);
    }
    Some(out)
}

#[cfg(test)]
#[path = "tests/test_cut.rs"]
mod tests;
