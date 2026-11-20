//! `_sharpness_sample`: the sample `bin/kitty-sharpness-check` places several ways, to
//! see where drawn text loses its sharpness on screen. A PNG on standard output, black
//! with white on it: a line of code drawn as the tour draws it (`Ink::cells`, at the
//! cell's size, on its grid), then a row of one-pixel lines (upright at the left, across
//! at the right), then a row of checkerboards (squares one pixel across at the left, two
//! at the right). The image can be bigger than the cells it's drawn on (the check's
//! image a pixel over its cells): the patterns fill it, the text stays on the grid.

use super::ink::{Face, Ink, Sizes, Type};
use crate::args::{pos, Args, Nargs};
use bullpen_core::fail::{Fail, Res};
use bullpen_draw::font::Fonts;
use bullpen_draw::theme::{Rgb, Theme};
use std::io::Write;
use tiny_skia::Pixmap;

const ARGS: &[crate::args::Arg] = &[
    pos("cell_w", Nargs::One, "a cell's width, in pixels"),
    pos("cell_h", Nargs::One, "a cell's height, in pixels"),
    pos("w", Nargs::One, "the image's width, in pixels"),
    pos("h", Nargs::One, "the image's height, in pixels"),
    pos("text", Nargs::Some, "the line of code, drawn on the first row"),
];

pub fn cmd_sharpness_sample(argv: &[String]) -> Res<i32> {
    let a = Args::parse("_sharpness_sample", argv, ARGS);
    let num = |name: &str| -> Res<u32> {
        let v = a.arg(name).unwrap_or_default();
        v.parse()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| Fail(format!("{name} isn't a number of pixels: {v}")))
    };
    let (cell, w, h) = ((num("cell_w")?, num("cell_h")?), num("w")?, num("h")?);
    let text = a.args("text").join(" ");
    let pm = sample(cell, w, h, &text).ok_or_else(|| Fail::from("can't make an image that size"))?;
    std::io::stdout().write_all(&bullpen_draw::png::encode(&pm))?;
    Ok(0)
}

/// The sample, w by h pixels, on cells `cell` pixels across and high.
pub fn sample(cell: (u32, u32), w: u32, h: u32, text: &str) -> Option<Pixmap> {
    let mut ty = Type::new(Fonts::builtin());
    let sz = Sizes::of(cell, &mut ty);
    let th = Theme {
        fg: Rgb(255, 255, 255),
        bg: Rgb(0, 0, 0),
        ..Theme::default()
    };
    let mut ink = Ink::new(w, h, &mut ty, th, sz)?;
    ink.rect(0.0, 0.0, w as f32, h as f32, [0, 0, 0, 255]);
    let cols = (w / cell.0) as usize;
    ink.cells(text, Face::Mono, 0.0, 0.0, cols, [255, 255, 255, 255]);
    let mut pm = ink.pm;
    for y in cell.1.min(h)..h {
        for x in 0..w {
            if pattern(x, y - cell.1, w / 2, cell.1) {
                pm.pixels_mut()[(y * w + x) as usize] = tiny_skia::PremultipliedColorU8::from_rgba(255, 255, 255, 255)?;
            }
        }
    }
    Some(pm)
}

/// Whether pixel (x, y) of the patterns (y from their top) is white: in their first row
/// a line upright every third pixel left of `mid`, across right of it; under that, a
/// checkerboard of one-pixel squares left of `mid`, of two-pixel squares right of it.
fn pattern(x: u32, y: u32, mid: u32, row: u32) -> bool {
    match (y < row, x < mid) {
        (true, true) => x % 3 == 0,
        (true, false) => y % 3 == 0,
        (false, true) => (x + y) % 2 == 0,
        (false, false) => (x / 2 + y / 2) % 2 == 0,
    }
}

#[cfg(test)]
#[path = "tests/test_sample.rs"]
mod tests;
