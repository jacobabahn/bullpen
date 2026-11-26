//! The header's status bar, on a tall board: under the top line, a segment for each open
//! task, in its group's colour (theme.rs's `group`) and the board's order, and under it
//! each group's dot, count and word, from what
//! needs you, as many as fit, then "+N more" for the tasks of the rest. It says
//! the whole repository at a glance, however the list is scrolled. It shows counts
//! only (ui/list.rs's `Tally`), so the header's image changes, and is sent again, only
//! when a count does.

use super::font::{tabular, Weight};
use super::render::{Canvas, Draw};
use crate::ui::list::Tally;

/// The bar's top and its height, of a cell's height.
const TOP: f32 = 1.75;
const HEIGHT: f32 = 0.24;
/// The bar and its counts, from where text starts to where it ends, as the rule under
/// a shorter header is; the counts' line midway between the bar and `bottom`, the
/// image's foot, so its descenders are never cut.
pub fn draw(d: &mut Draw, c: &Canvas, tallies: &[Tally], bottom: f32) {
    let ch = c.grid.cell_h as f32;
    let (l, r) = (c.left(), c.right());
    let (top, h) = ((TOP * ch).round(), (HEIGHT * ch).round().max(2.0));
    for (x, w, status) in segments(tallies, l, r, gap(c)) {
        mark(d, x, top, w, h, status);
    }
    counts(d, tallies, l, r, ((top + h + bottom - ch) / 2.0).round(), ch, c.m.label);
}

/// A segment, or a count's dot: a round-ended bar in its group's colour, filled.
pub(super) fn mark(d: &mut Draw, x: f32, y: f32, w: f32, h: f32, status: &str) {
    let (r, color) = ((h.min(w) / 2.0).floor(), d.theme().group(status));
    d.rounded_fill(x, y, w, h, r, color);
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
/// colour (the bar's segments' shape, whatever its tasks' mark), its number and its word, dim, in the bar's
/// order, so it keys the bar from its first segment (what needs you, red and amber).
/// The groups that don't fit before r go from the end, and "+N more" says how many
/// tasks they have.
fn counts(d: &mut Draw, tallies: &[Tally], l: f32, r: f32, y: f32, h: f32, px: f32) {
    let theme = d.theme();
    let tallies: Vec<&Tally> = tallies.iter().collect();
    let (shown, more) = fitting(d, &tallies, r - l, px);
    let mut x = l;
    for (i, t) in tallies[..shown].iter().enumerate() {
        let start = if i == 0 { x } else { x + PAIR * px };
        let dot = (DOT_R * 2.0 * px).round();
        let num = start + DOT * px;
        mark(d, start, (y + (h - dot) / 2.0).round(), dot, dot, t.status);
        // (its figures tabular, so a 1 is as wide as a 3 and every word as far from its number)
        let end = d.boxed(px, &tabular(&t.n.to_string()), Weight::Bold, num, y, h, theme.text()) + AFTER * px;
        x = d.boxed(px, &format!(" {}", t.word), Weight::Regular, end, y, h, theme.dim());
    }
    if !more.is_empty() {
        let start = if shown == 0 { x } else { x + PAIR * px };
        d.boxed(px, &more, Weight::Regular, start, y, h, theme.heading());
    }
}

/// How many of the groups' counts fit in `room` pixels, and what says the rest ("+N
/// more", their tasks; empty when all fit).
fn fitting(d: &Draw, tallies: &[&Tally], room: f32, px: f32) -> (usize, String) {
    let fonts = d.fonts();
    let pair = |t: &Tally| DOT * px + AFTER * px + fonts.width(&tabular(&t.n.to_string()), Weight::Bold, px) + fonts.width(&format!(" {}", t.word), Weight::Regular, px);
    for shown in (0..=tallies.len()).rev() {
        let rest: usize = tallies[shown..].iter().map(|t| t.n).sum();
        let more = if rest == 0 { String::new() } else { format!("+{rest} more") };
        let parts = shown + usize::from(rest > 0);
        let w: f32 = tallies[..shown].iter().map(|t| pair(t)).sum::<f32>() + fonts.width(&more, Weight::Regular, px);
        if shown == 0 || w + PAIR * px * parts.saturating_sub(1) as f32 <= room {
            return (shown, more);
        }
    }
    (0, String::new())
}

/// The room after a count's number besides its space, of the type's size.
const AFTER: f32 = 0.1;

/// A dot's radius, and how far its number starts from its left, of the type's size.
const DOT_R: f32 = 0.25;
const DOT: f32 = 0.8;
/// The space between one group's count and the next, of the type's size.
const PAIR: f32 = 0.9;

#[cfg(test)]
#[path = "tests/test_bar.rs"]
mod tests;
