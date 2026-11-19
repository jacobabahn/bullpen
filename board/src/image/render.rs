//! Drawing the board's list as an image: the rows rows.rs describes (the same the text
//! list draws), one under the other, each as tall as the type size makes it (a cell for
//! small type, more for medium and large), so a row needn't be a whole cell, and a
//! detail's text wrapped by pixels to its room; the image itself is the list's block of
//! cells. The type is real (font.rs), the marks are drawn shapes (marks.rs) in the
//! status colours (rings, a check, a diamond), the selected task sits on a card (a
//! faint fill and an edge, its mark inside, its first key in a cap at its lower right)
//! with its whole title (a task that's yours gets two lines), a task that needs you on
//! a wash of its colour with a round bar at its left, and a risk is a chip
//! (chip.rs). The title is the loudest thing on a row: the ID, the age and the group's
//! name are a detail's size. The marks that move are left out when the image look
//! places them over the list (sprite.rs draws their frames with `Draw`). The list's
//! last column has the scroll marks. Nothing here touches the terminal: given rows and
//! a size in pixels, it lays them out and returns an image, or a window of pixels of
//! them (pieces.rs), drawn as that part of the whole would be.

use super::chip;
use super::font::{Fonts, Weight};
use super::shape::{diamond, left_slice, paint, rounded};
use super::theme::{lit, solid, Rgb, Theme};
use crate::model::TextSize;
use crate::ui::rows::{Flag, ItemRow, Mark, Row, Rows, TaskRow};
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

/// The type's sizes and the space it gets, in pixels, for a text size and a cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// a task's title, the top line
    pub title: f32,
    /// a detail line, a task's ID and age, a group's name and the message
    pub detail: f32,
    /// a label: a chip's word (a task's risk, the header's counts) and a key in its cap
    pub label: f32,
    /// a title line's height, and a detail line's
    pub line: f32,
    pub detail_line: f32,
    /// the space above a task (or an item), and above a heading
    pub pad: f32,
    pub head_pad: f32,
    /// the space between groups: more than is under a group's name, so the name sits
    /// with its own tasks
    pub gap: f32,
    /// the space between a title's last line and the task's first detail line
    pub title_gap: f32,
    /// how far the selected card reaches past its first line, and past its last (a
    /// little more: a line's type sits low in it, with its descenders); a pixel inside
    /// them for small type, whose rows are a cell each
    pub inset: f32,
    pub below: f32,
    /// small type's: every row a cell, as the text list's (no title but the selected
    /// one's wraps)
    pub cells: bool,
}

