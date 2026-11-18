//! The Later group's keys: `L` expands the backlog into rows (or folds it back to its
//! line), and on a selected item `y` keeps it, `n` drops it (once you say y), `s` starts it (or, with
//! no brief, asks the coordinator to write one), `J`/`K` move it and ⏎ shows it in
//! full. Each runs `bullpen later`; the board redraws when backlog.json changes.

use super::jobs::{run, said};
use super::{App, Job, Mode, Note};
use crate::model::{self, Action, Item};
use ratatui::crossterm::event::KeyCode;

impl App {
    /// The item selected, when the Later group is expanded and the selection is one.
    pub fn current_item(&self) -> Option<&Item> {
        let sel = self.sel.as_deref().filter(|_| self.later_open)?;
        self.later.iter().find(|i| i.id == sel)
    }

    /// The IDs j and k move through: the tasks the board shows, then (expanded) the items.
    pub(super) fn nav_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.order().iter().map(|t| t.id.clone()).collect();
        if self.later_open {
            ids.extend(self.later.iter().map(|i| i.id.clone()));
        }
        ids
    }

    /// L: expand the Later group, its first item selected, or fold it back to its line.
    pub(super) fn toggle_later(&mut self) {
        if self.later_open {
            self.later_open = false;
            if self.sel.as_deref().is_some_and(model::is_item) {
                self.sel = self.order().last().map(|t| t.id.clone());
            }
            return;
        }
        let Some(first) = self.later.first() else {
            self.msg = Note::info("The backlog is empty: `bullpen later \"...\"` adds an idea to it.");
            return;
        };
        self.later_open = true;
        self.sel = Some(first.id.clone());
    }

    /// A key on the selected item. False when it isn't one of the item's keys: the
    /// board's own keys (moving, help) still work.
    pub(super) fn item_key(&mut self, code: KeyCode, ch: Option<char>) -> bool {
        let Some(item) = self.current_item().cloned() else { return false };
        let id = item.id.as_str();
        match (code, ch) {
            (_, Some('y')) => {
                self.later_says(&["later", id, "--keep"]);
            }
            // asked first, as a task's drop is: the drop itself is drop_item's
            (_, Some('n')) => self.pending = Some((Action::DropItem, id.into())),
            (_, Some('J')) => {
                self.later_says(&["later", id, "--down"]);
            }
            (_, Some('K')) => {
                self.later_says(&["later", id, "--up"]);
            }
            (_, Some('s')) if item.brief => self.spawn(id, Job::StartItem),
            (_, Some('s')) => self.ask_for_brief(&item),
            (KeyCode::Enter, _) => {
                let (ok, out) = run(&self.bullpen, &self.root, &["later".into(), id.into()]);
                self.mode = Mode::Overlay {
                    title: format!("{id}  {}", item.title),
                    lines: if ok { out.lines().map(String::from).collect() } else { vec![said(&super::jobs::last_line(&out))] },
                    peek: None,
                };
            }
            _ => return false,
        }
        true
    }

    /// y to an item's drop: `bullpen later L7 --drop`, and the selection goes on to the
    /// next item.
    pub(super) fn drop_item(&mut self, id: &str) {
        let next = self.neighbour(id);
        if self.later_says(&["later", id, "--drop"]) {
            self.sel = next;
        }
    }

    /// Run a quick `bullpen later ...` and say what it said. Whether it worked.
    fn later_says(&mut self, args: &[&str]) -> bool {
        let (ok, out) = self.bp(args);
        self.msg = Note::of(ok, said(&bullpen_core::text::upper_first(&out)), out);
        ok
    }

    /// The selection once item id leaves the list: the next item, else the one before,
    /// else the last task.
    fn neighbour(&self, id: &str) -> Option<String> {
        let at = self.later.iter().position(|i| i.id == id)?;
        let other = self.later.get(at + 1).or(at.checked_sub(1).and_then(|j| self.later.get(j)));
        other.map(|i| i.id.clone()).or_else(|| self.order().last().map(|t| t.id.clone()))
    }

    /// s on an item with no brief: the coordinator is asked to write one and start it,
    /// typed into its pane (`bullpen say`), and the board hands over to it, as c does.
    fn ask_for_brief(&mut self, item: &Item) {
        let text = model::brief_request(item);
        let mut args = vec!["say"];
        if let Some(me) = self.me.as_deref() {
            args.extend(["--coordinator", me]);
        }
        args.push(&text);
        let (ok, out) = self.bp(&args);
        if !ok {
            self.msg = Note::failed(out);
            return;
        }
        let (ok, out) = self.bp(&["go"]);
        self.msg = Note::of(ok, format!("Asked the coordinator to write {}'s brief and start it.", item.id), out);
    }

    /// An item's start finished: what it became, selected. "L7 is BP-150 now, started
    /// in ..." says the task.
    pub(super) fn item_started(&mut self, id: &str, ok: bool, out: String) {
        let task = out.strip_prefix(&format!("{id} is ")).and_then(|r| r.split(' ').next()).filter(|t| t.contains('-'));
        self.msg = match task {
            Some(t) if ok => {
                self.sel = Some(t.to_string());
                Note::done(format!("Started {id}: it's {t} now."))
            }
            _ => Note::of(ok, format!("Started {id}."), out),
        };
    }
}

#[cfg(test)]
#[path = "tests/test_later.rs"]
mod tests;
