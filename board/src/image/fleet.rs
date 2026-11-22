//! Mission control's own pieces in the image look (docs/mission.md): its header, the
//! whole fleet at a glance, and a repository's heading in its list. The header is the
//! name and the chips of what's blocked, needs you and is ready, then a status bar with
//! a run of segments for each repository (a segment a task, in its group's colour, as the
//! board's bar), its coordinator's mark and its name under it, and what needs you or is
//! ready there when it fits; the runs wrap onto more lines rather than cut a name. A
//! repository's heading is its name, its folder and how many are open, its coordinators
//! at the right (each a mark and its state); a blocked coordinator's heading sits on
//! the amber wash with its bar, its question under it, as a task that needs you does. A
//! child of render.rs, so it draws with `Draw`'s own parts.

use super::{Canvas, Draw, Flag, Weight};
use crate::image::bar;
use crate::image::chip;
use crate::image::font::Fonts;
use crate::image::theme::{lit, solid, Theme};
use crate::ui::rows::{Mark, RepoRow};
use tiny_skia::{PathBuilder, Pixmap};

/// The header's values: mission control's name, its chips, the time, and the fleet's
/// bar as its lines of repositories.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fleet {
    pub name: String,
    /// after it, dim: what's under way ("3 of 9 running · 4 repos")
    pub after: String,
    /// each chip's words, its number alone (for when the words don't fit) and the
    /// status whose colour it takes
    pub chips: Vec<(String, String, &'static str)>,
    /// at the right edge, dim; empty on a narrow pane
    pub clock: String,
    /// the bar's lines, each its repositories (`lines` packs them)
    pub lines: Vec<Vec<FleetRepo>>,
}

/// A repository in the header's bar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FleetRepo {
    pub name: String,
    /// its coordinators' state, as one: blocked, gone (or none), working, unknown, idle
    pub word: &'static str,
    /// its open tasks' statuses, in the board's order
    pub tasks: Vec<&'static str>,
    /// what needs you or is ready there ("2 need you · 1 ready"); dropped first
    pub said: String,
}

/// The header's first line, of a cell's height: its top and its height (as the board's).
const TOP: f32 = 0.2;
const LINE: f32 = 1.3;
/// A bar line's top, of a cell's height: the first, and each next one's step.
const BAR: f32 = 1.6;
const STEP: f32 = 1.6;
/// A bar's height, and the space under it before its names' line.
const BAR_H: f32 = 0.24;
const UNDER: f32 = 0.14;
/// The space between two repositories' runs, of a cell's width.
const APART: f32 = 1.5;
/// The name's size, of a title's (frame.rs's).
const NAME: f32 = 1.15;

/// How many cells the header takes with `lines` lines of bar: its line, then each
/// line's bar and names.
pub fn rows(lines: usize) -> u16 {
    if lines == 0 {
        return 2;
    }
    (BAR + (lines - 1) as f32 * STEP + BAR_H + UNDER + 1.0).ceil() as u16
}

/// Pack the repositories into the bar's lines, `cols` columns wide: each takes at least
/// its name and its mark (counted a column a character, which the type is narrower than)
/// and the space between runs; one that doesn't fit beside the others starts a line.
pub fn lines(repos: Vec<FleetRepo>, cols: usize) -> Vec<Vec<FleetRepo>> {
    let room = cols.saturating_sub(3);
    let mut out: Vec<Vec<FleetRepo>> = Vec::new();
    let mut used = 0;
    for r in repos {
        let need = crate::model::width(&r.name) + 3;
        match out.last_mut() {
            Some(line) if used + 2 + need <= room => {
                used += 2 + need;
                line.push(r);
            }
            _ => {
                used = need;
                out.push(vec![r]);
            }
        }
    }
    out
}

