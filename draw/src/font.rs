//! The image look's type: Inter (built in, regular and semibold), or a font file the
//! `board_font` setting names, and for code JetBrains Mono (built in, regular; its
//! bold drawn heavier), measured and drawn glyph by glyph. A glyph is drawn once per
//! size into a coverage mask, its strokes on whole pixels (hint.rs), and kept; a
//! character the font lacks is drawn from Inter, else as the font's empty box.

use std::collections::HashMap;
use std::path::Path;
use crate::hint::{self, Outline};
use tiny_skia::{FillRule, Mask, Pixmap, Transform};
use ttf_parser::{Face, GlyphId};

static REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular.ttf");
static SEMIBOLD: &[u8] = include_bytes!("../fonts/Inter-SemiBold.ttf");
/// The tour's code (SIL Open Font License, fonts/JetBrainsMono-LICENSE.txt)
static MONO: &[u8] = include_bytes!("../fonts/JetBrainsMonoNL-Regular.ttf");

/// The weight a piece of text is drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Bold,
}

/// A coverage mask of one glyph, and where it sits from the pen (left, top of the
/// baseline).
struct Glyph {
    mask: Option<Mask>,
    left: i32,
    top: i32,
}

/// The faces text is drawn from, and the glyphs drawn so far.
pub struct Fonts {
    regular: Face<'static>,
    bold: Face<'static>,
    /// a font file's: bold is its regular drawn heavier
    fake_bold: bool,
    /// Inter, for a character the font file lacks
    fallback: Option<(Face<'static>, Face<'static>)>,
    /// each face's tabular figures, regular's then bold's ("zero.tf"…), where it has them
    figures: [[Option<GlyphId>; 10]; 2],
    cache: HashMap<(Weight, bool, u16, u32, u8), Glyph>,
}

/// The least of its room a text cut at a word's end fills: under it, it's cut in the word.
const FILL: f32 = 0.5;

impl Fonts {
    /// Inter, built in.
    pub fn builtin() -> Fonts {
        let face = |data: &'static [u8]| Face::parse(data, 0).expect("the built-in font parses");
        Fonts::of(face(REGULAR), face(SEMIBOLD), false, None)
    }

    /// JetBrains Mono, built in, with Inter for what it lacks: code.
    pub fn mono() -> Fonts {
        let face = Face::parse(MONO, 0).expect("the built-in font parses");
        let inter = Fonts::builtin();
        Fonts::of(face.clone(), face, true, Some((inter.regular, inter.bold)))
    }

    fn of(regular: Face<'static>, bold: Face<'static>, fake_bold: bool, fallback: Option<(Face<'static>, Face<'static>)>) -> Fonts {
        let figures = [figures(&regular), figures(&bold)];
        Fonts { regular, bold, fake_bold, fallback, figures, cache: HashMap::new() }
    }

    /// The font file at path, with Inter for what it lacks; why not, when it can't be read.
    pub fn from_file(path: &Path) -> Result<Fonts, String> {
        let data = std::fs::read(path).map_err(|e| format!("can't read the font {}: {e}", path.display()))?;
        // kept for the run: the board loads a font file again only when the setting changes
        let data: &'static [u8] = Box::leak(data.into_boxed_slice());
        let face = Face::parse(data, 0).map_err(|e| format!("{} isn't a font the board can draw with: {e}", path.display()))?;
        let inter = Fonts::builtin();
        Ok(Fonts::of(face.clone(), face, true, Some((inter.regular, inter.bold))))
    }

    /// The font `board_font` names, else Inter.
    pub fn load(path: Option<&Path>) -> Result<Fonts, String> {
        match path {
            Some(p) => Fonts::from_file(p),
            None => Ok(Fonts::builtin()),
        }
    }

    /// The face to draw `c` from, and its glyph: the font's own, else the fallback's.
    fn glyph_of(&self, weight: Weight, c: char) -> (bool, &Face<'static>, GlyphId) {
        let own = match weight {
            Weight::Regular => &self.regular,
            Weight::Bold => &self.bold,
        };
        // a tabular figure (`tabular`): the face's own, else its figure
        let c = match figure(c) {
            Some(n) => match self.figures[usize::from(weight == Weight::Bold)][n] {
                Some(g) => return (false, own, g),
                None => char::from(b'0' + n as u8),
            },
            None => c,
        };
        if let Some(g) = own.glyph_index(c) {
            return (false, own, g);
        }
        if let Some((r, b)) = &self.fallback {
            let face = if weight == Weight::Bold { b } else { r };
            if let Some(g) = face.glyph_index(c) {
                return (true, face, g);
            }
        }
        (false, own, GlyphId(0))
    }

