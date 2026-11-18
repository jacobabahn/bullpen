//! Chips and keycaps, drawn with render.rs's `Draw`: a word in its colour on a fully
//! round pill of that colour, faint (a ready task's risk, the header's counts), and a
//! key in a small rounded cap with what it does after it, dim (the key row). Each is
//! measured first, so what's beside it can make room.

use super::font::{Fonts, Weight};
use super::render::Draw;

/// A chip's word is this far from each of its ends, of the word's size.
const SIDE: f32 = 0.45;

/// How wide a chip with this word is, in type `px`.
pub fn width(fonts: &Fonts, text: &str, px: f32) -> f32 {
    fonts.width(text, Weight::Bold, px) + 2.0 * SIDE * px
}

/// A chip whose right edge is at `right`, centred on `cy`: the word semibold in its
/// colour on that colour at 18%, a word and a half high. Returns its left edge.
pub fn chip(d: &mut Draw, text: &str, px: f32, right: f32, cy: f32, color: [u8; 4]) -> f32 {
    let w = width(d.fonts(), text, px);
    let h = (1.5 * px).round();
    let (x, top) = (right - w, (cy - h / 2.0).round());
    let fill = [color[0], color[1], color[2], (0.18 * 255.0f32).round() as u8];
    d.rounded_fill(x, top, w, h, h / 2.0, fill);
    d.boxed(px, text, Weight::Bold, x + SIDE * px, top, h, color);
    x
}
