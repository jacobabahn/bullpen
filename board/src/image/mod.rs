//! The image look (`board_look image`, the board's `I`): the board's task list, its top
//! line and its bottom rows drawn as images and shown with the kitty graphics protocol;
//! what's typed or asked at the bottom, overlays and mission control stay text. The
//! list's rows are the text list's (ui/rows.rs), drawn by render.rs; the top line and
//! the bottom rows are ui's values too, drawn by frame.rs.
//!
//! The board is three images, each pinned to its block of cells: the top line, the
//! rows of the list shown (scrolled by the rows' own heights, which the type size sets),
//! and the bottom rows. Each is drawn at the terminal's cell size and sent only when what
//! it shows changes; the list scrolls only to follow the selection, which changes it
//! anyway. A client with other cell sizes scales them rather than overflowing. A new
//! image is placed before the old one is deleted, so an update doesn't flicker; the
//! images are deleted when the board is text again and when it quits.
//!
//! Before the first image the board asks the terminal whether it draws images, and
//! its colours and cell size (term.rs). It asks for the colours and cell size again
//! only when the person does something that may have changed them: a resize, or
//! switching to the image look (`I`, so `I` twice follows a new theme). There is no
//! timer: each time it asks, the key reader stops for the round trip. When the
//! terminal doesn't draw images, the font can't be loaded or drawing fails, the list
//! is text again and the message line says why.

mod font;
mod frame;
mod kitty;
mod png;
mod render;
pub mod term;
mod theme;

use crate::model::{Layout, Look, TextSize};
use crate::ui::bottom::Foot;
use crate::ui::list::{scroll_by, Head};
use crate::ui::rows::Rows;
use crate::{App, Note};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use font::Fonts;
pub use render::{render, Canvas, Grid, Marks};
pub use theme::Theme;

/// The graphics query's image ID; images shown get IDs from FIRST up.
pub const PROBE: u32 = 0x4250_0000;
const FIRST: u32 = PROBE + 1;

/// The board as the frame laid it out, for the images: its top line, its rows (and which
/// was shown first last frame) and its bottom rows, and the blocks of cells they have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub rows: Rows,
    /// the columns the rows' detail lines were wrapped to (`app.detail_cols`)
    pub detail_cols: Option<usize>,
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

/// The images the board is drawn as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Part {
    Head,
    List,
    Foot,
}

const PARTS: [Part; 3] = [Part::Head, Part::List, Part::Foot];

