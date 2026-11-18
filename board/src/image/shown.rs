//! The images on the terminal, and where each is placed: a frame says the images it
//! wants (what each shows, as a key, and its placements), and only what changed is
//! written. A new image is sent and placed before the old one is deleted, so an update
//! doesn't flicker; a placement that moved is placed again (a few dozen bytes, the image
//! isn't sent); one no longer wanted is taken off, and an image the frame doesn't want
//! is hidden, its data kept to place again.

use super::kitty;
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
    /// the selected block's cells, and the cell where the last whole row ends
    Sel,
    Tail,
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
}

/// The images sent, by slot, and the next ID to give one.
pub struct Screen {
    shown: BTreeMap<Slot, Shown>,
    next_id: u32,
    first: u32,
}

impl Screen {
    /// Images get IDs from `first` up.
    pub fn new(first: u32) -> Screen {
        Screen { shown: BTreeMap::new(), next_id: first, first }
    }

    /// Does the slot's image show this already (so it needn't be drawn)?
    pub fn has(&self, slot: &Slot, key: u64) -> bool {
        self.shown.get(slot).is_some_and(|s| s.key == key)
    }

    /// Show what a frame wants: each image sent if it's new, its placements placed,
    /// moved or taken off; then the images it doesn't want hidden.
    pub fn show(&mut self, wants: Vec<Want>, out: &mut impl Write) -> std::io::Result<()> {
        let mut wanted = Vec::new();
        for w in wants {
            wanted.push(w.slot.clone());
            self.put(w, out)?;
        }
        for (slot, s) in self.shown.iter_mut() {
            if !wanted.contains(slot) && !s.places.is_empty() {
                s.places.clear();
                out.write_all(kitty::hide(s.id).as_bytes())?;
            }
        }
        out.flush()
    }

    fn put(&mut self, w: Want, out: &mut impl Write) -> std::io::Result<()> {
        let mut old = None;
        if let Some(png) = &w.png {
            let id = self.next_id;
            self.next_id = if self.next_id == u32::MAX { self.first } else { self.next_id + 1 };
            out.write_all(kitty::upload(id, png).as_bytes())?;
            old = self.shown.insert(w.slot.clone(), Shown { id, key: w.key, places: BTreeMap::new() });
        }
        let Some(s) = self.shown.get_mut(&w.slot) else { return Ok(()) };
        for mut at in w.places.iter().copied() {
            at.id = s.id;
            if s.places.get(&at.p) != Some(&at) {
                out.write_all(kitty::place(&at).as_bytes())?;
                s.places.insert(at.p, at);
            }
        }
        let gone: Vec<u32> = s.places.keys().copied().filter(|p| !w.places.iter().any(|at| at.p == *p)).collect();
        for p in gone {
            s.places.remove(&p);
            out.write_all(kitty::unplace(s.id, p).as_bytes())?;
        }
        // the new image is up: now the old one goes
        if let Some(old) = old {
            out.write_all(kitty::delete(old.id).as_bytes())?;
        }
        Ok(())
    }

    /// The terminal may have cleared the screen and the images with it (a resize): the
    /// next frame sends each again (the old one is deleted after, if it's there).
    pub fn forget(&mut self) {
        for s in self.shown.values_mut() {
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

    /// Delete the images: the board is text again, or it quits.
    pub fn clear(&mut self, out: &mut impl Write) {
        for (_, s) in std::mem::take(&mut self.shown) {
            let _ = out.write_all(kitty::delete(s.id).as_bytes());
            let _ = out.flush();
        }
    }
}
