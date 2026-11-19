//! The drawn tour's rail, down its left side: the task, then every step by its title
//! down a line, the steps behind you filled, the one you're on in blue, the ones ahead
//! as rings, with a count of your comments beside each step that has some; the end is
//! the last stop. You see the whole change's order and where you are in it. When the
//! steps don't all fit, the rail scrolls to keep yours in view. Its foot (`foot`): the
//! message, how much of the page is under, and the keys, in caps, under the page.

use super::ink::{keys_width, Face, Ink};
use bullpen_draw::theme::{bright, faint, solid};

/// What the rail shows.
#[derive(Debug, Clone, Hash)]
pub struct Rail {
    pub tid: String,
    pub title: String,
    pub steps: Vec<String>,
    /// the step shown, from 0; the number of steps at the end
    pub at: usize,
    /// your comments on each step
    pub comments: Vec<usize>,
}

/// The rail is laid out for its steps' type `TEXT` pixels high: its room and marks
/// grow with it (`u`, 1 at that size).
const TEXT: f32 = 20.0;
const ROW: f32 = 28.0;
const GAP: f32 = 26.0;

pub fn draw(ink: &mut Ink, rail: &Rail) {
    let (th, sz) = (ink.th, ink.sz);
    let h = ink.pm.height() as f32;
    let px = sz.body;
    let s = px / TEXT;
    let x = 36.0 * s;
    let room = ink.w() - x - 20.0 * s;
    let row = (ROW * s).round();
    let (dpx, drow) = (sz.detail, (1.45 * sz.detail).round());
    let mut y = 30.0 * s;
    ink.boxed(&rail.tid, Face::UiBold, dpx, x, y, drow, th.dim());
    y += drow + 2.0 * s;
    for line in ink.ty.wrap(&rail.title.replace('`', ""), Face::Ui, dpx, room, 2) {
        ink.boxed(&line, Face::Ui, dpx, x, y, drow, th.dim());
        y += drow;
    }
    y += 30.0 * s;
    let n = rail.steps.len();
    let (cx, tx) = (x + 12.0 * s, x + 40.0 * s);
    let count_w = 44.0 * s;
    let tw = (ink.w() - tx - count_w - 12.0 * s).max(10.0);
    // each stop's title lines, and where it is
    let mut stops: Vec<(Vec<String>, Face)> = Vec::new();
    for i in 0..=n {
        let title = if i == n {
            "End: send or accept".to_string()
        } else {
            rail.steps[i].replace('`', "")
        };
        let face = if i == rail.at { Face::UiBold } else { Face::Ui };
        stops.push((ink.ty.wrap(&title, face, px, tw, 2), face));
    }
    let mut tops = Vec::new();
    let mut at = y;
    for (lines, _) in &stops {
        tops.push(at);
        at += lines.len() as f32 * row + GAP * s;
    }
    // scrolled to keep the step shown in its middle third, when they don't all fit
    let bottom = h - 24.0 * s;
    let shift = if at > bottom {
        let want = tops[rail.at.min(n)] - (y + (bottom - y) / 3.0);
        want.clamp(0.0, at - bottom)
    } else {
        0.0
    };
    let tops: Vec<f32> = tops.iter().map(|t| t - shift).collect();
    let mid = |i: usize| tops[i] + row / 2.0;
    for i in 0..n {
        let (a, b) = (mid(i).max(y), mid(i + 1).max(y));
        let c = faint(th.fg, if i < rail.at { 0.45 } else { 0.16 });
        ink.rect(cx - 1.25 * s, a, 2.5 * s, b - a, c);
    }
    for (i, (lines, face)) in stops.iter().enumerate() {
        let top = tops[i];
        if top < y - row || top > h {
            continue;
        }
        let cy = mid(i);
        if i == rail.at {
            ink.disc(cx, cy, 13.0 * s, faint(th.blue, 0.25));
            ink.disc(cx, cy, 8.0 * s, solid(th.blue));
        } else if i < rail.at {
            ink.disc(cx, cy, 6.5 * s, faint(th.fg, 0.6));
        } else {
            ink.disc(cx, cy, 7.0 * s, solid(th.bg));
            ink.ring(cx, cy, 6.0 * s, 2.0 * s, faint(th.fg, 0.4));
        }
        // the steps behind and ahead read alike: their marks tell them apart
        let c = if i == rail.at { th.text() } else { th.dim() };
        for (k, line) in lines.iter().enumerate() {
            ink.boxed(line, *face, px, tx, top + k as f32 * row, row, c);
        }
        let k = rail.comments.get(i).copied().unwrap_or(0);
        if k > 0 {
            count(ink, k, ink.w() - 20.0 * s, top, s);
        }
    }
    // the rail's edge
    let edge = (2.0 * ink.s).max(1.0);
    ink.rect(ink.w() - edge, 0.0, edge, h, faint(th.fg, 0.09));
}

