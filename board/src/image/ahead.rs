//! Drawing ahead: while the board waits for a key, the images a j or a k would want
//! next (the selected block a task down or up, the cell under it, the key row for that
//! task) are drawn and sent but not placed, so that key only places them: a few hundred
//! bytes, and nothing to draw. shown.rs keeps them with the images a frame replaced
//! (bounded, and deleted with the rest).
//!
//! The list's base image, when a key would scroll it out of its window, is drawn on a
//! thread of its own (worker.rs), so a key that comes meanwhile doesn't wait for it.
//!
//! What a key would show is worked out by pressing it on the board, drawing the frame
//! off screen, and putting back what the key changed (the selection and the message).
//! Only on the board itself, with nothing asked: there j and k only move the selection.

use super::{fonts, wants, Canvas, Grid, Paint, Picture, View};
use crate::{App, Mode};
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::io::Write;

/// In the keys still to draw ahead for: the list's next window (`far`).
const FAR: char = '\0';

impl Picture {
    /// After a frame: what's left to draw ahead is j's and k's, for this view.
    pub(super) fn plan_ahead(&mut self, view: Option<&View>) {
        if self.ahead_of.as_ref() != view {
            self.ahead_of = view.cloned();
            // j's, k's, then the list's next window
            self.todo = vec![FAR, 'k', 'j'];
        }
    }

    /// While the board waits for a key: draw what one key would want next (j's first)
    /// and send it, unplaced. True while there's more to draw; the board looks for a key
    /// between each. `area` is the board's, in cells.
    pub fn ahead(&mut self, app: &mut App, area: Rect, out: &mut impl Write) -> bool {
        let Some(key) = self.todo.pop() else { return false };
        if !self.drawing(app) || self.ahead_of.is_none() || !matches!(app.mode, Mode::Normal) || app.pending.is_some() {
            // back on the board, it draws ahead again, even for the same view
            self.todo.clear();
            self.ahead_of = None;
            return false;
        }
        if key == FAR {
            let _ = self.far(app);
        } else if let Some(view) = super::meter::time(|| "view".into(), || moved(app, key, area)) {
            // a key that comes anyway draws what this couldn't
            let _ = self.preload(&view, app, out);
        }
        !self.todo.is_empty()
    }

    /// Start drawing the list's next window on the worker's thread when the page comes
    /// within a quarter of a page of its window's edge, so a key a few on that scrolls
    /// out of it only places it. Not for a window that's the page alone (the list just
    /// changed there): a j or k's own drawing ahead does that.
    fn far(&mut self, app: &App) -> Result<(), String> {
        let (Some(view), Some(drawn)) = (self.ahead_of.clone(), self.list.clone()) else { return Ok(()) };
        if self.worker.is_none() {
            return Ok(());
        }
        let cell = self.cell().ok_or("no cell size")?;
        let canvas = Canvas::new(Grid { cols: view.cols as usize, cell_w: cell.0, cell_h: cell.1 }, app.cfg.text_size, self.theme);
        let fonts = fonts(&mut self.fonts, app.cfg.font.as_deref())?;
        let plan = super::pieces::plan(&view, &canvas, fonts, Some(drawn.win));
        let room = view.rows_n as u32 * cell.1;
        let (wy, wh) = drawn.win;
        if plan.whole || wh <= room {
            return Ok(());
        }
        let total = plan.laid.tall(0, plan.laid.heights.len());
        let y0 = plan.laid.tall(0, plan.top);
        let paint = Paint { canvas, size: app.cfg.text_size, motion: app.cfg.motion, settling: &self.settling, ms: self.ms, list: None };
        for y in [y0 + room / 4, y0.saturating_sub(room / 4)] {
            let y = y.min(total.saturating_sub(room));
            if wy <= y && y + room <= wy + wh {
                continue;
            }
            let (key, mut job) = super::list_job(&view, &plan.with_window(super::pieces::grid(y, room)), &paint);
            if !self.screen.has(&super::Slot::List, key) {
                job.font = app.cfg.font.clone();
                self.worker.as_mut().expect("a worker").draw(job);
            }
        }
        Ok(())
    }

    /// Send what a frame of `view` wants that the terminal doesn't have, unplaced.
    fn preload(&mut self, view: &View, app: &App, out: &mut impl Write) -> Result<(), String> {
        let cell = self.cell().ok_or("no cell size")?;
        let canvas = Canvas::new(Grid { cols: view.cols as usize, cell_w: cell.0, cell_h: cell.1 }, app.cfg.text_size, self.theme);
        let fonts = fonts(&mut self.fonts, app.cfg.font.as_deref())?;
        let prev = self.list.as_ref().map(|l| l.win);
        let plan = super::meter::time(|| "plan".into(), || super::pieces::plan(view, &canvas, fonts, prev));
        let list = self.list.as_ref();
        let paint = Paint { canvas, size: app.cfg.text_size, motion: app.cfg.motion, settling: &self.settling, ms: self.ms, list };
        // the base image on the worker's thread, when there is one
        let mut jobs = Vec::new();
        let (wants, ..) = wants(view, plan, &paint, &self.screen, fonts, self.worker.is_some().then_some(&mut jobs))?;
        for mut job in jobs {
            job.font = app.cfg.font.clone();
            self.worker.as_mut().expect("a worker").draw(job);
        }
        self.screen.preload(wants, out).map_err(|e| e.to_string())
    }
}

/// The board's view after `key`, drawn off screen, with what the key changed put back;
/// None when the key wouldn't move the selection.
fn moved(app: &mut App, key: char, area: Rect) -> Option<View> {
    let saved = (app.sel.clone(), app.msg.clone(), app.view.take(), app.height);
    app.key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
    let view = if app.sel != saved.0 {
        let viewport = Viewport::Fixed(area);
        let term = Terminal::with_options(TestBackend::new(area.width, area.height), TerminalOptions { viewport });
        term.ok().and_then(|mut t| t.draw(|f| crate::ui::draw(f, app)).ok().map(|_| ())).and_then(|_| app.view.take())
    } else {
        None
    };
    (app.sel, app.msg, app.view, app.height) = saved;
    view
}

#[cfg(test)]
#[path = "tests/test_ahead.rs"]
mod tests;