impl Metrics {
    /// Small is the terminal's row: every row a cell, as the text list's. Medium and large
    /// set the type larger than a cell holds and give the rows the height it needs, a
    /// detail smaller than a title, and room: about three quarters of a title's size
    /// above each task, a little between its title and its lines, and the selected card
    /// a quarter of it clear of the type. A label is smaller than a detail at every size.
    pub fn of(size: TextSize, cell_h: u32) -> Metrics {
        let ch = cell_h as f32;
        let (t, d, l) = match size {
            TextSize::Small => (0.62, 0.62, 0.50),
            // a terminal's own type is about 0.8 of its cell's height
            TextSize::Medium => (0.80, 0.70, 0.60),
            TextSize::Large => (0.92, 0.80, 0.69),
        };
        let (title, detail, label) = ((t * ch).max(6.0), (d * ch).max(6.0), (l * ch).max(5.0));
        if size == TextSize::Small {
            let (line, detail_line, gap) = (ch, ch, ch);
            let (pad, head_pad, title_gap, inset) = (0.0, 0.0, 0.0, -1.0);
            return Metrics { title, detail, label, line, detail_line, pad, head_pad, gap, title_gap, inset, below: inset, cells: true };
        }
        Metrics {
            title,
            detail,
            label,
            line: (1.40 * title).round(),
            detail_line: (1.42 * detail).round(),
            pad: (0.86 * title).round(),
            head_pad: (0.5 * title).round(),
            gap: (0.75 * ch).round(),
            title_gap: (0.06 * title).round(),
            inset: (0.25 * title).round(),
            below: (0.25 * title).round() + (0.08 * title).round(),
            cells: false,
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

    /// Where text starts (a group's name, the top line, the keys) and ends: a column and
    /// a quarter in from each side (the right's last part has the list's scroll marks).
    /// The rules and the status bar run between the two, and a card reaches three
    /// quarters of a column past each, so it's as far from the pane's left edge as
    /// from its right.
    pub fn left(&self) -> f32 {
        (1.25 * self.c()).round()
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

/// The rows laid out: each one's height in pixels and the space left after it (the
/// selected block's, so the rows under it move by whole cells: pieces.rs), a task's (or
/// an item's) title, or a detail's text, as the lines it's drawn on, and the ID
/// column's width.
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    pub heights: Vec<u32>,
    pub after: Vec<u32>,
    lines: Vec<Vec<String>>,
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

/// The most lines the selected task's title wraps to; one that's yours (it waits on
/// you) wraps to two, but in small type, a cell a row as the text list's; any other
/// title is cut to one.
const TITLE_LINES: usize = 3;
const YOURS_LINES: usize = 2;

/// Lay the rows out: a title cut to its row, or wrapped (the selected one's, and on a
/// narrow board one that's yours), a detail's text wrapped by pixels to the room its
/// type has, and how tall each row is.
pub fn lay(rows: &Rows, c: &Canvas, fonts: &Fonts) -> Laid {
    let idw = id_width(rows, fonts, c.m.detail);
    let g = Geo { c: *c, idw };
    let mut all = Vec::new();
    let mut heights = Vec::new();
    // the room the selected task's key takes on its block's last line
    let mut key_w = 0.0;
    // the last task (or item) was on a wash, which reaches under its block
    let mut washed = false;
    for (i, row) in rows.rows.iter().enumerate() {
        let (title, most, weight, right, rest) = match row {
            Row::Task(t) => {
                key_w = t.key.as_ref().map_or(0.0, |(k, what)| g.key_width(k, what, fonts));
                washed = t.flag.is_some();
                let two = t.yours && !c.m.cells;
                let most = if t.selected { TITLE_LINES } else if two { YOURS_LINES } else { 1 };
                (&t.title, most, title_weight(t.asked.is_some()), g.task_right(t, fonts), g.title_right(t, fonts))
            }
            Row::Item(i) => (&i.title, if i.selected { TITLE_LINES } else { 1 }, title_weight(i.asked), c.right(), c.right()),
            Row::Detail(d) => {
                // a stale age is semibold: the first line has that much less room
                let (age, _) = crate::model::split_at_width(&d.text, d.lead);
                let bolder = if d.stale { fonts.width(&age, Weight::Bold, c.m.detail) - fonts.width(&age, Weight::Regular, c.m.detail) } else { 0.0 };
                // (each key the text names is in a cap: room for their sides on every line)
                let caps = key_letters(&d.text, d.lead).len() as f32 * 2.0 * KEY_SIDE * c.m.detail;
                let mut lines = g.detail_lines(&d.text, d.lines, bolder, caps, fonts);
                let last = !rows.rows.get(i + 1).is_some_and(Row::is_detail);
                // the key goes after its last line, which makes room when it must
                if last && key_w > 0.0 && lines.last().is_some_and(|l| fonts.width(l, Weight::Regular, c.m.detail) > g.detail_room() - key_w) {
                    lines = g.detail_lines(&d.text, d.lines, bolder, key_w + caps, fonts);
                }
                heights.push(g.height(row, lines.len()));
                all.push(lines);
                continue;
            }
            Row::ItemDetail(text, most) => {
                let lines = g.detail_lines(text, *most, 0.0, 0.0, fonts);
                heights.push(g.height(row, lines.len()));
                all.push(lines);
                continue;
            }
            _ => {
                // the space between groups starts under a wash, not under its block;
                // and the list's first group's name is clear of the header over it
                let under = match row {
                    Row::Gap if washed => c.m.below.max(0.0) as u32,
                    Row::Head { .. } if i == 0 && !c.m.cells => (c.m.gap / 2.0).round() as u32,
                    _ => 0,
                };
                washed = false;
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
        heights.push(g.height(row, lines.len().max(1)));
        all.push(lines);
    }
    let after = vec![0; heights.len()];
    Laid { heights, after, lines: all, idw }
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
    c.m.below.max(0.0).ceil() as u32 + 1
}

/// The selected card's edge, in pixels.
const RING: f32 = 3.0;

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
    let mut places = Vec::new();
    let mut y = cut.at;
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
        // (the selected card's edge is in the bar's place)
        if let (Some(f), false) = (b.flag, b.selected) {
            d.bar(b.top, b.bottom, f);
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
    // the selected task's key, at its card's lower right (lay made room for it)
    for b in blocks.iter().filter(|b| b.ends) {
        if let Some((key, what)) = b.key {
            d.action(key, what, b.bottom);
        }
    }
    let mut pm = pm.clone_rect(tiny_skia::IntRect::from_xywh(0, EDGE as i32, width, height).ok_or_else(made)?).ok_or_else(made)?;
    Draw::new(&mut pm, fonts, c, laid.idw).scroll_marks(cut.marks, height as f32);
    Ok(Sheet { pixmap: pm, places })
}

/// A task's (or an item's) block: from its first line's top to its last line's bottom,
/// whether it's selected, and its flag.
struct Block<'r> {
    top: f32,
    bottom: f32,
    selected: bool,
    flag: Option<Flag>,
    /// the selected task's key, and whether its block ends here with a line under it,
    /// where the key goes
    key: Option<&'r (String, String)>,
    ends: bool,
}

/// The blocks of the rows placed: each task's or item's row with the lines under it (a
/// window's first lines, under a row above it, a block of their own).
fn blocks<'r>(rows: &'r Rows, places: &[Place], pad: f32) -> Vec<Block<'r>> {
    let mut out: Vec<Block> = Vec::new();
    let mut open = false;
    for p in places {
        let row = &rows.rows[p.row];
        let (y, bottom) = (p.y as f32, (p.y + p.h) as f32);
        let block = |top: f32, selected: bool, flag: Option<Flag>| Block { top, bottom, selected, flag, key: None, ends: false };
        // a line under a task (or an item) that's its block's last
        let ends = row.is_detail() && !rows.rows.get(p.row + 1).is_some_and(Row::is_detail);
        match row {
            Row::Task(t) => out.push(Block { key: t.key.as_ref(), ..block(y + pad, t.selected, t.flag) }),
            Row::Item(i) => out.push(block(y + pad, i.selected, None)),
            Row::Detail(_) | Row::ItemDetail(..) if open => {
                let b = out.last_mut().expect("a block is open");
                (b.bottom, b.ends) = (bottom, ends);
            }
            Row::Detail(dr) => out.push(Block { ends, ..block(y, dr.selected, dr.flag) }),
            Row::ItemDetail(..) => out.push(Block { ends, ..block(y, true, None) }),
            _ => {}
        }
        open = row.is_entry() || row.is_detail();
    }
    out
}

/// The room each side of a key a detail's text names, inside its cap, of the type's size.
const KEY_SIDE: f32 = 0.28;

/// The keys a detail's text names ("Its agent exited. r resumes it."), as which of its
/// letters they are, spaces not counted (so they're found on its wrapped lines): a
/// lowercase letter alone, first or after a full stop, with a word after it. The first
/// `lead` columns are the task's age.
fn key_letters(text: &str, lead: usize) -> Vec<usize> {
    let (age, rest) = crate::model::split_at_width(text, lead);
    let mut n = age.chars().filter(|c| !c.is_whitespace()).count();
    let words: Vec<&str> = rest.split_whitespace().collect();
    let mut out = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let alone = w.len() == 1 && w.chars().all(|c| c.is_ascii_lowercase());
        if alone && i + 1 < words.len() && (i == 0 || words[i - 1].ends_with('.')) {
            out.push(n);
        }
        n += w.chars().count();
    }
    out
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
    (Geo { c: *c, idw: 0.0 }.mark_x(), c.m.pad + c.m.line / 2.0)
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
    /// A mark's radius, to half a pixel: a little under the title's capital's height
    /// across (small type's, whose title is a detail's size, a little more).
    fn mark_r(&self) -> f32 {
        let m = &self.c.m;
        let of = if m.cells { 0.87 } else { 0.76 };
        ((of * m.title).round() / 2.0).min(0.8 * self.c.c())
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

    /// Where a task's text starts (its ID, and its lines under it): two columns past
    /// its mark's left edge, or past a larger type's mark and a little space.
    fn text_x(&self) -> f32 {
        let c = self.c.c();
        (self.c.left() + 2.0 * c).max(self.mark_x() + self.mark_r() + 0.4 * self.c.m.title)
    }

    /// Where a title starts: past the ID column and a gap, narrower on a board under 44
    /// columns, where every pixel of title counts.
    fn title_x(&self) -> f32 {
        self.text_x() + self.idw + if self.narrow() { 0.5 } else { 0.8 } * self.c.m.title
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
        let mut lines = fonts.wrap(text, Weight::Regular, self.c.m.detail, (room - less, room), most);
        fonts.fill_last(&mut lines, Weight::Regular, self.c.m.detail, room);
        if lines.is_empty() { vec![String::new()] } else { lines }
    }

    /// A detail line's room: from under the ID to where text ends.
    fn detail_room(&self) -> f32 {
        self.c.right() - self.text_x()
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
        if let Some(level) = &t.risk {
            let gap = if t.age.is_empty() { 0.0 } else { 0.5 * self.c.m.detail };
            w += gap + self.risk_width(level, fonts);
        }
        w
    }

    /// How wide a task's risk is: its chip, which on a board under 44 columns, where
    /// the word would take the title's room, has the level's first letter (H, M, L).
    fn risk_width(&self, level: &str, fonts: &Fonts) -> f32 {
        chip::risk_width(fonts, &self.risk_word(level), self.c.m.label)
    }

    fn risk_word(&self, level: &str) -> String {
        if self.narrow() { chip::initial(level) } else { level.to_string() }
    }

    /// Where a task's title's other lines end: short of the age's column, so the ages
    /// have a column of their own down the list, whatever wraps beside them.
    fn title_right(&self, t: &TaskRow, fonts: &Fonts) -> f32 {
        let age = self.age_width(t, fonts);
        self.c.right() - if age > 0.0 { age + 0.8 * self.c.m.title } else { 0.0 }
    }

    /// Where a task's title's first line must end: short of its risk and age, by less
    /// before a chip, whose own room around its word is a gap too.
    fn task_right(&self, t: &TaskRow, fonts: &Fonts) -> f32 {
        let gap = if t.risk.is_some() { 0.65 } else { 0.8 };
        self.c.right() - self.right_side(t, fonts) - gap * self.c.m.title
    }

    /// A row's height in whole pixels, with its title (or a detail's text) on `lines` lines.
    fn height(&self, row: &Row, lines: usize) -> u32 {
        let m = &self.c.m;
        let h = match row {
            Row::Head { .. } => m.head_pad + m.detail_line,
            Row::Task(_) | Row::Item(_) => m.pad + lines as f32 * m.line + m.title_gap,
            Row::Later { .. } => m.pad + m.line,
            Row::Detail(_) | Row::ItemDetail(..) => lines as f32 * m.detail_line,
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

    /// What a block from `top` to `bottom` sits on (Block): three quarters of a cell past where text
    /// starts and ends, on each side (its mark inside it), clear of the type above by
    /// `inset` and under it by `below`, into the space above the next task. As x, y, w, h.
    fn panel(&self, top: f32, bottom: f32) -> (f32, f32, f32, f32) {
        let (m, out) = (self.m(), (0.75 * self.c()).round());
        let x = self.g.c.left() - out;
        (x, top - m.inset, self.right() + out - x, bottom - top + m.inset + m.below)
    }

    /// The selected task's (or item's) card, the same wherever it is: a fill a step
    /// stronger than what's around it (the foreground, faint, or its flag's colour
    /// well over its wash's strength when it needs you or stopped) and an edge three
    /// pixels wide, the terminal's blue (in the bar's place, on a task that needs you).
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
    /// wash's left edge, following its corners.
    fn bar(&mut self, top: f32, bottom: f32, flag: Flag) {
        let (x, y, _, h) = self.panel(top, bottom);
        let w = (0.25 * self.c()).max(3.0);
        let color = solid(lit(self.theme.flag(flag)));
        if let Some(p) = left_slice(x, y, w, h, self.radius()) {
            self.fill(&p, color);
        }
    }

    /// A task's key in a cap and what it does, at the right of its block's last line,
    /// whose bottom is `bottom`.
    fn action(&mut self, key: &str, what: &str, bottom: f32) {
        let px = self.g.key_px();
        let x = self.right() - chip::keycap_width(self.fonts, key, what, px);
        chip::keycap(self, key, what, px, x, bottom - self.m().detail_line / 2.0);
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
            Row::Detail(_) | Row::ItemDetail(..) => self.set(m.detail, m.detail_line),
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
                // a step under the title, but the selected task's and what you must act on
                let color = if d.selected || (d.loud && d.flag.is_some()) { fg } else { dim };
                let x = self.text_x();
                let keys = key_letters(&d.text, d.lead);
                let mut seen = 0;
                for (k, line) in lines.iter().enumerate() {
                    let y = y + k as f32 * m.detail_line;
                    // a narrow board's age starts the first line
                    let (age, rest) = crate::model::split_at_width(line, if k == 0 { d.lead } else { 0 });
                    // (an age that has waited too long is semibold, in the foreground)
                    let age_color = if d.stale { fg } else { dim };
                    let weight = if d.stale { Weight::Bold } else { Weight::Regular };
                    let end = self.text(&age, weight, x, y, self.right() - x, age_color);
                    seen += age.chars().filter(|c| !c.is_whitespace()).count();
                    self.said(&rest, end, y, color, &keys, &mut seen);
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
            Row::ItemDetail(..) => {
                let x = self.text_x();
                for (k, line) in lines.iter().enumerate() {
                    self.text(line, Weight::Regular, x, y + k as f32 * m.detail_line, self.right() - x, fg);
                }
            }
        }
    }

    /// A detail's line from x, on the line whose top is y: its text in `color`, the
    /// keys it names (`keys`: which of the text's letters, `seen` of them drawn on the
    /// lines before) semibold in the foreground, each in a small cap.
    fn said(&mut self, line: &str, x: f32, y: f32, color: [u8; 4], keys: &[usize], seen: &mut usize) {
        let fg = self.theme.text();
        let (mut x, mut run, mut key) = (x, String::new(), false);
        for c in line.chars().chain(std::iter::once('\0')) {
            let is_key = !c.is_whitespace() && keys.contains(seen);
            if (is_key != key || c == '\0') && !run.is_empty() {
                let (weight, color) = if key { (Weight::Bold, fg) } else { (Weight::Regular, color) };
                if key {
                    // in a small cap, as the key row's (lay left room for its sides)
                    let (w, px) = (self.width(&run, weight), self.px());
                    let (side, ch) = (KEY_SIDE * px, (1.25 * px).round());
                    let top = (y + (self.h() - ch) / 2.0).round();
                    let (cap, edge) = (self.theme.cap(), self.theme.cap_edge());
                    self.rounded_fill(x, top, w + 2.0 * side, ch, 0.25 * px, cap);
                    self.rounded_edge(x, top, w + 2.0 * side, ch, 0.25 * px, 1.0, edge);
                    x = self.text(&run, weight, x + side, y, f32::MAX, color) + side;
                } else {
                    x = self.text(&run, weight, x, y, f32::MAX, color);
                }
                run.clear();
            }
            key = is_key;
            if c != '\0' {
                run.push(c);
                *seen += usize::from(!c.is_whitespace());
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
            let color = solid(t.flag.map_or(self.theme.red, |f| self.theme.flag(f)));
            let ink = solid(self.theme.bg);
            chip::solid(self, &t.age, self.m().label, right, cy, color, ink);
        } else {
            let (weight, color) = if let Some(c) = tone { (Weight::Bold, c) } else { (Weight::Regular, self.theme.dim()) };
            let x = right - self.fonts.width(&t.age, weight, small);
            self.text_in(small, &t.age, weight, x, y, age_w + 1.0, color);
        }
        right -= age_w;
        if let Some(level) = &t.risk {
            let gap = if t.age.is_empty() { 0.0 } else { 0.5 * small };
            let color = self.theme.risk(level);
            let color = solid(lit(Rgb(color[0], color[1], color[2])));
            chip::risk(self, &self.g.risk_word(level), self.m().label, right - gap, cy, color);
        }
        let color = match tone {
            Some(c) => c,
            None if t.selected || t.yours => self.theme.text(),
            None if t.quiet => self.theme.quiet(),
            None => self.theme.soft(),
        };
        let (first, rest) = (self.g.task_right(t, self.fonts), self.g.title_right(t, self.fonts));
        self.title(lines, title_weight(tone.is_some()), (first, rest), y, color);
    }

    /// A title's lines from the title column, the first ending at `first`, the rest at `rest`.
    fn title(&mut self, lines: &[String], weight: Weight, (first, rest): (f32, f32), y: f32, color: [u8; 4]) {
        let x = self.title_x();
        for (k, line) in lines.iter().enumerate() {
            let end = if k == 0 { first } else { rest };
            self.text(line, weight, x, y + k as f32 * self.h(), end - x + 1.0, color);
        }
    }

    /// An ID, at a detail's size on the title's baseline: dim (a step up on a wash,
    /// `washed`, where dim is fainter), or semibold in the foreground when selected, or
    /// in `tone` when it's asked about. Returns where it ends.
    fn id(&mut self, id: &str, selected: bool, washed: bool, tone: Option<[u8; 4]>, y: f32) -> f32 {
        let (x, px) = (self.text_x(), self.m().detail);
        let (weight, color) = match tone {
            Some(c) => (Weight::Bold, c),
            None if selected => (Weight::Bold, self.theme.text()),
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
        self.title(lines, title_weight(i.asked), (right, right), y, color);
    }
}

#[path = "marks.rs"]
mod marks;
pub use marks::Marks;

#[cfg(test)]
#[path = "tests/test_render.rs"]
mod tests;
