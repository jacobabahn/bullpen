//! The drawn tour's images on the terminal: each sent once, under an ID, for a key that
//! says what it shows, then only placed (a crop of it on a block of cells: a few dozen
//! bytes) as often as it's wanted, and taken off when it isn't. Images sent and no
//! longer shown are kept, hidden, to place again (the step you go back to, a page
//! scrolled back), the oldest deleted past `KEEP`; all are deleted when the tour closes.
//!
//! An image over the terminal's limit (bullpen-draw's cut.rs: a rail on a tall pane) is
//! sent as pieces, each an image of its own, and a placement of it is placed as theirs;
//! the tour knows the image by its first piece's ID.

use bullpen_draw::cut::{self, Piece};
use bullpen_draw::kitty::{self, Place};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use tiny_skia::Pixmap;

/// The images kept on the terminal, shown or hidden.
const KEEP: usize = 40;

pub struct Shown {
    next: u32,
    /// a cell's size in pixels, which pieces are cut by
    cell: (u32, u32),
    /// what each image shows, by key: its ID
    ids: HashMap<u64, u32>,
    /// each image's pieces, by its ID: their own IDs, and where each is in it
    pieces: HashMap<u32, Vec<(u32, Piece)>>,
    /// the keys, least recently wanted first
    order: VecDeque<u64>,
    /// the placements on screen, by image and placement number (as wanted, of the whole
    /// image)
    placed: HashMap<(u32, u32), Place>,
}

impl Shown {
    /// Images get IDs from `first` up; they're cut for a cell `cell` pixels.
    pub fn new(first: u32, cell: (u32, u32)) -> Shown {
        Shown {
            next: first,
            cell,
            ids: HashMap::new(),
            pieces: HashMap::new(),
            order: VecDeque::new(),
            placed: HashMap::new(),
        }
    }

    fn put(&mut self, out: &mut impl Write, bytes: &str) -> std::io::Result<()> {
        out.write_all(bytes.as_bytes())
    }

    /// The image for key, when it's been sent: its ID (and it's wanted now).
    pub fn id(&mut self, key: u64) -> Option<u32> {
        let id = *self.ids.get(&key)?;
        self.order.retain(|k| *k != key);
        self.order.push_back(key);
        Some(id)
    }

    /// Send an image for key, unplaced, as PNG (in pieces, over the terminal's limit):
    /// its ID. The oldest hidden images go past KEEP.
    pub fn send(&mut self, key: u64, pm: &Pixmap, out: &mut impl Write) -> std::io::Result<u32> {
        if let Some(id) = self.id(key) {
            return Ok(id);
        }
        let pngs = cut::encode(pm, self.cell).map_err(std::io::Error::other)?.pieces;
        let id = self.next;
        let mut pieces = Vec::new();
        for (piece, png) in pngs {
            self.put(out, &kitty::upload(self.next, &png))?;
            pieces.push((self.next, piece));
            self.next += 1;
        }
        self.pieces.insert(id, pieces);
        self.ids.insert(key, id);
        self.order.push_back(key);
        while self.ids.len() > KEEP {
            let shown: Vec<u32> = self.placed.keys().map(|(id, _)| *id).collect();
            let Some(pos) = self.order.iter().position(|k| !shown.contains(&self.ids[k])) else {
                break;
            };
            let old = self.order.remove(pos).expect("a key");
            if let Some(old_id) = self.ids.remove(&old) {
                for (id, _) in self.pieces.remove(&old_id).unwrap_or_default() {
                    self.put(out, &kitty::delete(id))?;
                }
            }
        }
        Ok(id)
    }

    /// A placement of an image (its ID is the image's) as its pieces' placements.
    fn split(&self, p: &Place) -> Vec<Place> {
        let Some(pieces) = self.pieces.get(&p.id) else {
            return vec![*p];
        };
        let cut: Vec<Piece> = pieces.iter().map(|(_, piece)| *piece).collect();
        cut::split(p, &cut, self.cell)
            .into_iter()
            .map(|(i, at)| Place { id: pieces[i].0, ..at })
            .collect()
    }

    /// Show these placements: each new or moved one is placed, and any on screen that
    /// isn't one of them is taken off (its image kept).
    pub fn show(&mut self, places: &[Place], out: &mut impl Write) -> std::io::Result<()> {
        // those taken off or moved
        let mut gone: Vec<Place> = self.placed.values().filter(|p| !places.contains(p)).copied().collect();
        gone.sort_by_key(|p| (p.id, p.p));
        let now: Vec<Place> = places
            .iter()
            .filter(|p| self.placed.get(&(p.id, p.p)) != Some(p))
            .flat_map(|p| self.split(p))
            .collect();
        for p in &now {
            self.put(out, &kitty::place(p))?;
        }
        // a piece's placement the new ones didn't replace is taken off
        for old in gone {
            self.placed.remove(&(old.id, old.p));
            for at in self.split(&old) {
                if !now.iter().any(|p| (p.id, p.p) == (at.id, at.p)) {
                    self.put(out, &kitty::unplace(at.id, at.p))?;
                }
            }
        }
        for p in places {
            self.placed.insert((p.id, p.p), *p);
        }
        Ok(())
    }

    /// Take every image off the screen (a resize), keeping them.
    pub fn hide_all(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        let ids: std::collections::BTreeSet<u32> = self.placed.keys().map(|(id, _)| *id).collect();
        for id in ids {
            for (piece, _) in self.pieces.get(&id).cloned().unwrap_or_default() {
                self.put(out, &kitty::hide(piece))?;
            }
        }
        self.placed.clear();
        Ok(())
    }

    /// Delete every image sent: the tour is closing, or the images are drawn anew.
    pub fn delete_all(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        let mut ids: Vec<u32> = self.ids.values().copied().collect();
        ids.sort();
        for id in ids {
            for (piece, _) in self.pieces.remove(&id).unwrap_or_default() {
                self.put(out, &kitty::delete(piece))?;
            }
        }
        self.ids.clear();
        self.order.clear();
        self.placed.clear();
        Ok(())
    }

    /// The terminal's cell is now `cell` pixels: every image sent is deleted (each was
    /// drawn for the old one), and what's sent from now on is cut for the new one.
    pub fn recut(&mut self, cell: (u32, u32), out: &mut impl Write) -> std::io::Result<()> {
        self.delete_all(out)?;
        self.cell = cell;
        Ok(())
    }

    /// How many images (pieces) are kept on the terminal.
    #[cfg(test)]
    pub fn kept(&self) -> usize {
        self.pieces.values().map(Vec::len).sum()
    }
}
