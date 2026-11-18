//! The header's status bar, on a tall board: under the top line, a segment for each open
//! task, in its group's colour (theme.rs's `group`) and the board's order, and under it
//! each group's count and word, as many as fit, the rest dropped from the end. It says
//! the whole repository at a glance, however the list is scrolled. It shows counts
//! only (ui/list.rs's `Tally`), so the header's image changes, and is sent again, only
//! when a count does.

use super::font::Weight;
use super::render::{Canvas, Draw};
use crate::ui::list::Tally;

/// The bar's top and its height, of a cell's height.
const TOP: f32 = 1.5;
const HEIGHT: f32 = 0.24;
/// The space between the bar and its counts' line, of a cell's height.
const UNDER: f32 = 0.2;

/// The bar and its counts, from the first column to the last but one, as the rule
/// under a shorter header is.
pub fn draw(d: &mut Draw, c: &Canvas, tallies: &[Tally]) {
    let ch = c.grid.cell_h as f32;
    let (l, r) = (c.grid.cell_w as f32, (c.grid.width() - c.grid.cell_w) as f32);
    let (top, h) = ((TOP * ch).round(), (HEIGHT * ch).round().max(2.0));
    let theme = d.theme();
    for (x, w, status) in segments(tallies, l, r, gap(c)) {
        d.rounded_fill(x, top, w, h, (h.min(w) / 2.0).floor(), theme.group(status));
    }
    counts(d, tallies, l, r, (top + h + UNDER * ch).round(), ch, c.m.label);
}

/// The space between segments: a few pixels, as a cell's width sets it.
fn gap(c: &Canvas) -> f32 {
    (0.18 * c.grid.cell_w as f32).round().max(2.0)
}

/// The segments from l to r, each its left, its width and its group's status: one a
/// task, `gap` apart; with too many tasks for that, one a group, as wide as its share.
pub fn segments(tallies: &[Tally], l: f32, r: f32, gap: f32) -> Vec<(f32, f32, &'static str)> {
    let total: usize = tallies.iter().map(|t| t.n).sum();
    if total == 0 {
        return Vec::new();
    }
    let each = |n: usize| (r - l - gap * (n - 1) as f32) / n as f32;
    let per_task = each(total) >= 2.0;
    let mut out = Vec::new();
    let mut x = l;
    if per_task {
        let w = each(total);
        for t in tallies {
            for _ in 0..t.n {
                out.push((x, w, t.status));
                x += w + gap;
            }
        }
        return out;
    }
    let room = r - l - gap * (tallies.len() - 1) as f32;
    for t in tallies {
        let w = room * t.n as f32 / total as f32;
        out.push((x, w, t.status));
        x += w + gap;
    }
    out
}

/// The counts' line, whose top is y, `h` high, in type `px`: each group's dot in its
/// colour, its number and its word, dim; those that don't fit before r are dropped from
/// the end.
fn counts(d: &mut Draw, tallies: &[Tally], l: f32, r: f32, y: f32, h: f32, px: f32) {
    let theme = d.theme();
    let mut x = l;
    for (i, t) in tallies.iter().enumerate() {
        let num = t.n.to_string();
        let word = format!(" {}", t.word);
        let fonts = d.fonts();
        let w = DOT * px + fonts.width(&num, Weight::Bold, px) + fonts.width(&word, Weight::Regular, px);
        let start = if i == 0 { x } else { x + PAIR * px };
        if start + w > r {
            break;
        }
        let dot = (DOT_R * 2.0 * px).round();
        d.rounded_fill(start, (y + (h - dot) / 2.0).round(), dot, dot, dot / 2.0, theme.group(t.status));
        let end = d.boxed(px, &num, Weight::Bold, start + DOT * px, y, h, theme.text());
        x = d.boxed(px, &word, Weight::Regular, end, y, h, theme.dim());
    }
}

/// A dot's radius, and how far its number starts from its left, of the type's size.
const DOT_R: f32 = 0.25;
const DOT: f32 = 0.8;
/// The space between one group's count and the next, of the type's size.
const PAIR: f32 = 0.9;

#[cfg(test)]
#[path = "tests/test_bar.rs"]
mod tests;
