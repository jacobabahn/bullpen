//! The selected task's detail in mission control's image look, a page beside the list
//! (docs/mission.md): its ID, repository and folder, small; its title, large; its mark
//! and status, coloured as its mark, and its age; its keys in caps, `p` and `⏎` first;
//! what its agent is doing, where it stands and the size of its change; then, for a task
//! in review, ready or finished, what its worker said, under a rule. The image is only
//! as tall as what it says (a whole number of cells, at most the list's), and its type
//! no wider than reads well. A child of render.rs, so it draws with `Draw`'s own parts.

use super::{Canvas, Draw, Weight};
use crate::image::chip;
use crate::image::font::Fonts;
use crate::image::theme::{lit, solid, Rgb};
use crate::ui::rows::Mark;
use tiny_skia::Pixmap;

/// The page's values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Page {
    pub id: String,
    /// its repository and folder: "bullpen · ~/dev/bullpen"
    pub place: String,
    pub title: String,
    pub mark: Mark,
    /// its status, whose colour the status's words take
    pub status: String,
    /// its status in words ("needs you") and its age
    pub words: String,
    pub age: String,
    pub keys: Vec<(String, String)>,
    /// what its agent is doing, where it stands, its change's size: each with its ink
    pub lines: Vec<(String, Ink)>,
    /// what its worker said, a paragraph each
    pub said: Vec<String>,
}

/// A line's colour: the foreground, a step under it, or dim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ink {
    Text,
    Soft,
    Dim,
}

/// The widest the page's image is, in columns: its lines no longer than read well.
pub const MOST_COLS: usize = 96;

/// The title's size, of a task's title's, and its lines' height, of its size.
const TITLE: f32 = 1.1;
const TITLE_LINE: f32 = 1.32;

/// How many cells high the page is, at most `most`.
pub fn rows(page: &Page, c: &Canvas, fonts: &mut Fonts, most: u16) -> u16 {
    let mut pm = Pixmap::new(1, 1).expect("a pixel");
    let mut d = Draw::new(&mut pm, fonts, c, 0.0);
    let h = d.page(page, f32::MAX);
    ((h / c.grid.cell_h as f32).ceil() as u16).clamp(1, most.max(1))
}

/// The page drawn, `rows` cells high (what doesn't fit is left out).
pub fn draw(page: &Page, rows: u16, c: &Canvas, fonts: &mut Fonts) -> Result<Pixmap, String> {
    let (w, h) = (c.grid.width(), rows as u32 * c.grid.cell_h);
    let mut pm = Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| format!("can't make an image {w}×{h} pixels"))?;
    Draw::new(&mut pm, fonts, c, 0.0).page(page, h as f32);
    Ok(pm)
}

impl Draw<'_> {
    /// Draw the page down to `bottom` (lines past it are left out); returns how tall
    /// it is whole.
    fn page(&mut self, p: &Page, bottom: f32) -> f32 {
        let m = self.m();
        let theme = self.theme;
        let (l, right) = (self.g.c.left(), self.right());
        let room = right - l;
        let ink = |i: Ink| match i {
            Ink::Text => theme.text(),
            Ink::Soft => theme.soft(),
            Ink::Dim => theme.dim(),
        };
        let mut y = m.pad;
        // the ID and where it is, small
        self.set(m.detail, m.detail_line);
        let end = self.text(&p.id, Weight::Bold, l, y, room, theme.dim());
        let gap = 0.6 * m.detail;
        self.text(&p.place, Weight::Regular, end + gap, y, right - end - gap, theme.dim());
        y += m.detail_line + (0.2 * m.title).round();
        // the title, large, on up to three lines
        let px = (TITLE * m.title).round();
        let line = (TITLE_LINE * px).round();
        self.set(px, line);
        let mut lines = self.fonts.wrap(&p.title, Weight::Bold, px, (room, room), 3);
        self.fonts.no_widow(&mut lines, Weight::Bold, px, room);
        for s in &lines {
            self.text(s, Weight::Bold, l, y, room, theme.text());
            y += line;
        }
        y += (0.35 * m.title).round();
        // its mark and its status, in its colour, and its age
        self.set(m.title, m.line);
        let mr = self.mark_r();
        self.mark(&p.mark, l + mr, y + m.line / 2.0, 0.0);
        let color = theme.status(&p.status).map_or(theme.text(), |c| solid(lit(Rgb(c[0], c[1], c[2]))));
        let x = l + 2.0 * mr + 0.6 * m.title;
        let end = self.text_in(m.detail, &p.words, Weight::Bold, x, y, right - x, color);
        if !p.age.is_empty() {
            self.text_in(m.detail, &format!("  ·  {}", p.age), Weight::Regular, end, y, right - end, theme.dim());
        }
        y += m.line;
        // its keys in caps, as many as fit
        let kpx = m.label;
        let mut x = l;
        for (k, what) in &p.keys {
            let w = chip::keycap_width(self.fonts, k, what, kpx);
            if x + w > right {
                break;
            }
            x = chip::keycap(self, k, what, kpx, x, y + m.detail_line / 2.0) + 1.1 * kpx;
        }
        y += m.detail_line + (0.4 * m.title).round();
        // what its agent is doing, where it stands, its change
        self.set(m.body, m.body_line);
        for (text, i) in &p.lines {
            for s in self.fonts.wrap(text, Weight::Regular, m.body, (room, room), usize::MAX) {
                if y + m.body_line <= bottom {
                    self.text(&s, Weight::Regular, l, y, room, ink(*i));
                }
                y += m.body_line;
            }
            y += (0.3 * m.body_line).round();
        }
        if p.said.is_empty() {
            return y + m.pad;
        }
        // what its worker said, under a rule
        y += (0.4 * m.title).round();
        if y < bottom {
            self.rounded_fill(l, y, room, 1.5, 0.0, theme.rule());
        }
        y += (0.6 * m.title).round();
        self.set(m.detail, m.detail_line);
        if y + m.detail_line <= bottom {
            self.text("What its worker said", Weight::Bold, l, y, room, theme.heading());
        }
        y += m.detail_line + (0.15 * m.title).round();
        self.set(m.body, m.body_line);
        for para in &p.said {
            for s in self.fonts.wrap(para, Weight::Regular, m.body, (room, room), usize::MAX) {
                if y + m.body_line <= bottom {
                    self.text(&s, Weight::Regular, l, y, room, theme.dim());
                }
                y += m.body_line;
            }
            y += (0.4 * m.body_line).round();
        }
        y + m.pad
    }
}

#[cfg(test)]
#[path = "tests/test_page.rs"]
mod tests;
