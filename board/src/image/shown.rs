//! The images on the terminal, and where each is placed: a frame says the images it
//! wants (what each shows, as a key, and its placements), and only what changed is
//! written. The frame's new images are all sent before any placement moves, and the
//! old ones taken off after, so an update doesn't flicker or leave cells blank; a
//! placement that moved is placed again (a few dozen bytes, the image isn't sent); one
//! no longer wanted is taken off, and an image the frame doesn't want is hidden, its
//! data kept to place again.
//!
//! An image a frame replaces isn't deleted at once: the last few are kept on the
//! terminal, hidden (at most KEEP of them and KEEP_PX pixels, and one of the list's
//! base images, which are big), so going back to what
//! one showed (k after j) only places it again; the oldest is deleted past that. Images
//! drawn ahead (`preload`, what a j or k would want next) are kept the same way, sent
//! but not placed. All of them are deleted when the board is text again or quits.
//!
//! An image over the terminal's limit (bullpen-draw's cut.rs: Rex draws none over
//! 10,000,000 bytes, and the list's window of two pages is several times that on a tall
//! pane) is sent as pieces, each an image of its own with its own ID, and each placement
//! of it is placed as its pieces' (`cut::split`), so it shows as one image would.

use super::{kitty, meter};
use bullpen_draw::cut::{Piece, Pngs};
use super::sprite::Sprite;
use std::collections::BTreeMap;
use std::io::Write;

/// The board's images, in the order a frame sends them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Slot {
    /// the top line
    Head,
    /// the list's rows with nothing selected (pieces.rs), placed as crops
    List,
    /// the bottom rows
    Foot,
    /// the selected block's cells, the cell where the last whole row ends, and the
    /// list's first cell when rows are scrolled off above it
    Sel,
    Tail,
    Cap,
    /// a small image placed over the list: a mark's frames, the scroll marks
    Sprite(Sprite),
}

/// An image a frame wants: what it shows, the image when it's to be sent (what it shows
/// changed: its pieces' PNGs), and its placements of the whole image (their `id` is
/// filled in here).
pub struct Want {
    pub slot: Slot,
    pub key: u64,
    pub png: Option<Pngs>,
    pub places: Vec<kitty::Place>,
}

/// An image sent, and where it's shown.
struct Shown {
    /// its pieces' IDs (the first is the image's), where each is in it, and the cell
    ids: Vec<u32>,
    cut: Vec<Piece>,
    cell: (u32, u32),
    /// what it shows: a hash of what it draws, the size, the colours and the cells
    key: u64,
    /// its placements of the whole image, by number
    places: BTreeMap<u32, kitty::Place>,
    /// its pixels, its pieces'
    px: u64,
}

impl Shown {
    /// A placement of the image as its pieces' placements, with their IDs.
    fn split(&self, at: &kitty::Place) -> Vec<kitty::Place> {
        bullpen_draw::cut::split(at, &self.cut, self.cell).into_iter().map(|(i, p)| kitty::Place { id: self.ids[i], ..p }).collect()
    }

    /// Take the image off the screen, keeping it.
    fn hide(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        self.places.clear();
        for id in &self.ids {
            put(out, || "hide".into(), &kitty::hide(*id))?;
        }
        Ok(())
    }

    /// Delete the image and its data.
    fn delete(&self, out: &mut impl Write) -> std::io::Result<()> {
        for id in &self.ids {
            put(out, || "delete".into(), &kitty::delete(*id))?;
        }
        Ok(())
    }
}

/// The images kept on the terminal, hidden, at most: how many, and their pixels (a
/// selected block at a 2x cell is about 1 million), but for the one list's base image
/// kept (a 2x board's two pages are about 15 million).
const KEEP: usize = 16;
const KEEP_PX: u64 = 24_000_000;

/// The images sent, by slot; those kept, hidden, the latest first; the next ID to give one.
pub struct Screen {
    shown: BTreeMap<Slot, Shown>,
    kept: Vec<(Slot, Shown)>,
    next_id: u32,
    first: u32,
}

