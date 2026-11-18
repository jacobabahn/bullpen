//! The image look (`board_look image`, the board's `I`): the board's task list drawn as
//! one image and shown with the kitty graphics protocol; the rest of the board stays
//! text. The list's rows are the text list's (ui/rows.rs), drawn by render.rs.
//!
//! The whole list is drawn as one tall image at the terminal's cell size, sent once
//! each time what it shows changes, and scrolled by moving the placement's crop over
//! it, which costs a few bytes. The placement is pinned to the list's columns and
//! rows, so a client with other cell sizes scales it rather than overflowing. A new
//! image is placed before the old one is deleted, so an update doesn't flicker; the
//! image is deleted when the list is text again and when the board quits.
//!
//! Before the first image the board asks the terminal whether it draws images, and
//! its colours and cell size (term.rs). It asks for the colours and cell size again
//! only when the person does something that may have changed them: a resize, or
//! switching to the image look (`I`, so `I` twice follows a new theme). There is no
//! timer: each time it asks, the key reader stops for the round trip. When the
//! terminal doesn't draw images, the font can't be loaded or drawing fails, the list
//! is text again and the message line says why.

mod font;
mod kitty;
mod png;
mod render;
pub mod term;
mod theme;

use crate::model::{Layout, Look};
use crate::ui::rows::Rows;
use crate::{App, Note};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use font::Fonts;
pub use render::{render, Grid};
pub use theme::Theme;

/// The graphics query's image ID; images shown get IDs from FIRST up.
pub const PROBE: u32 = 0x4250_0000;
const FIRST: u32 = PROBE + 1;
/// The tallest image sent, in pixels: a longer list is sent a window of it at a time.
const MAX_PX: u32 = 16_000;

/// The list as the board draws it this frame, for the image: its rows, which is the
/// first shown, and the block of cells it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub rows: Rows,
    pub top: usize,
    /// the list's first row and column on the terminal, and how many it has
    pub row: u16,
    pub col: u16,
    pub rows_n: u16,
    pub cols: u16,
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