    /// How far the pen moves for `c` at `px` pixels to the em.
    fn advance(&self, weight: Weight, c: char, px: f32) -> f32 {
        let (_, face, g) = self.glyph_of(weight, c);
        face.glyph_hor_advance(g).unwrap_or(0) as f32 * px / face.units_per_em() as f32
    }

    /// The width of text at `px`.
    pub fn width(&self, text: &str, weight: Weight, px: f32) -> f32 {
        text.chars().map(|c| self.advance(weight, c, px)).sum()
    }

    /// The height of a capital at `px`: text is centred on it.
    pub fn cap_height(&self, px: f32) -> f32 {
        let f = &self.regular;
        f.capital_height().unwrap_or(f.ascender()) as f32 * px / f.units_per_em() as f32
    }

    /// How far text's ink reaches above its baseline and below it, at `px`: what a
    /// key's cap centres (a "p" hangs below its baseline, an "r" doesn't).
    pub fn ink(&self, text: &str, weight: Weight, px: f32) -> (f32, f32) {
        text.chars().fold((0.0, 0.0), |(up, down), c| {
            let (_, face, g) = self.glyph_of(weight, c);
            let k = px / face.units_per_em() as f32;
            face.glyph_bounding_box(g).map_or((up, down), |b| (f32::max(up, b.y_max as f32 * k), f32::max(down, -b.y_min as f32 * k)))
        })
    }

    /// Text cut to `max` pixels wide, with "…" when it was cut: after its last whole
    /// word that fits (and not after a full stop, a comma or a colon); in a word only
    /// when that would leave under FILL of the room used (one long word, a path).
    pub fn fit(&self, text: &str, weight: Weight, px: f32, max: f32) -> String {
        if self.width(text, weight, px) <= max {
            return text.to_string();
        }
        let room = max - self.width("…", weight, px);
        let mut out = String::new();
        let mut w = 0.0;
        for c in text.chars() {
            let a = self.advance(weight, c, px);
            if w + a > room {
                break;
            }
            w += a;
            out.push(c);
        }
        if room <= 0.0 {
            return String::new();
        }
        // (a word with only its comma or full stop left to come is whole)
        let left: String = text[out.len()..].chars().take_while(|c| !c.is_whitespace()).collect();
        let in_word = left.chars().any(char::is_alphanumeric) && !out.ends_with(char::is_whitespace);
        // a piece of a word says little: the cut goes back to the word before
        if let (true, Some(space)) = (in_word, out.rfind(char::is_whitespace)) {
            if self.width(&out[..space], weight, px) >= FILL * room {
                out.truncate(space);
            }
        }
        // (nor on a bracket just opened: "proposals (a…")
        if let Some((before, last)) = out.trim_end().rsplit_once(' ') {
            if last.starts_with('(') && !last.contains(')') {
                out = before.to_string();
            }
        }
        out.trim_end().trim_end_matches(['.', ',', ':', ';', '·', '-', '(']).trim_end().to_string() + "…"
    }

    /// Text wrapped at its spaces: a first line `first` pixels wide, then lines `rest`
    /// wide, `most` lines at most, the last cut with "…" when there's more. A word
    /// longer than a line is broken where the line ends, the rest of it on the next.
    pub fn wrap(&self, text: &str, weight: Weight, px: f32, (first, rest): (f32, f32), most: usize) -> Vec<String> {
        let mut words: std::collections::VecDeque<String> = text.split_whitespace().map(String::from).collect();
        let space = self.width(" ", weight, px);
        let mut lines: Vec<String> = Vec::new();
        while let Some(word) = words.pop_front() {
            let max = if lines.is_empty() { first } else { rest };
            if lines.len() + 1 >= most.max(1) {
                let all: Vec<String> = std::iter::once(word).chain(words).collect();
                lines.push(self.fit(&all.join(" "), weight, px, max));
                break;
            }
            let (mut line, mut w) = self.head_of(word, weight, px, max, &mut words);
            while let Some(next) = words.front() {
                let wn = self.width(next, weight, px);
                if w + space + wn > max {
                    break;
                }
                (w, line) = (w + space + wn, format!("{line} {next}"));
                words.pop_front();
            }
            lines.push(line);
        }
        lines
    }

