//! Chips and keycaps, drawn with render.rs's `Draw`: a word in its colour on a fully
//! round pill of that colour, faint (the header's counts); a ready task's risk, its
//! word dim after a dot of its colour, inside a faint line; and a key in a small
//! rounded cap with what it does after it, dim (the key row). Each is measured first,
//! so what's beside it can make room.

use super::font::{Fonts, Weight};
use super::render::Draw;
use super::theme::{bright, Rgb};

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
    // (the fill from the colour at its brightest: a light theme's amber is dark)
    let lit = bright(Rgb(color[0], color[1], color[2]));
    let fill = [lit.0, lit.1, lit.2, (0.18 * 255.0f32).round() as u8];
    d.rounded_fill(x, top, w, h, h / 2.0, fill);
    d.boxed(px, text, Weight::Bold, x + SIDE * px, top, h, color);
    x
}

/// How wide a risk's chip is: its dot, then its word, in type `px`.
pub fn risk_width(fonts: &Fonts, text: &str, px: f32) -> f32 {
    width(fonts, text, px) + DOT * px
}

/// A risk's level as a narrow board says it: its first letter, a capital.
pub fn initial(level: &str) -> String {
    level.chars().take(1).flat_map(char::to_uppercase).collect()
}

/// A risk's chip, whose right edge is at `right`, centred on `cy`: a dot in the
/// level's colour, then the word dim, inside a faint line of the foreground. The dot
/// alone has the colour, so a risk doesn't read as a status. Returns its left edge.
pub fn risk(d: &mut Draw, text: &str, px: f32, right: f32, cy: f32, color: [u8; 4]) -> f32 {
    let theme = d.theme();
    let w = risk_width(d.fonts(), text, px);
    let h = (1.5 * px).round();
    let (x, top) = (right - w, (cy - h / 2.0).round());
    d.rounded_edge(x, top, w, h, h / 2.0, 1.25, theme.cap_edge());
    let dot = dot_width(px);
    d.rounded_fill((x + SIDE * px).round(), top + ((h - dot) / 2.0).round(), dot, dot, dot / 2.0, color);
    d.boxed(px, text, Weight::Bold, x + (SIDE + DOT) * px, top, h, theme.dim());
    x
}

/// A risk's dot's width, in type `px`.
fn dot_width(px: f32) -> f32 {
    (0.5 * px).round()
}

/// The room a risk's dot takes before its word, of the word's size.
const DOT: f32 = 0.75;

/// An overdue age's chip, whose right edge is at `right`, centred on `cy`: the word
/// semibold in the pane's background on the colour, whole. Returns its left edge.
pub fn solid(d: &mut Draw, text: &str, px: f32, right: f32, cy: f32, color: [u8; 4], ink: [u8; 4]) -> f32 {
    let w = width(d.fonts(), text, px);
    let h = (1.5 * px).round();
    let (x, top) = (right - w, (cy - h / 2.0).round());
    d.rounded_fill(x, top, w, h, h / 2.0, color);
    d.boxed(px, text, Weight::Bold, x + SIDE * px, top, h, ink);
    x
}

/// How wide a key's cap and what it does after it are, in type `px`.
pub fn keycap_width(fonts: &Fonts, key: &str, what: &str, px: f32) -> f32 {
    let cap = cap_width(fonts, key, px);
    if what.is_empty() { cap } else { cap + 0.4 * px + fonts.width(what, Weight::Regular, px) }
}

/// A key's cap: its letter with a third of the type each side, square at least.
fn cap_width(fonts: &Fonts, key: &str, px: f32) -> f32 {
    (fonts.width(key, Weight::Bold, px) + 0.7 * px).max(1.45 * px)
}

/// A key in a cap from x, centred on cy: the key semibold in the foreground on the
/// foreground at 7% with a 1-pixel edge at 28%, its corners round by a third of the
/// type; then what it does, dim, 0.4 of the type after it. Returns where it ends.
pub fn keycap(d: &mut Draw, key: &str, what: &str, px: f32, x: f32, cy: f32) -> f32 {
    let theme = d.theme();
    let (w, h) = (cap_width(d.fonts(), key, px), (1.45 * px).round());
    let top = (cy - h / 2.0).round();
    let r = 0.3 * px;
    d.rounded_fill(x, top, w, h, r, theme.cap());
    d.rounded_edge(x, top, w, h, r, 1.0, theme.cap_edge());
    let kw = d.fonts().width(key, Weight::Bold, px);
    d.boxed(px, key, Weight::Bold, x + (w - kw) / 2.0, top, h, theme.text());
    if what.is_empty() {
        return x + w;
    }
    d.boxed(px, what, Weight::Regular, x + w + 0.4 * px, top, h, theme.dim())
}
