//! The image look (`board_look image`, the board's `I`): the board's task list, its top
//! line and its bottom rows drawn as images and shown with the kitty graphics protocol;
//! what's typed or asked at the bottom, overlays and mission control stay text. The
//! list's rows are the text list's (ui/rows.rs), drawn by render.rs; the top line and
//! the bottom rows are ui's values too, drawn by frame.rs.
//!
//! The board is images, each pinned to its block of cells: the top line, the rows of
//! the list shown (scrolled by the rows' own heights, which the type size sets) and the
//! bottom rows. The list is pieces (pieces.rs): its rows with nothing selected, and the
//! selected block over them, so moving the selection sends a small image; the marks that
//! move and the scroll marks are small images sent once and placed (sprite.rs), so a
//! tick sends a few placements. Each image is drawn at the terminal's cell size and sent
//! only when what it shows changes (list.rs, shown.rs). A client with other cell sizes
//! scales them rather than overflowing. A new image is placed before the old one is
//! taken off, so an update doesn't flicker; the old one is kept a while, hidden, and
//! while the board waits for a key it sends what a j or k would want next (ahead.rs),
//! so those keys only place images. The images are deleted when the board is text
//! again and when it quits. With motion on (`board_motion`), a task whose mark changes
//! to a still one has it settle in; the board redraws every 40 ms meanwhile (`tick`).
//!
//! Before the first image the board asks the terminal whether it draws images, and
//! its colours and cell size (term.rs). It asks for the colours and cell size again
//! only when the person does something that may have changed them: a resize, or
//! switching to the image look (`I`, so `I` twice follows a new theme). There is no
//! timer: each time it asks, the key reader stops for the round trip. When the
//! terminal doesn't draw images, the font can't be loaded or drawing fails, the list
//! is text again and the message line says why.

mod ahead;
mod chip;
mod font;
mod frame;
mod kitty;
mod list;
pub(crate) mod meter;
mod pieces;
mod png;
mod render;
mod shape;
mod shown;
mod sprite;
pub mod term;
mod theme;

use crate::model::{Layout, Look, TextSize};
use crate::ui::bottom::Foot;
use crate::ui::list::Head;
use crate::ui::rows::{Mark, Row, Rows};
use crate::{App, Note};
use shown::{Screen, Slot, Want};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub use font::Fonts;
pub use render::{render, Canvas, Grid};
pub use theme::Theme;

/// The graphics query's image ID; images shown get IDs from FIRST up.
pub const PROBE: u32 = 0x4250_0000;
const FIRST: u32 = PROBE + 1;

/// The board as the frame laid it out, for the images: its top line, its rows (and which
/// was shown first last frame) and its bottom rows, and the blocks of cells they have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub rows: Rows,
    /// the same rows with nothing selected (ui/rows.rs's `unselected`): the list's image
    /// is these, the selected block an image over them (pieces.rs)
    pub base: Rows,
    pub top: usize,
    pub head: Head,
    /// None while the bottom rows are text (something typed or asked there)
    pub foot: Option<Foot>,
    /// the board's first column and row, the list's first row and how many it has, the
    /// rows under it, and the images' columns
    pub col: u16,
    pub row0: u16,
    pub row: u16,
    pub rows_n: u16,
    pub foot_n: u16,
    pub cols: u16,
}

/// What drawing a frame came to: the list's first row, and the tasks a page has.
struct Drawn {
    top: usize,
    page: usize,
}

/// Whether the terminal draws images, as far as the board knows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Probe {
    Unasked,
    Asking,
    Yes,
    /// it doesn't: why, as the message says it
    No(String),
}

