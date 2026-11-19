//! The drawn tour's page: a step (its title, the files and lines it covers, its note,
//! your comments on it, then its code, code.rs) or, after the last step, the end (your
//! comments under their steps' titles, and the two ways out, s and a, as large keys).
//! A page is as wide as the page's columns and as tall as it is; it's drawn a tile at
//! a time, on an image that tile's height (`Ink`'s window), and the tour shows a window
//! of it, scrolled (mod.rs).

use super::code::{self, File};
use super::ink::{Face, Ink};
use bullpen_draw::theme::{faint, solid};

/// A step's page: what it says.
pub struct Step<'a> {
    pub title: &'a str,
    /// each file it covers and its line ranges ("1–8")
    pub refs: Vec<(String, Vec<String>)>,
    pub note: &'a str,
    /// your comments on it
    pub comments: Vec<&'a str>,
    pub files: Vec<File>,
}

/// The end: the task, how many steps, and your comments (their step, its title, the text).
pub struct End<'a> {
    pub tid: &'a str,
    pub steps: usize,
    pub comments: Vec<(String, String, String)>,
}

pub enum Page<'a> {
    Step(Step<'a>),
    End(End<'a>),
}

/// The page's margin at a 2× cell (to the nearest cell), and the widest a column of
/// prose gets.
const MARGIN: f32 = 72.0;
const COLUMN: f32 = 1500.0;
const CARD: f32 = 1300.0;

/// Draw the page on ink, `ink.w()` wide; the height it takes. On an ink one pixel high this
/// measures it.
pub fn draw(ink: &mut Ink, page: &Page) -> f32 {
    match page {
        Page::Step(st) => step(ink, st),
        Page::End(end) => self::end(ink, end),
    }
}

/// The page's left edge: a cell's, so its code is on the terminal's grid.
fn margin(ink: &Ink) -> f32 {
    let cw = ink.sz.cell.0;
    (MARGIN * ink.s / cw).round().max(1.0) * cw
}

fn step(ink: &mut Ink, st: &Step) -> f32 {
    let (th, s, sz) = (ink.th, ink.s, ink.sz);
    let x = margin(ink);
    let width = ink.w() - 2.0 * x;
    let col = width.min(COLUMN * s);
    let mut y = (46.0 * s).round();
    let (tpx, tlh) = (sz.heading, (1.35 * sz.heading).round());
    y += ink.prose(st.title, true, tpx, x, y, col, tlh, th.text());
    y += 6.0 * s;
    y += files(ink, &st.refs, x, y, width);
    y += 18.0 * s;
    if !st.note.trim().is_empty() {
        let lh = (1.5 * sz.body).round();
        y += ink.prose(st.note, false, sz.body, x, y, col, lh, faint(th.fg, 0.9));
        y += 26.0 * s;
    }
    for c in &st.comments {
        y += card(ink, c, x, y, col.min(CARD * s)) + 14.0 * s;
    }
    y += 18.0 * s;
    if st.files.is_empty() {
        let h = (1.5 * sz.body).round();
        ink.boxed("No code in this step.", Face::Ui, sz.body, x, y, h, th.dim());
        y += h;
    } else {
        // the code's rows are the terminal's
        y = sz.on_row(y);
        code::draw(ink, &st.files, x, y, width);
        y += code::height(&st.files, sz.cell.1);
    }
    y + 40.0 * s
}

/// The files a step covers: each file's folder dim, its name, and its lines, wrapped.
fn files(ink: &mut Ink, refs: &[(String, Vec<String>)], x: f32, top: f32, max: f32) -> f32 {
    let th = ink.th;
    let px = ink.sz.detail;
    let h = (1.6 * px).round();
    let (mut pen, mut y) = (x, top);
    for (file, lines) in refs {
        let (dir, base) = file
            .rsplit_once('/')
            .map_or((String::new(), file.as_str()), |(d, b)| (format!("{d}/"), b));
        let ls = lines.join(", ");
        let need = ink.ty.width(&dir, Face::Mono, px)
            + ink.ty.width(base, Face::MonoBold, px)
            + ink.ty.width(&ls, Face::Ui, px)
            + 0.5 * px;
        if pen > x && pen + need > x + max {
            (pen, y) = (x, y + h);
        }
        pen = ink.boxed(&dir, Face::Mono, px, pen, y, h, th.dim());
        pen = ink.boxed(base, Face::MonoBold, px, pen, y, h, th.text());
        if !ls.is_empty() {
            pen = ink.boxed(&ls, Face::Ui, px, pen + 0.5 * px, y, h, th.dim());
        }
        pen += 1.6 * px;
    }
    if refs.is_empty() {
        ink.boxed("No code", Face::Ui, px, x, y, h, th.dim());
    }
    y + h - top
}

/// A comment of yours: on a wash of amber, a bar of it at its left. Its height.
pub fn card(ink: &mut Ink, text: &str, x: f32, top: f32, w: f32) -> f32 {
    let (th, s) = (ink.th, ink.s);
    let px = ink.sz.body;
    let lh = (1.45 * px).round();
    let inner = w - 52.0 * s;
    let h = ink.prose_height(text, false, px, inner, lh) + 28.0 * s;
    ink.rrect(x, top, w, h, 10.0 * s, faint(th.amber(), 0.1));
    ink.rect(x, top, (5.0 * s).max(2.0), h, solid(th.yellow));
    ink.prose(text, false, px, x + 28.0 * s, top + 14.0 * s, inner, lh, th.text());
    h
}

fn end(ink: &mut Ink, end: &End) -> f32 {
    let (th, s, sz) = (ink.th, ink.s, ink.sz);
    let x = margin(ink);
    let width = ink.w() - 2.0 * x;
    let col = width.min(COLUMN * s);
    let mut y = (46.0 * s).round();
    let (tpx, tlh) = (sz.heading, (1.35 * sz.heading).round());
    let (px, lh) = (sz.body, (1.5 * sz.body).round());
    let n = end.comments.len();
    if n == 0 {
        y += ink.prose("End of the tour", true, tpx, x, y, col, tlh, th.text()) + 10.0 * s;
        let says = format!("No comments on its {}.", plural(end.steps, "step"));
        y += ink.prose(&says, false, px, x, y, col, lh, th.dim()) + 40.0 * s;
        let what = format!("Merge {} as it is.", end.tid);
        y += big_key(ink, "a", "Accept", &what, x, y, col.min(640.0 * s));
        return y + 40.0 * s;
    }
    y += ink.prose("Your comments", true, tpx, x, y, col, tlh, th.text()) + 10.0 * s;
    let on: std::collections::BTreeSet<&String> = end.comments.iter().map(|c| &c.0).collect();
    let says = format!(
        "{} on {} of the {}, not sent yet.",
        plural(n, "comment"),
        on.len(),
        plural(end.steps, "step")
    );
    y += ink.prose(&says, false, px, x, y, col, lh, th.dim()) + 40.0 * s;
    for (step, title, text) in &end.comments {
        let (spx, slh) = (sz.detail, (1.5 * sz.detail).round());
        let e = ink.boxed(step, Face::UiBold, spx, x, y, slh, th.dim());
        ink.prose(
            title,
            false,
            spx,
            e + 14.0 * s,
            y,
            col - (e - x) - 14.0 * s,
            slh,
            th.dim(),
        );
        y += slh + 8.0 * s;
        y += card(ink, text, x, y, col.min(CARD * s)) + 28.0 * s;
    }
    y += 20.0 * s;
    let one = 640.0 * s;
    let send = format!(
        "{} goes back to working: its worker answers each one and runs ready again.",
        end.tid
    );
    let accept = "Merge it as it is. With comments unsent, a asks again first.";
    if col >= 2.0 * one + 80.0 * s {
        let h = big_key(ink, "s", "Send them back", &send, x, y, one);
        y += h.max(big_key(ink, "a", "Accept", accept, x + one + 80.0 * s, y, one));
    } else {
        y += big_key(ink, "s", "Send them back", &send, x, y, col.min(one)) + 30.0 * s;
        y += big_key(ink, "a", "Accept", accept, x, y, col.min(one));
    }
    y + 40.0 * s
}

/// A way out: its key in a large cap, what it does, and what that means. Its height.
fn big_key(ink: &mut Ink, key: &str, what: &str, says: &str, x: f32, y: f32, w: f32) -> f32 {
    let (th, s, sz) = (ink.th, ink.s, ink.sz);
    let cap = (2.1 * sz.body).round();
    ink.rrect(x, y, cap, cap, 12.0 * s, th.cap());
    let edge = (2.0 * s).max(1.0);
    ink.rstroke(
        x + edge / 2.0,
        y + edge / 2.0,
        cap - edge,
        cap - edge,
        12.0 * s,
        edge,
        th.cap_edge(),
    );
    let kw = ink.ty.width(key, Face::UiBold, sz.body);
    ink.boxed(key, Face::UiBold, sz.body, x + (cap - kw) / 2.0, y, cap, th.text());
    let tx = x + cap + 22.0 * s;
    let head = (1.35 * sz.body).round();
    ink.boxed(what, Face::UiBold, sz.body, tx, y, head, th.text());
    let lh = (1.45 * sz.detail).round();
    let h = ink.prose(says, false, sz.detail, tx, y + head, w - cap - 22.0 * s, lh, th.dim());
    (head + h).max(cap)
}

fn plural(n: usize, word: &str) -> String {
    super::super::screen::plural(n, word)
}

#[cfg(test)]
#[path = "tests/test_page.rs"]
mod tests;