    /// A detail's last line of one word takes the last word of the line above it (or,
    /// when a key is among that line's last three words, the key with its phrase: "p
    /// shows its screen", from the letter on); a key whose phrase the break splits ("p
    /// shows / its screen") goes down with it. A key is the board's phrase's form (`is_key`). The lines are otherwise full to
    /// the measure.
    pub fn fill_last(&self, lines: &mut [String], weight: Weight, px: f32, max: f32) {
        let [.., above, last] = lines else { return };
        if last.ends_with('…') {
            return;
        }
        let words: Vec<&str> = above.split(' ').collect();
        let next: Vec<&str> = words.iter().copied().chain(last.split(' ')).collect();
        let letter = (0..words.len()).rev().find(|&j| is_key(&next[j..])).filter(|&j| j + 3 >= words.len() && j >= 2);
        let take = match letter {
            Some(j) if j + 3 > words.len() || !last.contains(' ') => words.len() - j,
            _ if !last.contains(' ') && words.len() >= 3 => 1,
            _ => return,
        };
        let keep = words.len() - take;
        let moved = format!("{} {last}", words[keep..].join(" "));
        if self.width(&moved, weight, px) <= max {
            (*above, *last) = (words[..keep].join(" "), moved);
        }
    }

    /// A last line of one word takes the last word of the line above it, when that line
    /// has three or more and the two fit: a word isn't left alone under a full line.
    pub fn no_widow(&self, lines: &mut [String], weight: Weight, px: f32, max: f32) {
        let [.., above, last] = lines else { return };
        if last.contains(' ') || last.ends_with('…') || above.split(' ').count() < 3 {
            return;
        }
        let Some((rest, word)) = above.rsplit_once(' ') else { return };
        let moved = format!("{word} {last}");
        if self.width(&moved, weight, px) <= max {
            (*above, *last) = (rest.to_string(), moved);
        }
    }

    /// A line's first word and its width: all of it, or as much as fits in `max` with the
    /// rest put back first in `words` (a word as wide as one character at least).
    fn head_of(&self, word: String, weight: Weight, px: f32, max: f32, words: &mut std::collections::VecDeque<String>) -> (String, f32) {
        let whole = self.width(&word, weight, px);
        if whole <= max {
            return (word, whole);
        }
        let (mut w, mut cut) = (0.0, 0);
        for (i, c) in word.char_indices() {
            let a = self.advance(weight, c, px);
            if i > 0 && w + a > max {
                cut = i;
                break;
            }
            w += a;
        }
        if cut == 0 {
            return (word, whole);
        }
        words.push_front(word[cut..].to_string());
        (word[..cut].to_string(), w)
    }

    /// Draw text from `at`, the start of its baseline, in colour; returns where the pen ends.
    pub fn draw(&mut self, pm: &mut Pixmap, text: &str, weight: Weight, px: f32, at: (f32, f32), color: [u8; 4]) -> f32 {
        let (mut pen, y) = at;
        for c in text.chars() {
            let (fallback, _, gid) = self.glyph_of(weight, c);
            // a quarter pixel's steps, so a glyph is drawn where it falls
            let phase = ((pen.fract() * 4.0) as u8).min(3);
            let key = (weight, fallback, gid.0, px.to_bits(), phase);
            if !self.cache.contains_key(&key) {
                let g = self.raster(weight, c, px, phase as f32 / 4.0);
                self.cache.insert(key, g);
            }
            let g = &self.cache[&key];
            if let Some(mask) = &g.mask {
                blend(pm, mask, pen.floor() as i32 + g.left, y.round() as i32 - g.top, color);
            }
            pen += self.advance(weight, c, px);
        }
        pen
    }

