//! Drawing the board's list as an image: the rows rows.rs describes (the same the text
//! list draws), one under the other, each as tall as the type size makes it (more than
//! a cell at every size), and a detail's text wrapped by pixels to its room; the image
//! itself is the list's block of cells. The type is real (font.rs), the marks are drawn
//! shapes (marks.rs) in the status colours (rings, a check, a diamond), the selected
//! task sits on a card (a faint fill and a quiet edge, its mark inside; mission
//! control's has ⏎ in a cap at its lower right when its last line leaves room), a task
//! that needs you on a wash of its colour with a round bar at its left, and a risk is a
//! chip (chip.rs), beside the age or, on a narrow board, at the right of the task's
//! last line. A task is one block: its mark and ID in a gutter, its title's two lines
//! and its details from the title's left edge, and under its last line the room its
//! card has there. A block is laid out the same selected or not, so moving the
//! selection moves nothing else. The title is the loudest thing on a row: the ID, the
//! age and the group's name are a detail's size. The marks that move are left out when
//! the image look places them over the list (sprite.rs draws their frames with
//! `Draw`). The list's last column has the scroll marks. Nothing here touches the
//! terminal: given rows and a size in pixels, it lays them out and returns an image, or
//! a window of pixels of them (pieces.rs), drawn as that part of the whole would be.

use super::chip;
use super::font::{Fonts, Weight};
use super::shape::{diamond, left_slice, paint, rounded};
use super::theme::{lit, solid, Flags, Rgb, Theme};
use crate::model::TextSize;
use crate::ui::rows::{DetailRow, Flag, ItemRow, Mark, Row, Rows, TaskRow};
use std::ops::Range;
use tiny_skia::{FillRule, LineCap, Path, Pixmap, Stroke, Transform};

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

    /// Where text starts (a group's name, the top line, the keys) and ends: a column and
    /// a half in from each side (the right's last part has the list's scroll marks).
    /// The rules and the status bar run between the two, and a card reaches a column
    /// past each (`out`), so it's as far from the pane's left edge as from its right.
    pub fn left(&self) -> f32 {
        (1.5 * self.c()).round()
    }

    /// How far a card or a wash reaches past where text starts and ends: its room
    /// beside its type.
    pub fn out(&self) -> f32 {
        self.c().round()
    }

    pub fn right(&self) -> f32 {
        self.grid.width() as f32 - self.left()
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

/// The rows laid out: each one's height in pixels, a task's (or an item's) title, or a
/// detail's text, as the lines it's drawn on, and the ID column's width. Which task is
/// selected changes none of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    pub heights: Vec<u32>,
    lines: Vec<Vec<String>>,
    pub idw: f32,
    /// the repositories' names' column (mission control's inbox): the widest name's width
    pub repow: f32,
}

impl Laid {
    /// Rows [a, b)'s heights, in pixels.
    pub fn tall(&self, a: usize, b: usize) -> u32 {
        self.heights[a.min(b)..b.min(self.heights.len())].iter().sum()
    }
}

/// The most lines a task's title wraps to, at any width, so a wider board never shows
/// less of it than a narrower one (an item's is cut to one).
const TITLE_LINES: usize = 2;

