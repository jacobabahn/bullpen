//! A terminal's image store, for the drawn tour's tests: the kitty commands the tour
//! writes, applied as Rex's terminal (libghostty's) applies them. Images are kept by ID
//! with their bytes decoded (width × height × 4) up to a quota; an upload past it deletes
//! the oldest images not on screen first, without a word. A placement of an image that
//! isn't there is a blank on screen, noted in `blanks`.

use std::collections::HashSet;

/// Rex's quota a pane (`bin/kitty-quota-check` found it).
pub const REX_QUOTA: usize = bullpen_draw::cut::QUOTA;

pub struct Terminal {
    quota: usize,
    /// the images kept, oldest first: ID and bytes
    images: Vec<(u32, usize)>,
    /// placements on screen: image and placement number
    placed: HashSet<(u32, u32)>,
    /// the images placed that weren't there
    pub blanks: Vec<u32>,
    /// the most bytes kept at once
    pub most: usize,
}

/// A command's control keys, as (key, value).
fn keys(control: &str) -> Vec<(&str, &str)> {
    control.split(',').filter_map(|kv| kv.split_once('=')).collect()
}

fn get<'a>(keys: &[(&'a str, &'a str)], k: &str) -> Option<&'a str> {
    keys.iter().find(|(key, _)| *key == k).map(|(_, v)| *v)
}

impl Terminal {
    pub fn new(quota: usize) -> Terminal {
        Terminal {
            quota,
            images: Vec::new(),
            placed: HashSet::new(),
            blanks: Vec::new(),
            most: 0,
        }
    }

    /// The bytes of the images kept.
    pub fn bytes(&self) -> usize {
        self.images.iter().map(|(_, b)| b).sum()
    }

    /// Apply what the tour wrote.
    pub fn feed(&mut self, out: &str) {
        for cmd in out.split("\x1b_G").skip(1) {
            let cmd = cmd.split("\x1b\\").next().unwrap_or("");
            let (control, _) = cmd.split_once(';').unwrap_or((cmd, ""));
            let ks = keys(control);
            let id = || get(&ks, "i").and_then(|v| v.parse::<u32>().ok());
            match get(&ks, "a") {
                Some("t") => {
                    let (w, h) = bullpen_draw::cut::uploaded(&format!("\x1b_G{cmd}"))[0];
                    self.upload(id().expect("an ID"), w as usize * h as usize * 4);
                }
                Some("p") => {
                    let (id, p) = (
                        id().expect("an ID"),
                        get(&ks, "p").and_then(|v| v.parse().ok()).unwrap_or(0),
                    );
                    if self.images.iter().any(|(i, _)| *i == id) {
                        self.placed.insert((id, p));
                    } else {
                        self.blanks.push(id);
                    }
                }
                Some("d") => {
                    let id = id().expect("an ID");
                    match (get(&ks, "d"), get(&ks, "p").and_then(|v| v.parse::<u32>().ok())) {
                        (Some("I"), _) => {
                            self.images.retain(|(i, _)| *i != id);
                            self.placed.retain(|(i, _)| *i != id);
                        }
                        (_, Some(p)) => {
                            self.placed.remove(&(id, p));
                        }
                        _ => self.placed.retain(|(i, _)| *i != id),
                    }
                }
                _ => {}
            }
        }
    }

    fn upload(&mut self, id: u32, bytes: usize) {
        self.images.retain(|(i, _)| *i != id);
        // the oldest not on screen go first, then the oldest of the rest
        while self.bytes() + bytes > self.quota && !self.images.is_empty() {
            let shown = |i: u32| self.placed.iter().any(|(p, _)| *p == i);
            let at = self.images.iter().position(|(i, _)| !shown(*i)).unwrap_or(0);
            let (gone, _) = self.images.remove(at);
            self.placed.retain(|(i, _)| *i != gone);
        }
        self.images.push((id, bytes));
        self.most = self.most.max(self.bytes());
    }
}
