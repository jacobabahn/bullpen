//! The marks render.rs's `Draw` draws: each status's own shape in its colour (a disc, a
//! rounded square, rings, a check, a diamond, a dotted ring), the marks that turn at a
//! turn of their way round and one settling in at its size (sprite.rs draws their
//! frames), the backlog's small circling arrow, and the list's scroll marks, small
//! chevrons in its last column. A child of render.rs, so it draws with `Draw`'s own
//! parts.

use super::Draw;
use crate::image::shape::{arc_path, diamond, half, rounded};
use crate::image::theme::solid;
use crate::ui::rows::Mark;
use std::f32::consts::PI;
use tiny_skia::PathBuilder;

/// Whether there are rows above the image's (↑ at its top right) and below (↓ at its bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Marks {
    pub up: bool,
    pub down: bool,
}

impl Draw<'_> {
    /// The scroll marks in the last column: ↑ at the top when rows are above, ↓ at the
    /// bottom of an image `height` high when rows are below.
    pub(in crate::image) fn scroll_marks(&mut self, marks: Marks, height: f32) {
        let cx = self.g.c.grid.width() as f32 - 0.28 * self.c();
        let half = 0.5 * self.m().line.min(self.g.c.grid.cell_h as f32);
        if marks.up {
            self.chevron(cx, half, -1.0);
        }
        if marks.down {
            self.chevron(cx, height - half, 1.0);
        }
    }

    /// A small chevron at (cx, cy), pointing down (dir 1) or up (-1).
    fn chevron(&mut self, cx: f32, cy: f32, dir: f32) {
        let r = 0.2 * self.c();
        let mut pb = PathBuilder::new();
        pb.move_to(cx - r, cy - dir * r / 2.0);
        pb.line_to(cx, cy + dir * r / 2.0);
        pb.line_to(cx + r, cy - dir * r / 2.0);
        if let Some(p) = pb.finish() {
            let dim = self.theme.dim();
            let lw = (0.12 * self.c()).max(1.2);
            self.stroke(&p, lw, dim);
        }
    }

    pub(super) fn mark_r(&self) -> f32 {
        self.g.mark_r() * self.scale
    }

    /// A mark as one frame of a sprite (sprite.rs): turned `turn` of the way round, at
    /// `scale` of its size.
    pub(in crate::image) fn mark_frame(&mut self, mark: &Mark, cx: f32, cy: f32, turn: f32, scale: f32) {
        self.scale = scale;
        self.mark(mark, cx, cy, turn);
        self.scale = 1.0;
    }

    pub(super) fn line(&self) -> f32 {
        (self.mark_r() * 0.3).max(1.2)
    }

    /// A task's mark: each status's own shape in its colour. The marks that turn are
    /// drawn `turn` of the way round (0: as they stand still).
    pub(super) fn mark(&mut self, mark: &Mark, cx: f32, cy: f32, turn: f32) {
        let r = self.mark_r();
        let lw = self.line();
        let (fg, dim) = (self.theme.text(), self.theme.dim());
        let color = |st: &str| self.theme.shape(st).unwrap_or(dim);
        let a0 = -0.5 * PI + 2.0 * PI * turn;
        match mark {
            // a command running, or a tour being written: an arc half the way round
            Mark::Job => self.arc(cx, cy, r, a0, 1.0 * PI, lw * 1.2, fg),
            Mark::Tour(st) => {
                let c = color(st);
                self.arc(cx, cy, r, a0, 1.0 * PI, lw * 1.2, c)
            }
            Mark::Stuck(st) => self.status_mark(st, cx, cy, self.theme.shape("needs"), 0.0),
            // working: a faint ring with a quarter of it drawn
            Mark::Working => {
                self.arc(cx, cy, r, 0.0, 2.0 * PI, lw, self.theme.tint_strong());
                self.arc(cx, cy, r, a0, 0.5 * PI, lw, fg);
            }
            Mark::Review => self.status_mark("review", cx, cy, None, turn),
            Mark::Still(st) => self.status_mark(st, cx, cy, None, 0.0),
        }
    }

    /// A status's mark, in its colour or `over` it; review's half turned `turn` round.
    fn status_mark(&mut self, st: &str, cx: f32, cy: f32, over: Option<[u8; 4]>, turn: f32) {
        let r = self.mark_r();
        let lw = self.line();
        let dim = self.theme.dim();
        let color = over.or(self.theme.shape(st)).unwrap_or(dim);
        let ink = solid(self.theme.bg);
        match st {
            // (a disc and a diamond look larger than a ring their size: a little under it)
            "needs" => self.disc(cx, cy, r * 0.88, color),
            "stopped" => {
                if let Some(p) = rounded(cx - r * 0.9, cy - r * 0.9, r * 1.8, r * 1.8, r * 0.35) {
                    self.fill(&p, color);
                }
            }
            "triage" => {
                self.arc(cx, cy, r, 0.0, 2.0 * PI, lw, color);
                self.disc(cx, cy, r * 0.38, color);
            }
            "ready" => {
                self.disc(cx, cy, r, color);
                self.check(cx, cy, r * 0.62, lw * 1.1, ink);
            }
            "finished" => {
                if let Some(p) = diamond(cx, cy, r) {
                    self.fill(&p, color);
                }
            }
            "done" => self.check(cx, cy, r * 0.8, lw, color),
            "review" => {
                self.arc(cx, cy, r, 0.0, 2.0 * PI, lw, color);
                if let Some(p) = half(cx, cy, r, 0.5 * PI + 2.0 * PI * turn) {
                    self.fill(&p, color);
                }
            }
            "working" => self.arc(cx, cy, r, 0.0, 2.0 * PI, lw, color),
            "todo" => {
                // a dotted ring: not started yet
                for k in 0..8 {
                    let a = k as f32 * PI / 4.0;
                    self.disc(cx + r * a.cos(), cy + r * a.sin(), lw * 0.62, color);
                }
            }
            "dropped" => {
                let mut pb = PathBuilder::new();
                pb.move_to(cx - r * 0.7, cy);
                pb.line_to(cx + r * 0.7, cy);
                if let Some(p) = pb.finish() {
                    self.stroke(&p, lw, color);
                }
            }
            _ => {}
        }
    }

    /// A task that came from the backlog: a small circling arrow.
    pub(super) fn backlog_mark(&mut self, cx: f32, cy: f32, r: f32) {
        let dim = self.theme.dim();
        let lw = (r * 0.35).max(1.0);
        self.arc(cx, cy, r, -0.3 * PI, 1.25 * PI, lw, dim);
        // the arrow's head, at the arc's start
        let (ax, ay) = (cx + r * (-0.3 * PI).cos(), cy + r * (-0.3 * PI).sin());
        let mut pb = PathBuilder::new();
        pb.move_to(ax - lw * 1.6, ay - lw * 0.6);
        pb.line_to(ax + lw * 0.4, ay + lw * 0.4);
        pb.line_to(ax + lw * 0.2, ay - lw * 1.8);
        pb.close();
        if let Some(p) = pb.finish() {
            self.fill(&p, dim);
        }
    }

    fn disc(&mut self, cx: f32, cy: f32, r: f32, color: [u8; 4]) {
        if let Some(p) = PathBuilder::from_circle(cx, cy, r) {
            self.fill(&p, color);
        }
    }

    fn check(&mut self, cx: f32, cy: f32, r: f32, lw: f32, color: [u8; 4]) {
        let mut pb = PathBuilder::new();
        pb.move_to(cx - r * 0.75, cy + r * 0.02);
        pb.line_to(cx - r * 0.2, cy + r * 0.55);
        pb.line_to(cx + r * 0.8, cy - r * 0.55);
        if let Some(p) = pb.finish() {
            self.stroke(&p, lw, color);
        }
    }

    /// An arc from angle a0 sweeping `sweep` radians (clockwise on screen).
    #[allow(clippy::too_many_arguments)]
    fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, sweep: f32, lw: f32, color: [u8; 4]) {
        if let Some(p) = arc_path(cx, cy, r, a0, sweep) {
            self.stroke(&p, lw, color);
        }
    }
}
