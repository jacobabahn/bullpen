//! What the drawn tour draws with: an image (`Ink`) in the terminal's colours, its type
//! (Inter for prose, JetBrains Mono for code), shapes, prose with `code` in it wrapped
//! by pixels (a code span on a faint chip, in the monospace), and the board's keycaps.
//! An image is a window on what's drawn on it: a tile of a page is the page drawn
//! shifted up by the tile's top, so a long page is never an image of its own.
//! The type's sizes come from the terminal's cell (`Sizes`): code at the size where a
//! character's advance is the cell's width, one to a cell as the terminal's own, and
//! prose from the cell's height as the board's medium (or large) type. The rest (room,
//! marks, cards) is the tour's at a 2× cell (34 pixels high), scaled to the terminal's (`s`).

use bullpen_core::text::char_width;
use bullpen_draw::font::{Fonts, Weight};
use bullpen_draw::shape::{paint, rounded};
use bullpen_draw::theme::{faint, Theme};
use tiny_skia::{FillRule, Pixmap, Rect, Stroke, Transform};

/// The faces text is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Ui,
    UiBold,
    Mono,
    MonoBold,
}

/// The tour's type: prose (Inter, or `board_font`) and code, and whether the prose is
/// large (`board_text_size large`; else medium, the board's default and the tour's least).
pub struct Type {
    pub ui: Fonts,
    pub mono: Fonts,
    pub large: bool,
}

impl Type {
    pub fn new(ui: Fonts) -> Type {
        Type {
            ui,
            mono: Fonts::mono(),
            large: false,
        }
    }

    fn face(&mut self, face: Face) -> (&mut Fonts, Weight) {
        match face {
            Face::Ui => (&mut self.ui, Weight::Regular),
            Face::UiBold => (&mut self.ui, Weight::Bold),
            Face::Mono => (&mut self.mono, Weight::Regular),
            Face::MonoBold => (&mut self.mono, Weight::Bold),
        }
    }

    pub fn width(&mut self, text: &str, face: Face, px: f32) -> f32 {
        let (f, w) = self.face(face);
        f.width(text, w, px)
    }

    /// Text cut to max pixels, with "…" when it was.
    pub fn fit(&mut self, text: &str, face: Face, px: f32, max: f32) -> String {
        let (f, w) = self.face(face);
        f.fit(text, w, px, max)
    }

    /// Text wrapped to max pixels, `most` lines at most, no word alone on its last line.
    pub fn wrap(&mut self, text: &str, face: Face, px: f32, max: f32, most: usize) -> Vec<String> {
        let (f, w) = self.face(face);
        let mut lines = f.wrap(text, w, px, (max, max), most);
        f.no_widow(&mut lines, w, px, max);
        lines
    }

    pub fn cap_height(&self, px: f32) -> f32 {
        self.ui.cap_height(px)
    }
}

/// A word of prose: its text, whether it's code, and whether it follows the word before
/// with no space ("`yes`," is a code span, then a comma glued to it).
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub code: bool,
    pub glued: bool,
}

/// The words of prose with `code` spans in it. A short span is one word, kept whole; a
/// backtick with none to close it is text.
pub fn words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut spaced = true;
    let parts: Vec<&str> = text.split('`').collect();
    let unclosed = parts.len() % 2 == 0;
    for (i, part) in parts.iter().enumerate() {
        let code = i % 2 == 1 && !(unclosed && i == parts.len() - 1);
        if code && !part.is_empty() && part.chars().count() < 28 {
            out.push(Word {
                text: part.to_string(),
                code,
                glued: !spaced,
            });
            spaced = false;
            continue;
        }
        for (k, w) in part.split_whitespace().enumerate() {
            let glued = k == 0 && !spaced && !part.starts_with(char::is_whitespace);
            out.push(Word {
                text: w.to_string(),
                code,
                glued,
            });
        }
        if !part.is_empty() {
            spaced = part.ends_with(char::is_whitespace);
        }
    }
    out
}

/// The sizes the tour is drawn at on a terminal, from its cell (pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// a cell's width and height
    pub cell: (f32, f32),
    /// the room, marks and cards: 1 at a 2× cell (34 pixels high)
    pub s: f32,
    /// the page's title
    pub heading: f32,
    /// what you read: the note, comments, the rail's steps (the board's title size)
    pub body: f32,
    /// the files line, the rail's task, the end's labels (the board's detail size)
    pub detail: f32,
    /// the foot's keys (the board's label size)
    pub label: f32,
    /// code's em: its advance a cell's width, unless that's too tall for a row
    pub code: f32,
}

/// The board's medium type, of a cell's height: its titles, its details and its labels
/// (board/src/image/render.rs's `Metrics`); large is about 1.15 times it. The page's
/// title is a step above.
const BODY: f32 = 0.80;
const DETAIL: f32 = 0.70;
const LABEL: f32 = 0.60;
const HEADING: f32 = 1.15;
const LARGE: f32 = 1.15;

