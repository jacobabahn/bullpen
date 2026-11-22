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
mod bar;
mod chip;
mod frame;
mod list;
pub(crate) mod meter;
mod pieces;
mod render;
mod shown;
mod sprite;
pub mod term;
mod theme;
pub mod worker;

use bullpen_draw::{font, kitty, png, shape};

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
pub use render::fleet::{self, Fleet, FleetRepo};
pub use render::page::{self, Ink, Page};
pub use render::{render, Canvas, Grid};
pub use theme::Theme;

/// The graphics query's image ID; images shown get IDs from FIRST up.
pub const PROBE: u32 = 0x4250_0000;
const FIRST: u32 = PROBE + 1;

/// The board (or mission control) as the frame laid it out, for the images: its top
/// line, its rows (and which was shown first last frame) and its bottom rows, and the
/// blocks of cells they have; mission control's header and the page beside its list.
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
    /// the list's columns
    pub cols: u16,
    /// the top line's and the bottom rows' columns: the list's on the board, the
    /// pane's on mission control, whose list has the page beside it
    pub full: u16,
    /// mission control's header, drawn in the top line's place (fleet.rs)
    pub fleet: Option<Fleet>,
    /// mission control's page beside the list (page.rs)
    pub side: Option<Side>,
}

/// The page beside mission control's list: its first column (from the view's), its
/// columns, and what it says. It's placed from the list's first row, as many rows as it
/// needs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Side {
    pub col: u16,
    pub cols: u16,
    pub page: Page,
}

/// What the image look draws for: the board, or mission control. It says the look and
/// the type, hands over the frame's view, takes back where the list starts and a page's
/// tasks, and goes back to text when the images can't be drawn; drawing ahead presses
/// j and k on it off screen.
pub trait Host {
    fn look(&self) -> Look;
    /// the type's size, the font file it's drawn with, and whether the marks move
    fn style(&self) -> (TextSize, Option<PathBuf>, bool);
    fn started(&self) -> std::time::Instant;
    /// the frame's view; None when nothing is images this frame (an overlay)
    fn view(&self) -> Option<View>;
    /// the list's first row and the tasks a page has, as the images scrolled it
    fn placed(&mut self, top: usize, page: usize);
    /// back to text, saying why
    fn fall_back(&mut self, why: String);
    /// can it be drawn as images at all (the board's compact layout can't)
    fn drawable(&self) -> bool;
    /// nothing asked or typed: j and k only move the selection
    fn at_rest(&self) -> bool;
    /// its view after `key`, drawn off screen in `area`, with what the key changed put
    /// back; None when the key wouldn't move the selection
    fn moved(&mut self, key: char, area: ratatui::layout::Rect) -> Option<View>;
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
    /// the thread drawing ahead's base images are drawn on (worker.rs), once the board
    /// gives it its loop's channel
    worker: Option<worker::Worker>,
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
            worker: None,
            ahead_of: None,
            todo: Vec::new(),
        }
    }

    /// A picture whose terminal has said it draws images, with cells `cell` pixels (no
    /// window size), the time standing still: for tests of what's drawn for it.
    #[cfg(test)]
    pub fn answered_for_test(cell: (u32, u32)) -> Picture {
        let p = Picture::new(Arc::new(term::Gate::default()));
        Picture { window: || None, clock: Some(0), probe: Probe::Yes, cell: Some(cell), on: true, ..p }
    }

    /// Is the list drawn as an image this frame? With the image look, once the terminal
    /// has said it draws images, when it can be (the board's compact layout stays text).
    pub fn drawing(&self, h: &impl Host) -> bool {
        h.look() == Look::Image && self.probe == Probe::Yes && h.drawable()
    }

    /// After a frame: ask the terminal what the image look needs, then send, place,
    /// move or delete the images as the frame's view says. Returns true when the board
    /// should draw again at once (the list went back to text).
    pub fn show(&mut self, app: &mut impl Host, out: &mut impl Write) -> bool {
        // the time first: `tick` reads it whether or not the images are drawn
        self.ms = self.clock.unwrap_or_else(|| app.started().elapsed().as_millis());
        if app.look() != Look::Image {
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
            Probe::No(why) => {
                app.fall_back(why.clone());
                return true;
            }
            Probe::Yes => {}
        }
        if self.due {
            self.ask(false);
        }
        let Some(view) = app.view() else {
            self.hide(out);
            return false;
        };
        let (size, font, motion) = app.style();
        match self.draw(&view, size, font.as_deref(), motion, out) {
            Ok(Drawn { top, page }) => {
                app.placed(top, page);
                self.plan_ahead(Some(&view));
                false
            }
            Err(why) => {
                self.clear(out);
                app.fall_back(why);
                true
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
        let (canvas, frame, side) = canvases(view, (cell_w, cell_h), size, self.theme);
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
        let paint = Paint { canvas, frame, side, size, motion, settling: &self.settling, ms, list: self.list.as_ref() };
        let (wants, plan, list) = wants(view, plan, &paint, &self.screen, fonts, None)?;
        self.screen.show(wants, out).map_err(|e| format!("Drawing the list as text: {e}."))?;
        self.list = Some(list);
        Ok(Drawn { top: plan.top, page: plan.page })
    }

    /// Draw ahead's base images on a thread of their own, which answers on the board's
    /// loop's channel (`Msg::Ready`).
    pub fn use_thread(&mut self, board: std::sync::mpsc::Sender<crate::Msg>) {
        self.worker = Some(worker::Worker::start(board));
    }

    /// A base image drawn ahead came back from its thread: send it, unplaced, if it's
    /// still wanted and the board is still images.
    pub fn ready(&mut self, r: worker::Ready, out: &mut impl Write) {
        let wanted = self.worker.as_mut().is_some_and(|w| w.done(r.key));
        if let (true, true, Ok(png)) = (wanted, self.on, r.png) {
            let want = Want { slot: Slot::List, key: r.key, png: Some(png), places: vec![] };
            let _ = self.screen.preload(vec![want], out);
        }
    }

    /// Is a base image being drawn ahead on its thread?
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        self.worker.as_ref().is_some_and(worker::Worker::busy)
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
        self.worker.as_mut().map(worker::Worker::drop_all);
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
        self.worker.as_mut().map(worker::Worker::drop_all);
    }
}