/// The image look's state, beside the board's: the main loop owns it, and the
/// terminal's output.
pub struct Picture {
    gate: Arc<term::Gate>,
    probe: Probe,
    /// the questions written are waiting for answers; with the graphics query
    asking: Option<bool>,
    /// the colours and cell size are to be asked again (a resize, or the look switched)
    due: bool,
    /// the image look was on last frame
    on: bool,
    theme: Theme,
    cell: Option<(u32, u32)>,
    /// the font, for the font file it was loaded from
    fonts: Option<(Option<PathBuf>, Fonts)>,
    /// the images sent and where they're placed
    screen: Screen,
    /// each task's mark last frame: a task whose mark changes to a still one has it
    /// settle in (sprite.rs), with motion on
    seen: HashMap<String, Mark>,
    /// the tasks whose marks are settling in, or did while the list's image stayed the
    /// one drawn then, and when they began (ms since the board started)
    settling: HashMap<String, u128>,
    /// what the list's base image shows (list::base_shows): when it changes, the marks
    /// done settling go back in it
    base_shows: u64,
    /// the time, in ms since the board started (set each frame; tests fix it with `clock`)
    ms: u128,
    clock: Option<u128>,
    /// a cell's size from the terminal's window size (tests have none)
    window: fn() -> Option<(u32, u32)>,
    /// the list's base image on the terminal (list.rs)
    list: Option<list::ListImage>,
    /// the view last drawn, and the keys whose images are still to be drawn ahead for
    /// it (ahead.rs)
    ahead_of: Option<View>,
    todo: Vec<char>,
}

impl Picture {
    pub fn new(gate: Arc<term::Gate>) -> Picture {
        Picture {
            gate,
            probe: Probe::Unasked,
            asking: None,
            due: false,
            on: false,
            theme: Theme::default(),
            cell: None,
            fonts: None,
            screen: Screen::new(FIRST),
            seen: HashMap::new(),
            settling: HashMap::new(),
            base_shows: 0,
            ms: 0,
            clock: None,
            window: window_cell,
            list: None,
            ahead_of: None,
            todo: Vec::new(),
        }
    }

    /// Is the list drawn as an image this frame? With the image look, once the terminal
    /// has said it draws images, in the grouped layout (the compact one stays text).
    pub fn drawing(&self, app: &App) -> bool {
        app.look == Look::Image && self.probe == Probe::Yes && app.cfg.layout == Layout::Grouped
    }

    /// After a frame: ask the terminal what the image look needs, then send, place,
    /// move or delete the images as the frame's view says. Returns true when the board
    /// should draw again at once (the list went back to text).
    pub fn show(&mut self, app: &mut App, out: &mut impl Write) -> bool {
        // the time first: `tick` reads it whether or not the images are drawn
        self.ms = self.clock.unwrap_or_else(|| app.started.elapsed().as_millis());
        if app.look != Look::Image {
            self.on = false;
            self.clear(out);
            return false;
        }
        // switched to the image look: the terminal's theme may have changed meanwhile
        self.due |= !std::mem::replace(&mut self.on, true);
        match &self.probe {
            Probe::Unasked => {
                self.ask(true);
                return false;
            }
            Probe::Asking => return false,
            Probe::No(why) => return fall_back(app, why.clone()),
            Probe::Yes => {}
        }
        if self.due {
            self.ask(false);
        }
        let Some(view) = app.view.clone() else {
            self.hide(out);
            return false;
        };
        match self.draw(&view, app.cfg.text_size, app.cfg.font.as_deref(), app.cfg.motion, out) {
            Ok(Drawn { top, page }) => {
                (app.top, app.page) = (top, page);
                self.plan_ahead(Some(&view));
                false
            }
            Err(why) => {
                self.clear(out);
                fall_back(app, why)
            }
        }
    }

    /// Have the key reader stop, so the questions can be written (term.rs). While
    /// it's still reading answers asked before, `due` stays, for once it's done.
    fn ask(&mut self, graphics: bool) {
        if self.asking.is_none() && self.gate.want() {
            self.asking = Some(graphics);
            self.due = false;
            if graphics {
                self.probe = Probe::Asking;
            }
        }
    }

    /// Is the board done asking the terminal, its answers all read? A reload waits for
    /// it: answers that came after the exec would reach the new board as keys.
    pub fn idle(&self) -> bool {
        self.asking.is_none() && self.gate.open()
    }

