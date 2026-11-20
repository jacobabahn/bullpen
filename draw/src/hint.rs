//! Putting a glyph's strokes on whole pixels, a light auto-hinting as FreeType's autofit
//! does it: the outline's upright stems and its bars, found from its edges, each get a
//! whole number of pixels and edges on pixel boundaries; the baseline, the x-height and
//! the capitals' height land on rows, with the bars that sit on them; the rest of the
//! outline moves with the nearest of these, straight between them. A glyph keeps its
//! shape and its advance (nothing is laid out differently), and its strokes are solid:
//! unhinted, a stem 2.5 pixels wide is drawn wherever it falls, as grey-edged columns
//! (the code font's "m" at a 19×37 cell was 192/255/192 across its middle stem).

use tiny_skia::{Path, PathBuilder};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

/// A stem this much of a pixel short of a whole number is rounded up (2.4 pixels is
/// 3): plain rounding left the type a little light at most sizes (about 0.93 of its
/// ink unhinted), this about as heavy as it was.
const ROUND_UP: f32 = 0.1;

/// The most a stem's control polygon leans and still runs along an axis (a run across
/// it to eight along).
const LEAN: f32 = 8.0;

/// The furthest an edge is from a blue and on it, of the em: an overshoot (a round
/// letter's top over the x-height), and no more (a pixel).
const OVERSHOOT: f32 = 0.025;

/// The least two edges run beside each other to be a stem, of the longer: the m's left
/// edge pairs with the far side of its stem, not with a notch at its top.
const BESIDE: f32 = 0.3;

/// A point of an outline, in font units (y up).
#[derive(Debug, Clone, Copy, PartialEq)]
struct P(f32, f32);

#[derive(Debug, Clone, Copy)]
enum Cmd {
    Move(P),
    Line(P),
    Quad(P, P),
    Cubic(P, P, P),
    Close,
}

