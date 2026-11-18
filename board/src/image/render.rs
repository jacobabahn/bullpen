//! Drawing the board's list as an image: the rows rows.rs describes (the same the text
//! list draws), one under the other, each as tall as the type size makes it (a cell
//! for small type, more for medium and large), so a row needn't be a whole cell; the
//! image itself is the list's block of cells. The type is real (font.rs), the marks
//! are drawn shapes in the status colours (rings, a check, a diamond), the selected
//! task sits on a card (a faint fill and edge, its mark inside) with its whole title,
//! and a task that needs you has a bar at its left. The title is the loudest thing on
//! a row: the ID, the age and the group's name are a detail's size. The marks that move are left out when the
//! image look places them over the list (sprite.rs draws their frames with `Draw`). The
//! list's last column has the scroll marks. Nothing here touches the terminal: given
//! rows and a size in pixels, it lays them out and returns an image, or a window of
//! pixels of them (pieces.rs), drawn as that part of the whole would be.

use super::font::{Fonts, Weight};
use super::theme::{solid, Theme};
use crate::model::TextSize;
use crate::ui::rows::{Flag, ItemRow, Mark, Row, Rows, TaskRow};
use std::f32::consts::PI;
use std::ops::Range;
use tiny_skia::{FillRule, LineCap, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// An image's place on the terminal: its columns, and a cell's size in pixels.
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

/// The type's sizes and the space it gets, in pixels, for a text size and a cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// a task's title, the top line
    pub title: f32,
    /// a detail line, a task's ID and age, a group's name, the message and the keys
    pub detail: f32,
    /// a title line's height, and a detail line's
    pub line: f32,
    pub detail_line: f32,
    /// the space above a task (or an item), and above a heading
    pub pad: f32,
    pub head_pad: f32,
    /// the space between groups
    pub gap: f32,
    /// the space between a title's last line and the task's first detail line
    pub title_gap: f32,
    /// how far the selected card reaches past its first and last lines (a pixel inside
    /// them for small type, whose rows are a cell each)
    pub inset: f32,
}

impl Metrics {
    /// Small is the terminal's row: every row a cell, as the text list's. Medium and large
    /// set the type larger than a cell holds and give the rows the height it needs, a
    /// detail smaller than a title, and room: about three quarters of a title's size
    /// above each task, a little between its title and its lines, and the selected card
    /// a quarter of it clear of the type.
    pub fn of(size: TextSize, cell_h: u32) -> Metrics {
        let ch = cell_h as f32;
        let (t, d) = match size {
            TextSize::Small => (0.62, 0.62),
            // a terminal's own type is about 0.8 of its cell's height
            TextSize::Medium => (0.80, 0.70),
            TextSize::Large => (0.92, 0.80),
        };
        let (title, detail) = ((t * ch).max(6.0), (d * ch).max(6.0));
        if size == TextSize::Small {
            let (line, detail_line, gap) = (ch, ch, ch);
            return Metrics { title, detail, line, detail_line, pad: 0.0, head_pad: 0.0, gap, title_gap: 0.0, inset: -1.0 };
        }
        Metrics {
            title,
            detail,
            line: (1.40 * title).round(),
            detail_line: (1.42 * detail).round(),
            pad: (0.74 * title).round(),
            head_pad: (0.5 * title).round(),
            gap: (0.45 * ch).round(),
            title_gap: (0.06 * title).round(),
            inset: (0.28 * title).round(),
        }
    }
}

/// What an image is drawn on and in: its grid, the type's sizes and the colours.
#[derive(Debug, Clone, Copy)]
pub struct Canvas {
    pub grid: Grid,
    pub m: Metrics,
    pub theme: Theme,
}

impl Canvas {
    pub fn new(grid: Grid, size: TextSize, theme: Theme) -> Canvas {
        Canvas { grid, m: Metrics::of(size, grid.cell_h), theme }
    }

    fn c(&self) -> f32 {
        self.grid.cell_w as f32
    }

