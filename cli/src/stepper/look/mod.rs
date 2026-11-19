//! The tour drawn as images (`board_look image`, on a terminal that draws kitty
//! graphics): the stepper's state (screen.rs's `Stepper`, the same keys) shown as the
//! rail of every step at the left (rail.rs), the step as a page at the right, its code
//! drawn (page.rs, code.rs), and a foot under the page with the message and the keys.
//! What's typed (a comment, a tell) stays text, in the foot's place.
//!
//! A page is measured once (its rows kept) and sent in tiles: strips of as many rows as
//! fit under the terminal's limit on an image (bullpen-draw's cut.rs: Rex draws none
//! over 10,000,000 bytes) at the page's width, and never more than the window. Each is
//! drawn on an image the tile's height, the page shifted up by the tile's top, so a long
//! step is never an image of its own. The window on the page is crops of the tiles it
//! covers, so scrolling places crops (a few dozen bytes each) and sends a tile only the
//! first time it's reached. The rail and the foot are cut into pieces when they're over
//! the limit (shown.rs). An image the tour can't make (past `MOST`, or not cut under the
//! limit) is an error, and the tour is text from then on (view.rs). While no key waits
//! (`ahead`), the tile under the window and the one above it, the next step's rail, the
//! tiles of its first window and its foot, a window's tiles under and over it, and the
//! previous step's rail, first tile and foot are drawn and sent unplaced, an image at a time with a key looked for before
//! each, so `n` and `p` usually only place images and a key waits for one strip at
//! most. Images are kept on the terminal, hidden, to place again, and deleted when the
//! tour closes (shown.rs).

pub mod ask;
pub mod code;
pub mod ink;
pub mod page;
pub mod rail;
mod shown;

use super::screen::{Mode, Stepper};
use bullpen_core::json::{s, Obj};
use bullpen_draw::cut;
use bullpen_draw::kitty::Place;
use bullpen_draw::theme::Theme;
use ink::{Ink, Type};
use serde_json::Value;
use shown::Shown;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Write;

/// The tour's images get IDs from here up (the graphics query's is FIRST - 1).
pub const FIRST: u32 = 0x5450_0001;

/// A step's patch, from git: patch(step).
pub type PatchFn = Box<dyn Fn(usize) -> String>;

/// Where things go on a terminal `cols` by `rows`: the rail's columns (none on a narrow
/// pane), the page's first column and its columns, the rows a tile is (the window's, or
/// as many as fit under the terminal's limit: `fitted`), and the rows the window on the
/// page has now (fewer while something is typed under it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lay {
    pub cols: u16,
    pub rows: u16,
    pub rail: u16,
    pub page_cols: u16,
    pub tile: u16,
    pub view: u16,
}

/// The rows the foot has.
const FOOT: u16 = 2;

/// The most pixels (bytes) an image is drawn on: a pane's, at a 2× cell, is a few tens
/// of MB; past this the tour is text rather than risk the memory.
const MOST: usize = 256 << 20;

impl Lay {
    pub fn new(cols: i64, rows: i64, typed: usize) -> Lay {
        let (cols, rows) = (cols.clamp(1, 1000) as u16, rows.clamp(3, 1000) as u16);
        let rail = if cols >= 80 {
            (cols as u32 * 23 / 100).clamp(24, 40) as u16
        } else {
            0
        };
        let tile = rows - FOOT;
        let view = if typed > 0 {
            rows.saturating_sub(typed as u16 + 1).max(1)
        } else {
            tile
        };
        Lay {
            cols,
            rows,
            rail,
            page_cols: cols - rail,
            tile,
            view,
        }
    }