impl Screen {
    /// Images get IDs from `first` up.
    pub fn new(first: u32) -> Screen {
        Screen { shown: BTreeMap::new(), kept: Vec::new(), next_id: first, first }
    }

    /// Is the slot's image this one, or one kept (so it needn't be drawn)?
    pub fn has(&self, slot: &Slot, key: u64) -> bool {
        self.shown.get(slot).is_some_and(|s| s.key == key) || self.kept.iter().any(|(k, s)| k == slot && s.key == key)
    }

    /// Send an image, a piece at a time, each as the next ID.
    fn send(&mut self, png: &Pngs, key: u64, out: &mut impl Write, slot: &Slot) -> std::io::Result<Shown> {
        let mut ids = Vec::new();
        for (_, piece) in &png.pieces {
            let id = self.next_id;
            self.next_id = if self.next_id == u32::MAX { self.first } else { self.next_id + 1 };
            let up = meter::time(|| "base64".into(), || kitty::upload(id, piece));
            put(out, || format!("upload {slot:?}"), &up)?;
            ids.push(id);
        }
        let cut: Vec<Piece> = png.pieces.iter().map(|(p, _)| *p).collect();
        let px = cut.iter().map(|p| p.w as u64 * p.h as u64).sum();
        Ok(Shown { ids, cut, cell: png.cell, key, places: BTreeMap::new(), px })
    }

    /// Show what a frame wants, in three steps, so nothing is left blank meanwhile: every
    /// new image sent (which shows nothing yet), then every placement placed, moved or
    /// taken off and the images the frame doesn't want hidden, then the images replaced
    /// taken off and kept. A j or k's new block is up before the list's crops move around
    /// it; one kept (or drawn ahead) is only placed.
    pub fn show(&mut self, wants: Vec<Want>, out: &mut impl Write) -> std::io::Result<()> {
        let mut old = Vec::new();
        for w in &wants {
            if self.shown.get(&w.slot).is_some_and(|s| s.key == w.key) {
                continue;
            }
            let new = match (&w.png, self.kept.iter().position(|(k, s)| *k == w.slot && s.key == w.key)) {
                (_, Some(i)) => self.kept.remove(i).1,
                (Some(png), None) => self.send(png, w.key, out, &w.slot)?,
                (None, None) => continue,
            };
            old.extend(self.shown.insert(w.slot.clone(), new).map(|s| (w.slot.clone(), s)));
        }
        for w in &wants {
            self.place(w, out)?;
        }
        for (slot, s) in self.shown.iter_mut() {
            if !wants.iter().any(|w| w.slot == *slot) && !s.places.is_empty() {
                s.hide(out)?;
            }
        }
        // the new images are up: now the old ones go, kept a while
        for (slot, s) in old {
            self.keep(slot, s, out)?;
        }
        meter::time(|| "write".into(), || out.flush())
    }

    /// Send the images a frame may want next (a j's or a k's), not placed: kept, so that
    /// frame only places them.
    pub fn preload(&mut self, wants: Vec<Want>, out: &mut impl Write) -> std::io::Result<()> {
        // those kept already are the latest now, so sending the others doesn't drop them
        for w in &wants {
            if let Some(i) = self.kept.iter().position(|(k, s)| *k == w.slot && s.key == w.key) {
                let it = self.kept.remove(i);
                self.kept.insert(0, it);
            }
        }
        for w in wants {
            if let Some(png) = w.png.filter(|_| !self.has(&w.slot, w.key)) {
                let s = self.send(&png, w.key, out, &w.slot)?;
                self.keep(w.slot, s, out)?;
            }
        }
        meter::time(|| "write".into(), || out.flush())
    }