    /// Where text ends: short of the last column, which has the list's scroll marks.
    pub fn right(&self) -> f32 {
        self.grid.width() as f32 - 1.6 * self.c()
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

/// The rows laid out: each one's height in pixels and the space left after it (the
/// selected block's, so the rows under it move by whole cells: pieces.rs), a task's (or
/// an item's) title as the lines it's drawn on, and the ID column's width.
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    pub heights: Vec<u32>,
    pub after: Vec<u32>,
    titles: Vec<Vec<String>>,
    pub idw: f32,
}

impl Laid {
    /// The pixels row i takes: its height and the space after it.
    pub fn span(&self, i: usize) -> u32 {
        self.heights[i] + self.after[i]
    }

    /// Every row's span.
    pub fn spans(&self) -> Vec<u32> {
        (0..self.heights.len()).map(|i| self.span(i)).collect()
    }

    /// Rows [a, b)'s spans, in pixels.
    pub fn tall(&self, a: usize, b: usize) -> u32 {
        (a..b.min(self.heights.len())).map(|i| self.span(i)).sum()
    }
}

/// The most lines the selected task's title wraps to; any other title is cut to one.
const TITLE_LINES: usize = 3;

/// Lay the rows out: a title cut to its row, or the selected one's wrapped, and how tall
/// each row is.
pub fn lay(rows: &Rows, c: &Canvas, fonts: &Fonts) -> Laid {
    let idw = id_width(rows, fonts, c.m.detail);
    let g = Geo { c: *c, idw };
    let mut titles = Vec::new();
    let mut heights = Vec::new();
    for row in &rows.rows {
        let (title, selected, weight, right) = match row {
            Row::Task(t) => (&t.title, t.selected, title_weight(t.asked.is_some()), g.task_right(t, fonts)),
            Row::Item(i) => (&i.title, i.selected, title_weight(i.asked), c.right()),
            _ => {
                titles.push(vec![]);
                heights.push(g.height(row, 1));
                continue;
            }
        };
        let px = c.m.title;
        let x = g.title_x();
        let first = right - x;
        let lines = if selected {
            fonts.wrap(title, weight, px, (first, c.right() - x), TITLE_LINES)
        } else {
            vec![fonts.fit(title, weight, px, first)]
        };
        heights.push(g.height(row, lines.len().max(1)));
        titles.push(lines);
    }
    let after = vec![0; heights.len()];
    Laid { heights, after, titles, idw }
}

/// How many columns of text a detail line holds, from under the ID to the right edge:
/// the image's type is narrower than the terminal's, so its detail lines wrap later
/// than the text list's. Measured on a typical line, with a little to spare.
pub fn detail_cols(c: &Canvas, fonts: &Fonts) -> usize {
    const SAMPLE: &str = "Asks: should the header show the time too? 3 files, +13 −16 · Not toured yet.";
    let room = c.right() - Geo { c: *c, idw: 0.0 }.text_x();
    let each = fonts.width(SAMPLE, Weight::Regular, c.m.detail) / SAMPLE.chars().count() as f32;
    (room / (each * 1.08)).max(1.0) as usize
}

/// The rows from `top` that fit in `room` pixels whole: [top, end). One at least, cut if
/// it must be.
pub fn fitting(laid: &Laid, top: usize, room: u32) -> usize {
    let mut used = 0;
    let mut end = top;
    while end < laid.heights.len() && (end == top || used + laid.span(end) <= room) {
        used += laid.span(end);
        end += 1;
    }
    end
}

/// How far the selected card reaches under its block's last row, in whole pixels with
/// one for its smoothed edge: pieces.rs leaves this room under the block.
pub fn card_below(c: &Canvas) -> u32 {
    c.m.inset.max(0.0).ceil() as u32 + 1
}

/// Whether there are rows above the image's (↑ at its top right) and below (↓ at its bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Marks {
    pub up: bool,
    pub down: bool,
}

/// The pixels drawn past a window's edges, and cut off (render).
const EDGE: u32 = 4;

/// What of the laid-out rows an image shows: rows[range], the first at the top of the
/// layout, of which the pixels [y, y + h); the scroll marks; and the rows whose marks
/// are left out (`bare`), for sprite.rs to place over them.
pub struct Cut<'a> {
    pub range: Range<usize>,
    pub y: u32,
    pub h: u32,
    pub marks: Marks,
    pub bare: &'a dyn Fn(usize) -> bool,
}