/// Lay the rows out: a title cut to its row, or wrapped (one that's yours), a detail's
/// text wrapped by pixels to the room its type has, and how tall each row is: a
/// block's last row with the room its card has under it.
pub fn lay(rows: &Rows, c: &Canvas, fonts: &Fonts) -> Laid {
    let idw = id_width(rows, fonts, Geo { c: *c, idw: 0.0, repow: 0.0 }.id_px());
    // (at most a fifth of the image: a longer name is cut)
    let repow = repo_width(rows, fonts, c.m.title).min((REPO_MOST * c.grid.width() as f32).round());
    let g = Geo { c: *c, idw, repow };
    let mut all = Vec::new();
    let mut heights = Vec::new();
    // the room under a block's last row
    let under = |i: usize| if rows.rows.get(i + 1).is_some_and(Row::is_detail) { 0 } else { c.m.below as u32 };
    for (i, row) in rows.rows.iter().enumerate() {
        let (title, most, weight, right, rest) = match row {
            Row::Task(t) => {
                let most = TITLE_LINES;
                // (its repository's name before it, on mission control, takes its first line's room)
                (&t.title, most, title_weight(), g.task_right(t, fonts) - g.repo_width(t), g.title_right(t, fonts))
            }
            Row::Item(i) => (&i.title, 1, title_weight(), c.right(), c.right()),
            Row::Detail(d) => {
                // a stale age is semibold: the first line has that much less room
                let (age, _) = crate::model::split_at_width(&d.text, d.lead);
                let bolder = if d.stale { fonts.width(&age, Weight::Bold, c.m.body) - fonts.width(&age, Weight::Regular, c.m.body) } else { 0.0 };
                // (a narrow board's risk chip at its last line's right: one line more for it)
                let (most, chip) = match g.risk_below(d) {
                    Some(level) => (d.lines.max(2), g.risk_width(level, fonts) + RISK_GAP * c.m.detail),
                    None => (d.lines, 0.0),
                };
                let lines = g.detail_lines(&d.text, most, bolder, g.caps(d) + chip, fonts);
                heights.push(g.height(row, lines.len()) + under(i));
                all.push(lines);
                continue;
            }
            Row::ItemDetail(text, most, _) => {
                let lines = g.detail_lines(text, *most, 0.0, 0.0, fonts);
                heights.push(g.height(row, lines.len()) + under(i));
                all.push(lines);
                continue;
            }
            _ => {
                // the list's first group's name is clear of the header over it
                let under = match row {
                    Row::Head { .. } if i == 0 => (c.m.gap / 2.0).round() as u32,
                    _ => 0,
                };
                all.push(vec![]);
                heights.push(g.height(row, 1) + under);
                continue;
            }
        };
        let px = c.m.title;
        let x = g.title_x();
        let first = right - x;
        let lines = if most > 1 {
            // (its first line full beside its age, but a last line of one word takes another)
            let mut lines = fonts.wrap(title, weight, px, (first, rest - x), most);
            fonts.no_widow(&mut lines, weight, px, rest - x);
            lines
        } else {
            vec![fonts.fit(title, weight, px, first)]
        };
        heights.push(g.height(row, lines.len().max(1)) + under(i));
        all.push(lines);
    }
    Laid { heights, lines: all, idw, repow }
}

/// The rows from `top` that fit in `room` pixels whole: [top, end). One at least, cut if
/// it must be.
pub fn fitting(laid: &Laid, top: usize, room: u32) -> usize {
    let mut used = 0;
    let mut end = top;
    while end < laid.heights.len() && (end == top || used + laid.heights[end] <= room) {
        used += laid.heights[end];
        end += 1;
    }
    end
}

/// The space between a title's first line and its risk or age, of a title's size.
const TITLE_GAP: f32 = 0.85;

/// The space between a risk's chip and the age's column, of a detail's size.
const RISK_GAP: f32 = 0.75;

/// The space after a task's repository's name, before its title, of a title's size.
const REPO_GAP: f32 = 0.5;

/// The most of an image's width the repositories' names' column takes.
const REPO_MOST: f32 = 0.2;

/// The selected card's edge, in pixels.
const RING: f32 = 2.0;

/// The pixels drawn past a window's edges, and cut off (render).
const EDGE: u32 = 4;

/// What of the laid-out rows an image shows: rows[range], the first `at` pixels down the
/// layout (where it is in the whole list, so a row is drawn at the same place in every
/// image of it, to the bit), of which the pixels [y, y + h) from the first one's top;
/// the scroll marks; and the rows whose marks are left out (`bare`), for sprite.rs to
/// place over them.
pub struct Cut<'a> {
    pub range: Range<usize>,
    pub at: u32,
    pub y: u32,
    pub h: u32,
    pub marks: Marks,
    pub bare: &'a dyn Fn(usize) -> bool,
}

