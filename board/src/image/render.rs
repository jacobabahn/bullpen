//! Drawing the board's list as one image: the rows rows.rs describes (the same the
//! text list draws), a terminal row's height each, so the board scrolls the image by
//! whole rows as it scrolls the text. The type is real (font.rs), the marks are drawn
//! shapes in the status colours (rings, a check, a diamond), the selected task sits on
//! a rounded tint with its ID in a pill, and a task that needs you has a bar at its
//! left. The marks that turn on the text board stand still here. Nothing here touches
//! the terminal: given rows and a size in pixels, it returns an image and where each
//! row is.

use super::font::{Fonts, Weight};
use super::theme::{solid, Theme};
use crate::ui::rows::{Flag, ItemRow, Mark, Row, Rows, TaskRow};
use std::f32::consts::PI;
use std::ops::Range;
use tiny_skia::{FillRule, LineCap, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// The list's place on the terminal: its columns, and a cell's size in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Grid {
    pub cols: usize,
    pub cell_w: u32,
    pub cell_h: u32,
}

impl Grid {
    pub fn width(&self) -> u32 {
        self.cols as u32 * self.cell_w
    }
}

/// Where one row is in the image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// its index in the rows
    pub row: usize,
    /// its top, in pixels from the image's
    pub y: u32,
    pub h: u32,
    /// the task's or item's ID, on its own row
    pub id: Option<String>,
}

/// The rows drawn: the image (premultiplied RGBA), and where each row is.
pub struct Sheet {
    pub pixmap: Pixmap,
    /// for the mouse, which the image look doesn't take yet
    #[cfg_attr(not(test), allow(dead_code))]
    pub places: Vec<Place>,
}

/// Draw rows[range] as one image, `grid.cols` columns wide and a row a cell high.
pub fn render(rows: &Rows, range: Range<usize>, grid: Grid, theme: &Theme, fonts: &mut Fonts) -> Result<Sheet, String> {
    let range = range.start.min(rows.rows.len())..range.end.min(rows.rows.len());
    let h = grid.cell_h;
    let height = (range.len() as u32 * h).max(1);
    let mut pm = Pixmap::new(grid.width().max(1), height)
        .ok_or_else(|| format!("can't make an image {}×{} pixels", grid.width(), height))?;
    let idw = id_width(rows, &range, fonts, grid);
    let mut d = Draw { pm: &mut pm, fonts, theme, grid, idw };
    let mut places = Vec::new();
    // the selected task's block (its row and its details) sits on one rounded tint
    let mut block: Option<(f32, f32)> = None;
    for (j, i) in range.clone().enumerate() {
        let row = &rows.rows[i];
        let y = j as f32 * h as f32;
        let tinted = match row {
            Row::Task(t) => t.selected && t.asked.is_none(),
            Row::Detail(dr) => dr.selected,
            Row::Item(it) => it.selected && !it.asked,
            Row::ItemDetail(_) => true,
            _ => false,
        };
        block = match (block, tinted) {
            (Some((top, _)), true) if !row.is_entry() => Some((top, y + h as f32)),
            (b, true) => {
                d.tint(b);
                Some((y, y + h as f32))
            }
            (b, false) => {
                d.tint(b);
                None
            }
        };
        let id = match row {
            Row::Task(t) => Some(t.id.clone()),
            Row::Item(it) => Some(it.id.clone()),
            _ => None,
        };
        places.push(Place { row: i, y: j as u32 * h, h, id });
    }
    d.tint(block);
    for (j, i) in range.enumerate() {
        d.row(&rows.rows[i], j as f32 * h as f32);
    }
    Ok(Sheet { pixmap: pm, places })
}

/// The ID column's width in pixels: the widest ID drawn, so titles line up.
fn id_width(rows: &Rows, range: &Range<usize>, fonts: &Fonts, grid: Grid) -> f32 {
    let px = font_px(grid);
    rows.rows[range.clone()]
        .iter()
        .filter_map(|r| match r {
            Row::Task(t) => Some(fonts.width(&t.id, Weight::Bold, px)),
            Row::Item(i) => Some(fonts.width(&i.id, Weight::Bold, px)),
            _ => None,
        })
        .fold(0.0, f32::max)
}