    /// The layout with tiles under the terminal's limit at a cell `cell` pixels: the
    /// window's rows shared evenly by as few tiles as fit (at least a row each: a row
    /// wider is cut across), so the first frame sends no rows past the window.
    pub fn fitted(self, (cw, ch): (u32, u32)) -> Lay {
        let fit = cut::rows_fitting(self.page_cols as u32 * cw, ch).clamp(1, u16::MAX as u32) as u16;
        Lay {
            tile: self.tile.div_ceil(self.tile.div_ceil(fit)),
            ..self
        }
    }
}

pub struct Look {
    pub th: Theme,
    /// a cell's width and height in pixels
    pub cell: (u32, u32),
    ty: Type,
    shown: Shown,
    patch: PatchFn,
    /// the rows of every page measured, by its key: a page is never kept as pixels
    rows: HashMap<u64, i64>,
    /// the most pixels (bytes) an image drawn has had
    largest: usize,
    /// the layout last drawn, and whether text was under the page
    last: Option<Lay>,
    texted: bool,
}

fn key(of: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    of.hash(&mut h);
    h.finish()
}

impl Look {
    pub fn new(th: Theme, cell: (u32, u32), ty: Type, patch: PatchFn) -> Look {
        Look {
            th,
            cell,
            ty,
            shown: Shown::new(FIRST, cell),
            patch,
            rows: HashMap::new(),
            largest: 0,
            last: None,
            texted: false,
        }
    }

    /// The scale things are drawn at: 1 at a 2× cell (34 pixels high).
    fn scale(&self) -> f32 {
        self.cell.1 as f32 / 34.0
    }

    /// How many images are kept on the terminal.
    #[cfg(test)]
    pub fn kept(&self) -> usize {
        self.shown.kept()
    }