/// The most of a row's height code's em is: a taller one's capitals and descenders
/// would touch the next row's.
const CODE_ROW: f32 = 0.9;

impl Sizes {
    pub fn of((cw, ch): (u32, u32), ty: &mut Type) -> Sizes {
        let (cw, ch) = (cw.max(1) as f32, ch.max(1) as f32);
        let k = if ty.large { LARGE } else { 1.0 };
        let advance = ty.width("0", Face::Mono, 1.0).max(0.1);
        Sizes {
            cell: (cw, ch),
            s: ch / 34.0,
            heading: HEADING * k * ch,
            body: BODY * k * ch,
            detail: DETAIL * k * ch,
            label: LABEL * k * ch,
            code: (cw / advance).min(CODE_ROW * ch),
        }
    }

    /// y moved down to the next row's top.
    pub fn on_row(&self, y: f32) -> f32 {
        (y / self.cell.1 - 0.001).ceil() * self.cell.1
    }
}

/// An image being drawn: its pixels, the type, the colours and the sizes (`s`, the
/// scale of the rest, is theirs: 1 at a 2× cell's 34 pixels).
pub struct Ink<'a> {
    pub pm: Pixmap,
    /// where the image's top is in what's drawn: what's above it or under it isn't drawn
    pub oy: f32,
    pub ty: &'a mut Type,
    pub th: Theme,
    pub sz: Sizes,
    pub s: f32,
}

