use super::*;
use crate::font::{Fonts, Weight};
use tiny_skia::Pixmap;

static MONO: &[u8] = include_bytes!("../../fonts/JetBrainsMonoNL-Regular.ttf");
static INTER: &[u8] = include_bytes!("../../fonts/Inter-Regular.ttf");
static SEMIBOLD: &[u8] = include_bytes!("../../fonts/Inter-SemiBold.ttf");

/// The code font's em when its advance is a cell `cw` wide (the tour's `Sizes::code`).
fn code_px(cw: f32) -> f32 {
    cw / 0.6
}

/// Text drawn white from (x, base) on a clear image; its alpha by row.
fn drawn(fonts: &mut Fonts, text: &str, weight: Weight, px: f32, x: f32, base: f32) -> Vec<Vec<u8>> {
    let mut pm = Pixmap::new(400, 80).unwrap();
    fonts.draw(&mut pm, text, weight, px, (x, base), [255, 255, 255, 255]);
    let a: Vec<u8> = pm.pixels().iter().map(|p| p.alpha()).collect();
    a.chunks(400).map(|r| r.to_vec()).collect()
}

/// The alphas of a row's ink, in runs.
fn runs(row: &[u8]) -> Vec<Vec<u8>> {
    row.split(|a| *a == 0).filter(|r| !r.is_empty()).map(|r| r.to_vec()).collect()
}

#[test]
fn the_code_fonts_m_has_each_stem_solid_at_the_tours_cell() {
    // unhinted, at a 19×37 cell its middle stem was 192/255/192 and its outer ones 255/255/128
    let mut f = Fonts::mono();
    let px = code_px(19.0);
    let rows = drawn(&mut f, "m", Weight::Regular, px, 0.0, 40.0);
    let mid = &rows[(40.0 - 0.55 * px / 2.0) as usize];
    let stems = runs(mid);
    assert_eq!(stems.len(), 3, "{mid:?}");
    for s in stems {
        assert!(s.len() >= 2 && s.iter().all(|a| *a == 255), "{s:?}");
    }
}

#[test]
fn stems_are_solid_wherever_the_pen_falls() {
    for (face, px) in [(Fonts::builtin(), 13.6), (Fonts::builtin(), 27.2), (Fonts::mono(), code_px(16.0))] {
        let mut f = face;
        for x in [10.0, 10.25, 10.5, 10.75] {
            let rows = drawn(&mut f, "lIl", Weight::Regular, px, x, 40.0);
            let mid = &rows[(40.0 - 0.3 * px) as usize];
            for s in runs(mid) {
                assert!(s.iter().all(|a| *a == 255), "{px} at {x}: {s:?}");
            }
        }
    }
}

#[test]
fn a_letter_stands_on_the_baseline_and_reaches_the_x_height_on_whole_rows() {
    let mut f = Fonts::mono();
    let px = code_px(19.0); // an x-height of 17.4 pixels: it's 17 rows
    let rows = drawn(&mut f, "z", Weight::Regular, px, 0.0, 40.0);
    let full = |y: usize| rows[y].iter().filter(|a| **a == 255).count() > 6;
    let clear = |y: usize| rows[y].iter().all(|a| *a == 0);
    assert!(full(39) && clear(40), "the bar on the baseline");
    assert!(full(40 - 17) && clear(40 - 18), "the bar at the x-height");
}

/// Every point of every printable ASCII glyph, and how far the grid moves it.
fn furthest(data: &'static [u8], px: f32) -> f32 {
    let face = Face::parse(data, 0).unwrap();
    let upem = face.units_per_em() as f32;
    let scale = px / upem;
    let mut most: f32 = 0.0;
    for c in '!'..='~' {
        let gid = face.glyph_index(c).unwrap();
        let o = Outline::of(&face, gid).unwrap();
        for dx in [0.0, 0.25, 0.5, 0.75] {
            let g = o.fitted(scale, upem, dx, blues(&face));
            for p in o.contours().concat() {
                most = most.max((g.x.at(p.0) - (p.0 * scale + dx)).abs());
                most = most.max((g.y.at(p.1) - p.1 * scale).abs());
            }
        }
    }
    most
}