    /// The step's comments, for this tour's commit.
    fn on_step<'a>(st: &'a Stepper, i: usize) -> Vec<&'a str> {
        let n = Value::from(i as i64 + 1);
        st.comments
            .iter()
            .filter(|c| c.get("step") == Some(&n) && c.get("commit").unwrap_or(&Value::Null) == &st.commit)
            .map(|c| s(c, "text"))
            .collect()
    }

    /// What page i shows, as a key: the step and your comments on it (or the end and all
    /// of them), at this width, cell and theme.
    fn page_key(&self, st: &Stepper, i: usize, width: u32) -> u64 {
        let what = (width, self.cell, self.th);
        if i >= st.steps.len() {
            let comments: Vec<(String, String, &str)> = st
                .comments
                .iter()
                .map(|c| (field(c, "step"), field(c, "title"), s(c, "text")))
                .collect();
            return key((what, "end", &st.tid, st.steps.len(), comments));
        }
        let step = &st.steps[i];
        let (title, note) = (s(step, "title"), s(step, "note"));
        key((what, i, title, note, Self::on_step(st, i)))
    }

    /// What page i says (the end's at i == the number of steps), its code from git.
    fn page_of<'a>(&self, st: &'a Stepper, i: usize) -> page::Page<'a> {
        if i >= st.steps.len() {
            let comments = st
                .comments
                .iter()
                .map(|c| (field(c, "step"), field(c, "title"), s(c, "text").to_string()))
                .collect();
            return page::Page::End(page::End {
                tid: &st.tid,
                steps: st.steps.len(),
                comments,
            });
        }
        let step = &st.steps[i];
        page::Page::Step(page::Step {
            title: s(step, "title"),
            refs: refs(step),
            note: s(step, "note"),
            comments: Self::on_step(st, i),
            files: code::parse(&(self.patch)(i)),
        })
    }

    /// The rows page i has: measured once, on an image a pixel high (the code under it
    /// isn't drawn), and kept.
    pub fn page_rows(&mut self, st: &Stepper, i: usize, lay: Lay) -> i64 {
        let width = lay.page_cols as u32 * self.cell.0;
        let k = self.page_key(st, i, width);
        if let Some(rows) = self.rows.get(&k) {
            return *rows;
        }
        let p = self.page_of(st, i);
        let (th, s, ch) = (self.th, self.scale(), self.cell.1);
        let Some(mut ink) = Ink::new(width, 1, &mut self.ty, th, s) else {
            return 1;
        };
        let rows = ((page::draw(&mut ink, &p) / ch as f32).ceil() as i64).max(1);
        self.rows.insert(k, rows);
        rows
    }

    /// An image w by h pixels to draw on, from oy down what's drawn; an error when it
    /// can't be made.
    fn ink(&mut self, w: u32, h: u32, oy: f32) -> std::io::Result<Ink<'_>> {
        let bytes = w as usize * h as usize * 4;
        let (th, s) = (self.th, self.scale());
        let ink = (bytes <= MOST)
            .then(|| Ink::window(w, h, oy, &mut self.ty, th, s))
            .flatten();
        let ink = ink.ok_or_else(|| std::io::Error::other(format!("can't make an image {w}×{h} pixels")))?;
        self.largest = self.largest.max(bytes);
        Ok(ink)
    }

    /// Tile t of page i sent: its ID, and the rows it has. A tile is the page drawn on an
    /// image the tile's height, shifted up by its top; one sent before is placed again.
    fn tile(
        &mut self,
        st: &Stepper,
        i: usize,
        t: i64,
        lay: Lay,
        out: &mut impl Write,
    ) -> std::io::Result<Option<(u32, i64)>> {
        let rows = self.page_rows(st, i, lay);
        let from = t * lay.tile as i64;
        if from >= rows || t < 0 {
            return Ok(None);
        }
        let n = (rows - from).min(lay.tile as i64);
        let width = lay.page_cols as u32 * self.cell.0;
        let k = key((self.page_key(st, i, width), t, lay.tile));
        if let Some(id) = self.shown.id(k) {
            return Ok(Some((id, n)));
        }
        let ch = self.cell.1;
        let p = self.page_of(st, i);
        let mut ink = self.ink(width, n as u32 * ch, (from * ch as i64) as f32)?;
        page::draw(&mut ink, &p);
        let pm = ink.pm;
        Ok(Some((self.shown.send(k, &pm, out)?, n)))
    }

    /// The rail at step i sent: its ID.
    fn rail(&mut self, st: &Stepper, i: usize, lay: Lay, out: &mut impl Write) -> std::io::Result<Option<u32>> {
        let comments = (0..st.steps.len()).map(|k| Self::on_step(st, k).len()).collect();
        let r = rail::Rail {
            tid: st.tid.clone(),
            title: st.title.clone(),
            steps: st.steps.iter().map(|o| s(o, "title").to_string()).collect(),
            at: i,
            comments,
        };
        let k = key((&r, lay.rail, lay.rows, self.cell, self.th));
        if let Some(id) = self.shown.id(k) {
            return Ok(Some(id));
        }
        let (w, h) = (lay.rail as u32 * self.cell.0, lay.rows as u32 * self.cell.1);
        let mut ink = self.ink(w, h, 0.0)?;
        rail::draw(&mut ink, &r);
        let pm = ink.pm;
        Ok(Some(self.shown.send(k, &pm, out)?))
    }

    /// The foot sent: its ID.
    fn foot(&mut self, f: &rail::Foot, lay: Lay, out: &mut impl Write) -> std::io::Result<Option<u32>> {
        let k = key((f, lay.page_cols, self.cell, self.th));
        if let Some(id) = self.shown.id(k) {
            return Ok(Some(id));
        }
        let (w, h) = (lay.page_cols as u32 * self.cell.0, FOOT as u32 * self.cell.1);
        let ch = self.cell.1 as f32;
        let mut ink = self.ink(w, h, 0.0)?;
        rail::foot(&mut ink, f, ch);
        let pm = ink.pm;
        Ok(Some(self.shown.send(k, &pm, out)?))
    }

    fn place(&self, id: u32, p: u32, (col, row): (u16, u16), crop_y: i64, (cols, rows): (u16, u16)) -> Place {
        let (cw, ch) = self.cell;
        Place {
            id,
            p,
            col,
            row,
            crop_x: 0,
            crop_y: (crop_y * ch as i64) as u32,
            crop_w: cols as u32 * cw,
            crop_h: rows as u32 * ch,
            cols,
            rows,
            z: 0,
        }
    }

    /// Draw the stepper on a terminal `cols` by `rows`: send what's new, place what
    /// moved, take off what's gone; what's typed is text under the page. Scrolls are
    /// clamped to the page here.
    pub fn frame(&mut self, st: &mut Stepper, (cols, rows): (i64, i64), out: &mut impl Write) -> std::io::Result<()> {
        let typing = st.mode != Mode::Read;
        let typed = if typing {
            st.typed_box(cols - Lay::new(cols, rows, 0).rail as i64, rows).0
        } else {
            Vec::new()
        };
        let lay = Lay::new(cols, rows, typed.len()).fitted(self.cell);
        if self.last.is_some_and(|l| (l.cols, l.rows) != (lay.cols, lay.rows)) {
            self.shown.hide_all(out)?;
            self.put(out, "\x1b[2J")?;
        }
        let page_rows = self.page_rows(st, st.at, lay);
        st.top = st.top.clamp(0, (page_rows - lay.view as i64).max(0));
        // what's typed goes first: text, where the foot was
        if typing {
            let first = lay.rows - typed.len() as u16 - 1;
            let mut text = String::new();
            for (k, line) in typed
                .iter()
                .chain(std::iter::once(&format!(
                    " \x1b[2m{}\x1b[0m",
                    st.typing_keys(lay.page_cols as i64)
                )))
                .enumerate()
            {
                let line = super::screen::clip_line(line, lay.page_cols as i64 - 1);
                text += &format!("\x1b[{};{}H\x1b[K{line}\x1b[0m", first + k as u16 + 1, lay.rail + 1);
            }
            self.put(out, &text)?;
        } else if self.texted {
            let mut text = String::new();
            for r in lay.view..lay.rows {
                text += &format!("\x1b[{};{}H\x1b[K", r + 1, lay.rail + 1);
            }
            self.put(out, &text)?;
        }
        self.texted = typing;
        let mut wants = Vec::new();
        if lay.rail > 0 {
            if let Some(id) = self.rail(st, st.at, lay, out)? {
                wants.push(self.place(id, 1, (0, 0), 0, (lay.rail, lay.rows)));
            }
        }
        // the window on the page: one tile's crop, or the end of one and the start of the next
        let (top, view, tile) = (st.top, lay.view as i64, lay.tile as i64);
        let mut row = 0i64;
        let mut t = top / tile;
        while row < view {
            let Some((id, n)) = self.tile(st, st.at, t, lay, out)? else {
                break;
            };
            let from = (top + row) - t * tile;
            let rows = (n - from).min(view - row);
            if rows <= 0 {
                break;
            }
            let p = 1 + (t % 2) as u32;
            wants.push(self.place(id, p, (lay.rail, row as u16), from, (lay.page_cols, rows as u16)));
            row += rows;
            t += 1;
        }
        if !typing {
            let f = rail::Foot {
                msg: st.msg.clone(),
                more: page_rows - top - view > 0,
                end: st.at_end(),
            };
            if let Some(id) = self.foot(&f, lay, out)? {
                wants.push(self.place(id, 1, (lay.rail, lay.rows - FOOT), 0, (lay.page_cols, FOOT)));
            }
        }
        self.shown.show(&wants, out)?;
        let cursor = if typing {
            let (_, (r, c)) = st.typed_box(lay.page_cols as i64, lay.rows as i64);
            let first = lay.rows as i64 - typed.len() as i64 - 1;
            format!("\x1b[?25h\x1b[{};{}H", first + r + 1, lay.rail as i64 + c + 1)
        } else {
            "\x1b[?25l".to_string()
        };
        self.put(out, &cursor)?;
        self.last = Some(lay);
        out.flush()
    }

    /// While no key waits: send what a key would want next, unplaced, an image at a
    /// time. The tile under the window and the one over it, then the next step's rail,
    /// the tiles of its first window and its foot, then the rest of a window's tiles
    /// under and over it, then the previous step's rail, first tile and foot. On a large pane a tile is a strip under the terminal's limit (a
    /// couple of hundred KB of PNG), and a key is looked for before each, so a key waits
    /// for one image at most. Stops when `waiting` says a key came; whether there's more
    /// to send.
    pub fn ahead(&mut self, st: &Stepper, waiting: &dyn Fn() -> bool, out: &mut impl Write) -> std::io::Result<bool> {
        let Some(lay) = self.last else { return Ok(false) };
        if st.mode != Mode::Read {
            return Ok(false);
        }
        let tile = lay.tile as i64;
        let (first, last) = (st.top / tile, (st.top + lay.view as i64 - 1) / tile);
        let n = st.steps.len();
        let window = (lay.view as i64 + tile - 1) / tile;
        // step i's rail, its first `tiles` tiles and its foot (none past the end)
        let step = |i: usize, tiles: i64| -> Vec<Ahead> {
            let rail = (i <= n && lay.rail > 0).then_some(Ahead::Rail(i));
            let tiles = (0..tiles).map(move |t| Ahead::Tile(i, t));
            rail.into_iter()
                .chain(tiles)
                .chain([Ahead::Foot(i)])
                .filter(|_| i <= n)
                .collect()
        };
        let mut jobs = vec![Ahead::Tile(st.at, last + 1), Ahead::Tile(st.at, first - 1)];
        jobs.extend(step(st.at + 1, window));
        jobs.extend((2..=window).flat_map(|k| [Ahead::Tile(st.at, last + k), Ahead::Tile(st.at, first - k)]));
        jobs.extend(step(st.at.wrapping_sub(1), 1));
        for job in jobs {
            if waiting() {
                out.flush()?;
                return Ok(true);
            }
            match job {
                Ahead::Tile(i, t) => {
                    self.tile(st, i, t, lay, out)?;
                }
                Ahead::Rail(i) => {
                    self.rail(st, i, lay, out)?;
                }
                Ahead::Foot(i) => {
                    let rows = self.page_rows(st, i, lay);
                    let f = rail::Foot {
                        msg: String::new(),
                        more: rows > lay.view as i64,
                        end: i == n,
                    };
                    self.foot(&f, lay, out)?;
                }
            }
        }
        out.flush()?;
        Ok(false)
    }

    fn put(&mut self, out: &mut impl Write, text: &str) -> std::io::Result<()> {
        out.write_all(text.as_bytes())
    }

    /// The tour closes: every image it sent is deleted.
    pub fn close(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        self.shown.delete_all(out)?;
        out.flush()
    }
}

/// An image drawn ahead: a tile of step i's page, its rail or its foot.
enum Ahead {
    Tile(usize, i64),
    Rail(usize),
    Foot(usize),
}

/// A comment's field as the text tour says it.
fn field(c: &Obj, k: &str) -> String {
    c.get(k).map_or("None".into(), bullpen_core::json::py_str)
}

/// A step's files and their line ranges ("1–8"), in its order.
fn refs(step: &Obj) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for r in step
        .get("hunks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
    {
        let file = s(r, "file").to_string();
        let lines = r.get("lines").and_then(Value::as_array);
        let range = match lines.map(|l| (l.first().and_then(Value::as_i64), l.get(1).and_then(Value::as_i64))) {
            Some((Some(a), Some(b))) => vec![format!("{a}–{b}")],
            _ => vec![],
        };
        match out.iter_mut().find(|(f, _)| *f == file) {
            Some((_, v)) => v.extend(range),
            None => out.push((file, range)),
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/test_look.rs"]
mod tests;
