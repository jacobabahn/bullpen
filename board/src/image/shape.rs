//! The shapes the image look draws with: the paint a colour fills with, arcs, a half
//! disc, a diamond and a rectangle with rounded corners, as tiny-skia paths. The marks
//! (render.rs), the cards and washes behind tasks, and the chips and keycaps (chip.rs)
//! are made of these.

use std::f32::consts::PI;
use tiny_skia::{Paint, Path, PathBuilder};

/// A colour (straight RGBA) to fill or stroke with, smoothed at its edges.
pub(super) fn paint(color: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(color[0], color[1], color[2], color[3]);
    p.anti_alias = true;
    p
}

/// An arc as cubic curves, a quarter turn at most each.
pub(super) fn arc_path(cx: f32, cy: f32, r: f32, a0: f32, sweep: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let n = (sweep.abs() / (PI / 2.0)).ceil().max(1.0) as usize;
    let step = sweep / n as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    pb.move_to(cx + r * a0.cos(), cy + r * a0.sin());
    for i in 0..n {
        let (s, e) = (a0 + step * i as f32, a0 + step * (i + 1) as f32);
        let (x0, y0, x3, y3) = (cx + r * s.cos(), cy + r * s.sin(), cx + r * e.cos(), cy + r * e.sin());
        pb.cubic_to(x0 - k * r * s.sin(), y0 + k * r * s.cos(), x3 + k * r * e.sin(), y3 - k * r * e.cos(), x3, y3);
    }
    pb.finish()
}

/// Half a disc, from angle a0 half way round (clockwise on screen): from the bottom
/// (a0 = π/2) it's the left half, the review mark's ◐.
pub(super) fn half(cx: f32, cy: f32, r: f32, a0: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let k = 4.0 / 3.0 * (PI / 8.0).tan();
    pb.move_to(cx + r * a0.cos(), cy + r * a0.sin());
    for i in 0..2 {
        let (s, e) = (a0 + PI / 2.0 * i as f32, a0 + PI / 2.0 * (i + 1) as f32);
        let (x0, y0, x3, y3) = (cx + r * s.cos(), cy + r * s.sin(), cx + r * e.cos(), cy + r * e.sin());
        pb.cubic_to(x0 - k * r * s.sin(), y0 + k * r * s.cos(), x3 + k * r * e.sin(), y3 - k * r * e.cos(), x3, y3);
    }
    pb.close();
    pb.finish()
}

pub(super) fn diamond(cx: f32, cy: f32, r: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy - r);
    pb.line_to(cx + r, cy);
    pb.line_to(cx, cy + r);
    pb.line_to(cx - r, cy);
    pb.close();
    pb.finish()
}

/// A rectangle with rounded corners.
pub(super) fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.5523 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}
