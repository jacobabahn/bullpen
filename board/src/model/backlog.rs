//! The backlog as the board shows it (docs/backlog.md): the items still on the list,
//! in order, as one line under Queued (Later) or, expanded with `L`, a row each, and
//! the keys and words each item gets (its mark and line). The list itself is
//! bullpen-core's: `load_later` reads it with `backlog::read`.

use bullpen_core::backlog;
use bullpen_core::json::{s, truthy, Obj};
use std::path::Path;

/// An item on the list: one that asks for a yes, or one that's kept.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub id: String,
    pub title: String,
    /// "asks" or "kept"
    pub state: String,
    /// who added it: "person", a coordinator's ID, a task's
    pub by: String,
    /// it has a brief: `s` starts it without the coordinator
    pub brief: bool,
    /// why the coordinator passed over it this time
    pub skipped: String,
}

impl Item {
    pub fn from_obj(o: &Obj) -> Item {
        Item {
            id: s(o, "id").into(),
            title: s(o, "title").into(),
            state: s(o, "state").into(),
            by: s(o, "by").into(),
            brief: truthy(o.get("brief")),
            skipped: s(o, "skipped").into(),
        }
    }

    /// Does it wait on the person: a worker's idea that asks for a yes?
    pub fn asks(&self) -> bool {
        self.state == "asks"
    }
}

/// Is this ID an item's (`L7`), not a task's?
pub fn is_item(id: &str) -> bool {
    backlog::is_item_id(id)
}

/// The repo's open items, in list order. A file caught half-written reads as empty
/// until next time.
pub fn load_later(dir: &Path) -> Vec<Item> {
    let b = backlog::read(&dir.join("backlog.json")).unwrap_or(backlog::Backlog { next: 1, items: Vec::new() });
    b.open().map(Item::from_obj).collect()
}

/// The Later line, collapsed: "Later · 6 · next: Tidy the watcher", or what waits on
/// you there, "Later · 6 · 2 to decide (L8, L9)". The bool: something does (amber).
pub fn later_line(items: &[Item]) -> (String, bool) {
    let asks: Vec<&str> = items.iter().filter(|i| i.asks()).map(|i| i.id.as_str()).collect();
    let n = items.len();
    if !asks.is_empty() {
        return (format!("Later · {n} · {} to decide ({})", asks.len(), asks.join(", ")), true);
    }
    match items.first() {
        Some(first) => (format!("Later · {n} · next: {}", first.title), false),
        None => (String::new(), false),
    }
}

/// The mark an item gets: `?` for one that asks (amber), `◇` for a kept one (dim).
pub fn item_mark(i: &Item) -> &'static str {
    if i.asks() { "?" } else { "◇" }
}

/// The line under the selected item: what's to know, and what its keys do.
pub fn item_detail(i: &Item) -> String {
    let mut out = Vec::new();
    if i.asks() {
        let from = if i.by.is_empty() { String::new() } else { format!(" (from {})", i.by) };
        out.push(format!("Asks for a yes{from}: y keeps it, n drops it."));
    }
    if !i.skipped.is_empty() {
        out.push(format!("Skipped: {}.", i.skipped.trim_end_matches('.')));
    }
    out.push(if i.brief {
        "Has a brief: s starts it.".to_string()
    } else {
        "No brief: s asks the coordinator for one.".to_string()
    });
    out.join(" ")
}

/// The keys for the selected item, for the footer.
pub fn item_keys(i: &Item) -> Vec<(&'static str, &'static str)> {
    let mut keys = if i.asks() { vec![("y", "keep"), ("n", "drop")] } else { vec![("s", "start"), ("n", "drop")] };
    if i.asks() {
        keys.push(("s", "start"));
    }
    keys.push(("J K", "move"));
    keys
}

/// The y/n question `n` asks before an item is dropped: `Drop L7 "Tidy the errors"?`.
pub fn item_confirm(i: &Item) -> String {
    format!("Drop {} \"{}\"?", i.id, i.title)
}

/// What the board says to the coordinator for an item with no brief: write one, then
/// start it. Typed into its pane as the person would (`bullpen say`).
pub fn brief_request(i: &Item) -> String {
    format!(
        "Please write a brief for {id} from the backlog (\"{title}\") and start it: `bullpen later {id} --start --brief-file FILE`.",
        id = i.id,
        title = i.title
    )
}

#[cfg(test)]
#[path = "test_backlog.rs"]
mod tests;