    /// The key reader has stopped: write the questions.
    pub fn parked(&mut self, out: &mut impl Write) {
        let graphics = self.asking.unwrap_or(false);
        let _ = out.write_all(term::questions(graphics.then_some(PROBE)).as_bytes());
        let _ = out.flush();
        self.gate.asked();
    }

    /// The terminal's answers: whether it draws images, its colours and cell size.
    pub fn answered(&mut self, a: &term::Answers) {
        let graphics = self.asking.take().unwrap_or(false);
        // all the answers came: the colours are what they say; some came late: what
        // they say over what was known
        let known = if a.done { Theme::default() } else { self.theme };
        self.theme = known.with(&a.said);
        if a.cell.is_some() {
            self.cell = a.cell;
        }
        if graphics {
            self.probe = if a.graphics {
                Probe::Yes
            } else if a.done {
                Probe::No("Drawing the list as text: the terminal doesn't draw kitty graphics.".into())
            } else {
                Probe::No("Drawing the list as text: the terminal didn't answer the kitty graphics query.".into())
            };
        }
    }

    /// A cell's size in pixels: the terminal's window size says it, else its answer.
    fn cell(&self) -> Option<(u32, u32)> {
        (self.window)().or(self.cell).filter(|(w, h)| *w > 0 && *h > 0)
    }

    /// Send each image if what it shows changed, and place it. The list scrolls from the
    /// row shown first last frame, by its rows' heights, to keep the selection in view:
    /// returns the row it starts at and the tasks a page has (`app.top`, `app.page`).
    fn draw(&mut self, view: &View, size: TextSize, font: Option<&Path>, motion: bool, out: &mut impl Write) -> Result<Drawn, String> {
        let (cell_w, cell_h) =
            self.cell().ok_or("Drawing the list as text: the terminal doesn't say its cell size in pixels.")?;
        let canvas = Canvas::new(Grid { cols: view.cols as usize, cell_w, cell_h }, size, self.theme);
        let fonts = fonts(&mut self.fonts, font)?;
        let ms = self.ms;
        settle(&mut self.seen, &mut self.settling, &view.rows, motion, ms);
        let prev = self.list.as_ref().map(|l| l.win);
        let plan = meter::time(|| "plan".into(), || pieces::plan(view, &canvas, fonts, prev));
        let f = list::Frame { view, plan: &plan, c: &canvas, size, bare: &|_| false, began: &|_| 0, ms };
        // the list's image is drawn again: the marks done settling go back in it
        let shows = list::base_shows(&f);
        if shows != self.base_shows {
            self.base_shows = shows;
            self.settling.retain(|_, t| ms.saturating_sub(*t) < sprite::settling_ms());
        }
        let paint = Paint { canvas, size, motion, settling: &self.settling, ms, list: self.list.as_ref() };
        let (wants, plan, list) = wants(view, plan, &paint, &self.screen, fonts)?;
        self.screen.show(wants, out).map_err(|e| format!("Drawing the list as text: {e}."))?;
        self.list = Some(list);
        Ok(Drawn { top: plan.top, page: plan.page })
    }

    /// Redraw this soon, though nothing else changes: while a mark settles in.
    pub fn tick(&self) -> Option<Duration> {
        let busy = self.settling.values().any(|t| self.ms.saturating_sub(*t) < sprite::settling_ms());
        busy.then(|| Duration::from_millis(sprite::POP_MS as u64))
    }

    /// The terminal was resized, and may have cleared the screen and the images with it:
    /// the next frame sends the image again (the old one is deleted after, if it's there).
    /// Its cell size may have changed with it, and its colours are asked with that.
    pub fn resized(&mut self) {
        self.due = true;
        self.screen.forget();
        (self.list, self.ahead_of) = (None, None);
    }

    /// Take the images off the screen, keeping them for when the board shows again.
    fn hide(&mut self, out: &mut impl Write) {
        self.screen.hide(out);
    }

    /// Delete the images: the board is text again, or it quits. Back, its marks don't
    /// settle in for what changed meanwhile.
    pub fn clear(&mut self, out: &mut impl Write) {
        self.screen.clear(out);
        self.seen.clear();
        self.settling.clear();
        (self.list, self.ahead_of) = (None, None);
    }
}