/// The header, in an image the grid's columns wide and `rows` cells high: the name, the
/// chips (their numbers alone when the words don't fit) and the time at the right, then
/// the fleet's bar.
pub fn head(f: &Fleet, rows: u16, c: &Canvas, fonts: &mut Fonts) -> Result<Pixmap, String> {
    let (w, h) = (c.grid.width(), rows as u32 * c.grid.cell_h);
    let mut pm = Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| format!("can't make an image {w}×{h} pixels"))?;
    let mut d = Draw::new(&mut pm, fonts, c, 0.0);
    let ch = c.grid.cell_h as f32;
    let (top, line) = ((TOP * ch).round(), (LINE * ch).round());
    d.set((NAME * c.m.title).round(), line);
    let theme = d.theme;
    let left = c.left();
    let mut x = d.right();
    if !f.clock.is_empty() {
        let cw = d.fonts.width(&f.clock, Weight::Regular, c.m.detail);
        d.text_in(c.m.detail, &f.clock, Weight::Regular, x - cw, top, cw + 1.0, theme.dim());
        x -= cw + 1.2 * c.m.title;
    }
    let px = c.m.label + 1.0;
    let name_w = d.width(&f.name, Weight::Bold);
    let words: f32 = f.chips.iter().map(|(t, ..)| chip::width(d.fonts, t, px) + 0.4 * px).sum();
    let short = words > x - left - name_w - 0.8 * c.m.title;
    for (text, number, st) in f.chips.iter().rev() {
        let text = if short { number } else { text };
        x = chip::chip(&mut d, text, px, x, top + line / 2.0, theme.status(st).unwrap_or(theme.dim())) - 0.4 * px;
    }
    let end = d.text(&f.name, Weight::Bold, left, top, x - left, theme.text());
    let gap = 0.6 * c.m.title;
    if end + gap < x - 0.8 * c.m.title {
        d.text_in(c.m.title, &f.after, Weight::Regular, end + gap, top, x - 0.8 * c.m.title - end - gap, theme.dim());
    }
    for (i, repos) in f.lines.iter().enumerate() {
        bar_line(&mut d, c, repos, ((BAR + i as f32 * STEP) * ch).round());
    }
    Ok(pm)
}

/// A line of the bar, whose top is y: each repository a run as wide as its name needs
/// and a share of the rest by its open tasks, its segments, then its mark and name
/// under them, and what needs you or is ready there when it fits.
fn bar_line(d: &mut Draw, c: &Canvas, repos: &[FleetRepo], y: f32) {
    let ch = c.grid.cell_h as f32;
    let px = c.m.label;
    let (l, r) = (c.left(), c.right());
    let apart = (APART * c.c()).round();
    let mark_w = 2.0 * label_mark_r(d, px) + 0.45 * px;
    let needs: Vec<f32> = repos.iter().map(|f| mark_w + d.fonts.width(&f.name, name_weight(f), px)).collect();
    let spare = (r - l - apart * (repos.len() as f32 - 1.0) - needs.iter().sum::<f32>()).max(0.0);
    let weights: Vec<f32> = repos.iter().map(|f| f.tasks.len().max(1) as f32).collect();
    let total: f32 = weights.iter().sum();
    let (bar_h, gap) = ((BAR_H * ch).round().max(2.0), (0.18 * c.c()).round().max(2.0));
    let names_y = (y + bar_h + UNDER * ch).round();
    let theme = d.theme;
    let mut x = l;
    for ((f, need), weight) in repos.iter().zip(&needs).zip(&weights) {
        let width = (need + spare * weight / total).min(r - x);
        if f.tasks.is_empty() {
            d.rounded_fill(x, y, width, bar_h, (bar_h / 2.0).floor(), theme.rule());
        }
        for (sx, sw, st) in bar::segments(&runs(&f.tasks), x, x + width, gap) {
            bar::mark(d, sx, y, sw, bar_h, st);
        }
        d.set(px, ch);
        let mr = label_mark_r(d, px);
        let scale = std::mem::replace(&mut d.scale, mr / d.g.mark_r());
        d.coordinator_mark(f.word, x + mr, names_y + ch / 2.0);
        d.scale = scale;
        let color = match f.word {
            "blocked" | "gone" => solid(theme.yellow),
            _ => theme.heading(),
        };
        let end = d.text(&f.name, name_weight(f), x + mark_w, names_y, width - mark_w, color);
        let said = format!("  {}", f.said);
        if !f.said.is_empty() && end + d.fonts.width(&said, Weight::Regular, px) <= x + width {
            d.text(&said, Weight::Regular, end, names_y, x + width - end, theme.dim());
        }
        x += width + apart;
    }
}

/// A repository's name in the bar: semibold while its coordinator is blocked.
fn name_weight(f: &FleetRepo) -> Weight {
    if f.word == "blocked" { Weight::Bold } else { Weight::Regular }
}

/// A coordinator's mark's radius beside a label's type: a little under its capitals.
fn label_mark_r(d: &Draw, px: f32) -> f32 {
    ((0.62 * px).round() / 2.0).min(d.g.mark_r())
}