/// A glyph's outline as the font has it, to be fitted to a size's pixels.
pub struct Outline {
    cmds: Vec<Cmd>,
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.cmds.push(Cmd::Move(P(x, y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.cmds.push(Cmd::Line(P(x, y)));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.cmds.push(Cmd::Quad(P(x1, y1), P(x, y)));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.cmds.push(Cmd::Cubic(P(x1, y1), P(x2, y2), P(x, y)));
    }
    fn close(&mut self) {
        self.cmds.push(Cmd::Close);
    }
}

/// The heights a face's letters stand on and reach (font units): the baseline, the
/// x-height and the capitals'.
pub fn blues(face: &Face) -> [f32; 3] {
    let cap = face.capital_height().unwrap_or(face.ascender()) as f32;
    let x = face.x_height().map(f32::from).unwrap_or(0.7 * cap);
    [0.0, x, cap]
}

impl Outline {
    /// A glyph's outline, if it has one.
    pub fn of(face: &Face, gid: GlyphId) -> Option<Outline> {
        let mut o = Outline { cmds: Vec::new() };
        face.outline_glyph(gid, &mut o)?;
        Some(o)
    }

    /// Its contours' points in order, on the curve and off it (the control polygon).
    fn contours(&self) -> Vec<Vec<P>> {
        let mut out: Vec<Vec<P>> = Vec::new();
        for c in &self.cmds {
            let ps: &[P] = match c {
                Cmd::Move(p) => {
                    out.push(vec![]);
                    std::slice::from_ref(p)
                }
                Cmd::Line(p) => std::slice::from_ref(p),
                Cmd::Quad(a, p) => &[*a, *p],
                Cmd::Cubic(a, b, p) => &[*a, *b, *p],
                Cmd::Close => &[],
            };
            if let Some(c) = out.last_mut() {
                c.extend_from_slice(ps);
            }
        }
        out.retain(|c| c.len() > 1);
        out
    }

    /// Where the outline goes at `scale` pixels to the unit, `dx` of a pixel right of
    /// the pen, its stems and bars on whole pixels and its letters on the face's blues.
    pub fn fitted(&self, scale: f32, upem: f32, dx: f32, blues: [f32; 3]) -> Grid {
        let contours = self.contours();
        let cw = clockwise(&contours);
        let stems = |axis| stems(&contours, axis, cw, scale, upem);
        let near = (OVERSHOOT * upem).min(1.0 / scale);
        let x = Map::fit(stems(Axis::X), scale, dx, &[], near);
        Grid { x, y: Map::fit(stems(Axis::Y), scale, 0.0, &blues, near) }
    }

    /// The outline through the grid, as a path in pixels from the pen's baseline (y down).
    pub fn path(&self, g: &Grid) -> Option<Path> {
        let at = |p: P| (g.x.at(p.0), -g.y.at(p.1));
        let mut pb = PathBuilder::new();
        for c in &self.cmds {
            match *c {
                Cmd::Move(p) => {
                    let (x, y) = at(p);
                    pb.move_to(x, y);
                }
                Cmd::Line(p) => {
                    let (x, y) = at(p);
                    pb.line_to(x, y);
                }
                Cmd::Quad(a, p) => {
                    let ((ax, ay), (x, y)) = (at(a), at(p));
                    pb.quad_to(ax, ay, x, y);
                }
                Cmd::Cubic(a, b, p) => {
                    let ((ax, ay), (bx, by), (x, y)) = (at(a), at(b), at(p));
                    pb.cubic_to(ax, ay, bx, by, x, y);
                }
                Cmd::Close => pb.close(),
            }
        }
        pb.finish()
    }
}

/// Whether the contours run clockwise, as TrueType's outer ones do, by their area.
fn clockwise(contours: &[Vec<P>]) -> bool {
    let mut area = 0.0;
    for c in contours {
        for (i, p) in c.iter().enumerate() {
            let q = c[(i + 1) % c.len()];
            area += p.0 * q.1 - q.0 * p.1;
        }
    }
    area < 0.0
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Axis {
    /// upright stems, fitted across x
    X,
    /// bars, fitted across y
    Y,
}

/// An edge of the outline along an axis: where it is across it, its extent along it,
/// and which side of the ink it's on (low: a stem's left, a bar's underside).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Edge {
    at: f32,
    from: f32,
    to: f32,
    low: bool,
}

/// A stem or a bar: its low and high edges across the axis, and how long the two run
/// beside each other (font units).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Stem {
    lo: f32,
    hi: f32,
    len: f32,
}

/// The outline's edges along an axis: runs of its control polygon that go (nearly)
/// straight along it, a pixel and a half long at least. A curve's extreme is one too:
/// its control points beside it are in line with it.
fn edges(contours: &[Vec<P>], axis: Axis, cw: bool, scale: f32) -> Vec<Edge> {
    let split = |p: P| match axis {
        Axis::X => (p.0, p.1),
        Axis::Y => (p.1, p.0),
    };
    // a clockwise outline has its ink on its right: going up, it's a stem's left edge
    let low_forward = cw == (axis == Axis::X);
    let mut out = Vec::new();
    for c in contours {
        // a run: the sum of across × length, its length, where it starts and ends, its way
        let mut run: Option<(f32, f32, f32, f32, bool)> = None;
        for i in 0..=c.len() {
            let piece = (i < c.len()).then(|| (split(c[i]), split(c[(i + 1) % c.len()])));
            let along = piece.filter(|((a0, b0), (a1, b1))| b1 != b0 && (a1 - a0).abs() * LEAN <= (b1 - b0).abs());
            let forward = along.map(|((_, b0), (_, b1))| b1 > b0);
            if let Some((sum, len, from, to, way)) = run {
                if forward != Some(way) {
                    if len * scale >= 1.5 {
                        out.push(Edge { at: sum / len, from: from.min(to), to: from.max(to), low: way == low_forward });
                    }
                    run = None;
                }
            }
            if let (Some(((a0, b0), (a1, b1))), Some(way)) = (along, forward) {
                let r = run.get_or_insert((0.0, 0.0, b0, b0, way));
                let len = (b1 - b0).abs();
                r.0 += (a0 + a1) / 2.0 * len;
                r.1 += len;
                r.3 = b1;
            }
        }
    }
    out
}

/// The outline's stems along an axis: each low edge paired with the nearest high edge
/// past it that runs beside it, a quarter of the em away at most; the narrowest pairs
/// first, an edge in one stem at most.
fn stems(contours: &[Vec<P>], axis: Axis, cw: bool, scale: f32, upem: f32) -> Vec<Stem> {
    let es = edges(contours, axis, cw, scale);
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for (i, l) in es.iter().enumerate().filter(|(_, e)| e.low) {
        for (j, h) in es.iter().enumerate().filter(|(_, e)| !e.low) {
            let w = h.at - l.at;
            let beside = l.to.min(h.to) - l.from.max(h.from);
            let longer = (l.to - l.from).max(h.to - h.from);
            if w > 0.0 && w <= 0.25 * upem && beside >= BESIDE * longer {
                pairs.push((w, i, j));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut used = vec![false; es.len()];
    let mut out = Vec::new();
    for (_, i, j) in pairs {
        if used[i] || used[j] {
            continue;
        }
        (used[i], used[j]) = (true, true);
        let (l, h) = (es[i], es[j]);
        out.push(Stem { lo: l.at, hi: h.at, len: l.to.min(h.to) - l.from.max(h.from) });
    }
    out
}

/// Where a coordinate of the outline goes, in pixels: the points fitted to the grid go
/// where they were put, a coordinate between two goes straight between them, and one
/// beyond them moves as the nearest did; with none, it's only scaled.
#[derive(Debug, Clone, PartialEq)]
pub struct Map {
    scale: f32,
    off: f32,
    /// (font units, pixels), rising in both
    at: Vec<(f32, f32)>,
}

/// Where an outline's x and y go.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub x: Map,
    pub y: Map,
}

impl Map {
    fn scaled(&self, u: f32) -> f32 {
        u * self.scale + self.off
    }

    pub fn at(&self, u: f32) -> f32 {
        let a = &self.at;
        let k = a.partition_point(|p| p.0 < u);
        match (k, a.len()) {
            (_, 0) => self.scaled(u),
            (0, _) => self.scaled(u) + a[0].1 - self.scaled(a[0].0),
            (k, n) if k == n => self.scaled(u) + a[n - 1].1 - self.scaled(a[n - 1].0),
            (k, _) => {
                let ((u0, p0), (u1, p1)) = (a[k - 1], a[k]);
                p0 + (p1 - p0) * (u - u0) / (u1 - u0)
            }
        }
    }

    /// The map that puts each stem on whole pixels, the longest first (one that would
    /// cross those before it is left as it falls), a stem as wide as it rounds to and
    /// where its middle rounds it, unless an edge is on a blue: then that edge is on the
    /// blue's row (a bar on the baseline stays on it, a bar at the x-height under it).
    /// Then each blue no stem is on goes on the row nearest it. An edge `near` a blue
    /// (font units) is on it.
    fn fit(mut stems: Vec<Stem>, scale: f32, off: f32, blues: &[f32], near: f32) -> Map {
        let mut m = Map { scale, off, at: Vec::new() };
        let on = |u: f32| blues.iter().find(|b| (u - **b).abs() < near).map(|b| (b * scale + off).round());
        stems.sort_by(|a, b| b.len.total_cmp(&a.len));
        for s in stems {
            let w = ((s.hi - s.lo) * scale + ROUND_UP).round().max(1.0);
            let lo = match (on(s.lo), on(s.hi)) {
                (Some(row), _) => row,
                (None, Some(row)) => row - w,
                _ => ((s.lo + s.hi) / 2.0 * scale + off - w / 2.0).round(),
            };
            m.add(&[(s.lo, lo), (s.hi, lo + w)]);
        }
        for b in blues {
            if !m.at.iter().any(|p| (p.0 - b).abs() < near) {
                m.add(&[(*b, (b * scale + off).round())]);
            }
        }
        m
    }

    /// Adds points fitted (rising), unless they'd cross those there.
    fn add(&mut self, ps: &[(f32, f32)]) {
        let (first, last) = (ps[0], ps[ps.len() - 1]);
        let k = self.at.partition_point(|p| p.0 < first.0);
        let below = k == 0 || (self.at[k - 1].0 < first.0 && self.at[k - 1].1 <= first.1);
        let above = k == self.at.len() || (self.at[k].0 > last.0 && self.at[k].1 >= last.1);
        if below && above {
            self.at.splice(k..k, ps.iter().copied());
        }
    }
}

#[cfg(test)]
#[path = "tests/test_hint.rs"]
mod tests;