/// What drawing a frame came to.
enum Drawn {
    /// the images are up: the list's first row, and the tasks a page has
    At { top: usize, page: usize },
    /// nothing drawn: the rows are to be wrapped to these columns first
    Rewrap(usize),
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

/// An image sent, and where it's shown.
struct Shown {
    id: u32,
    /// what it shows: a hash of what it draws, the size, the colours and the cells
    key: u64,
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
    /// the top line's, the list's and the bottom rows' images
    shown: [Option<Shown>; 3],
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
            shown: [None, None, None],
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
    /// move or delete the images as the frame's view says. Returns true when the board
    /// should draw again at once (the list went back to text, or its rows are to be
    /// wrapped again).
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
        let Some(view) = app.view.clone() else {
            self.hide(out);
            return false;
        };
        match self.draw(&view, app.cfg.text_size, app.cfg.font.as_deref(), out) {
            Ok(Drawn::At { top, page }) => {
                (app.top, app.page) = (top, page);
                false
            }
            Ok(Drawn::Rewrap(cols)) => {
                app.detail_cols = Some(cols);
                true
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

    /// Send each part's image if what it shows changed, and place it. The list scrolls
    /// from the row shown first last frame, by its rows' heights, to keep the selection
    /// in view: returns the row it starts at and the tasks a page has (`app.top`,
    /// `app.page`). Rows wrapped to other columns than the image's detail lines hold
    /// aren't drawn: the board wraps them again first.
    fn draw(&mut self, view: &View, size: TextSize, font: Option<&Path>, out: &mut impl Write) -> Result<Drawn, String> {
        let (cell_w, cell_h) =
            self.cell().ok_or("Drawing the list as text: the terminal doesn't say its cell size in pixels.")?;
        let canvas = Canvas::new(Grid { cols: view.cols as usize, cell_w, cell_h }, size, self.theme);
        let fonts = fonts(&mut self.fonts, font)?;
        let cols = render::detail_cols(&canvas, fonts);
        if view.detail_cols != Some(cols) {
            return Ok(Drawn::Rewrap(cols));
        }
        let laid = render::lay(&view.rows, &canvas, fonts);
        let room = view.rows_n as u32 * cell_h;
        let top = scroll_by(&view.rows.rows, view.top, &laid.heights, room);
        let end = render::fitting(&laid, top, room);
        let page = view.rows.rows[top..end].iter().filter(|r| r.is_entry()).count().max(1);
        let marks = Marks { up: top > 0, down: end < view.rows.rows.len() };
        let what = |part: Part| -> u64 {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (part, canvas.grid, size, canvas.theme).hash(&mut h);
            match part {
                Part::Head => (&view.head, view.row - view.row0).hash(&mut h),
                // the rows shown (an age ticking off screen changes nothing), and the ID
                // column, which all of them set
                Part::List => (&view.rows.rows[top..end], laid.idw.to_bits(), view.rows_n, marks).hash(&mut h),
                Part::Foot => (&view.foot, view.foot_n).hash(&mut h),
            }
            h.finish()
        };
        let mut images = Vec::new();
        for part in PARTS {
            let (row, rows) = match part {
                Part::Head => (view.row0, view.row - view.row0),
                Part::List => (view.row, view.rows_n),
                Part::Foot => (view.row + view.rows_n, if view.foot.is_some() { view.foot_n } else { 0 }),
            };
            let key = what(part);
            let k = part as usize;
            let new = if rows > 0 && self.shown[k].as_ref().map(|s| s.key) != Some(key) {
                let drawn = match part {
                    Part::Head => frame::head(&view.head, rows, &canvas, fonts),
                    Part::List => render(&view.rows, &laid, top..end, room, marks, &canvas, fonts).map(|s| s.pixmap),
                    Part::Foot => frame::foot(view.foot.as_ref().expect("a foot"), rows, &canvas, fonts),
                };
                Some(png::encode(&drawn.map_err(|e| format!("Drawing the list as text: {e}."))?))
            } else {
                None
            };
            images.push((part, key, row, rows, new));
        }
        for (part, key, row, rows, png) in images {
            let at = kitty::Place { id: 0, col: view.col, row, crop_y: 0, crop_w: canvas.grid.width(), crop_h: rows as u32 * cell_h, cols: view.cols, rows };
            self.put(part, key, png, at, out)?;
        }
        out.flush().map_err(|e| format!("Drawing the list as text: {e}."))?;
        Ok(Drawn::At { top, page })
    }

    /// Show a part: send its new image (`png`) and place it, then delete the old one; or
    /// move or hide (`at` has no rows) the one there is.
    fn put(&mut self, part: Part, key: u64, png: Option<Vec<u8>>, mut at: kitty::Place, out: &mut impl Write) -> Result<(), String> {
        let k = part as usize;
        let mut old = None;
        if let Some(png) = png {
            let id = self.next_id;
            self.next_id = if self.next_id == u32::MAX { FIRST } else { self.next_id + 1 };
            write(out, &kitty::upload(id, &png))?;
            old = self.shown[k].replace(Shown { id, key, place: None });
        }
        let Some(s) = self.shown[k].as_mut() else { return Ok(()) };
        at.id = s.id;
        if at.rows == 0 {
            if s.place.take().is_some() {
                write(out, &kitty::hide(s.id))?;
            }
        } else if s.place != Some(at) {
            write(out, &kitty::place(&at))?;
            s.place = Some(at);
        }
        // the new image is up: now the old one goes
        if let Some(old) = old {
            write(out, &kitty::delete(old.id))?;
        }
        Ok(())
    }

    /// The terminal was resized, and may have cleared the screen and the images with it:
    /// the next frame sends the image again (the old one is deleted after, if it's there).
    /// Its cell size may have changed with it, and its colours are asked with that.
    pub fn resized(&mut self) {
        self.due = true;
        for s in self.shown.iter_mut().flatten() {
            s.key = 0;
            s.place = None;
        }
    }

    /// Take the images off the screen, keeping them for when the board shows again.
    fn hide(&mut self, out: &mut impl Write) {
        for s in self.shown.iter_mut().flatten() {
            if s.place.take().is_some() {
                let _ = write(out, &kitty::hide(s.id));
                let _ = out.flush();
            }
        }
    }

    /// Delete the images: the board is text again, or it quits.
    pub fn clear(&mut self, out: &mut impl Write) {
        for s in self.shown.iter_mut() {
            if let Some(s) = s.take() {
                let _ = write(out, &kitty::delete(s.id));
                let _ = out.flush();
            }
        }
    }
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

fn write(out: &mut impl Write, s: &str) -> Result<(), String> {
    out.write_all(s.as_bytes()).map_err(|e| format!("Drawing the list as text: {e}."))
}

/// The columns of text a detail line holds in images `cols` wide (render::detail_cols).
pub fn detail_cols(cols: usize, size: TextSize, cell: (u32, u32), font: Option<&Path>) -> Result<usize, String> {
    let canvas = Canvas::new(Grid { cols, cell_w: cell.0, cell_h: cell.1 }, size, Theme::default());
    Ok(render::detail_cols(&canvas, &Fonts::load(font)?))
}

/// Write the board as the image look draws it to a PNG file (`bullpen-board … --render
/// FILE`): its top line, its whole list and its key row, `view.cols` columns wide, at a
/// cell of `cell` pixels, in the default colours. For looking at the image look without
/// a terminal.
pub fn render_file(view: &View, size: TextSize, cell: (u32, u32), font: Option<&Path>, path: &Path) -> Result<(), String> {
    let mut fonts = Fonts::load(font)?;
    let theme = Theme::default();
    let canvas = Canvas::new(Grid { cols: view.cols as usize, cell_w: cell.0, cell_h: cell.1 }, size, theme);
    let laid = render::lay(&view.rows, &canvas, &fonts);
    let list_h = laid.heights.iter().sum::<u32>().div_ceil(cell.1) * cell.1;
    let n = view.rows.rows.len();
    let parts = [
        frame::head(&view.head, view.row - view.row0, &canvas, &mut fonts)?,
        render(&view.rows, &laid, 0..n, list_h, Marks::default(), &canvas, &mut fonts)?.pixmap,
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