    /// Take an image off the screen and keep it, the latest first; delete the oldest
    /// past KEEP images or KEEP_PX pixels. One forgotten (a resize) is deleted, and so are those kept before it.
    fn keep(&mut self, slot: Slot, mut s: Shown, out: &mut impl Write) -> std::io::Result<()> {
        if !s.places.is_empty() {
            s.hide(out)?;
        }
        self.kept.insert(0, (slot, s));
        for (_, s) in self.kept.extract_if(.., |(_, s)| s.key == 0) {
            s.delete(out)?;
        }
        // one list's image kept (the window before, or the next drawn ahead): they're big
        let lists: Vec<usize> = self.kept.iter().enumerate().filter(|(_, (k, _))| *k == Slot::List).map(|(i, _)| i).skip(1).collect();
        for i in lists.into_iter().rev() {
            self.kept.remove(i).1.delete(out)?;
        }
        // the rest by count and pixels
        let mut px = 0;
        let fits = self.kept.iter().take_while(|(k, s)| {
            px += if *k == Slot::List { 0 } else { s.px };
            px <= KEEP_PX
        });
        let n = fits.count().clamp(1, KEEP).min(self.kept.len());
        for (_, s) in self.kept.split_off(n) {
            s.delete(out)?;
        }
        Ok(())
    }

    /// Place a wanted image's placements, move those that moved, take off the rest: each
    /// as its pieces' placements, a piece's taken off when the new ones don't replace it.
    fn place(&mut self, w: &Want, out: &mut impl Write) -> std::io::Result<()> {
        let Some(s) = self.shown.get_mut(&w.slot) else { return Ok(()) };
        let mut now = Vec::new();
        let mut gone = Vec::new();
        for mut at in w.places.iter().copied() {
            at.id = s.ids[0];
            if s.places.get(&at.p) != Some(&at) {
                now.extend(s.split(&at));
                gone.extend(s.places.insert(at.p, at));
            }
        }
        let off: Vec<u32> = s.places.keys().copied().filter(|p| !w.places.iter().any(|at| at.p == *p)).collect();
        gone.extend(off.iter().filter_map(|p| s.places.remove(p)));
        for at in &now {
            put(out, || "place".into(), &kitty::place(at))?;
        }
        for old in gone.iter().flat_map(|at| s.split(at)) {
            if !now.iter().any(|at| (at.id, at.p) == (old.id, old.p)) {
                put(out, || "unplace".into(), &kitty::unplace(old.id, old.p))?;
            }
        }
        Ok(())
    }

    /// The ID of the image the slot shows (its first piece's).
    #[cfg(test)]
    pub fn id(&self, slot: &Slot) -> Option<u32> {
        self.shown.get(slot).map(|s| s.ids[0])
    }

    /// The terminal may have cleared the screen and the images with it (a resize): the
    /// next frame sends each again (the old one is deleted after, if it's there), and
    /// the kept ones are deleted then.
    pub fn forget(&mut self) {
        for s in self.shown.values_mut().chain(self.kept.iter_mut().map(|(_, s)| s)) {
            s.key = 0;
            s.places.clear();
        }
    }

    /// Take the images off the screen, keeping them for when the board shows again.
    pub fn hide(&mut self, out: &mut impl Write) {
        for s in self.shown.values_mut() {
            if !s.places.is_empty() {
                let _ = s.hide(out);
                let _ = out.flush();
            }
        }
    }

    /// Delete the images, the kept ones too: the board is text again, or it quits.
    pub fn clear(&mut self, out: &mut impl Write) {
        let kept = std::mem::take(&mut self.kept).into_iter();
        for (_, s) in std::mem::take(&mut self.shown).into_iter().chain(kept) {
            let _ = s.delete(out);
        }
        let _ = out.flush();
    }

    /// The IDs of the images kept, hidden (their first pieces').
    #[cfg(test)]
    pub fn kept(&self) -> Vec<u32> {
        self.kept.iter().map(|(_, s)| s.ids[0]).collect()
    }
}

/// Write a command, noting its bytes as `part` and the time it took as writing.
fn put(out: &mut impl Write, part: impl FnOnce() -> String, cmd: &str) -> std::io::Result<()> {
    meter::bytes(part, cmd.len());
    meter::time(|| "write".into(), || out.write_all(cmd.as_bytes()))
}

#[cfg(test)]
#[path = "tests/test_shown.rs"]
mod tests;