impl Cut<'_> {
    /// All of rows[range], from their top, in an image `h` high, every mark drawn.
    pub fn rows(range: Range<usize>, h: u32) -> Cut<'static> {
        Cut { range, at: 0, y: 0, h, marks: Marks::default(), bare: &|_| false }
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
    d.g.repow = laid.repow;
    let mut places = Vec::new();
    let mut y = cut.at;
    for i in range.clone() {
        let id = match &rows.rows[i] {
            Row::Task(t) => Some(t.id.clone()),
            Row::Item(it) => Some(it.id.clone()),
            _ => None,
        };
        places.push(Place { row: i, y, h: laid.heights[i], id });
        y += laid.heights[i];
    }
    // the window: the rows are drawn where they are in the layout, moved up by its top
    // (whole pixels), so a row's edges fall on the same fractions of a pixel in any image
    d.shift = (cut.at + cut.y) as f32 - EDGE as f32;
    // behind each task's block (its row and its lines): the selected one's card, or the
    // wash of one that needs you, and that one's bar
    let blocks = blocks(rows, &places, c.m.pad);
    for b in &blocks {
        match (b.selected, b.flag) {
            (true, f) => d.card(b.top, b.bottom, f),
            (false, Some(f)) => d.wash(b.top, b.bottom, f),
            _ => {}
        }
        if let Some(f) = b.flag {
            d.bar(b.top, b.bottom, f, if b.selected { RING } else { 0.0 });
        }
    }
    let at = |p: &Place| p.y as f32;
    for p in &places {
        let (y, h) = (at(p), p.h as f32);
        // a row outside the window draws nothing (its card, wash and bar may reach in, above)
        let top = (cut.at + cut.y) as f32;
        if y + h + c.m.pad + EDGE as f32 >= top && y <= top + (height + EDGE) as f32 {
            d.row(&rows.rows[p.row], &laid.lines[p.row], (y, h), (cut.bare)(p.row));
        }
    }
    // the selected task's key (mission control's), at its card's lower right, when its
    // last line leaves room
    for b in blocks.iter().filter(|b| b.ends) {
        if let Some((key, what)) = b.key.filter(|(k, what)| d.g.key_fits(&rows.rows[b.last], &laid.lines[b.last], k, what, d.fonts)) {
            d.action(key, what, &rows.rows[b.last], b.bottom - c.m.below);
        }
    }
    let mut pm = pm.clone_rect(tiny_skia::IntRect::from_xywh(0, EDGE as i32, width, height).ok_or_else(made)?).ok_or_else(made)?;
    Draw::new(&mut pm, fonts, c, laid.idw).scroll_marks(cut.marks, height as f32);
    Ok(Sheet { pixmap: pm, places })
}

/// A task's (or an item's) block: from its first line's top to its last row's bottom
/// (its card's), whether it's selected, and its flag.
struct Block<'r> {
    top: f32,
    bottom: f32,
    selected: bool,
    flag: Option<Flag>,
    /// the selected task's key, and whether its block ends here with a line under it,
    /// where the key goes
    key: Option<&'r (String, String)>,
    ends: bool,
    /// its last row
    last: usize,
}

/// The blocks of the rows placed: each task's or item's row with the lines under it (a
/// window's first lines, under a row above it, a block of their own).
fn blocks<'r>(rows: &'r Rows, places: &[Place], pad: f32) -> Vec<Block<'r>> {
    let mut out: Vec<Block> = Vec::new();
    let mut open = false;
    for p in places {
        let row = &rows.rows[p.row];
        let (y, bottom) = (p.y as f32, (p.y + p.h) as f32);
        let block = |top: f32, selected: bool, flag: Option<Flag>| Block { top, bottom, selected, flag, key: None, ends: false, last: p.row };
        // a line under a task (or an item) that's its block's last
        let ends = row.is_detail() && !rows.rows.get(p.row + 1).is_some_and(Row::is_detail);
        match row {
            Row::Task(t) => out.push(Block { key: t.key.as_ref(), ..block(y + pad, t.selected, t.flag) }),
            Row::Item(i) => out.push(block(y + pad, i.selected, None)),
            Row::Detail(_) | Row::ItemDetail(..) if open => {
                let b = out.last_mut().expect("a block is open");
                (b.bottom, b.ends, b.last) = (bottom, ends, p.row);
            }
            Row::Detail(dr) => out.push(Block { ends, ..block(y, dr.selected, dr.flag) }),
            Row::ItemDetail(_, _, sel) => out.push(Block { ends, ..block(y, *sel, None) }),
            _ => {}
        }
        open = row.is_entry() || row.is_detail();
    }
    out
}