impl Cut<'_> {
    /// All of rows[range], from their top, in an image `h` high, every mark drawn.
    pub fn rows(range: Range<usize>, h: u32) -> Cut<'static> {
        Cut { range, y: 0, h, marks: Marks::default(), bare: &|_| false }
    }
}

/// Draw the rows `cut` says, one under the other, as `laid` lays them out, as an image
/// the grid's columns wide and `cut.h` pixels high, with the scroll marks.
pub fn render(rows: &Rows, laid: &Laid, cut: Cut, c: &Canvas, fonts: &mut Fonts) -> Result<Sheet, String> {
    let range = cut.range.start.min(rows.rows.len())..cut.range.end.min(rows.rows.len());
    let (width, height) = (c.grid.width().max(1), cut.h.max(1));
    let made = || format!("can't make an image {width}×{height} pixels");
    // drawn with EDGE pixels more above and below, then cut to the window: a shape cut
    // by an image's edge is smoothed there differently, and the window's rows must look
    // as they do in any other
    let mut pm = Pixmap::new(width, height + 2 * EDGE).ok_or_else(made)?;
    let mut d = Draw::new(&mut pm, fonts, c, laid.idw);
    let mut places = Vec::new();
    let mut y = 0;
    for i in range.clone() {
        let id = match &rows.rows[i] {
            Row::Task(t) => Some(t.id.clone()),
            Row::Item(it) => Some(it.id.clone()),
            _ => None,
        };
        places.push(Place { row: i, y, h: laid.heights[i], id });
        y += laid.span(i);
    }
    // the window: the rows are drawn where they are in the layout, moved up by its top
    // (whole pixels), so a row's edges fall on the same fractions of a pixel in any image
    d.shift = cut.y as f32 - EDGE as f32;
    let at = |p: &Place| p.y as f32;
    // the selected task's block (its row and its details) sits on one card
    let mut block: Option<(f32, f32)> = None;
    for p in &places {
        let row = &rows.rows[p.row];
        let (y, bottom) = (at(p), at(p) + p.h as f32);
        let tinted = match row {
            Row::Task(t) => t.selected,
            Row::Detail(dr) => dr.selected,
            Row::Item(it) => it.selected,
            Row::ItemDetail(_) => true,
            _ => false,
        };
        block = match (block, tinted) {
            (Some((top, _)), true) if !row.is_entry() => Some((top, bottom)),
            (b, true) => {
                d.tint(b);
                Some((if row.is_entry() { y + c.m.pad } else { y }, bottom))
            }
            (b, false) => {
                d.tint(b);
                None
            }
        };
    }
    d.tint(block);
    for p in &places {
        let (y, h) = (at(p), p.h as f32);
        // a row outside the window draws nothing (its tint and bar may reach in, above)
        if y + h + c.m.pad + EDGE as f32 >= cut.y as f32 && y <= (cut.y + height + EDGE) as f32 {
            d.row(&rows.rows[p.row], &laid.titles[p.row], y, h, (cut.bare)(p.row));
        }
    }
    let mut pm = pm.clone_rect(tiny_skia::IntRect::from_xywh(0, EDGE as i32, width, height).ok_or_else(made)?).ok_or_else(made)?;
    Draw::new(&mut pm, fonts, c, laid.idw).scroll_marks(cut.marks, height as f32);
    Ok(Sheet { pixmap: pm, places })
}

/// The ID column's width in pixels: the widest ID in the list, so titles line up as it scrolls.
fn id_width(rows: &Rows, fonts: &Fonts, px: f32) -> f32 {
    rows.rows
        .iter()
        .filter_map(|r| match r {
            Row::Task(t) => Some(fonts.width(&t.id, Weight::Bold, px)),
            Row::Item(i) => Some(fonts.width(&i.id, Weight::Bold, px)),
            _ => None,
        })
        .fold(0.0, f32::max)
}

/// Where a task's (or item's) mark is in its row: across from the image's left edge,
/// down from the row's top.
pub(super) fn mark_at(c: &Canvas) -> (f32, f32) {
    (1.5 * c.c(), c.m.pad + c.m.line / 2.0)
}

