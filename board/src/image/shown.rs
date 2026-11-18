//! The images on the terminal, and where each is placed: a frame says the images it
//! wants (what each shows, as a key, and its placements), and only what changed is
//! written. The frame's new images are all sent before any placement moves, and the
//! old ones taken off after, so an update doesn't flicker or leave cells blank; a
//! placement that moved is placed again (a few dozen bytes, the image isn't sent); one
//! no longer wanted is taken off, and an image the frame doesn't want is hidden, its
//! data kept to place again.
//!
//! An image a frame replaces isn't deleted at once: the last few are kept on the
//! terminal, hidden (at most KEEP of them and KEEP_PX pixels), so going back to what
//! one showed (k after j) only places it again; the oldest is deleted past that. Images
//! drawn ahead (`preload`, what a j or k would want next) are kept the same way, sent
//! but not placed. All of them are deleted when the board is text again or quits.

use super::{kitty, meter};
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
/// changed), and its placements (their `id` is filled in here).
pub struct Want {
    pub slot: Slot,
    pub key: u64,
    pub png: Option<Vec<u8>>,
    pub places: Vec<kitty::Place>,
}

/// An image sent, and where it's shown.
struct Shown {
    id: u32,
    /// what it shows: a hash of what it draws, the size, the colours and the cells
    key: u64,
    /// its placements, by number
    places: BTreeMap<u32, kitty::Place>,
    /// its pixels, as its PNG says
    px: u64,
}

/// The images kept on the terminal, hidden, at most: how many, and their pixels (a
/// 2x board's list is about 8 million, a selected block 1).
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

    /// Send an image, as the next ID.
    fn send(&mut self, png: &[u8], key: u64, out: &mut impl Write, slot: &Slot) -> std::io::Result<Shown> {
        let id = self.next_id;
        self.next_id = if self.next_id == u32::MAX { self.first } else { self.next_id + 1 };
        let up = meter::time(|| "base64".into(), || kitty::upload(id, png));
        put(out, || format!("upload {slot:?}"), &up)?;
        Ok(Shown { id, key, places: BTreeMap::new(), px: pixels(png) })
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
                s.places.clear();
                put(out, || "hide".into(), &kitty::hide(s.id))?;
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
            s.places.clear();
            put(out, || "hide".into(), &kitty::hide(s.id))?;
        }
        self.kept.insert(0, (slot, s));
        for (_, s) in self.kept.extract_if(.., |(_, s)| s.key == 0) {
            put(out, || "delete".into(), &kitty::delete(s.id))?;
        }
        let mut px = 0;
        let fits = self.kept.iter().take_while(|(_, s)| {
            px += s.px;
            px <= KEEP_PX
        });
        let n = fits.count().clamp(1, KEEP).min(self.kept.len());
        for (_, s) in self.kept.split_off(n) {
            put(out, || "delete".into(), &kitty::delete(s.id))?;
        }
        Ok(())
    }

    /// Place a wanted image's placements, move those that moved, take off the rest.
    fn place(&mut self, w: &Want, out: &mut impl Write) -> std::io::Result<()> {
        let Some(s) = self.shown.get_mut(&w.slot) else { return Ok(()) };
        for mut at in w.places.iter().copied() {
            at.id = s.id;
            if s.places.get(&at.p) != Some(&at) {
                put(out, || "place".into(), &kitty::place(&at))?;
                s.places.insert(at.p, at);
            }
        }
        let gone: Vec<u32> = s.places.keys().copied().filter(|p| !w.places.iter().any(|at| at.p == *p)).collect();
        for p in gone {
            s.places.remove(&p);
            put(out, || "unplace".into(), &kitty::unplace(s.id, p))?;
        }
        Ok(())
    }

    /// The ID of the image the slot shows.
    #[cfg(test)]
    pub fn id(&self, slot: &Slot) -> Option<u32> {
        self.shown.get(slot).map(|s| s.id)
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
                s.places.clear();
                let _ = out.write_all(kitty::hide(s.id).as_bytes());
                let _ = out.flush();
            }
        }
    }

    /// Delete the images, the kept ones too: the board is text again, or it quits.
    pub fn clear(&mut self, out: &mut impl Write) {
        let kept = std::mem::take(&mut self.kept).into_iter();
        for (_, s) in std::mem::take(&mut self.shown).into_iter().chain(kept) {
            let _ = out.write_all(kitty::delete(s.id).as_bytes());
        }
        let _ = out.flush();
    }

    /// The IDs of the images kept, hidden.
    #[cfg(test)]
    pub fn kept(&self) -> Vec<u32> {
        self.kept.iter().map(|(_, s)| s.id).collect()
    }
}

/// An image's pixels, from its PNG's header.
fn pixels(png: &[u8]) -> u64 {
    let n = |at: usize| png.get(at..at + 4).map_or(0, |b| u32::from_be_bytes(b.try_into().expect("4 bytes")) as u64);
    n(16) * n(20)
}

/// Write a command, noting its bytes as `part` and the time it took as writing.
fn put(out: &mut impl Write, part: impl FnOnce() -> String, cmd: &str) -> std::io::Result<()> {
    meter::bytes(part, cmd.len());
    meter::time(|| "write".into(), || out.write_all(cmd.as_bytes()))
}

#[cfg(test)]
#[path = "tests/test_shown.rs"]
mod tests;