#[test]
fn no_point_of_a_letter_moves_a_pixel_and_a_half() {
    // an overshoot pulled onto its row and a bar rounded up move the counter under it
    // by both: the code font's h at a 19×37 cell, 1.33 pixels
    for (data, sizes) in [
        (MONO, [code_px(8.0), code_px(16.0), code_px(19.0)]),
        (INTER, [13.6, 27.2, 29.6]),
        (SEMIBOLD, [13.6, 27.2, 29.6]),
    ] {
        for px in sizes {
            let most = furthest(data, px);
            assert!(most < 1.5, "a point moved {most} pixels at {px}");
        }
    }
}

#[test]
fn a_stem_is_as_wide_as_it_rounds_to_up_from_four_tenths() {
    let fit = |w: f32| {
        let m = Map::fit(vec![Stem { lo: 10.0, hi: 10.0 + w, len: 100.0 }], 1.0, 0.0, &[], 1.0);
        m.at(10.0 + w) - m.at(10.0)
    };
    assert_eq!((fit(2.3), fit(2.45), fit(2.6), fit(0.3)), (2.0, 3.0, 3.0, 1.0));
}

#[test]
fn a_bar_on_a_blue_keeps_that_edge_on_its_row() {
    // a bar from 0 to 2.6 on the baseline, one from 15.2 to 17.4 under an x-height of 17.4
    let m = Map::fit(
        vec![Stem { lo: 0.0, hi: 2.6, len: 50.0 }, Stem { lo: 15.2, hi: 17.4, len: 50.0 }],
        1.0,
        0.0,
        &[0.0, 17.4, 23.1],
        1.0,
    );
    assert_eq!((m.at(0.0), m.at(2.6)), (0.0, 3.0));
    assert_eq!((m.at(15.2).round(), m.at(17.4)), (15.0, 17.0));
    assert_eq!(m.at(23.1), 23.0, "the capitals' height, no stem on it");
}

#[test]
fn between_fitted_points_is_straight_and_beyond_them_moves_with_the_nearest() {
    let m = Map { scale: 1.0, off: 0.0, at: vec![(1.0, 1.5), (3.0, 2.5)] };
    assert_eq!(m.at(2.0), 2.0);
    assert_eq!((m.at(0.0), m.at(5.0)), (0.5, 4.5));
    assert_eq!(Map { scale: 2.0, off: 0.5, at: vec![] }.at(3.0), 6.5);
}

#[test]
fn a_stem_that_would_cross_one_fitted_before_it_is_left_as_it_falls() {
    let mut m = Map { scale: 1.0, off: 0.0, at: vec![(10.0, 10.0), (12.0, 12.0)] };
    m.add(&[(12.5, 11.0), (14.0, 13.0)]);
    assert_eq!(m.at.len(), 2);
    m.add(&[(13.0, 13.0), (15.0, 15.0)]);
    assert_eq!(m.at.len(), 4);
}

#[test]
fn the_code_fonts_bold_has_its_stems_solid_too() {
    // drawn heavier (it has no bold of its own) before it's fitted, not stroked after:
    // a stroke put every edge back on a half pixel (128/255/255/128)
    let mut f = Fonts::mono();
    for cw in [19.0, 16.0] {
        let px = code_px(cw);
        let rows = drawn(&mut f, "m lIl", Weight::Bold, px, 0.0, 40.0);
        let mid = &rows[(40.0 - 0.55 * px / 2.0) as usize];
        let stems = runs(mid);
        assert_eq!(stems.len(), 6, "{cw}: {mid:?}");
        for s in stems {
            assert!(s.len() >= 3 && s.iter().all(|a| *a == 255), "{cw}: {s:?}");
        }
    }
}

#[test]
fn heavier_moves_each_edge_out_by_half() {
    // a clockwise square (TrueType's way round), 100 across, made 20 heavier
    let mut o = Outline { cmds: vec![] };
    o.move_to(0.0, 0.0);
    o.line_to(0.0, 100.0);
    o.line_to(100.0, 100.0);
    o.line_to(100.0, 0.0);
    o.close();
    let ps = o.heavier(20.0).points().concat();
    assert_eq!(ps, vec![P(-10.0, -10.0), P(-10.0, 110.0), P(110.0, 110.0), P(110.0, -10.0)]);
}