/// How far a mark reaches from its centre at `scale` (Geo::mark_reach).
pub(super) fn mark_reach(c: &Canvas, scale: f32) -> f32 {
    Geo { c: *c, idw: 0.0 }.mark_reach(scale)
}

fn title_weight(asked: bool) -> Weight {
    if asked { Weight::Bold } else { Weight::Regular }
}

/// Where things go across a row, and how tall a row is.
#[derive(Clone, Copy)]
struct Geo {
    c: Canvas,
    idw: f32,
}

impl Geo {
    /// A mark's radius.
    fn mark_r(&self) -> f32 {
        (0.27 / 0.62 * self.c.m.title).min(0.95 * self.c.c())
    }

    /// How far a mark reaches from its centre, at `scale` (a mark settling in grows past
    /// its size: sprite.rs), with its stroke and a pixel for its edge.
    pub(super) fn mark_reach(&self, scale: f32) -> f32 {
        1.15 * self.mark_r() * scale + (self.mark_r() * 0.26).max(1.2) + 2.0
    }

    /// Where text starts (under the ID: column 3, as on the text board, or past a larger
    /// type's mark and a little space).
    fn text_x(&self) -> f32 {
        let c = self.c.c();
        (3.0 * c).max(1.5 * c + self.mark_r() + 0.4 * self.c.m.title)
    }

    /// Where a title starts: past the ID column and a gap, narrower on a board under 44
    /// columns, where every pixel of title counts.
    fn title_x(&self) -> f32 {
        let narrow = self.c.grid.cols + 1 < 44;
        self.text_x() + self.idw + if narrow { 0.5 } else { 0.8 } * self.c.m.title
    }

    /// The width a task's age (and risk) take at the right, as `task` draws them.
    fn right_side(&self, t: &TaskRow, fonts: &Fonts) -> f32 {
        let px = self.c.m.title;
        let mut w = fonts.width(&t.age, Weight::Bold, self.c.m.detail);
        if let Some(level) = &t.risk {
            let gap = if t.age.is_empty() { 0.0 } else { 0.5 * self.c.m.detail };
            w += gap + fonts.width(level, Weight::Bold, px);
        }
        w
    }

    /// Where a task's title's first line must end: short of its risk and age.
    fn task_right(&self, t: &TaskRow, fonts: &Fonts) -> f32 {
        self.c.right() - self.right_side(t, fonts) - 0.8 * self.c.m.title
    }

    /// A row's height in whole pixels, with its title on `lines` lines.
    fn height(&self, row: &Row, lines: usize) -> u32 {
        let m = &self.c.m;
        let h = match row {
            Row::Head { .. } => m.head_pad + m.line,
            Row::Task(_) | Row::Item(_) => m.pad + lines as f32 * m.line + m.title_gap,
            Row::Later { .. } => m.pad + m.line,
            Row::Detail(_) | Row::ItemDetail(_) => m.detail_line,
            Row::Gap => m.gap,
            Row::Empty { .. } => m.line,
        };
        h.round() as u32
    }
}

pub(super) struct Draw<'a> {
    pm: &'a mut Pixmap,
    fonts: &'a mut Fonts,
    theme: Theme,
    g: Geo,
    /// the type being drawn, and the height of the line it's centred in
    px: f32,
    line_h: f32,
    /// the marks' size, 1 but while one settles in (sprite.rs)
    scale: f32,
    /// how far up what's drawn goes: the top of the window of the rows drawn (render)
    shift: f32,
}

