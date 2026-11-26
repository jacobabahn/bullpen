//! The keys a task's line names ("Its agent exited. r resumes it."): finding them in a
//! detail's text, and drawing the line with each in a small cap, as the key row's. A
//! child of render.rs, so it draws with `Draw`'s own parts.

use super::Draw;
use crate::image::font::Weight;
use crate::image::theme::{bright, faint, luminance, Rgb};

/// The room each side of a key a detail's text names, inside its cap, of the type's size.
pub(super) const KEY_SIDE: f32 = 0.28;

/// The keys a detail's text names ("Its agent exited. r resumes it."), as which of its
/// letters they are, spaces not counted (so they're found on its wrapped lines): a
/// lowercase letter alone, first or after a full stop, in the form the board's own
/// phrases have (core's words.rs: "r resumes it", "p shows its screen", "t tells it
/// to continue"): the letter, a verb of four letters or more ending in s, then "it"
/// or "its". A detail is often an agent's own words, where a letter alone ("a token
/// is needed") is no key. The first `lead` columns are the task's age.
pub(super) fn key_letters(text: &str, lead: usize) -> Vec<usize> {
    let (age, rest) = crate::model::split_at_width(text, lead);
    let mut n = age.chars().filter(|c| !c.is_whitespace()).count();
    let words: Vec<&str> = rest.split_whitespace().collect();
    let mut out = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let alone = w.len() == 1 && w.chars().all(|c| c.is_ascii_lowercase());
        let verb = words.get(i + 1).is_some_and(|v| v.len() >= 4 && v.ends_with('s') && v.chars().all(|c| c.is_ascii_lowercase()));
        let it = words.get(i + 2).is_some_and(|o| matches!(o.trim_end_matches(['.', ',', ';', ':']), "it" | "its"));
        if alone && verb && it && (i == 0 || words[i - 1].ends_with('.')) {
            out.push(n);
        }
        n += w.chars().count();
    }
    out
}

impl Draw<'_> {
    /// A detail's line from x, on the line whose top is y: its text in `color`, the
    /// keys it names (`keys`: which of the text's letters, `seen` of them drawn on the
    /// lines before) semibold in the foreground, each in a small cap. Returns where it ends.
    pub(super) fn said(&mut self, line: &str, x: f32, y: f32, color: [u8; 4], keys: &[usize], seen: &mut usize) -> f32 {
        let fg = self.theme.text();
        let (mut x, mut run, mut key) = (x, String::new(), false);
        for c in line.chars().chain(std::iter::once('\0')) {
            let is_key = !c.is_whitespace() && keys.contains(seen);
            if (is_key != key || c == '\0') && !run.is_empty() {
                let (weight, color) = if key { (Weight::Bold, fg) } else { (Weight::Regular, color) };
                if key {
                    // in a small cap, as the key row's (lay left room for its sides),
                    // centred on the key's ink, which stays on the line's baseline
                    let (w, px) = (self.width(&run, weight), self.px());
                    let (side, ch) = (KEY_SIDE * px, (1.3 * px).round());
                    let (up, down) = self.fonts().ink(&run, weight, px);
                    let top = (self.baseline(y) - (up - down) / 2.0 - ch / 2.0).round();
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
        x
    }

    /// A stat line's line from x ("14 files, +612 −148 · Not toured yet"), on the line
    /// whose top is y: in `color`, but the lines added in green and those taken out in
    /// red, a little dimmed, so its figures read as facts. Returns where it ends.
    pub(super) fn stat(&mut self, line: &str, x: f32, y: f32, color: [u8; 4]) -> f32 {
        // (on a dark pane at their brightest and a third of the way to the foreground,
        // where a terminal's red is dark)
        let (dark, fg) = (luminance(self.theme.bg) < luminance(self.theme.fg), self.theme.fg);
        let lift = |c: Rgb| {
            let (c, mix) = (bright(c), |a: u8, b: u8| ((a as u16 * 2 + b as u16) / 3) as u8);
            Rgb(mix(c.0, fg.0), mix(c.1, fg.1), mix(c.2, fg.2))
        };
        let tone = |c: Rgb| faint(if dark { lift(c) } else { c }, FIGURES);
        let (green, red) = (tone(self.theme.green), tone(self.theme.red));
        let mut x = x;
        for (i, word) in line.split(' ').enumerate() {
            let figures = |sign: char| word.strip_prefix(sign).is_some_and(|n| !n.is_empty() && n.trim_end_matches(',').chars().all(is_figure));
            let tone = if figures('+') { green } else if figures('−') || figures('-') { red } else { color };
            let word = if i == 0 { word.to_string() } else { format!(" {word}") };
            x = self.text(&word, Weight::Regular, x, y, f32::MAX, tone);
        }
        x
    }
}

/// How strong a stat line's coloured figures are.
const FIGURES: f32 = 0.85;

/// A figure, plain or tabular (font.rs's `tabular`).
fn is_figure(c: char) -> bool {
    c.is_ascii_digit() || ('\u{E000}'..='\u{E009}').contains(&c)
}