/// Lines wrapped from parts said apart by " · " (a stat line's "5 files, +96 −31 · Not
/// toured yet") break between parts where they can: a line's last part, cut by the
/// break, goes down whole when the line under it has room (`fits`); and no line starts
/// or ends with the dot. (So a line's text can lose a dot: what's drawn on it is read
/// from the lines, not the text.)
fn at_dots(lines: &mut [String], fits: impl Fn(&str) -> bool) {
    for k in 1..lines.len() {
        // (a line that starts at a dot has the part above it whole)
        if lines[k].ends_with('…') || lines[k].starts_with("· ") {
            continue;
        }
        if let Some((head, tail)) = lines[k - 1].rsplit_once(" · ") {
            let down = format!("{tail} {}", lines[k].trim_start_matches("· "));
            if head.contains(' ') && fits(&down) {
                (lines[k - 1], lines[k]) = (head.to_string(), down);
            }
        }
    }
    for l in lines.iter_mut() {
        *l = l.trim_start_matches("· ").trim_end_matches(" ·").to_string();
    }
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

/// The repositories' names' column in pixels (mission control's inbox): the widest, so
/// its titles line up.
fn repo_width(rows: &Rows, fonts: &Fonts, px: f32) -> f32 {
    rows.rows
        .iter()
        .filter_map(|r| match r {
            Row::Task(t) if !t.repo.is_empty() => Some(fonts.width(&t.repo, Weight::Regular, px)),
            _ => None,
        })
        .fold(0.0, f32::max)
}

/// Where a task's (or item's) mark is in its row: across from the image's left edge,
/// down from the row's top.
pub(super) fn mark_at(c: &Canvas) -> (f32, f32) {
    (Geo { c: *c, idw: 0.0, repow: 0.0 }.mark_x(), c.m.pad + c.m.line / 2.0)
}

/// How far a mark reaches from its centre at `scale` (Geo::mark_reach).
pub(super) fn mark_reach(c: &Canvas, scale: f32) -> f32 {
    Geo { c: *c, idw: 0.0, repow: 0.0 }.mark_reach(scale)
}

/// A title is semibold, whatever the task: its lines under it are regular.
fn title_weight() -> Weight {
    Weight::Bold
}

/// Where things go across a row, and how tall a row is.
#[derive(Clone, Copy)]
struct Geo {
    c: Canvas,
    idw: f32,
    repow: f32,
}

impl Geo {
    /// A mark's radius, to half a pixel: a little under the title's capital's height across.
    fn mark_r(&self) -> f32 {
        ((0.76 * self.c.m.title).round() / 2.0).min(0.8 * self.c.c())
    }

    /// A mark's centre, on a whole pixel (so a mark placed over the list is smoothed as
    /// one drawn in it): its left edge is where a group's name starts.
    fn mark_x(&self) -> f32 {
        (self.c.left() + self.mark_r()).round()
    }

    /// How far a mark reaches from its centre, at `scale` (a mark settling in grows past
    /// its size: sprite.rs), with its stroke and a pixel for its edge.
    pub(super) fn mark_reach(&self, scale: f32) -> f32 {
        1.15 * self.mark_r() * scale + (self.mark_r() * 0.3).max(1.2) + 2.0
    }

    /// Where a task's ID starts: two columns past its mark's left edge, or past a
    /// larger type's mark and a little space.
    fn text_x(&self) -> f32 {
        let c = self.c.c();
        (self.c.left() + 2.0 * c).max(self.mark_x() + self.mark_r() + 0.4 * self.c.m.title)
    }

    /// Where a title starts, and every line of it and of the task's details under it
    /// (the ID and the mark hang in a gutter at their left): past the ID column and a
    /// gap, narrower on a board under 44 columns, where every pixel counts.
    fn title_x(&self) -> f32 {
        self.text_x() + self.idw + if self.narrow() { 0.6 } else { 0.8 } * self.c.m.title
    }

    /// An ID's type: a detail's, at any width.
    fn id_px(&self) -> f32 {
        self.c.m.detail
    }

    /// A board under 44 columns (the image's are one fewer, the last column the text's).
    fn narrow(&self) -> bool {
        self.c.grid.cols + 1 < 44
    }

    /// A detail's text wrapped at its spaces to the room under the ID, `most` lines at
    /// most (the first `less` narrower, and every one `key` narrower), the last cut with
    /// "…" when there's more. One line at least, so a row is never empty.
    fn detail_lines(&self, text: &str, most: usize, less: f32, key: f32, fonts: &Fonts) -> Vec<String> {
        let room = self.detail_room() - key;
        let mut lines = fonts.wrap(text, Weight::Regular, self.c.m.body, (room - less, room), most);
        fonts.fill_last(&mut lines, Weight::Regular, self.c.m.body, room);
        at_dots(&mut lines, |l| fonts.width(l, Weight::Regular, self.c.m.body) <= room);
        if lines.is_empty() { vec![String::new()] } else { lines }
    }

    /// The room the caps of the keys a detail's text names take on a line: their sides.
    fn caps(&self, d: &DetailRow) -> f32 {
        key_letters(&d.text, d.lead).len() as f32 * 2.0 * KEY_SIDE * self.c.m.body
    }

    /// Does the selected task's key fit at the right of its block's last line (`row`,
    /// a detail, drawn on `lines`) as that line is, beside a narrow board's risk chip?
    /// The line is never cut or moved for it: where it doesn't fit it's left out, and
    /// the key row has it.
    fn key_fits(&self, row: &Row, lines: &[String], key: &str, what: &str, fonts: &Fonts) -> bool {
        let Row::Detail(d) = row else { return false };
        let line = lines.last().map_or(0.0, |l| fonts.width(l, Weight::Bold, self.c.m.body));
        let risk = self.risk_below(d).map_or(0.0, |level| self.risk_width(level, fonts) + RISK_GAP * self.c.m.detail);
        line + self.caps(d) + risk + self.key_width(key, what, fonts) <= self.detail_room()
    }

    /// The room the selected task's key takes at the right of its last line: its cap,
    /// what it does, and a gap before it.
    fn key_width(&self, key: &str, what: &str, fonts: &Fonts) -> f32 {
        let px = self.key_px();
        chip::keycap_width(fonts, key, what, px) + 0.8 * px
    }

    /// The type of the selected task's key: a label's, as the key row's.
    fn key_px(&self) -> f32 {
        self.c.m.label
    }

    /// A detail line's room: from the title's left edge to where text ends.
    fn detail_room(&self) -> f32 {
        self.c.right() - self.title_x()
    }

    /// The age column's width: an age of two figures', so the risks' chips before it
    /// line up down the list whatever the ages are (a wider one, or an overdue one's
    /// chip, takes its own).
    fn age_width(&self, t: &TaskRow, fonts: &Fonts) -> f32 {
        if t.age.is_empty() {
            return 0.0;
        }
        let px = self.c.m.detail;
        let own = if self.overdue(t) { chip::width(fonts, &t.age, self.c.m.label) } else { fonts.width(&t.age, Weight::Bold, px) };
        own.max(fonts.width("00m", Weight::Bold, px))
    }

    /// A task whose age has waited too long, and isn't being asked about: its age is a chip.
    fn overdue(&self, t: &TaskRow) -> bool {
        t.stale && t.asked.is_none()
    }

    /// The width a task's age (and risk) take at the right, as `task` draws them.
    fn right_side(&self, t: &TaskRow, fonts: &Fonts) -> f32 {
        let mut w = self.age_width(t, fonts);
        if let Some(level) = self.risk_beside(t) {
            let gap = if t.age.is_empty() { 0.0 } else { RISK_GAP * self.c.m.detail };
            w += gap + self.risk_width(level, fonts);
        }
        w
    }

    /// A task's risk, when its chip is beside its title: but on a board under 44
    /// columns, where it would take the title's room (it's at its last line's right).
    fn risk_beside<'t>(&self, t: &'t TaskRow) -> Option<&'t String> {
        t.risk.as_ref().filter(|_| !self.narrow())
    }

    /// A line's risk, when its task's chip is at its right: its task's last line, on a
    /// board under 44 columns.
    fn risk_below<'d>(&self, d: &'d DetailRow) -> Option<&'d String> {
        d.risk.as_ref().filter(|_| self.narrow())
    }

    /// How wide a task's risk is: its chip.
    fn risk_width(&self, level: &str, fonts: &Fonts) -> f32 {
        chip::risk_width(fonts, level, self.c.m.label)
    }

    /// Where a task's title's other lines end: short of the age's column, so the ages
    /// have a column of their own down the list, whatever wraps beside them.
    fn title_right(&self, t: &TaskRow, fonts: &Fonts) -> f32 {
        let age = self.age_width(t, fonts);
        self.c.right() - if age > 0.0 { age + TITLE_GAP * self.c.m.title } else { 0.0 }
    }

    /// The room a task's repository's name takes before its title's first line (mission
    /// control's inbox): the name at a title's size and a gap; none on a board.
    fn repo_width(&self, t: &TaskRow) -> f32 {
        if t.repo.is_empty() {
            return 0.0;
        }
        self.repow + REPO_GAP * self.c.m.title
    }

    /// Where a task's title's first line must end: short of its risk and age, by a gap.
    fn task_right(&self, t: &TaskRow, fonts: &Fonts) -> f32 {
        self.c.right() - self.right_side(t, fonts) - TITLE_GAP * self.c.m.title
    }

    /// A row's height in whole pixels, with its title (or a detail's text) on `lines` lines.
    fn height(&self, row: &Row, lines: usize) -> u32 {
        let m = &self.c.m;
        let h = match row {
            // (and a little under it, clear of a selected card's edge)
            Row::Head { .. } => m.head_pad + m.detail_line + (0.35 * m.title).round(),
            Row::Task(_) | Row::Item(_) => m.pad + lines as f32 * m.line + m.title_gap,
            Row::Later { .. } => m.pad + m.line,
            Row::Detail(_) | Row::ItemDetail(..) => lines as f32 * m.body_line,
            Row::Gap => m.gap,
            Row::Empty { .. } => m.line,
            // (a blocked coordinator's question under it, and a block's room under that)
            Row::Repo(r) => m.head_pad + m.line + if r.question.is_some() { m.body_line + m.below } else { m.title_gap },
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
        Draw { pm, fonts, theme: c.theme, g: Geo { c: *c, idw, repow: 0.0 }, px: c.m.title, line_h: c.m.line, scale: 1.0, shift: 0.0 }
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
    pub(super) fn text_in(&mut self, px: f32, s: &str, weight: Weight, x: f32, y: f32, max: f32, color: [u8; 4]) -> f32 {
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

    /// What a block from `top` to `bottom` sits on (Block): a column past where text
    /// starts and ends, on each side (its mark inside it), clear of the type above by
    /// `inset`, to its last row's bottom, which has the room under its type. As x, y, w, h.
    fn panel(&self, top: f32, bottom: f32) -> (f32, f32, f32, f32) {
        let (m, out) = (self.m(), self.g.c.out());
        let x = self.g.c.left() - out;
        (x, top - m.inset, self.right() + out - x, bottom - top + m.inset)
    }

    /// The selected task's (or item's) card, the same wherever it is: a fill a step
    /// stronger than what's around it (the foreground, faint, or its flag's colour
    /// well over its wash's strength when it needs you or stopped) and a quiet edge two
    /// pixels wide, the terminal's blue mixed into the pane's colour (a task's bar
    /// inside it, when it needs you, following its corners).
    fn card(&mut self, top: f32, bottom: f32, flag: Option<Flag>) {
        let (x, y, w, h) = self.panel(top, bottom);
        let r = self.radius();
        let fill = flag.map_or(self.theme.tint(), |f| self.theme.flag_tint(f));
        let edge = self.theme.edge();
        self.rounded_fill(x, y, w, h, r, fill);
        self.rounded_edge(x, y, w, h, r, RING, edge);
    }

    /// A card's and a wash's corners.
    fn radius(&self) -> f32 {
        0.35 * self.m().title
    }

    /// Behind a task that needs you (amber) or whose agent stopped (red): its colour,
    /// faintly, over the card's rectangle, with no edge: only the selection is boxed.
    fn wash(&mut self, top: f32, bottom: f32, flag: Flag) {
        let (x, y, w, h) = self.panel(top, bottom);
        let (color, r) = (self.theme.wash(flag), self.radius());
        self.rounded_fill(x, y, w, h, r, color);
    }

    /// The bar at the left of a task that needs you or stopped, in its colour: its
    /// wash's left edge, following its corners, `inside` a selected card's edge.
    fn bar(&mut self, top: f32, bottom: f32, flag: Flag, inside: f32) {
        let (x, y, _, h) = self.panel(top, bottom);
        let w = (0.25 * self.c()).max(3.0);
        let color = solid(lit(self.theme.flag(flag)));
        if let Some(p) = left_slice(x + inside, y + inside, w, h - 2.0 * inside, self.radius() - inside) {
            self.fill(&p, color);
        }
    }

    /// A task's key in a cap and what it does, at the right of its block's last line
    /// (`row`), whose bottom is `bottom`: left of a narrow board's risk chip, which is
    /// there too.
    fn action(&mut self, key: &str, what: &str, row: &Row, bottom: f32) {
        let px = self.g.key_px();
        let chip = match row {
            Row::Detail(d) => self.g.risk_below(d).map_or(0.0, |level| self.g.risk_width(level, self.fonts) + RISK_GAP * self.m().detail),
            _ => 0.0,
        };
        let x = self.right() - chip - chip::keycap_width(self.fonts, key, what, px);
        chip::keycap(self, key, what, px, x, bottom - self.m().body_line / 2.0);
    }

    /// A rectangle with rounded corners, filled.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn rounded_fill(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: [u8; 4]) {
        if let Some(p) = rounded(x, y, w, h, r) {
            self.fill(&p, color);
        }
    }

    /// A rounded rectangle's edge, `lw` wide, drawn inside it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn rounded_edge(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, lw: f32, color: [u8; 4]) {
        if let Some(p) = rounded(x + lw / 2.0, y + lw / 2.0, w - lw, h - lw, r - lw / 2.0) {
            let t = Transform::from_translate(0.0, -self.shift);
            let stroke = Stroke { width: lw, ..Stroke::default() };
            self.pm.stroke_path(&p, &paint(color), &stroke, t, None);
        }
    }

    /// Text in type `px`, centred in a box `h` high whose top is y (a chip's or a
    /// keycap's), from x; returns where it ends.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn boxed(&mut self, px: f32, s: &str, weight: Weight, x: f32, y: f32, h: f32, color: [u8; 4]) -> f32 {
        let base = y + ((h + self.fonts.cap_height(px)) / 2.0).round() - self.shift;
        self.fonts.draw(self.pm, s, weight, px, (x, base), color)
    }

    /// Text in type `px` on the baseline `base`, from x; returns where it ends.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn on_baseline(&mut self, px: f32, s: &str, weight: Weight, x: f32, base: f32, color: [u8; 4]) -> f32 {
        self.fonts.draw(self.pm, s, weight, px, (x, base - self.shift), color)
    }

    pub(super) fn fonts(&self) -> &Fonts {
        self.fonts
    }

    /// A row whose top is y, `h` high; a task's or item's title, or a detail's text, on
    /// `lines`; its mark left out when `bare`.
    fn row(&mut self, row: &Row, lines: &[String], (y, h): (f32, f32), bare: bool) {
        let m = self.m();
        let (fg, dim) = (self.theme.text(), self.theme.dim());
        let left = self.g.c.left();
        let full = self.right() - left;
        match row {
            Row::Detail(_) | Row::ItemDetail(..) => self.set(m.body, m.body_line),
            _ => self.set(m.title, m.line),
        }
        match row {
            // a group's name at a detail's size, a little dim: the titles under it lead
            // (at its row's foot: the first one's row has more room over it)
            Row::Head { name, count } => {
                let y = y + h - m.detail_line;
                self.set(m.detail, m.detail_line);
                let head = self.theme.heading();
                let end = self.text(name, Weight::Bold, left, y, full, head);
                let gap = 0.5 * self.px();
                self.text(&count.to_string(), Weight::Regular, end + gap, y, self.right() - end - gap, dim);
            }
            Row::Task(t) => self.task(t, lines, y, bare),
            Row::Detail(d) => {
                // dim, but the selected task's and what you must act on, which are a
                // step under the title's foreground
                let color = if d.selected || (d.loud && d.flag.is_some()) { self.theme.soft() } else { dim };
                let x = self.title_x();
                // (found in the lines as drawn, which may have dropped a dot)
                let keys = key_letters(&lines.join(" "), d.lead);
                let mut seen = 0;
                for (k, line) in lines.iter().enumerate() {
                    let y = y + k as f32 * m.body_line;
                    // a narrow board's age starts the first line
                    let (age, rest) = crate::model::split_at_width(line, if k == 0 { d.lead } else { 0 });
                    // (an age that has waited too long is semibold, in the foreground)
                    let age_color = if d.stale { fg } else { dim };
                    let weight = if d.stale { Weight::Bold } else { Weight::Regular };
                    let end = self.text(&age, weight, x, y, self.right() - x, age_color);
                    seen += age.chars().filter(|c| !c.is_whitespace()).count();
                    self.said(&rest, end, y, color, &keys, &mut seen);
                }
                // a narrow board's risk chip, at the right of the task's last line
                if let Some(level) = self.g.risk_below(d) {
                    let cy = y + (lines.len().max(1) - 1) as f32 * m.body_line + m.body_line / 2.0;
                    let color = self.theme.risk(level);
                    let color = solid(lit(Rgb(color[0], color[1], color[2])));
                    chip::risk(self, level, m.label, self.right(), cy, color);
                }
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
            Row::Repo(r) => self.repo(r, y + m.head_pad, y + h),
            Row::ItemDetail(_, _, selected) => {
                let x = self.title_x();
                for (k, line) in lines.iter().enumerate() {
                    let color = if *selected { self.theme.soft() } else { dim };
                    self.text(line, Weight::Regular, x, y + k as f32 * m.body_line, self.right() - x, color);
                }
            }
        }
    }

    /// A task's row, whose top is y: the space above it, then its first
    /// line (the mark, ID, title, risk and age), then the rest of its title's lines.
    fn task(&mut self, t: &TaskRow, lines: &[String], y: f32, bare: bool) {
        let y = y + self.m().pad;
        let (cx, cy) = (self.g.mark_x(), y + self.h() / 2.0);
        if !bare {
            self.mark(&t.mark, cx, cy, 0.0);
        }
        let tone = t.asked.map(|risky| solid(if risky { self.theme.red } else { self.theme.green }));
        let id_end = self.id(&t.id, t.selected, t.flag.is_some(), tone, y);
        if t.from_backlog {
            let r = 0.2 * self.px();
            self.backlog_mark(id_end + 0.25 * self.px() + r, cy, r);
        }
        // the right side: the risk as a chip, then the age at a detail's size
        let small = self.m().detail;
        let mut right = self.right();
        let age_w = self.g.age_width(t, self.fonts);
        if self.g.overdue(t) {
            // an age that has waited too long: a chip of its task's flag's colour, whole
            // (red with none), so it reads as a mark, not as an odd weight
            let color = t.flag.map_or(self.theme.red, |f| self.theme.flag(f));
            chip::solid(self, &t.age, self.m().label, right, cy, color);
        } else {
            let (weight, color) = if let Some(c) = tone { (Weight::Bold, c) } else { (Weight::Regular, self.theme.dim()) };
            let x = right - self.fonts.width(&t.age, weight, small);
            self.text_in(small, &t.age, weight, x, y, age_w + 1.0, color);
        }
        right -= age_w;
        if let Some(level) = self.g.risk_beside(t) {
            let gap = if t.age.is_empty() { 0.0 } else { RISK_GAP * small };
            let color = self.theme.risk(level);
            let color = solid(lit(Rgb(color[0], color[1], color[2])));
            chip::risk(self, level, self.m().label, right - gap, cy, color);
        }
        let color = match tone {
            Some(c) => c,
            None if t.selected || t.yours => self.theme.text(),
            None if t.quiet => self.theme.quiet(),
            None => self.theme.soft(),
        };
        let (first, rest) = (self.g.task_right(t, self.fonts), self.g.title_right(t, self.fonts));
        // mission control's inbox: its repository's name first, regular and dim
        let lead = self.g.repo_width(t);
        if lead > 0.0 {
            let (x, dim) = (self.title_x(), self.theme.dim());
            self.text(&t.repo, Weight::Regular, x, y, lead, dim);
        }
        self.title(lines, title_weight(), (first, rest), lead, y, color);
    }

    /// A title's lines from the title column (the first `lead` further in), the first
    /// ending at `first`, the rest at `rest`.
    fn title(&mut self, lines: &[String], weight: Weight, (first, rest): (f32, f32), lead: f32, y: f32, color: [u8; 4]) {
        let x = self.title_x();
        for (k, line) in lines.iter().enumerate() {
            let (x, end) = if k == 0 { (x + lead, first) } else { (x, rest) };
            self.text(line, weight, x, y + k as f32 * self.h(), end - x + 1.0, color);
        }
    }

    /// An ID, at a detail's size on the title's baseline: dim (a step up on a wash,
    /// `washed`, where dim is fainter), or in the foreground when selected (its weight
    /// the same, so it keeps its width), or semibold in `tone` when it's asked about.
    /// Returns where it ends.
    fn id(&mut self, id: &str, selected: bool, washed: bool, tone: Option<[u8; 4]>, y: f32) -> f32 {
        let (x, px) = (self.text_x(), self.g.id_px());
        let (weight, color) = match tone {
            Some(c) => (Weight::Bold, c),
            None if selected => (Weight::Regular, self.theme.text()),
            None if washed => (Weight::Regular, self.theme.soft()),
            None => (Weight::Regular, self.theme.dim()),
        };
        let w = self.fonts.width(id, weight, px);
        self.text_in(px, id, weight, x, y, w + 1.0, color)
    }

    /// An item's row, from its first line's top y.
    fn item(&mut self, i: &ItemRow, lines: &[String], y: f32, bare: bool) {
        let (cx, cy) = (self.g.mark_x(), y + self.h() / 2.0);
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
        self.id(&i.id, i.selected, false, i.asked.then_some(red), y);
        let color = if i.asked { red } else { self.theme.text() };
        let right = self.right();
        self.title(lines, title_weight(), (right, right), 0.0, y, color);
    }
}

#[path = "metrics.rs"]
mod metrics;
pub use metrics::Metrics;
#[path = "marks.rs"]
mod marks;
#[path = "said.rs"]
mod said;
#[path = "fleet.rs"]
pub mod fleet;
#[path = "page.rs"]
pub mod page;
use said::{key_letters, KEY_SIDE};
pub use marks::Marks;

#[cfg(test)]
#[path = "tests/test_render.rs"]
mod tests;
