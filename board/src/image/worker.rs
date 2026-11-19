//! Drawing the list's base image on a thread of its own, for drawing ahead (ahead.rs).
//! A window of the list is the board's biggest image (a few hundred milliseconds to
//! draw and make a PNG at a 2x cell), and a key that comes meanwhile mustn't wait for
//! it. The thread has its own fonts; what it draws comes back to the board's loop as a
//! message (`Msg::Ready`), and is sent then, unplaced. A key that wants the image
//! before it's back draws it itself, and what comes back after is dropped.

use super::font::Fonts;
use super::render::{render, Canvas, Cut, Laid, Marks};
use crate::app::Msg;
use crate::ui::rows::Rows;
use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};

/// A base image to draw: the rows and their layout, the part of them it shows (render's
/// `Cut`), which rows' marks are left out, the canvas and the font.
pub struct Job {
    pub key: u64,
    pub rows: Rows,
    pub laid: Laid,
    pub range: Range<usize>,
    pub at: u32,
    pub y: u32,
    pub h: u32,
    pub bare: Vec<bool>,
    pub canvas: Canvas,
    pub font: Option<PathBuf>,
}

/// A base image drawn, as a PNG, or why it couldn't be.
pub struct Ready {
    pub key: u64,
    pub png: Result<Vec<u8>, String>,
}

/// The thread, and the images it's drawing.
pub struct Worker {
    jobs: Sender<Job>,
    pending: HashSet<u64>,
}

impl Worker {
    /// Start the thread; it answers on `board`.
    pub fn start(board: Sender<Msg>) -> Worker {
        let (jobs, rx) = channel::<Job>();
        std::thread::spawn(move || {
            let mut fonts: Option<(Option<PathBuf>, Fonts)> = None;
            for job in rx {
                let png = draw(&job, &mut fonts);
                if board.send(Msg::Ready(Ready { key: job.key, png })).is_err() {
                    return;
                }
            }
        });
        Worker { jobs, pending: HashSet::new() }
    }

    /// Draw this image, unless it's being drawn.
    pub fn draw(&mut self, job: Job) {
        if self.pending.insert(job.key) {
            let _ = self.jobs.send(job);
        }
    }

    /// An image came back: was it still wanted?
    pub fn done(&mut self, key: u64) -> bool {
        self.pending.remove(&key)
    }

    /// Is an image being drawn?
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Whatever comes back now is dropped (the images were deleted, or the cell changed).
    pub fn drop_all(&mut self) {
        self.pending.clear();
    }
}

/// Draw a job's image as a PNG, with the font it names (loaded once).
fn draw(job: &Job, fonts: &mut Option<(Option<PathBuf>, Fonts)>) -> Result<Vec<u8>, String> {
    let fonts = super::fonts(fonts, job.font.as_deref())?;
    let bare = |i: usize| job.bare.get(i).copied().unwrap_or(false);
    let cut = Cut { range: job.range.clone(), at: job.at, y: job.y, h: job.h, marks: Marks::default(), bare: &bare };
    let sheet = render(&job.rows, &job.laid, cut, &job.canvas, fonts)?;
    Ok(super::png::encode(&sheet.pixmap))
}