/// Statuses in the board's order as the bar's tallies: a run of each.
fn runs(tasks: &[&'static str]) -> Vec<crate::ui::list::Tally> {
    let mut out: Vec<crate::ui::list::Tally> = Vec::new();
    for st in tasks {
        match out.last_mut() {
            Some(t) if t.status == *st => t.n += 1,
            _ => out.push(crate::ui::list::Tally { n: 1, word: "", status: st }),
        }
    }
    out
}

/// What a coordinator's state's words are drawn in: blocked semibold amber, gone (or
/// none) amber, working a step under the foreground, idle and unknown dim.
fn coordinator_ink(theme: &Theme, word: &str) -> (Weight, [u8; 4]) {
    match word {
        "blocked" => (Weight::Bold, solid(theme.yellow)),
        "gone" => (Weight::Regular, solid(theme.yellow)),
        "working" => (Weight::Regular, theme.soft()),
        _ => (Weight::Regular, theme.dim()),
    }
}

impl Draw<'_> {
    /// A coordinator's state's mark at (cx, cy): working the working task's ring with a
    /// quarter drawn, blocked the amber disc of one that needs you, idle a dim ring, gone
    /// an amber cross, unknown a dotted ring.
    pub(in crate::image) fn coordinator_mark(&mut self, word: &str, cx: f32, cy: f32) {
        match word {
            "working" => self.mark(&Mark::Working, cx, cy, 0.0),
            "blocked" => self.mark(&Mark::Still("needs".into()), cx, cy, 0.0),
            "unknown" => self.mark(&Mark::Still("todo".into()), cx, cy, 0.0),
            "gone" => {
                let r = self.mark_r() * 0.72;
                let mut pb = PathBuilder::new();
                pb.move_to(cx - r, cy - r);
                pb.line_to(cx + r, cy + r);
                pb.move_to(cx + r, cy - r);
                pb.line_to(cx - r, cy + r);
                if let Some(p) = pb.finish() {
                    let (lw, amber) = (self.line() * 1.2, solid(lit(self.theme.yellow)));
                    self.stroke(&p, lw, amber);
                }
            }
            _ => self.mark(&Mark::Still("working".into()), cx, cy, 0.0),
        }
    }

    /// A repository's heading, its first line's top at y, its row's bottom at `bottom`:
    /// its name, where it is, its coordinators at the right; blocked, on the amber wash
    /// with its bar, the question under it. Without room the place goes first, then the
    /// coordinators' words (their marks stay).
    pub(super) fn repo(&mut self, r: &RepoRow, y: f32, bottom: f32) {
        let m = self.m();
        if r.question.is_some() {
            let under = if m.cells { 0.0 } else { m.below };
            self.wash(y, bottom - under, Flag::Needs);
            self.bar(y, bottom - under, Flag::Needs);
        }
        self.set(m.title, m.line);
        let theme = self.theme;
        let (left, right) = (self.g.c.left(), self.right());
        let px = m.detail;
        let name_w = self.width(&r.name, Weight::Bold);
        let gap = 0.8 * m.title;
        let full = self.coordinators_width(r, px, true);
        let words = full + name_w + gap <= right - left;
        let start = right - if words { full } else { self.coordinators_width(r, px, false) };
        let end = self.text(&r.name, Weight::Bold, left, y, start - gap - left, theme.text());
        self.coordinators(r, px, start, y, words);
        // the place, when a few words of it fit
        let room = start - gap - end - 0.6 * m.title;
        if room > 4.0 * m.body {
            self.text_in(m.body, &r.place, Weight::Regular, end + 0.6 * m.title, y, room, theme.dim());
        }
        if let Some(q) = &r.question {
            self.set(m.body, m.body_line);
            let x = self.text_x();
            self.text(q, Weight::Regular, x, y + m.line, right - x, theme.soft());
        }
    }

    /// How wide a repository's coordinators are in type px, with their words or without.
    fn coordinators_width(&self, r: &RepoRow, px: f32, words: bool) -> f32 {
        let each = |(text, word): &(String, &str)| {
            let text = if words { text.as_str() } else { short(text) };
            let (weight, _) = coordinator_ink(&self.theme, word);
            2.0 * self.mark_r() + 0.5 * px + self.fonts.width(text, weight, px)
        };
        let gap = 1.2 * px * r.coordinators.len().saturating_sub(1) as f32;
        r.coordinators.iter().map(each).sum::<f32>() + gap
    }

    /// The coordinators from x on the line whose top is y: each its mark, then its ID and
    /// state (its ID alone without `words`).
    fn coordinators(&mut self, r: &RepoRow, px: f32, mut x: f32, y: f32, words: bool) {
        let theme = self.theme;
        for (text, word) in &r.coordinators {
            let mr = self.mark_r();
            self.coordinator_mark(word, x + mr, y + self.h() / 2.0);
            x += 2.0 * mr + 0.5 * px;
            let (weight, color) = coordinator_ink(&theme, word);
            let text = if words { text.as_str() } else { short(text) };
            let w = self.fonts.width(text, weight, px);
            x = self.text_in(px, text, weight, x, y, w + 1.0, color) + 1.2 * px;
        }
    }
}

/// A coordinator's words without its state: its ID ("c4"), or "none".
fn short(text: &str) -> &str {
    if text == "no coordinator" { "none" } else { text.split(' ').next().unwrap_or(text) }
}

#[cfg(test)]
#[path = "tests/test_fleet.rs"]
mod tests;
