//! The drawn tour's images on the terminal: each sent once, under an ID, for a key that
//! says what it shows, then only placed (a crop of it on a block of cells: a few dozen
//! bytes) as often as it's wanted, and taken off when it isn't. Images sent and no
//! longer shown are kept, hidden, to place again (the step you go back to, a page
//! scrolled back), the oldest deleted past `KEEP`; all are deleted when the tour closes.

use bullpen_draw::kitty::{self, Place};
use std::collections::{HashMap, VecDeque};
use std::io::Write;

/// The images kept on the terminal, shown or hidden.
const KEEP: usize = 40;

pub struct Shown {
    next: u32,
    /// what each image shows, by key: its ID
    ids: HashMap<u64, u32>,
    /// the keys, least recently wanted first
    order: VecDeque<u64>,
    /// the placements on screen, by image and placement number
    placed: HashMap<(u32, u32), Place>,
}

impl Shown {
    /// Images get IDs from `first` up.
    pub fn new(first: u32) -> Shown {
        Shown {
            next: first,
            ids: HashMap::new(),
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

    /// Send a PNG for key, unplaced: its ID. The oldest hidden images go past KEEP.
    pub fn send(&mut self, key: u64, png: &[u8], out: &mut impl Write) -> std::io::Result<u32> {
        if let Some(id) = self.id(key) {
            return Ok(id);
        }
        let id = self.next;
        self.next += 1;
        self.put(out, &kitty::upload(id, png))?;
        self.ids.insert(key, id);
        self.order.push_back(key);
        while self.ids.len() > KEEP {
            let shown: Vec<u32> = self.placed.keys().map(|(id, _)| *id).collect();
            let Some(pos) = self.order.iter().position(|k| !shown.contains(&self.ids[k])) else {
                break;
            };
            let old = self.order.remove(pos).expect("a key");
            if let Some(old_id) = self.ids.remove(&old) {
                self.put(out, &kitty::delete(old_id))?;
            }
        }
        Ok(id)
    }

    /// Show these placements: each new or moved one is placed, and any on screen that
    /// isn't one of them is taken off (its image kept).
    pub fn show(&mut self, places: &[Place], out: &mut impl Write) -> std::io::Result<()> {
        for p in places {
            if self.placed.get(&(p.id, p.p)) != Some(p) {
                self.put(out, &kitty::place(p))?;
                self.placed.insert((p.id, p.p), *p);
            }
        }
        let gone: Vec<(u32, u32)> = self
            .placed
            .keys()
            .filter(|k| !places.iter().any(|p| (p.id, p.p) == **k))
            .copied()
            .collect();
        for (id, p) in gone {
            self.put(out, &kitty::unplace(id, p))?;
            self.placed.remove(&(id, p));
        }
        Ok(())
    }

    /// Take every image off the screen (a resize), keeping them.
    pub fn hide_all(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        let ids: std::collections::BTreeSet<u32> = self.placed.keys().map(|(id, _)| *id).collect();
        for id in ids {
            self.put(out, &kitty::hide(id))?;
        }
        self.placed.clear();
        Ok(())
    }

    /// Delete every image sent: the tour is closing, or the images are drawn anew.
    pub fn delete_all(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        let mut ids: Vec<u32> = self.ids.values().copied().collect();
        ids.sort();
        for id in ids {
            self.put(out, &kitty::delete(id))?;
        }
        self.ids.clear();
        self.order.clear();
        self.placed.clear();
        Ok(())
    }

    /// How many images are kept on the terminal.
    #[cfg(test)]
    pub fn kept(&self) -> usize {
        self.ids.len()
    }
}