/// The type's size: about the terminal's own in a cell this high.
pub fn font_px(grid: Grid) -> f32 {
    (grid.cell_h as f32 * 0.62).max(6.0)
}

struct Draw<'a> {
    pm: &'a mut Pixmap,
    fonts: &'a mut Fonts,
    theme: &'a Theme,
    grid: Grid,
    idw: f32,
}

impl Draw<'_> {
    fn c(&self) -> f32 {
        self.grid.cell_w as f32
    }
    fn h(&self) -> f32 {
        self.grid.cell_h as f32
    }
    fn px(&self) -> f32 {
        font_px(self.grid)
    }
    fn right(&self) -> f32 {
        self.grid.width() as f32 - 0.6 * self.c()
    }
    /// Where text starts (under the ID: column 3, as on the text board).
    fn text_x(&self) -> f32 {
        3.0 * self.c()
    }
    /// Where a title starts: past the ID column and a gap.
    fn title_x(&self) -> f32 {
        self.text_x() + self.idw + 0.6 * self.px() + 0.5 * self.px()
    }
    fn baseline(&self, y: f32) -> f32 {
        y + (self.h() + self.fonts.cap_height(self.px())) / 2.0
    }

    fn text(&mut self, s: &str, weight: Weight, x: f32, y: f32, max: f32, color: [u8; 4]) -> f32 {
        let px = self.px();
        let s = self.fonts.fit(s, weight, px, max.max(0.0));
        let base = self.baseline(y);
        self.fonts.draw(self.pm, &s, weight, px, x, base, color)
    }

    fn fill(&mut self, path: &Path, color: [u8; 4]) {
        self.pm.fill_path(path, &paint(color), FillRule::Winding, Transform::identity(), None);
    }

    fn stroke(&mut self, path: &Path, width: f32, color: [u8; 4]) {
        let stroke = Stroke { width, line_cap: LineCap::Round, ..Stroke::default() };
        self.pm.stroke_path(path, &paint(color), &stroke, Transform::identity(), None);
    }

    /// The selected task's (or item's) block: a rounded tint from its ID to the right edge.
    fn tint(&mut self, block: Option<(f32, f32)>) {
        let Some((top, bottom)) = block else { return };
        let x = self.text_x() - 0.35 * self.px();
        let color = self.theme.tint();
        if let Some(p) = rounded(x, top + 1.0, self.right() + 0.35 * self.px() - x, bottom - top - 2.0, 0.28 * self.h()) {
            self.fill(&p, color);
        }
    }

    fn row(&mut self, row: &Row, y: f32) {
        let (fg, dim) = (self.theme.text(), self.theme.dim());
        let left = self.c();
        let full = self.right() - left;
        match row {
            Row::Head { name, count } => {
                let end = self.text(name, Weight::Bold, left, y, full, fg);
                let gap = 0.5 * self.px();
                self.text(&count.to_string(), Weight::Regular, end + gap, y, self.right() - end - gap, dim);
            }
            Row::Task(t) => self.task(t, y),
            Row::Detail(d) => {
                self.flag(d.flag, y);
                let color = if d.selected || d.loud { fg } else { dim };
                let (age, rest) = crate::model::split_at_width(&d.text, d.lead);
                let x = self.text_x();
                let age_color = if d.stale { solid(self.theme.red) } else { dim };
                let weight = if d.stale { Weight::Bold } else { Weight::Regular };
                let end = self.text(&age, weight, x, y, self.right() - x, age_color);
                self.text(&rest, Weight::Regular, end, y, self.right() - end, color);
            }
            Row::Gap => {}
            Row::Empty { text, first } => {
                let (weight, color) = if *first { (Weight::Bold, fg) } else { (Weight::Regular, dim) };
                self.text(text, weight, left, y, full, color);
            }
            Row::Later { text, amber } => {
                let (head, rest) = text.split_at(text.find(" ·").unwrap_or(text.len()));
                let amber_c = solid(self.theme.yellow);
                let (hc, rc) = if *amber { (amber_c, amber_c) } else { (fg, dim) };
                let end = self.text(head, Weight::Bold, left, y, full, hc);
                self.text(rest, Weight::Regular, end, y, self.right() - end, rc);
            }
            Row::Item(i) => self.item(i, y),
            Row::ItemDetail(text) => {
                let x = self.text_x();
                self.text(text, Weight::Regular, x, y, self.right() - x, fg);
            }
        }
    }

    /// The bar beside a task that needs you (amber) or whose agent stopped (red): its
    /// rows' bars meet, so it runs down the task's block.
    fn flag(&mut self, flag: Option<Flag>, y: f32) {
        let Some(f) = flag else { return };
        let color = if f == Flag::Stopped { self.theme.red } else { self.theme.yellow };
        let w = (0.22 * self.c()).max(2.0);
        let rect = Rect::from_xywh(0.2 * self.c(), y, w, self.h());
        if let Some(p) = rect.map(PathBuilder::from_rect) {
            self.fill(&p, solid(color));
        }
    }

    fn task(&mut self, t: &TaskRow, y: f32) {
        self.flag(t.flag, y);
        let (cx, cy) = (1.5 * self.c(), y + self.h() / 2.0);
        self.mark(&t.mark, cx, cy);
        let tone = t.asked.map(|risky| solid(if risky { self.theme.red } else { self.theme.green }));
        let id_end = self.id(&t.id, t.selected || tone.is_some(), tone, y);
        if t.from_backlog {
            let r = 0.2 * self.px();
            self.backlog_mark(id_end + 0.25 * self.px() + r, cy, r);
        }
        // the right side: the risk, a coloured word, then the age
        let px = self.px();
        let mut right = self.right();
        let age_w = self.fonts.width(&t.age, Weight::Bold, px);
        let age_color = match tone {
            Some(c) => c,
            None if t.stale => solid(self.theme.red),
            None => self.theme.dim(),
        };
        let weight = if t.stale || tone.is_some() { Weight::Bold } else { Weight::Regular };
        self.text(&t.age, weight, right - self.fonts.width(&t.age, weight, px), y, age_w + 1.0, age_color);
        right -= age_w;
        if let Some(level) = &t.risk {
            let gap = if t.age.is_empty() { 0.0 } else { 0.7 * px };
            let w = self.fonts.width(level, Weight::Bold, px);
            let color = self.theme.risk(level);
            self.text(level, Weight::Bold, right - gap - w, y, w + 1.0, color);
            right -= gap + w;
        }
        let x = self.title_x();
        let color = match tone {
            Some(c) => c,
            None if t.quiet && !t.selected => self.theme.dim(),
            None => self.theme.text(),
        };
        let weight = if tone.is_some() { Weight::Bold } else { Weight::Regular };
        self.text(&t.title, weight, x, y, right - 0.8 * px - x, color);
    }

    /// An ID: dim, or (selected, or asked about in `tone`) in a pill. Returns where it ends.
    fn id(&mut self, id: &str, pill: bool, tone: Option<[u8; 4]>, y: f32) -> f32 {
        let px = self.px();
        let x = self.text_x();
        let w = self.fonts.width(id, Weight::Bold, px);
        if !pill {
            let dim = self.theme.dim();
            return self.text(id, Weight::Bold, x, y, w + 1.0, dim);
        }
        let (padx, h) = (0.35 * px, (self.h() * 0.78).min(px * 1.45));
        let color = tone.unwrap_or(self.theme.text());
        if let Some(p) = rounded(x - padx, y + (self.h() - h) / 2.0, w + 2.0 * padx, h, h / 2.0) {
            self.fill(&p, color);
        }
        let ink = solid(self.theme.bg);
        self.text(id, Weight::Bold, x, y, w + 1.0, ink);
        x + w + padx
    }

    fn item(&mut self, i: &ItemRow, y: f32) {
        let (cx, cy) = (1.5 * self.c(), y + self.h() / 2.0);
        let r = self.mark_r();
        if i.job {
            self.mark(&Mark::Job, cx, cy);
        } else if i.asks {
            let amber = solid(self.theme.yellow);
            let px = self.px();
            let w = self.fonts.width("?", Weight::Bold, px);
            self.text("?", Weight::Bold, cx - w / 2.0, y, w + 1.0, amber);
        } else if let Some(p) = diamond(cx, cy, r * 0.9) {
            let dim = self.theme.dim();
            self.stroke(&p, self.line(), dim);
        }
        let red = solid(self.theme.red);
        self.id(&i.id, i.selected || i.asked, i.asked.then_some(red), y);
        let x = self.title_x();
        let color = if i.asked { red } else { self.theme.text() };
        let weight = if i.asked { Weight::Bold } else { Weight::Regular };
        self.text(&i.title, weight, x, y, self.right() - x, color);
    }

    fn mark_r(&self) -> f32 {
        (0.27 * self.h()).min(0.95 * self.c())
    }

    fn line(&self) -> f32 {
        (self.mark_r() * 0.26).max(1.2)
    }

    /// A task's mark, still: each status's own shape in its colour.
    fn mark(&mut self, mark: &Mark, cx: f32, cy: f32) {
        let r = self.mark_r();
        let lw = self.line();
        let (fg, dim) = (self.theme.text(), self.theme.dim());
        let color = |st: &str| self.theme.status(st).unwrap_or(dim);
        match mark {
            // a command running, or a tour being written: an arc most of the way round
            Mark::Job => self.arc(cx, cy, r, -0.5 * PI, 1.0 * PI, lw * 1.2, fg),
            Mark::Tour(st) => {
                let c = color(st);
                self.arc(cx, cy, r, -0.5 * PI, 1.0 * PI, lw * 1.2, c)
            }
            Mark::Stuck(st) => self.status_mark(st, cx, cy, Some(solid(self.theme.yellow))),
            // working: a faint ring with a third of it drawn
            Mark::Working => {
                self.arc(cx, cy, r, 0.0, 2.0 * PI, lw, self.theme.tint_strong());
                self.arc(cx, cy, r, -0.5 * PI, 0.17 * PI, lw, fg);
            }
            Mark::Review => self.status_mark("review", cx, cy, None),
            Mark::Still(st) => self.status_mark(st, cx, cy, None),
        }
    }

    /// A status's mark, in its colour or `over` it.
    fn status_mark(&mut self, st: &str, cx: f32, cy: f32, over: Option<[u8; 4]>) {
        let r = self.mark_r();
        let lw = self.line();
        let dim = self.theme.dim();
        let color = over.or(self.theme.status(st)).unwrap_or(dim);
        let ink = solid(self.theme.bg);
        match st {
            "needs" => self.disc(cx, cy, r, color),
            "stopped" => {
                if let Some(p) = rounded(cx - r * 0.85, cy - r * 0.85, r * 1.7, r * 1.7, r * 0.35) {
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
                if let Some(p) = diamond(cx, cy, r * 1.1) {
                    self.fill(&p, color);
                }
            }
            "done" => self.check(cx, cy, r * 0.8, lw, color),
            "review" => {
                self.arc(cx, cy, r, 0.0, 2.0 * PI, lw, color);
                if let Some(p) = half(cx, cy, r) {
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
    fn backlog_mark(&mut self, cx: f32, cy: f32, r: f32) {
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

impl Theme {
    /// A working mark's ring: the foreground, a little stronger than the tint.
    fn tint_strong(&self) -> [u8; 4] {
        super::theme::faint(self.fg, 0.25)
    }
}

fn paint(color: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(color[0], color[1], color[2], color[3]);
    p.anti_alias = true;
    p
}

/// An arc as cubic curves, a quarter turn at most each.
fn arc_path(cx: f32, cy: f32, r: f32, a0: f32, sweep: f32) -> Option<Path> {
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

/// The left half of a disc (the review mark's ◐).
fn half(cx: f32, cy: f32, r: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let k = 0.5523 * r;
    pb.move_to(cx, cy - r);
    pb.cubic_to(cx - k, cy - r, cx - r, cy - k, cx - r, cy);
    pb.cubic_to(cx - r, cy + k, cx - k, cy + r, cx, cy + r);
    pb.close();
    pb.finish()
}

fn diamond(cx: f32, cy: f32, r: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy - r);
    pb.line_to(cx + r, cy);
    pb.line_to(cx, cy + r);
    pb.line_to(cx - r, cy);
    pb.close();
    pb.finish()
}

/// A rectangle with rounded corners.
fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
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

#[cfg(test)]
#[path = "tests/test_render.rs"]
mod tests;