/// How many comments a step has, in a small amber pill ending at `right` (`s`, the
/// rail's scale).
fn count(ink: &mut Ink, k: usize, right: f32, top: f32, s: f32) {
    let th = ink.th;
    let label = k.to_string();
    let px = 16.0 * s;
    let lw = ink.ty.width(&label, Face::UiBold, px);
    let (pw, ph) = ((lw + 14.0 * s).max(24.0 * s), 24.0 * s);
    let x = right - pw;
    let y = top + (ROW - 24.0) / 2.0 * s;
    ink.rrect(x, y, pw, ph, ph / 2.0, faint(bright(th.yellow), 0.22));
    let light = super::code::luma(th.bg) > super::code::luma(th.fg);
    let c = if light { th.text() } else { solid(th.yellow) };
    ink.boxed(&label, Face::UiBold, px, x + (pw - lw) / 2.0, y, ph, c);
}

/// What the foot shows: a message (or nothing), whether more of the page is under the
/// window (not how much: a scroll then sends nothing new here), and the keys.
#[derive(Debug, Clone, Hash)]
pub struct Foot {
    pub msg: String,
    pub more: bool,
    pub end: bool,
}

/// The keys under a step, longest first: the first that fits is shown.
const STEP_KEYS: &[&[(&str, &str)]] = &[
    &[
        ("n", "next"),
        ("p", "back"),
        ("j k", "scroll"),
        ("c", "comment"),
        ("t", "tell"),
        ("v", "diff"),
        ("⏎", "open"),
        ("a", "accept"),
        ("esc", "close"),
    ],
    &[
        ("n", "next"),
        ("p", "back"),
        ("c", "comment"),
        ("t", "tell"),
        ("v", "diff"),
        ("⏎", "open"),
        ("a", "accept"),
        ("esc", "close"),
    ],
    &[
        ("n", "next"),
        ("p", "back"),
        ("c", "comment"),
        ("t", "tell"),
        ("esc", "close"),
    ],
    &[("n", ""), ("p", ""), ("c", "comment"), ("esc", "close")],
];
const END_KEYS: &[&[(&str, &str)]] = &[
    &[
        ("s", "send them back"),
        ("a", "accept"),
        ("p", "back"),
        ("t", "tell"),
        ("v", "diff"),
        ("⏎", "open"),
        ("esc", "close"),
    ],
    &[
        ("s", "send"),
        ("a", "accept"),
        ("p", "back"),
        ("t", "tell"),
        ("esc", "close"),
    ],
    &[("s", "send"), ("a", "accept"), ("esc", "close")],
];

/// The foot, two rows: the message at the left of the first (or how much is under, at
/// its right), a rule, and the keys on the second.
pub fn foot(ink: &mut Ink, foot: &Foot, cell_h: f32) {
    let (th, s) = (ink.th, ink.s);
    let x = 72.0 * s;
    let w = ink.w();
    // the message is read, at the detail's size; the keys are labels
    let (said, px) = (ink.sz.detail, ink.sz.label);
    if !foot.msg.is_empty() {
        let t = ink.ty.fit(&foot.msg, Face::UiBold, said, w - 2.0 * x);
        ink.boxed(&t, Face::UiBold, said, x, 0.0, cell_h, th.text());
    } else if foot.more {
        let t = "↓ more";
        let tw = ink.ty.width(t, Face::Ui, said);
        ink.boxed(t, Face::Ui, said, w - x - tw, 0.0, cell_h, th.dim());
    }
    ink.rect(0.0, cell_h - (2.0 * s).max(1.0), w, (2.0 * s).max(1.0), th.rule());
    let sets = if foot.end { END_KEYS } else { STEP_KEYS };
    let keys = sets
        .iter()
        .find(|k| keys_width(ink.ty, k, px) <= w - 2.0 * x)
        .unwrap_or(&sets[sets.len() - 1]);
    let mut pen = x;
    for (k, what) in keys.iter() {
        pen = ink.keycap(k, what, px, pen, cell_h * 1.5) + 1.1 * px;
    }
}

#[cfg(test)]
#[path = "tests/test_rail.rs"]
mod tests;