    /// Draw one glyph's outline into a mask, `dx` of a pixel to the right, its strokes
    /// on whole pixels (hint.rs).
    fn raster(&self, weight: Weight, c: char, px: f32, dx: f32) -> Glyph {
        let (_, face, gid) = self.glyph_of(weight, c);
        let none = Glyph { mask: None, left: 0, top: 0 };
        let Some(mut outline) = Outline::of(face, gid) else { return none };
        let upem = face.units_per_em() as f32;
        if weight == Weight::Bold && self.fake_bold {
            // a thirtieth of the em heavier, before it's fitted: its stems are solid too
            outline = outline.heavier(upem / 30.0);
        }
        let grid = outline.fitted(px / upem, upem, dx, hint::blues(face));
        let Some(path) = outline.path(&grid) else { return none };
        let pad = 2.0;
        let b = path.bounds();
        let left = (b.left() - pad).floor();
        let top = (pad - b.top()).ceil();
        let w = ((b.right() + pad).ceil() - left) as u32;
        let h = ((b.bottom() + pad).ceil() + top) as u32;
        let Some(mut mask) = Mask::new(w.max(1), h.max(1)) else { return none };
        // from the pen's baseline to the mask's corner
        let t = Transform::from_translate(-left, top);
        mask.fill_path(&path, FillRule::Winding, true, t);
        Glyph { mask: Some(mask), left: left as i32, top: top as i32 }
    }
}

/// Text with its figures tabular, each as wide as the others (an ID's, a count's), so
/// figures in a column line up: each is a character of the Private Use Area, which
/// `Fonts` draws as the face's tabular figure ("one.tf", with a foot: not a narrow one
/// spaced out), or its own figure when the face has none.
pub fn tabular(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_digit() { char::from_u32(TABULAR + (c as u32 - '0' as u32)).unwrap_or(c) } else { c }).collect()
}

/// Where `tabular`'s figures start: U+E000 is zero.
const TABULAR: u32 = 0xE000;

/// Which tabular figure a character is.
fn figure(c: char) -> Option<usize> {
    (TABULAR..TABULAR + 10).contains(&(c as u32)).then(|| (c as u32 - TABULAR) as usize)
}

/// A face's tabular figures, by their glyphs' names ("zero.tf" to "nine.tf").
fn figures(face: &Face) -> [Option<GlyphId>; 10] {
    const NAMES: [&str; 10] = ["zero.tf", "one.tf", "two.tf", "three.tf", "four.tf", "five.tf", "six.tf", "seven.tf", "eight.tf", "nine.tf"];
    let mut out = [None; 10];
    for i in 0..face.number_of_glyphs() {
        let g = GlyphId(i);
        if let Some(n) = face.glyph_name(g).and_then(|name| NAMES.iter().position(|&x| x == name)) {
            out[n] = Some(g);
        }
    }
    out
}

/// Do these words start with a key the board names, in its phrases' form: a lowercase
/// letter alone, a verb of four letters or more ending in s, then "it" or "its" ("p
/// shows its screen", "r resumes it")?
fn is_key(words: &[&str]) -> bool {
    let letter = words.first().is_some_and(|w| w.len() == 1 && w.chars().all(|c| c.is_ascii_lowercase()));
    let verb = words.get(1).is_some_and(|v| v.len() >= 4 && v.ends_with('s') && v.chars().all(|c| c.is_ascii_lowercase()));
    let it = words.get(2).is_some_and(|o| matches!(o.trim_end_matches(['.', ',', ';', ':']), "it" | "its"));
    letter && verb && it
}

/// Lay a mask's coverage over the pixmap at (x, y), in colour (straight RGBA).
fn blend(pm: &mut Pixmap, mask: &Mask, x: i32, y: i32, color: [u8; 4]) {
    let (pw, ph) = (pm.width() as i32, pm.height() as i32);
    let (mw, mh) = (mask.width() as i32, mask.height() as i32);
    let data = pm.data_mut();
    let cov = mask.data();
    for my in 0..mh {
        let py = y + my;
        if py < 0 || py >= ph {
            continue;
        }
        for mx in 0..mw {
            let px = x + mx;
            if px < 0 || px >= pw {
                continue;
            }
            let a = cov[(my * mw + mx) as usize] as u32 * color[3] as u32 / 255;
            if a == 0 {
                continue;
            }
            let i = ((py * pw + px) * 4) as usize;
            // premultiplied source over premultiplied destination
            for k in 0..3 {
                let src = color[k] as u32 * a / 255;
                data[i + k] = (src + data[i + k] as u32 * (255 - a) / 255) as u8;
            }
            data[i + 3] = (a + data[i + 3] as u32 * (255 - a) / 255) as u8;
        }
    }
}

#[cfg(test)]
#[path = "tests/test_font.rs"]
mod tests;
