//! The board's top line and bottom rows drawn as images, in the list's type and colours
//! (render.rs), so the board reads as one drawn surface: the repository and its counts
//! above the list, the message and the key hints under it. What's typed or asked there
//! (a tell, a y/n question) stays text (ui/bottom.rs), and so does everything else that
//! isn't the board's list.

use super::font::{Fonts, Weight};
use super::render::{Canvas, Draw};
use super::theme::solid;
use crate::ui::bottom::{Foot, FootLine, Tone};
use crate::ui::list::Head;
use tiny_skia::Pixmap;

/// The top line, in an image the grid's columns wide and `rows` cells high: the
/// repository's name, what's after it, the other repositories, and the counts at the
/// right edge. Its line is a title line's height (a cell at most for small type).
pub fn head(head: &Head, rows: u16, c: &Canvas, fonts: &mut Fonts) -> Result<Pixmap, String> {
    let mut pm = pixmap(c, rows)?;
    let mut d = Draw::new(&mut pm, fonts, c, 0.0);
    let line = c.m.line.min(rows as f32 * c.grid.cell_h as f32);
    d.set(c.m.title, line);
    let theme = d.theme();
    let (fg, dim) = (theme.text(), theme.dim());
    // the counts first, at the right edge: the rest gets what's left
    let gap = d.width("   ", Weight::Regular);
    let parts: Vec<(&str, Weight, [u8; 4])> = head
        .counts
        .iter()
        .enumerate()
        .flat_map(|(i, (text, st))| {
            let sep = (i > 0).then_some((head.sep, Weight::Regular, dim));
            let color = theme.status(st);
            let weight = if color.is_some() { Weight::Bold } else { Weight::Regular };
            sep.into_iter().chain([(text.as_str(), weight, color.unwrap_or(dim))])
        })
        .collect();
    let counts_w: f32 = parts.iter().map(|(t, w, _)| d.width(t, *w)).sum();
    let mut x = d.right() - counts_w;
    let left_end = x - gap;
    for (text, weight, color) in parts {
        x = d.text(text, weight, x, 0.0, f32::MAX, color);
    }
    let left = c.grid.cell_w as f32;
    let end = d.text(&head.name, Weight::Bold, left, 0.0, left_end - left, fg);
    let end = d.text(&head.auto, Weight::Regular, end, 0.0, left_end - end, dim);
    if !head.others.is_empty() {
        let end = d.text(" · ", Weight::Regular, end, 0.0, left_end - end, dim);
        let tone = if head.others_amber { solid(theme.yellow) } else { dim };
        d.text(&head.others, Weight::Regular, end, 0.0, left_end - end, tone);
    }
    Ok(pm)
}

/// The bottom rows, in an image the grid's columns wide and `rows` cells high: each
/// line on its row, as the text has it, in the detail lines' type.
pub fn foot(foot: &Foot, rows: u16, c: &Canvas, fonts: &mut Fonts) -> Result<Pixmap, String> {
    let mut pm = pixmap(c, rows)?;
    let mut d = Draw::new(&mut pm, fonts, c, 0.0);
    let ch = c.grid.cell_h as f32;
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
            FootLine::Keys(keys) => key_row(&mut d, keys, left, y),
        }
    }
    Ok(pm)
}

fn pixmap(c: &Canvas, rows: u16) -> Result<Pixmap, String> {
    let (w, h) = (c.grid.width(), rows as u32 * c.grid.cell_h);
    Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| format!("can't make an image {w}×{h} pixels"))
}

/// The key row: the keys from the left, each bold, what it does dim, and `? keys` at the
/// right edge. As the text's: `?` is never dropped; with no room for "? keys" it goes
/// bare, and then the last keys go first.
fn key_row(d: &mut Draw, keys: &[(String, String)], left: f32, y: f32) {
    let width = |d: &Draw, keys: &[(String, String)]| -> f32 {
        let gap = d.width("  ", Weight::Regular);
        let each: f32 = keys.iter().map(|(k, what)| d.width(k, Weight::Bold) + d.width(&format!(" {what}"), Weight::Regular)).sum();
        each + gap * keys.len().saturating_sub(1) as f32
    };
    let room = d.right() - left;
    let full = vec![("?".to_string(), "keys".to_string())];
    let mut keys = keys.to_vec();
    let mut help = full.clone();
    if keys.is_empty() {
        draw_keys(d, &help, left, y);
        return;
    }
    if width(d, &keys) + width(d, &help) + d.width("   ", Weight::Regular) > room {
        help = vec![("?".to_string(), String::new())];
        while !keys.is_empty() && width(d, &keys) + d.width("? ", Weight::Bold) + d.width("  ", Weight::Regular) > room {
            keys.pop();
        }
    }
    draw_keys(d, &keys, left, y);
    let help_w = if help == full { width(d, &help) } else { d.width("?", Weight::Bold) };
    draw_keys(d, &help, d.right() - help_w, y);
}

fn draw_keys(d: &mut Draw, keys: &[(String, String)], mut x: f32, y: f32) {
    let theme = d.theme();
    let gap = d.width("  ", Weight::Regular);
    for (i, (key, what)) in keys.iter().enumerate() {
        if i > 0 {
            x += gap;
        }
        x = d.text(key, Weight::Bold, x, y, f32::MAX, theme.text());
        if !what.is_empty() {
            x = d.text(&format!(" {what}"), Weight::Regular, x, y, f32::MAX, theme.dim());
        }
    }
}

#[cfg(test)]
#[path = "tests/test_frame.rs"]
mod tests;
