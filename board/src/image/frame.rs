//! The board's top line and bottom rows drawn as images, in the list's type and colours
//! (render.rs), so the board reads as one drawn surface: the repository and its counts
//! (as chips, chip.rs) above the list, the message and the keys (in caps) under it, a
//! faint rule between each and the list. What's typed or asked there (a tell, a y/n
//! question) stays text (ui/bottom.rs).

use super::font::{Fonts, Weight};
use super::chip;
use super::render::{Canvas, Draw};
use super::theme::solid;
use crate::ui::bottom::{Foot, FootLine, Tone};
use crate::ui::list::Head;
use tiny_skia::Pixmap;

/// The top line, in an image the grid's columns wide and `rows` cells high: the
/// repository's name, what's after it and the other repositories, then the counts at
/// the right edge as chips (in words while they fit beside the name, else their
/// numbers); with two cells or more, a faint rule under it all. Its line is a little
/// more than a cell, from a tenth of one down (a cell for a one-cell header).
pub fn head(head: &Head, rows: u16, c: &Canvas, fonts: &mut Fonts) -> Result<Pixmap, String> {
    let mut pm = pixmap(c, rows)?;
    let mut d = Draw::new(&mut pm, fonts, c, 0.0);
    let ch = c.grid.cell_h as f32;
    let (top, line) = if rows >= 2 { ((0.1 * ch).round(), (1.3 * ch).round()) } else { (0.0, ch) };
    d.set(c.m.title, line);
    let theme = d.theme();
    let (fg, dim) = (theme.text(), theme.dim());
    let left = c.grid.cell_w as f32;
    // the counts first, at the right edge: the rest gets what's left
    let px = c.m.label + 1.0;
    let name_w = d.width(&head.name, Weight::Bold);
    let cy = top + line / 2.0;
    let mut x = d.right();
    for (text, st) in counts(head, d.fonts(), px, d.right() - left - name_w - 0.8 * c.m.title).into_iter().rev() {
        x = chip::chip(&mut d, &text, px, x, cy, theme.status(st).unwrap_or(dim)) - CHIP_GAP * px;
    }
    let left_end = x + CHIP_GAP * px - 0.8 * c.m.title;
    let end = d.text(&head.name, Weight::Bold, left, top, left_end - left, fg);
    let end = d.text(&head.auto, Weight::Regular, end, top, left_end - end, dim);
    if !head.others.is_empty() {
        let end = d.text(" · ", Weight::Regular, end, top, left_end - end, dim);
        let tone = if head.others_amber { solid(theme.yellow) } else { dim };
        d.text(&head.others, Weight::Regular, end, top, left_end - end, tone);
    }
    if rows >= 2 {
        rule(&mut d, c, (1.55 * ch).round());
    }
    Ok(pm)
}

/// The space between the header's chips, of their type.
const CHIP_GAP: f32 = 0.4;

/// The header's counts as its chips say them: in words when they fit in `room` pixels
/// in type `px`, else their numbers alone (their colours say which is which).
fn counts(head: &Head, fonts: &Fonts, px: f32, room: f32) -> Vec<(String, &'static str)> {
    let wide = |parts: &[(String, &str)]| -> f32 { parts.iter().map(|(t, _)| chip::width(fonts, t, px) + CHIP_GAP * px).sum() };
    let words = head.counts.clone();
    if wide(&words) <= room {
        return words;
    }
    words.into_iter().map(|(t, st)| (t.chars().filter(char::is_ascii_digit).collect(), st)).collect()
}

/// The rule under the top line and over the bottom rows: 1.5 pixels of the foreground,
/// faint, from the first column to the last but one, whose top is at y.
fn rule(d: &mut Draw, c: &Canvas, y: f32) {
    let (l, r) = (c.grid.cell_w as f32, (c.grid.width() - c.grid.cell_w) as f32);
    let color = d.theme().rule();
    d.rounded_fill(l, y, r - l, 1.5, 0.0, color);
}

