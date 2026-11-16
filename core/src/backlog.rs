//! The backlog: work that isn't a task yet (docs/backlog.md). One list per repo,
//! `backlog.json` in its state folder, in list order (first is next), its items `L1`,
//! `L2`, ... Read and written under its lock (`backlog.lock`), so two commands adding at
//! once both land. Shared by the command line (`bullpen later`) and the board.

use crate::fail::{fail, Res};
use crate::json::{self, s, Obj};
use crate::repo::Repo;
use crate::timeutil::iso_now;
use serde_json::{json, Value};
use std::path::PathBuf;

/// An item's states: `asks` waits for a yes (a worker's idea), `kept` is on the list,
/// `started` became a task (its `task`), `dropped` was said no to, `done` went away by
/// itself.
pub const STATES: [&str; 5] = ["asks", "kept", "started", "dropped", "done"];

/// The states of an item still on the list: it may be moved, kept, dropped or started.
pub fn is_open(item: &Obj) -> bool {
    matches!(s(item, "state"), "asks" | "kept")
}

/// The list: the next item's number, and the items in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Backlog {
    pub next: i64,
    pub items: Vec<Obj>,
}

/// Where an item moves to, among the open ones.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Move {
    Top,
    Up,
    Down,
    Bottom,
}

pub fn path(repo: &Repo) -> PathBuf {
    repo.dir.join("backlog.json")
}

fn lock_path(repo: &Repo) -> PathBuf {
    repo.dir.join("backlog.lock")
}

/// The repo's backlog as it is now (empty when it has none).
pub fn load(repo: &Repo) -> Res<Backlog> {
    read(&path(repo))
}

/// The backlog in this file (empty when there's none): the board reads its repo's
/// `backlog.json` by path.
pub fn read(file: &std::path::Path) -> Res<Backlog> {
    let Some(o) = json::read_obj(file)? else {
        return Ok(Backlog {
            next: 1,
            items: Vec::new(),
        });
    };
    let items = o.get("items").and_then(Value::as_array).into_iter().flatten();
    Ok(Backlog {
        next: o.get("next").and_then(Value::as_i64).unwrap_or(1),
        items: items.filter_map(Value::as_object).cloned().collect(),
    })
}

/// Load the backlog under its lock, change it with f and write it back; f's answer.
/// Nothing is written when f fails.
pub fn change<T>(repo: &Repo, f: impl FnOnce(&mut Backlog) -> Res<T>) -> Res<T> {
    let _held = crate::lock::lock(&lock_path(repo))?;
    let mut b = load(repo)?;
    let out = f(&mut b)?;
    let mut o = Obj::new();
    o.insert("next".into(), json!(b.next));
    o.insert(
        "items".into(),
        Value::Array(b.items.into_iter().map(Value::Object).collect()),
    );
    json::write_obj(&path(repo), &o)?;
    Ok(out)
}

/// An item ID as it's kept: "l7" and "7" are "L7". None for what isn't one.
pub fn norm_id(word: &str) -> Option<String> {
    let digits = word.strip_prefix(['L', 'l']).unwrap_or(word);
    let ok = !digits.is_empty() && digits.len() < 10 && digits.bytes().all(|c| c.is_ascii_digit());
    let n: u64 = digits.parse().ok().filter(|_| ok)?;
    Some(format!("L{n}"))
}

/// Whether word names an item (`L7`, `l7`), not a title: a bare number doesn't.
pub fn is_item_id(word: &str) -> bool {
    word.starts_with(['L', 'l']) && norm_id(word).is_some()
}

impl Backlog {
    /// Add an item: at the bottom, or at the top of the list. Its ID.
    pub fn add(&mut self, title: &str, by: &str, state: &str, top: bool) -> String {
        let id = format!("L{}", self.next);
        self.next += 1;
        let mut item = Obj::new();
        item.insert("id".into(), json!(id));
        item.insert("title".into(), json!(title));
        item.insert("added".into(), json!(iso_now()));
        item.insert("by".into(), json!(by));
        item.insert("state".into(), json!(state));
        for k in ["brief", "note", "task"] {
            item.insert(k.into(), Value::Null);
        }
        let at = if top { self.first_open() } else { self.items.len() };
        self.items.insert(at, item);
        id
    }

    fn first_open(&self) -> usize {
        self.items.iter().position(is_open).unwrap_or(self.items.len())
    }

    fn index(&self, id: &str) -> Res<usize> {
        match self.items.iter().position(|i| s(i, "id") == id) {
            Some(i) => Ok(i),
            None => fail(format!("no item {id} in the backlog: `bullpen later` lists them")),
        }
    }

    pub fn item(&self, id: &str) -> Res<&Obj> {
        Ok(&self.items[self.index(id)?])
    }

    pub fn item_mut(&mut self, id: &str) -> Res<&mut Obj> {
        let i = self.index(id)?;
        Ok(&mut self.items[i])
    }

    /// The items still on the list (asks and kept), in order.
    pub fn open(&self) -> impl Iterator<Item = &Obj> {
        self.items.iter().filter(|i| is_open(i))
    }

    /// Move an open item among the open ones (started and dropped items keep their
    /// places). Whether it moved: not when it's there already.
    pub fn move_item(&mut self, id: &str, to: Move) -> Res<bool> {
        let at = self.index(id)?;
        if !is_open(&self.items[at]) {
            return fail(format!(
                "{id} isn't on the list ({}): only an open item moves",
                s(&self.items[at], "state")
            ));
        }
        let open: Vec<usize> = (0..self.items.len()).filter(|i| is_open(&self.items[*i])).collect();
        let k = open.iter().position(|i| *i == at).unwrap_or(0);
        let target = match to {
            Move::Top => 0,
            Move::Up => k.saturating_sub(1),
            Move::Down => (k + 1).min(open.len() - 1),
            Move::Bottom => open.len() - 1,
        };
        if target == k {
            return Ok(false);
        }
        let item = self.items.remove(at);
        // open[target] either way: moving down, removing the item shifted the rest up one
        self.items.insert(open[target], item);
        Ok(true)
    }
}

#[cfg(test)]
#[path = "test_backlog.rs"]
mod tests;