/// The image sent, and where it's shown.
struct Shown {
    id: u32,
    /// what it shows: a hash of the rows, the size, the colours and the window
    key: u64,
    /// the rows it has
    from: usize,
    to: usize,
    place: Option<kitty::Place>,
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
    shown: Option<Shown>,
    next_id: u32,
    /// a cell's size from the terminal's window size (tests have none)
    window: fn() -> Option<(u32, u32)>,
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
            shown: None,
            next_id: FIRST,
            window: window_cell,
        }
    }

    /// Is the list drawn as an image this frame? With the image look, once the terminal
    /// has said it draws images, in the grouped layout (the compact one stays text).
    pub fn drawing(&self, app: &App) -> bool {
        app.look == Look::Image && self.probe == Probe::Yes && app.cfg.layout == Layout::Grouped
    }

    /// After a frame: ask the terminal what the image look needs, then send, place,
    /// move or delete the image as the frame's view says. Returns true when the board
    /// should draw again at once (the list went back to text).
    pub fn show(&mut self, app: &mut App, out: &mut impl Write) -> bool {
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
        let Some(view) = app.view.as_ref() else {
            self.hide(out);
            return false;
        };
        match self.draw(view, app.cfg.font.as_deref(), out) {
            Ok(()) => false,
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

    /// The font for `path` (the board_font setting), loaded once.
    fn fonts(&mut self, path: Option<&Path>) -> Result<&mut Fonts, String> {
        if self.fonts.as_ref().map(|(p, _)| p.as_deref()) != Some(path) {
            let fonts = Fonts::load(path).map_err(|e| format!("Drawing the list as text: {e}."))?;
            self.fonts = Some((path.map(Path::to_path_buf), fonts));
        }
        Ok(&mut self.fonts.as_mut().expect("loaded").1)
    }

    /// A cell's size in pixels: the terminal's window size says it, else its answer.
    fn cell(&self) -> Option<(u32, u32)> {
        (self.window)().or(self.cell).filter(|(w, h)| *w > 0 && *h > 0)
    }

    /// Send the view's image if what it shows changed, and place it.
    fn draw(&mut self, view: &View, font: Option<&Path>, out: &mut impl Write) -> Result<(), String> {
        let (cell_w, cell_h) =
            self.cell().ok_or("Drawing the list as text: the terminal doesn't say its cell size in pixels.")?;
        let grid = Grid { cols: view.cols as usize, cell_w, cell_h };
        let total = view.rows.rows.len();
        let shown = self.shown.as_ref().map(|s| (s.from, s.to));
        let (from, to) = window(total, view.top, view.rows_n as usize, (MAX_PX / cell_h).max(1) as usize, shown);
        let key = content_key(&view.rows, grid, &self.theme, from, to);
        let mut new = None;
        if self.shown.as_ref().map(|s| s.key) != Some(key) {
            let theme = self.theme;
            let fonts = self.fonts(font)?;
            let sheet = render(&view.rows, from..to, grid, &theme, fonts)
                .map_err(|e| format!("Drawing the list as text: {e}."))?;
            let id = self.next_id;
            self.next_id = if self.next_id == u32::MAX { FIRST } else { self.next_id + 1 };
            write(out, &kitty::upload(id, &png::encode(&sheet.pixmap)))?;
            new = Some(Shown { id, key, from, to, place: None });
        }
        let mut old = None;
        if let Some(n) = new {
            old = self.shown.replace(n);
        }
        let s = self.shown.as_mut().expect("an image was sent");
        let visible = (view.rows_n as usize).min(total.saturating_sub(view.top));
        let place = kitty::Place {
            id: s.id,
            col: view.col,
            row: view.row,
            crop_y: (view.top.saturating_sub(s.from)) as u32 * cell_h,
            crop_w: grid.width(),
            crop_h: visible as u32 * cell_h,
            cols: view.cols,
            rows: visible as u16,
        };
        if visible == 0 {
            if s.place.take().is_some() {
                write(out, &kitty::hide(s.id))?;
            }
        } else if s.place != Some(place) {
            write(out, &kitty::place(&place))?;
            s.place = Some(place);
        }
        // the new image is up: now the old one goes
        if let Some(old) = old {
            write(out, &kitty::delete(old.id))?;
        }
        out.flush().map_err(|e| format!("Drawing the list as text: {e}."))
    }

    /// The terminal was resized, and may have cleared the screen and the images with it:
    /// the next frame sends the image again (the old one is deleted after, if it's there).
    /// Its cell size may have changed with it, and its colours are asked with that.
    pub fn resized(&mut self) {
        self.due = true;
        if let Some(s) = self.shown.as_mut() {
            s.key = 0;
            s.place = None;
        }
    }

    /// Take the image off the screen, keeping it for when the list shows again.
    fn hide(&mut self, out: &mut impl Write) {
        if let Some(s) = self.shown.as_mut() {
            if s.place.take().is_some() {
                let _ = write(out, &kitty::hide(s.id));
                let _ = out.flush();
            }
        }
    }

    /// Delete the image: the list is text again, or the board quits.
    pub fn clear(&mut self, out: &mut impl Write) {
        if let Some(s) = self.shown.take() {
            let _ = write(out, &kitty::delete(s.id));
            let _ = out.flush();
        }
    }
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

fn write(out: &mut impl Write, s: &str) -> Result<(), String> {
    out.write_all(s.as_bytes()).map_err(|e| format!("Drawing the list as text: {e}."))
}

/// The rows the image has, [from, to): all of them, or for a list taller than `max`
/// rows, a window round the rows shown (the one sent before while it still holds them).
fn window(total: usize, top: usize, avail: usize, max: usize, shown: Option<(usize, usize)>) -> (usize, usize) {
    if total <= max {
        return (0, total);
    }
    let end = (top + avail).min(total);
    if let Some((from, to)) = shown {
        if from <= top && end <= to && to <= total && to - from == max {
            return (from, to);
        }
    }
    let from = top.saturating_sub(max.saturating_sub(avail) / 2).min(total - max);
    (from, from + max)
}

fn content_key(rows: &Rows, grid: Grid, theme: &Theme, from: usize, to: usize) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (rows, grid, theme, from, to).hash(&mut h);
    h.finish()
}

/// Write the board's list as a PNG file (`bullpen-board … --render FILE`): `cols`
/// columns wide, at a cell of `cell` pixels, in the default colours. For looking at
/// the image look without a terminal.
pub fn render_file(rows: &Rows, cols: usize, cell: (u32, u32), font: Option<&Path>, path: &Path) -> Result<(), String> {
    let mut fonts = Fonts::load(font)?;
    let grid = Grid { cols, cell_w: cell.0, cell_h: cell.1 };
    let theme = Theme::default();
    let sheet = render(rows, 0..rows.rows.len(), grid, &theme, &mut fonts)?;
    // on the background, as a terminal shows it
    let mut pm = tiny_skia::Pixmap::new(sheet.pixmap.width(), sheet.pixmap.height()).ok_or("can't make the image")?;
    pm.fill(tiny_skia::Color::from_rgba8(theme.bg.0, theme.bg.1, theme.bg.2, 255));
    pm.draw_pixmap(0, 0, sheet.pixmap.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
    std::fs::write(path, png::encode(&pm)).map_err(|e| format!("can't write {}: {e}", path.display()))
}

#[cfg(test)]
#[path = "tests/test_image.rs"]
mod tests;