/// What a frame's images are drawn with, besides its view and plan: the canvas, the
/// type size, motion, the tasks whose marks are settling in (and since when), the time,
/// and the list's base image on the terminal.
struct Paint<'a> {
    canvas: Canvas,
    /// the top line's and the bottom rows' (`View::full`), and the page's beside the list
    frame: Canvas,
    side: Option<Canvas>,
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
fn wants(
    view: &View,
    plan: pieces::Plan,
    p: &Paint,
    screen: &Screen,
    fonts: &mut Fonts,
    jobs: Option<&mut Vec<worker::Job>>,
) -> Result<(Vec<Want>, pieces::Plan, list::ListImage), String> {
    let (canvas, settling, motion) = (&p.canvas, p.settling, p.motion);
    let bare = |row: &Row| pieces::bare(row, motion, &|id| settling.contains_key(id));
    // a turning mark turns with the text board's, from the board's start
    let began = |row: &Row| match row {
        Row::Task(t) if !sprite::turns(&t.mark) => settling.get(&t.who()).copied().unwrap_or(0),
        _ => 0,
    };
    // (a macro, not a closure: each frame borrows its own plan)
    macro_rules! frame {
        ($plan:expr) => {
            list::Frame { view, plan: $plan, c: canvas, size: p.size, bare: &bare, began: &began, ms: p.ms }
        };
    }
    let now = list::list_image(&frame!(&plan));
    let kept = p.list.filter(|d| screen.has(&Slot::List, d.key) && list::still(d, &now, plan.top..plan.end));
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
    let full = &p.frame;
    let head_n = view.row - view.row0;
    let foot_n = if view.foot.is_some() { view.foot_n } else { 0 };
    let mut wants = Vec::new();
    let k = list::key(&Slot::Head, &f, (&view.head, &view.fleet, head_n, full.grid));
    if head_n > 0 {
        let draw = || match &view.fleet {
            Some(fleet) => fleet::head(fleet, head_n, full, fonts),
            None => frame::head(&view.head, head_n, full, fonts),
        };
        let png = (!screen.has(&Slot::Head, k)).then(|| list::encode(&Slot::Head, full, draw)).transpose()?;
        wants.push(Want { slot: Slot::Head, key: k, png, places: vec![whole(view.col, full, view.row0, head_n)] });
    }
    let mut list = list::wants(&f, image.key, screen, fonts, jobs)?;
    let sprites = list.split_off(list.iter().position(|w| matches!(w.slot, Slot::Sel | Slot::Tail | Slot::Cap | Slot::Sprite(_))).unwrap_or(list.len()));
    wants.extend(list);
    if let (Some(side), Some(c)) = (&view.side, &p.side) {
        // as tall as the page says, from the list's first row
        let rows = page::rows(&side.page, c, fonts, view.rows_n);
        let k = list::key(&Slot::Side, &f, (&side.page, rows, c.grid));
        let png = (!screen.has(&Slot::Side, k)).then(|| list::encode(&Slot::Side, c, || page::draw(&side.page, rows, c, fonts))).transpose()?;
        wants.push(Want { slot: Slot::Side, key: k, png, places: vec![whole(view.col + side.col, c, view.row, rows)] });
    }
    if let Some(foot) = view.foot.as_ref().filter(|_| foot_n > 0) {
        let k = list::key(&Slot::Foot, &f, (foot, foot_n, full.grid));
        let png = (!screen.has(&Slot::Foot, k)).then(|| list::encode(&Slot::Foot, full, || frame::foot(foot, foot_n, full, fonts))).transpose()?;
        wants.push(Want { slot: Slot::Foot, key: k, png, places: vec![whole(view.col, full, view.row + view.rows_n, foot_n)] });
    }
    wants.extend(sprites);
    drop(f);
    Ok((wants, plan, image))
}