impl<'a> Draw<'a> {
    pub(super) fn new(pm: &'a mut Pixmap, fonts: &'a mut Fonts, c: &Canvas, idw: f32) -> Draw<'a> {
        Draw { pm, fonts, theme: c.theme, g: Geo { c: *c, idw }, px: c.m.title, line_h: c.m.line, scale: 1.0, shift: 0.0 }
    }

    /// Draw in this type from here on, centred in lines this high.
    pub(super) fn set(&mut self, px: f32, line_h: f32) {
        (self.px, self.line_h) = (px, line_h);
    }

    pub(super) fn theme(&self) -> Theme {
        self.theme
    }

    fn c(&self) -> f32 {
        self.g.c.c()
    }
    fn m(&self) -> Metrics {
        self.g.c.m
    }
    /// The height of the line being drawn.
    fn h(&self) -> f32 {
        self.line_h
    }
    fn px(&self) -> f32 {
        self.px
    }
    pub(super) fn right(&self) -> f32 {
        self.g.c.right()
    }
    fn text_x(&self) -> f32 {
        self.g.text_x()
    }
    fn title_x(&self) -> f32 {
        self.g.title_x()
    }
    /// The baseline of a line whose top is y: a whole pixel from its top (as the glyphs
    /// are drawn), so a row drawn at any height in any image looks the same.
    fn baseline(&self, y: f32) -> f32 {
        y + ((self.h() + self.fonts.cap_height(self.px())) / 2.0).round()
    }

    /// The width of text in the type being drawn.
    pub(super) fn width(&self, s: &str, weight: Weight) -> f32 {
        self.fonts.width(s, weight, self.px)
    }

    /// Text on the line whose top is y, from x, cut to `max` pixels; returns where it ends.
    pub(super) fn text(&mut self, s: &str, weight: Weight, x: f32, y: f32, max: f32, color: [u8; 4]) -> f32 {
        let px = self.px();
        self.text_in(px, s, weight, x, y, max, color)
    }

    /// Text in type `px` on the baseline of the line being drawn (its type's), whose top
    /// is y: an ID or an age beside a title.
    #[allow(clippy::too_many_arguments)]
    fn text_in(&mut self, px: f32, s: &str, weight: Weight, x: f32, y: f32, max: f32, color: [u8; 4]) -> f32 {
        let s = self.fonts.fit(s, weight, px, max.max(0.0));
        let base = self.baseline(y) - self.shift;
        self.fonts.draw(self.pm, &s, weight, px, (x, base), color)
    }

    fn fill(&mut self, path: &Path, color: [u8; 4]) {
        let t = Transform::from_translate(0.0, -self.shift);
        self.pm.fill_path(path, &paint(color), FillRule::Winding, t, None);
    }

    fn stroke(&mut self, path: &Path, width: f32, color: [u8; 4]) {
        let stroke = Stroke { width, line_cap: LineCap::Round, ..Stroke::default() };
        let t = Transform::from_translate(0.0, -self.shift);
        self.pm.stroke_path(path, &paint(color), &stroke, t, None);
    }

    /// The selected task's (or item's) block, from its first line's top to its last
    /// line's bottom: a card, a faint fill with a faint edge, from a quarter of a cell in
    /// (its mark inside it) to half a cell past where text ends, clear of the type above
    /// and below by `inset`, its last into the space above the next task.
    fn tint(&mut self, block: Option<(f32, f32)>) {
        let Some((top, bottom)) = block else { return };
        let m = self.m();
        let (top, bottom) = (top - m.inset, bottom + m.inset);
        let (x, w) = (0.25 * self.c(), self.right() + 0.25 * self.c());
        let r = 0.35 * m.title;
        if let Some(p) = rounded(x, top, w, bottom - top, r) {
            let fill = self.theme.tint();
            self.fill(&p, fill);
        }
        // the edge, drawn inside the card's own rectangle
        let lw = 1.5;
        if let Some(p) = rounded(x + lw / 2.0, top + lw / 2.0, w - lw, bottom - top - lw, r - lw / 2.0) {
            let edge = self.theme.edge();
            let t = Transform::from_translate(0.0, -self.shift);
            let stroke = Stroke { width: lw, ..Stroke::default() };
            self.pm.stroke_path(&p, &paint(edge), &stroke, t, None);
        }
    }

    /// A row whose top is y and height h; a task's or item's title on `lines`; its mark
    /// left out when `bare`.
    fn row(&mut self, row: &Row, lines: &[String], y: f32, h: f32, bare: bool) {
        let m = self.m();
        let (fg, dim) = (self.theme.text(), self.theme.dim());
        let left = self.c();
        let full = self.right() - left;
        match row {
            Row::Detail(_) | Row::ItemDetail(_) => self.set(m.detail, m.detail_line),
            _ => self.set(m.title, m.line),
        }
        match row {
            // a group's name at a detail's size, a little dim: the titles under it lead
            Row::Head { name, count } => {
                let y = y + m.head_pad;
                self.set(m.detail, m.line);
                let head = self.theme.heading();
                let end = self.text(name, Weight::Bold, left, y, full, head);
                let gap = 0.5 * self.px();
                self.text(&count.to_string(), Weight::Regular, end + gap, y, self.right() - end - gap, dim);
            }
            Row::Task(t) => self.task(t, lines, y, h, bare),
            Row::Detail(d) => {
                self.flag(d.flag, y, h);
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
            // the Later group, as a group's name
            Row::Later { text, amber } => {
                let y = y + m.pad;
                self.set(m.detail, m.line);
                let (head, rest) = text.split_at(text.find(" ·").unwrap_or(text.len()));
                let amber_c = solid(self.theme.yellow);
                let (hc, rc) = if *amber { (amber_c, amber_c) } else { (self.theme.heading(), dim) };
                let end = self.text(head, Weight::Bold, left, y, full, hc);
                self.text(rest, Weight::Regular, end, y, self.right() - end, rc);
            }
            Row::Item(i) => self.item(i, lines, y + m.pad, bare),
            Row::ItemDetail(text) => {
                let x = self.text_x();
                self.text(text, Weight::Regular, x, y, self.right() - x, fg);
            }
        }
    }

    /// The bar beside a task that needs you (amber) or whose agent stopped (red), from y
    /// for h: its rows' bars meet, so it runs down the task's block.
    fn flag(&mut self, flag: Option<Flag>, y: f32, h: f32) {
        let Some(f) = flag else { return };
        let color = if f == Flag::Stopped { self.theme.red } else { self.theme.yellow };
        let w = (0.22 * self.c()).max(2.0);
        let rect = Rect::from_xywh(0.2 * self.c(), y, w, h);
        if let Some(p) = rect.map(PathBuilder::from_rect) {
            self.fill(&p, solid(color));
        }
    }

    /// A task's row, whose top is y and height h: the space above it, then its first
    /// line (the mark, ID, title, risk and age), then the rest of its title's lines.
    fn task(&mut self, t: &TaskRow, lines: &[String], y: f32, h: f32, bare: bool) {
        let y = y + self.m().pad;
        self.flag(t.flag, y, h - self.m().pad);
        let (cx, cy) = (1.5 * self.c(), y + self.h() / 2.0);
        if !bare {
            self.mark(&t.mark, cx, cy, 0.0);
        }
        let tone = t.asked.map(|risky| solid(if risky { self.theme.red } else { self.theme.green }));
        let id_end = self.id(&t.id, t.selected, tone, y);
        if t.from_backlog {
            let r = 0.2 * self.px();
            self.backlog_mark(id_end + 0.25 * self.px() + r, cy, r);
        }
        // the right side: the risk, a coloured word, then the age at a detail's size
        let small = self.m().detail;
        let mut right = self.right();
        let age_w = self.fonts.width(&t.age, Weight::Bold, small);
        let age_color = match tone {
            Some(c) => c,
            None if t.stale => solid(self.theme.red),
            None => self.theme.dim(),
        };
        let weight = if t.stale || tone.is_some() { Weight::Bold } else { Weight::Regular };
        let x = right - self.fonts.width(&t.age, weight, small);
        self.text_in(small, &t.age, weight, x, y, age_w + 1.0, age_color);
        right -= age_w;
        if let Some(level) = &t.risk {
            let gap = if t.age.is_empty() { 0.0 } else { 0.5 * small };
            let w = self.width(level, Weight::Bold);
            let color = self.theme.risk(level);
            self.text(level, Weight::Bold, right - gap - w, y, w + 1.0, color);
        }
        let color = match tone {
            Some(c) => c,
            None if t.quiet && !t.selected => self.theme.dim(),
            None => self.theme.text(),
        };
        let first = self.g.task_right(t, self.fonts);
        self.title(lines, title_weight(tone.is_some()), first, y, color);
    }

    /// A title's lines from the title column, the first ending at `first`, the rest at the right edge.
    fn title(&mut self, lines: &[String], weight: Weight, first: f32, y: f32, color: [u8; 4]) {
        let x = self.title_x();
        for (k, line) in lines.iter().enumerate() {
            let end = if k == 0 { first } else { self.right() };
            self.text(line, weight, x, y + k as f32 * self.h(), end - x + 1.0, color);
        }
    }

    /// An ID, at a detail's size on the title's baseline: dim, or semibold in the
    /// foreground when selected, or in `tone` when it's asked about. Returns where it ends.
    fn id(&mut self, id: &str, selected: bool, tone: Option<[u8; 4]>, y: f32) -> f32 {
        let (x, px) = (self.text_x(), self.m().detail);
        let (weight, color) = match tone {
            Some(c) => (Weight::Bold, c),
            None if selected => (Weight::Bold, self.theme.text()),
            None => (Weight::Regular, self.theme.dim()),
        };
        let w = self.fonts.width(id, weight, px);
        self.text_in(px, id, weight, x, y, w + 1.0, color)
    }

    /// An item's row, from its first line's top y.
    fn item(&mut self, i: &ItemRow, lines: &[String], y: f32, bare: bool) {
        let (cx, cy) = (1.5 * self.c(), y + self.h() / 2.0);
        let r = self.mark_r();
        if i.job {
            if !bare {
                self.mark(&Mark::Job, cx, cy, 0.0);
            }
        } else if i.asks {
            let amber = solid(self.theme.yellow);
            let w = self.width("?", Weight::Bold);
            self.text("?", Weight::Bold, cx - w / 2.0, y, w + 1.0, amber);
        } else if let Some(p) = diamond(cx, cy, r * 0.9) {
            let dim = self.theme.dim();
            self.stroke(&p, self.line(), dim);
        }
        let red = solid(self.theme.red);
        self.id(&i.id, i.selected, i.asked.then_some(red), y);
        let color = if i.asked { red } else { self.theme.text() };
        let right = self.right();
        self.title(lines, title_weight(i.asked), right, y, color);
    }

    /// The scroll marks in the last column: ↑ at the top when rows are above, ↓ at the
    /// bottom of an image `height` high when rows are below.
    pub(super) fn scroll_marks(&mut self, marks: Marks, height: f32) {
        let cx = self.g.c.grid.width() as f32 - 0.6 * self.c();
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
        let r = 0.28 * self.c();
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

    fn mark_r(&self) -> f32 {
        self.g.mark_r() * self.scale
    }

    /// A mark as one frame of a sprite (sprite.rs): turned `turn` of the way round, at
    /// `scale` of its size.
    pub(super) fn mark_frame(&mut self, mark: &Mark, cx: f32, cy: f32, turn: f32, scale: f32) {
        self.scale = scale;
        self.mark(mark, cx, cy, turn);
        self.scale = 1.0;
    }

    fn line(&self) -> f32 {
        (self.mark_r() * 0.26).max(1.2)
    }

    /// A task's mark: each status's own shape in its colour. The marks that turn are
    /// drawn `turn` of the way round (0: as they stand still).
    fn mark(&mut self, mark: &Mark, cx: f32, cy: f32, turn: f32) {
        let r = self.mark_r();
        let lw = self.line();
        let (fg, dim) = (self.theme.text(), self.theme.dim());
        let color = |st: &str| self.theme.status(st).unwrap_or(dim);
        let a0 = -0.5 * PI + 2.0 * PI * turn;
        match mark {
            // a command running, or a tour being written: an arc half the way round
            Mark::Job => self.arc(cx, cy, r, a0, 1.0 * PI, lw * 1.2, fg),
            Mark::Tour(st) => {
                let c = color(st);
                self.arc(cx, cy, r, a0, 1.0 * PI, lw * 1.2, c)
            }
            Mark::Stuck(st) => self.status_mark(st, cx, cy, Some(solid(self.theme.yellow)), 0.0),
            // working: a faint ring with a third of it drawn
            Mark::Working => {
                self.arc(cx, cy, r, 0.0, 2.0 * PI, lw, self.theme.tint_strong());
                self.arc(cx, cy, r, a0, 0.17 * PI, lw, fg);
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

/// Half a disc, from angle a0 half way round (clockwise on screen): from the bottom
/// (a0 = π/2) it's the left half, the review mark's ◐.
fn half(cx: f32, cy: f32, r: f32, a0: f32) -> Option<Path> {
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