impl Ink<'_> {
    pub fn new(w: u32, h: u32, ty: &mut Type, th: Theme, sz: Sizes) -> Option<Ink<'_>> {
        Some(Ink {
            pm: Pixmap::new(w.max(1), h.max(1))?,
            oy: 0.0,
            ty,
            th,
            sz,
            s: sz.s,
        })
    }

    pub fn w(&self) -> f32 {
        self.pm.width() as f32
    }

    /// An image `h` high showing what's drawn from `oy` down.
    pub fn window(w: u32, h: u32, oy: f32, ty: &mut Type, th: Theme, sz: Sizes) -> Option<Ink<'_>> {
        let mut ink = Ink::new(w, h, ty, th, sz)?;
        ink.oy = oy;
        Some(ink)
    }

    /// Does the band from y, h high, show in the image?
    pub fn shows(&self, y: f32, h: f32) -> bool {
        y + h > self.oy && y < self.oy + self.pm.height() as f32
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: [u8; 4]) {
        if let Some(r) = Rect::from_xywh(x, y - self.oy, w, h) {
            self.pm.fill_rect(r, &paint(c), Transform::identity(), None);
        }
    }

    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: [u8; 4]) {
        if let Some(p) = rounded(x, y - self.oy, w, h, r) {
            self.pm
                .fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
        }
    }

    pub fn rstroke(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, c: [u8; 4]) {
        if let Some(p) = rounded(x, y - self.oy, w, h, r) {
            let st = Stroke {
                width,
                ..Stroke::default()
            };
            self.pm.stroke_path(&p, &paint(c), &st, Transform::identity(), None);
        }
    }

    pub fn disc(&mut self, cx: f32, cy: f32, r: f32, c: [u8; 4]) {
        self.rrect(cx - r, cy - r, 2.0 * r, 2.0 * r, r, c);
    }

    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32, c: [u8; 4]) {
        self.rstroke(cx - r, cy - r, 2.0 * r, 2.0 * r, r, width, c);
    }

    /// Text from (x, its baseline); where the pen ends.
    pub fn text(&mut self, text: &str, face: Face, px: f32, x: f32, base: f32, c: [u8; 4]) -> f32 {
        let shows = self.shows(base - 1.5 * px, 2.5 * px);
        let (f, w) = self.ty.face(face);
        if !shows {
            return x + f.width(text, w, px);
        }
        f.draw(&mut self.pm, text, w, px, (x, base - self.oy), c)
    }

    /// Text in a band `h` high from `top`, centred on its capitals; where the pen ends.
    pub fn boxed(&mut self, text: &str, face: Face, px: f32, x: f32, top: f32, h: f32, c: [u8; 4]) -> f32 {
        let base = (top + (h + self.ty.cap_height(px)) / 2.0).round();
        self.text(text, face, px, x, base, c)
    }

    /// Code on the terminal's grid, in a row from `top`: from x (a cell's edge), a
    /// character a cell (two for a wide one), each glyph's pen on a whole pixel, as the
    /// terminal's own text; `most` cells at most. The cells it took.
    pub fn cells(&mut self, text: &str, face: Face, x: f32, top: f32, most: usize, c: [u8; 4]) -> usize {
        let ((cw, ch), px) = (self.sz.cell, self.sz.code);
        let base = (top + (ch + self.ty.mono.cap_height(px)) / 2.0).round() - self.oy;
        let shows = self.shows(top, ch);
        let mut col = 0;
        let mut one = [0u8; 4];
        for k in text.chars() {
            let w = char_width(k);
            if w == 0 {
                continue;
            }
            if col + w > most {
                break;
            }
            if shows && k != ' ' {
                let pen = (x + col as f32 * cw).round();
                let (f, weight) = self.ty.face(face);
                f.draw(&mut self.pm, k.encode_utf8(&mut one), weight, px, (pen, base), c);
            }
            col += w;
        }
        col
    }

    /// A code span's width on its chip.
    fn code_width(&mut self, text: &str, px: f32) -> f32 {
        self.ty.width(text, Face::Mono, CODE_IN_PROSE * px) + 0.4 * px
    }

    /// Prose wrapped to max pixels: each line's words.
    pub fn lines(&mut self, text: &str, bold: bool, px: f32, max: f32) -> Vec<Vec<Word>> {
        let face = if bold { Face::UiBold } else { Face::Ui };
        let space = self.ty.width(" ", face, px);
        let mut lines: Vec<Vec<Word>> = vec![vec![]];
        let mut used = 0.0;
        for word in words(text) {
            let w = if word.code {
                self.code_width(&word.text, px)
            } else {
                self.ty.width(&word.text, face, px)
            };
            let line = lines.last_mut().expect("a line");
            let gap = if word.glued || line.is_empty() { 0.0 } else { space };
            if !line.is_empty() && !word.glued && used + gap + w > max {
                used = w;
                lines.push(vec![word]);
            } else {
                used += gap + w;
                line.push(word);
            }
        }
        lines.retain(|l| !l.is_empty());
        lines
    }

    /// Prose with `code` in it, wrapped to max from x, a line every `lh` from top: the
    /// height it took.
    #[allow(clippy::too_many_arguments)]
    pub fn prose(&mut self, text: &str, bold: bool, px: f32, x: f32, top: f32, max: f32, lh: f32, c: [u8; 4]) -> f32 {
        let face = if bold { Face::UiBold } else { Face::Ui };
        let space = self.ty.width(" ", face, px);
        let cap = self.ty.cap_height(px);
        let mut y = top;
        for line in text.split('\n') {
            for words in self.lines(line, bold, px, max) {
                let base = (y + (lh + cap) / 2.0).round();
                let mut pen = x;
                for (k, word) in words.iter().enumerate() {
                    if k > 0 && !word.glued {
                        pen += space;
                    }
                    if word.code {
                        let w = self.code_width(&word.text, px);
                        let h = (1.25 * px).round();
                        let chip = faint(self.th.fg, 0.09);
                        self.rrect(pen, base - cap - (h - cap) / 2.0, w, h, 0.22 * px, chip);
                        self.text(&word.text, Face::Mono, CODE_IN_PROSE * px, pen + 0.2 * px, base, c);
                        pen += w;
                    } else {
                        pen = self.text(&word.text, face, px, pen, base, c);
                    }
                }
                y += lh;
            }
        }
        y - top
    }

    /// How tall prose would be.
    pub fn prose_height(&mut self, text: &str, bold: bool, px: f32, max: f32, lh: f32) -> f32 {
        text.split('\n')
            .map(|l| self.lines(l, bold, px, max).len())
            .sum::<usize>() as f32
            * lh
    }

    /// A key in a cap, then what it does, dim; where it ends.
    pub fn keycap(&mut self, key: &str, what: &str, px: f32, x: f32, cy: f32) -> f32 {
        let kw = keycap_cap(self.ty, key, px);
        let h = (1.45 * px).round();
        let top = (cy - h / 2.0).round();
        self.rrect(x, top, kw, h, 0.28 * px, self.th.cap());
        let edge = (1.5 * self.s).max(1.0);
        self.rstroke(
            x + edge / 2.0,
            top + edge / 2.0,
            kw - edge,
            h - edge,
            0.28 * px,
            edge,
            self.th.cap_edge(),
        );
        let tw = self.ty.width(key, Face::UiBold, px);
        self.boxed(key, Face::UiBold, px, x + (kw - tw) / 2.0, top, h, self.th.text());
        if what.is_empty() {
            return x + kw;
        }
        self.boxed(what, Face::Ui, px, x + kw + 0.4 * px, top, h, self.th.dim())
    }
}

/// Code in a note is a little smaller than the prose around it.
const CODE_IN_PROSE: f32 = 0.86;

/// A keycap's cap alone.
fn keycap_cap(ty: &mut Type, key: &str, px: f32) -> f32 {
    (ty.width(key, Face::UiBold, px) + 0.7 * px).max(1.45 * px)
}

/// How wide a row of keys is, each cap and what it does, a gap between.
pub fn keys_width(ty: &mut Type, keys: &[(&str, &str)], px: f32) -> f32 {
    let one = |ty: &mut Type, (k, w): &(&str, &str)| keycap_cap(ty, k, px) + 0.4 * px + ty.width(w, Face::Ui, px);
    let sum: f32 = keys.iter().map(|k| one(ty, k)).sum();
    sum + keys.len().saturating_sub(1) as f32 * 1.1 * px
}

#[cfg(test)]
#[path = "tests/test_ink.rs"]
mod tests;
