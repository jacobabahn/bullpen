//! The image look's type: Inter (built in, regular and semibold), or a font file the
//! `board_font` setting names, measured and drawn glyph by glyph. A glyph is drawn
//! once per size into a coverage mask and kept; a character the font lacks is drawn
//! from Inter, else as the font's empty box.

use std::collections::HashMap;
use std::path::Path;
use tiny_skia::{FillRule, Mask, PathBuilder, Pixmap, Stroke, Transform};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

static REGULAR: &[u8] = include_bytes!("../../fonts/Inter-Regular.ttf");
static SEMIBOLD: &[u8] = include_bytes!("../../fonts/Inter-SemiBold.ttf");

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
    cache: HashMap<(Weight, bool, u16, u32, u8), Glyph>,
}

/// The least of its room a text cut at a word's end fills: under it, it's cut in the word.
const FILL: f32 = 0.6;

impl Fonts {
    /// Inter, built in.
    pub fn builtin() -> Fonts {
        let face = |data: &'static [u8]| Face::parse(data, 0).expect("the built-in font parses");
        Fonts { regular: face(REGULAR), bold: face(SEMIBOLD), fake_bold: false, fallback: None, cache: HashMap::new() }
    }

    /// The font file at path, with Inter for what it lacks; why not, when it can't be read.
    pub fn from_file(path: &Path) -> Result<Fonts, String> {
        let data = std::fs::read(path).map_err(|e| format!("can't read the font {}: {e}", path.display()))?;
        // kept for the run: the board loads a font file again only when the setting changes
        let data: &'static [u8] = Box::leak(data.into_boxed_slice());
        let face = Face::parse(data, 0).map_err(|e| format!("{} isn't a font the board can draw with: {e}", path.display()))?;
        let inter = Fonts::builtin();
        Ok(Fonts { regular: face.clone(), bold: face, fake_bold: true, fallback: Some((inter.regular, inter.bold)), cache: HashMap::new() })
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

    /// Text cut to `max` pixels wide, with "…" when it was cut: after its last whole
    /// word that fits (and not after a full stop, a comma or a colon), or in a word when
    /// that would leave under FILL of the room used (one long word, a path).
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
        let in_word = text[out.len()..].chars().next().is_some_and(|c| !c.is_whitespace()) && !out.ends_with(char::is_whitespace);
        // a piece of a word says little: the cut goes back to the word before
        if let (true, Some(space)) = (in_word, out.rfind(char::is_whitespace)) {
            if self.width(&out[..space], weight, px) >= FILL * room {
                out.truncate(space);
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

    /// A last line of one word takes the last word of the line above it, when that line
    /// has three or more and the two fit: a word isn't left alone under a full line (a
    /// detail's lines; a title keeps its first line full beside its age).
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

    /// Draw one glyph's outline into a mask, `dx` of a pixel to the right.
    fn raster(&self, weight: Weight, c: char, px: f32, dx: f32) -> Glyph {
        let (_, face, gid) = self.glyph_of(weight, c);
        let scale = px / face.units_per_em() as f32;
        let mut pb = Outline(PathBuilder::new());
        let none = Glyph { mask: None, left: 0, top: 0 };
        let Some(bbox) = face.outline_glyph(gid, &mut pb) else { return none };
        let Some(path) = pb.0.finish() else { return none };
        let heavier = weight == Weight::Bold && self.fake_bold;
        let pad = 2.0 + if heavier { px / 30.0 } else { 0.0 };
        let left = (bbox.x_min as f32 * scale + dx - pad).floor();
        let top = (bbox.y_max as f32 * scale + pad).ceil();
        let w = ((bbox.x_max - bbox.x_min) as f32 * scale + 2.0 * pad + 2.0).ceil() as u32;
        let h = ((bbox.y_max - bbox.y_min) as f32 * scale + 2.0 * pad + 2.0).ceil() as u32;
        let Some(mut mask) = Mask::new(w.max(1), h.max(1)) else { return none };
        // font units, y up, to the mask's pixels, y down
        let t = Transform::from_row(scale, 0.0, 0.0, -scale, dx - left, top);
        mask.fill_path(&path, FillRule::Winding, true, t);
        if heavier {
            let stroke = Stroke { width: px / 30.0 / scale, ..Stroke::default() };
            if let Some(outline) = path.stroke(&stroke, 1.0) {
                mask.fill_path(&outline, FillRule::Winding, true, t);
            }
        }
        Glyph { mask: Some(mask), left: left as i32, top: top as i32 }
    }
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

/// A glyph's outline, as a path.
struct Outline(PathBuilder);

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
}
