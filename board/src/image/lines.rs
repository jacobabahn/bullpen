//! A task's lines laid out for the image look, a child of render.rs (`Geo`'s): a
//! detail's text wrapped by pixels to the room under its title (a cut one clearly
//! inside where text ends), a stat line broken at its dots and set apart from the words
//! over it, and the room for a ready task's risk's chip after its last line.

use super::said::{key_letters, KEY_SIDE};
use super::{chip, Geo, Row, Rows, RISK_GAP};
use crate::image::font::{tabular, Fonts, Weight};
use crate::ui::rows::DetailRow;

/// How far inside where text ends a detail cut with "…" ends, of its type.
const CUT_IN: f32 = 0.3;

/// The space over a stat line under other lines, of a line's height.
pub(super) const STAT_GAP: f32 = 0.3;

impl Geo {
    /// A detail's text wrapped at its spaces to the room under the ID, `most` lines at
    /// most (the first `less` narrower, and every one `key` narrower), the last cut with
    /// "…" when there's more. One line at least, so a row is never empty.
    pub(super) fn detail_lines(&self, text: &str, most: usize, less: f32, key: f32, fonts: &Fonts) -> Vec<String> {
        let room = self.detail_room() - key;
        let mut lines = fonts.wrap(text, Weight::Regular, self.c.m.body, (room - less, room), most);
        // (a line cut with "…" ends clearly inside where text ends: the dots' ink reaches
        // further right than a letter's)
        if lines.last().is_some_and(|l| l.ends_with('…')) {
            let room = room - CUT_IN * self.c.m.body;
            lines = fonts.wrap(text, Weight::Regular, self.c.m.body, (room - less, room), most);
        }
        fonts.fill_last(&mut lines, Weight::Regular, self.c.m.body, room);
        at_dots(&mut lines, |l| fonts.width(l, Weight::Regular, self.c.m.body) <= room);
        if lines.is_empty() { vec![String::new()] } else { lines }
    }

    /// The room the caps of the keys a detail's text names take on a line: their sides.
    pub(super) fn caps(&self, d: &DetailRow) -> f32 {
        key_letters(&d.text, d.lead).len() as f32 * 2.0 * KEY_SIDE * self.c.m.body
    }

    /// A detail's lines with room for its task's risk's chip after the last: as they
    /// wrap when it fits there; else with the chip's room off every line, when that
    /// cuts no more (a stat line breaks at its dot: "Not toured yet ● high"); else the
    /// chip on a line of its own. A stat line takes a second line when it must, and
    /// one `stacked` is a part a line, broken at its dots. Its figures are tabular.
    pub(super) fn with_risk(&self, d: &DetailRow, less: f32, stacked: bool, fonts: &Fonts) -> Vec<String> {
        // (every figure tabular: a stat line's line up, and a 1 keeps its space)
        let (text, most) = (tabular(&d.text), if d.stat { d.lines.max(2) } else { d.lines });
        let parts: Vec<&str> = text.split(" · ").collect();
        let lines = if stacked && d.stat && parts.len() <= most {
            parts.iter().map(|p| fonts.fit(p, Weight::Regular, self.c.m.body, self.detail_room())).collect()
        } else {
            self.detail_lines(&text, most, less, self.caps(d), fonts)
        };
        let Some(level) = &d.risk else { return lines };
        let chip = self.risk_room(level, fonts);
        let ends = |l: &[String]| l.last().map_or(0.0, |l| fonts.width(l, Weight::Regular, self.c.m.body)) + self.caps(d);
        if ends(&lines) + chip <= self.detail_room() {
            return lines;
        }
        let cut = |l: &[String]| l.last().is_some_and(|l| l.ends_with('…'));
        let narrower = self.detail_lines(&text, most, less, self.caps(d) + chip, fonts);
        if cut(&narrower) == cut(&lines) && ends(&narrower) + chip <= self.detail_room() {
            return narrower;
        }
        let mut lines = lines;
        lines.push(String::new());
        lines
    }

    /// Which rows' stat lines are stacked, a part a line: every one in a group where
    /// one must break at its dot to have room for its risk's chip, so the group's facts
    /// take one shape.
    pub(super) fn stacked(&self, rows: &Rows, fonts: &Fonts) -> Vec<bool> {
        let mut out = vec![false; rows.rows.len()];
        let mut start = 0;
        for i in 0..=rows.rows.len() {
            if i < rows.rows.len() && !matches!(rows.rows[i], Row::Head { .. }) {
                continue;
            }
            let breaks = rows.rows[start..i].iter().any(|r| matches!(r, Row::Detail(d) if d.stat && d.risk.is_some() && self.with_risk(d, 0.0, false, fonts).len() > 1));
            out[start..i].fill(breaks);
            start = i;
        }
        out
    }

    /// How far a detail's risk's chip reaches under its line, in whole pixels: its
    /// block has that much more room under it, so the chip is as clear of a card's
    /// foot as type is.
    pub(super) fn risk_under(&self, d: &DetailRow, fonts: &Fonts) -> u32 {
        if d.risk.is_none() {
            return 0;
        }
        let m = self.c.m;
        let base = ((m.body_line + fonts.cap_height(m.body)) / 2.0).round();
        (base + chip::risk_reach(fonts, m.label).1 - m.body_line).max(0.0).round() as u32
    }

    /// The room a risk's chip takes after a line's text: its gap and its chip.
    pub(super) fn risk_room(&self, level: &str, fonts: &Fonts) -> f32 {
        RISK_GAP * self.c.m.body + self.risk_width(level, fonts)
    }

    /// The space over a stat line under other lines of its task's, setting its facts
    /// apart from their words, in whole pixels.
    pub(super) fn stat_gap(&self, rows: &Rows, i: usize) -> f32 {
        let stat = matches!(rows.rows.get(i), Some(Row::Detail(d)) if d.stat);
        let after = i > 0 && rows.rows[i - 1].is_detail();
        if stat && after { (STAT_GAP * self.c.m.body_line).round() } else { 0.0 }
    }
}

/// Lines wrapped from parts said apart by " · " (a stat line's "5 files, +96 −31 · Not
/// toured yet") break between parts where they can: a line's last part, cut by the
/// break, goes down whole when the line under it has room (`fits`); and no line starts
/// or ends with the dot. (So a line's text can lose a dot: what's drawn on it is read
/// from the lines, not the text.)
pub(super) fn at_dots(lines: &mut [String], fits: impl Fn(&str) -> bool) {
    for k in 1..lines.len() {
        // (a line that starts at a dot has the part above it whole)
        if lines[k].ends_with('…') || lines[k].starts_with("· ") {
            continue;
        }
        if let Some((head, tail)) = lines[k - 1].rsplit_once(" · ") {
            let down = format!("{tail} {}", lines[k].trim_start_matches("· "));
            if head.contains(' ') && fits(&down) {
                (lines[k - 1], lines[k]) = (head.to_string(), down);
            }
        }
    }
    for l in lines.iter_mut() {
        *l = l.trim_start_matches("· ").trim_end_matches(" ·").to_string();
    }
}