/// What a frame's images are drawn with, besides its view and plan: the canvas, the
/// type size, motion, the tasks whose marks are settling in (and since when), the time,
/// and the list's base image on the terminal.
struct Paint<'a> {
    canvas: Canvas,
    size: TextSize,
    motion: bool,
    settling: &'a HashMap<String, u128>,
    ms: u128,
    list: Option<&'a list::ListImage>,
}

/// The images a frame of `view` wants, in the order they're sent, each drawn when the
/// screen doesn't have it; with its plan and the list's base image then. The base image
/// on the terminal stays while it shows the rows shown as they are; when one of them
/// changes, the page is drawn again; scrolled out of it, its plan's window is.
fn wants(view: &View, plan: pieces::Plan, p: &Paint, screen: &Screen, fonts: &mut Fonts) -> Result<(Vec<Want>, pieces::Plan, list::ListImage), String> {
    let (canvas, settling, motion) = (&p.canvas, p.settling, p.motion);
    let bare = |row: &Row| pieces::bare(row, motion, &|id| settling.contains_key(id));
    // a turning mark turns with the text board's, from the board's start
    let began = |row: &Row| match row {
        Row::Task(t) if !sprite::turns(&t.mark) => settling.get(&t.id).copied().unwrap_or(0),
        _ => 0,
    };
    // (a macro, not a closure: each frame borrows its own plan)
    macro_rules! frame {
        ($plan:expr) => {
            list::Frame { view, plan: $plan, c: canvas, size: p.size, bare: &bare, began: &began, ms: p.ms }
        };
    }
    let now = list::list_image(&frame!(&plan));
    let kept = p.list.filter(|d| screen.has(&Slot::List, d.key) && list::still(d, &now, plan.shown.clone()));
    let (plan, image) = match kept {
        _ if screen.has(&Slot::List, now.key) => (plan, now),
        Some(d) => (plan, d.clone()),
        // the same window, but the rows shown changed: the page alone
        None if p.list.is_some_and(|d| d.win == now.win) => {
            let plan = pieces::plan(view, canvas, fonts, None);
            let image = list::list_image(&frame!(&plan));
            (plan, image)
        }
        // a window of its own (scrolled out of the last, or the first)
        None => (plan, now),
    };
    let f = frame!(&plan);
    let head_n = view.row - view.row0;
    let foot_n = if view.foot.is_some() { view.foot_n } else { 0 };
    let mut wants = Vec::new();
    let k = list::key(&Slot::Head, &f, (&view.head, head_n));
    if head_n > 0 {
        let png = (!screen.has(&Slot::Head, k)).then(|| list::encode(&Slot::Head, || frame::head(&view.head, head_n, canvas, fonts))).transpose()?;
        wants.push(Want { slot: Slot::Head, key: k, png, places: vec![whole(view, canvas, view.row0, head_n)] });
    }
    let mut list = list::wants(&f, image.key, screen, fonts)?;
    let sprites = list.split_off(list.iter().position(|w| matches!(w.slot, Slot::Sel | Slot::Tail | Slot::Cap | Slot::Sprite(_))).unwrap_or(list.len()));
    wants.extend(list);
    if let Some(foot) = view.foot.as_ref().filter(|_| foot_n > 0) {
        let k = list::key(&Slot::Foot, &f, (foot, foot_n));
        let png = (!screen.has(&Slot::Foot, k)).then(|| list::encode(&Slot::Foot, || frame::foot(foot, foot_n, canvas, fonts))).transpose()?;
        wants.push(Want { slot: Slot::Foot, key: k, png, places: vec![whole(view, canvas, view.row + view.rows_n, foot_n)] });
    }
    wants.extend(sprites);
    drop(f);
    Ok((wants, plan, image))
}