/// The base image of `plan`, as the worker's thread draws it: its key, and the job.
fn list_job(view: &View, plan: &pieces::Plan, p: &Paint) -> (u64, worker::Job) {
    let bare = |row: &Row| pieces::bare(row, p.motion, &|id| p.settling.contains_key(id));
    let f = list::Frame { view, plan, c: &p.canvas, size: p.size, bare: &bare, began: &|_| 0, ms: p.ms };
    let k = list::list_image(&f).key;
    (k, list::job(&f, k))
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
            if seen.get(&t.who()).is_some_and(|m| *m != t.mark) && !sprite::turns(&t.mark) {
                settling.insert(t.who(), ms);
            }
            now.insert(t.who(), t.mark.clone());
        }
    }
    *seen = now;
}

/// The placement of an image `c` wide, from the pane's column `col` and row `row`,
/// `rows` cells high: all of it.
fn whole(col: u16, c: &Canvas, row: u16, rows: u16) -> kitty::Place {
    let (crop_w, crop_h) = (c.grid.width(), rows as u32 * c.grid.cell_h);
    kitty::Place { id: 0, p: 1, col, row, crop_x: 0, crop_y: 0, crop_w, crop_h, cols: c.grid.cols as u16, rows, z: 0 }
}

/// What a view's images are drawn on at a cell of `cell` pixels: the list's canvas, the
/// top line's and bottom rows' (as wide as the pane on mission control), and the page's
/// beside the list (no wider than its lines read well).
fn canvases(view: &View, cell: (u32, u32), size: TextSize, theme: Theme) -> (Canvas, Canvas, Option<Canvas>) {
    let canvas = |cols: u16| Canvas::new(Grid { cols: cols as usize, cell_w: cell.0, cell_h: cell.1 }, size, theme);
    let side = view.side.as_ref().map(|s| canvas(s.cols.min(page::MOST_COLS as u16)));
    (canvas(view.cols), canvas(view.full), side)
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

/// The board draws for the image look as its frame laid it out (ui/), in its settings'
/// type; drawing ahead presses j and k on it (ahead.rs).
impl Host for App {
    fn look(&self) -> Look {
        self.look
    }

    fn style(&self) -> (TextSize, Option<PathBuf>, bool) {
        (self.cfg.text_size, self.cfg.font.clone(), self.cfg.motion)
    }

    fn started(&self) -> std::time::Instant {
        self.started
    }

    fn view(&self) -> Option<View> {
        self.view.clone()
    }

    fn placed(&mut self, top: usize, page: usize) {
        (self.top, self.page) = (top, page);
    }

    /// Back to text, saying why on the message line.
    fn fall_back(&mut self, why: String) {
        self.look = Look::Text;
        self.msg = Note::info(why);
    }

    fn drawable(&self) -> bool {
        self.cfg.layout == Layout::Grouped
    }

    fn at_rest(&self) -> bool {
        matches!(self.mode, crate::Mode::Normal) && self.pending.is_none()
    }

    fn moved(&mut self, key: char, area: ratatui::layout::Rect) -> Option<View> {
        ahead::moved(self, key, area)
    }
}

/// Write the board (or mission control) as the image look draws it to a PNG file
/// (`bullpen-board … --render FILE`): its top line, its whole list (with mission
/// control's page beside it) and its key row, `view.full` columns wide, at a cell of
/// `cell` pixels, in `theme`'s colours on its background. For looking at the image look
/// without a terminal.
pub fn render_file(view: &View, size: TextSize, cell: (u32, u32), theme: Theme, font: Option<&Path>, path: &Path) -> Result<(), String> {
    let mut fonts = Fonts::load(font)?;
    let (canvas, full, side) = canvases(view, cell, size, theme);
    let laid = render::lay(&view.rows, &canvas, &fonts);
    // (the list as tall as its rows, not rounded up to cells as on a terminal, where
    // it's cut to the pane: the space a short list leaves isn't the look's)
    let n = view.rows.rows.len();
    let list_h = laid.tall(0, n);
    let head_n = view.row - view.row0;
    let head = match &view.fleet {
        Some(fleet) => fleet::head(fleet, head_n, &full, &mut fonts)?,
        None => frame::head(&view.head, head_n, &full, &mut fonts)?,
    };
    let list = render(&view.rows, &laid, render::Cut::rows(0..n, list_h), &canvas, &mut fonts)?.pixmap;
    let page = match (&view.side, &side) {
        (Some(s), Some(c)) => Some((s.col, page::draw(&s.page, page::rows(&s.page, c, &mut fonts, u16::MAX), c, &mut fonts)?)),
        _ => None,
    };
    let foot = match &view.foot {
        Some(foot) => frame::foot(foot, view.foot_n, &full, &mut fonts)?,
        None => tiny_skia::Pixmap::new(1, 1).ok_or("can't make the image")?,
    };
    // one under the other, on the background, as a terminal shows them; the page beside the list
    let body = list.height().max(page.as_ref().map_or(0, |(_, p)| p.height()));
    let height = head.height() + body + foot.height();
    let mut pm = tiny_skia::Pixmap::new(full.grid.width(), height).ok_or("can't make the image")?;
    pm.fill(tiny_skia::Color::from_rgba8(theme.bg.0, theme.bg.1, theme.bg.2, 255));
    let mut put = |p: &tiny_skia::Pixmap, x: u32, y: u32| {
        pm.draw_pixmap(x as i32, y as i32, p.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
    };
    put(&head, 0, 0);
    put(&list, 0, head.height());
    if let Some((col, p)) = &page {
        put(p, *col as u32 * cell.0, head.height());
    }
    put(&foot, 0, head.height() + body);
    std::fs::write(path, png::encode(&pm)).map_err(|e| format!("can't write {}: {e}", path.display()))
}

#[cfg(test)]
#[path = "tests/test_image.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/test_bench.rs"]
mod bench;