/// The bottom rows, in an image the grid's columns wide and `rows` cells high: each
/// line on its row, as the text has it, the message in the detail lines' type and the
/// keys in caps; a faint rule above the first line, as under the top line (at the
/// image's top when the first line is on its first row).
pub fn foot(foot: &Foot, rows: u16, c: &Canvas, fonts: &mut Fonts) -> Result<Pixmap, String> {
    let mut pm = pixmap(c, rows)?;
    let mut d = Draw::new(&mut pm, fonts, c, 0.0);
    let ch = c.grid.cell_h as f32;
    if let Some(first) = foot.lines.iter().map(|(row, _)| *row).min() {
        rule(&mut d, c, (first as f32 * ch - 0.45 * ch).round().max(0.0));
    }
    d.set(c.m.detail, ch);
    let theme = d.theme();
    let left = c.grid.cell_w as f32;
    for (row, line) in &foot.lines {
        let y = *row as f32 * ch;
        match line {
            FootLine::Msg(text, tone) => {
                let (weight, color) = match tone {
                    Tone::Plain => (Weight::Bold, theme.text()),
                    Tone::Red => (Weight::Bold, solid(theme.red)),
                    Tone::Green => (Weight::Bold, solid(theme.green)),
                    Tone::Dim => (Weight::Regular, theme.dim()),
                };
                let right = d.right();
                d.text(text, weight, left, y, right - left, color);
            }
            FootLine::Keys(keys) => key_row(&mut d, keys, left, y + ch / 2.0, c.m.label),
        }
    }
    Ok(pm)
}

fn pixmap(c: &Canvas, rows: u16) -> Result<Pixmap, String> {
    let (w, h) = (c.grid.width(), rows as u32 * c.grid.cell_h);
    Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| format!("can't make an image {w}×{h} pixels"))
}

/// The space between a key's pair and the next, of their type.
const KEY_GAP: f32 = 0.9;

type Keys = Vec<(String, String)>;

/// The key row: the keys from the left, each in a cap with what it does dim after it,
/// and `? keys` at the right edge, centred on cy, in type `px`.
fn key_row(d: &mut Draw, keys: &[(String, String)], left: f32, cy: f32, px: f32) {
    let room = d.right() - left;
    let (keys, help) = fit_keys(d.fonts(), keys, px, room);
    draw_keys(d, &keys, left, cy, px);
    // with no keys, `? keys` starts the row
    let x = if keys.is_empty() { left } else { d.right() - keys_width(d.fonts(), &help, px) };
    draw_keys(d, &help, x, cy, px);
}

/// The keys that fit in `room` pixels, and `?`'s pair, as the text's: `?` is never
/// dropped; with no room for "? keys" it goes bare, and then the last keys go first.
fn fit_keys(fonts: &Fonts, keys: &[(String, String)], px: f32, room: f32) -> (Keys, Keys) {
    let width = |keys: &[(String, String)]| keys_width(fonts, keys, px);
    let gap = KEY_GAP * px;
    let mut keys = keys.to_vec();
    let mut help = vec![("?".to_string(), "keys".to_string())];
    if !keys.is_empty() && width(&keys) + 1.5 * gap + width(&help) > room {
        help = vec![("?".to_string(), String::new())];
        while !keys.is_empty() && width(&keys) + gap + width(&help) > room {
            keys.pop();
        }
    }
    (keys, help)
}

/// How wide keys in caps are, with the space between them.
fn keys_width(fonts: &Fonts, keys: &[(String, String)], px: f32) -> f32 {
    let each: f32 = keys.iter().map(|(k, what)| chip::keycap_width(fonts, k, what, px)).sum();
    each + KEY_GAP * px * keys.len().saturating_sub(1) as f32
}

/// Keys in caps from x, centred on cy.
fn draw_keys(d: &mut Draw, keys: &[(String, String)], mut x: f32, cy: f32, px: f32) {
    for (i, (key, what)) in keys.iter().enumerate() {
        if i > 0 {
            x += KEY_GAP * px;
        }
        x = chip::keycap(d, key, what, px, x, cy);
    }
}

#[cfg(test)]
#[path = "tests/test_frame.rs"]
mod tests;