/// Note the tasks' marks: one that changed to a still mark settles in from now (`ms`).
/// With motion off, none does.
fn settle(seen: &mut HashMap<String, Mark>, settling: &mut HashMap<String, u128>, rows: &Rows, motion: bool, ms: u128) {
    if !motion {
        seen.clear();
        settling.clear();
        return;
    }
    let mut now = HashMap::new();
    for row in &rows.rows {
        if let Row::Task(t) = row {
            if seen.get(&t.id).is_some_and(|m| *m != t.mark) && !sprite::turns(&t.mark) {
                settling.insert(t.id.clone(), ms);
            }
            now.insert(t.id.clone(), t.mark.clone());
        }
    }
    *seen = now;
}

/// The placement of an image as wide as the board's images, from the board's row `row`,
/// `rows` cells high: all of it.
fn whole(view: &View, c: &Canvas, row: u16, rows: u16) -> kitty::Place {
    let (crop_w, crop_h) = (c.grid.width(), rows as u32 * c.grid.cell_h);
    kitty::Place { id: 0, p: 1, col: view.col, row, crop_x: 0, crop_y: 0, crop_w, crop_h, cols: view.cols, rows, z: 0 }
}

/// The font for `path` (the board_font setting), loaded once into `slot`.
fn fonts<'a>(slot: &'a mut Option<(Option<PathBuf>, Fonts)>, path: Option<&Path>) -> Result<&'a mut Fonts, String> {
    if slot.as_ref().map(|(p, _)| p.as_deref()) != Some(path) {
        let fonts = Fonts::load(path).map_err(|e| format!("Drawing the list as text: {e}."))?;
        *slot = Some((path.map(Path::to_path_buf), fonts));
    }
    Ok(&mut slot.as_mut().expect("loaded").1)
}

/// A cell's size from the window's size in pixels and in cells, when the terminal says both.
fn window_cell() -> Option<(u32, u32)> {
    let s = ratatui::crossterm::terminal::window_size().ok()?;
    (s.width > 0 && s.height > 0 && s.columns > 0 && s.rows > 0)
        .then(|| (s.width as u32 / s.columns as u32, s.height as u32 / s.rows as u32))
}

/// Back to text, saying why on the message line.
fn fall_back(app: &mut App, why: String) -> bool {
    app.look = Look::Text;
    app.msg = Note::info(why);
    true
}

/// Write the board as the image look draws it to a PNG file (`bullpen-board … --render
/// FILE`): its top line, its whole list and its key row, `view.cols` columns wide, at a
/// cell of `cell` pixels, in `theme`'s colours on its background. For looking at the
/// image look without a terminal.
pub fn render_file(view: &View, size: TextSize, cell: (u32, u32), theme: Theme, font: Option<&Path>, path: &Path) -> Result<(), String> {
    let mut fonts = Fonts::load(font)?;
    let canvas = Canvas::new(Grid { cols: view.cols as usize, cell_w: cell.0, cell_h: cell.1 }, size, theme);
    let laid = render::lay(&view.rows, &canvas, &fonts);
    let list_h = laid.heights.iter().sum::<u32>().div_ceil(cell.1) * cell.1;
    let n = view.rows.rows.len();
    let parts = [
        frame::head(&view.head, view.row - view.row0, &canvas, &mut fonts)?,
        render(&view.rows, &laid, render::Cut::rows(0..n, list_h), &canvas, &mut fonts)?.pixmap,
        match &view.foot {
            Some(foot) => frame::foot(foot, view.foot_n, &canvas, &mut fonts)?,
            None => tiny_skia::Pixmap::new(1, 1).ok_or("can't make the image")?,
        },
    ];
    // one under the other, on the background, as a terminal shows them
    let height = parts.iter().map(|p| p.height()).sum();
    let mut pm = tiny_skia::Pixmap::new(canvas.grid.width(), height).ok_or("can't make the image")?;
    pm.fill(tiny_skia::Color::from_rgba8(theme.bg.0, theme.bg.1, theme.bg.2, 255));
    let mut y = 0;
    for p in &parts {
        pm.draw_pixmap(0, y, p.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        y += p.height() as i32;
    }
    std::fs::write(path, png::encode(&pm)).map_err(|e| format!("can't write {}: {e}", path.display()))
}

#[cfg(test)]
#[path = "tests/test_image.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/test_bench.rs"]
mod bench;
