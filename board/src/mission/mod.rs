//! Mission control: one screen across every repo bullpen knows (`bullpen mission`).
//! It lists every repo's tasks as one inbox (what needs you, then what's ready), then
//! each repo with its coordinators and its other open tasks, and the selected task's
//! detail beside them.
//!
//! What it shows comes from the fleet snapshot (bullpen_core::fleet), read again when
//! the state folder changes and once a second, through a kept Reader that reads a file
//! again only when it changed. Whether each coordinator's pane is alive is Rex's to say:
//! `bullpen repos --json` asks it, in the background every few seconds.
//!
//! The keys on a task are the board's: each repo has a board's `App` of its own, with
//! its tasks from the snapshot, and a key on a task goes to its repo's App with that
//! task selected. So every action runs the `bullpen` command in that task's repo, as
//! the board does, and its questions, answers and lines to type are the board's.

mod draw;
pub(crate) mod items;
mod look;
pub mod run;
#[cfg(test)]
#[path = "tests/testing.rs"]
pub(crate) mod testing;

pub use items::{Item, Row};

use crate::app::{App, Mode, Msg, Note};
use crate::model::{self, Config, Look, Task};
use bullpen_core::fleet::{Fleet, Reader};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

/// The global settings over the defaults; the defaults when they can't be read (each
/// repository's board says why).
fn global() -> Config {
    Config::from_obj(bullpen_core::config::inherited_config().unwrap_or_default())
}

/// What mission control's loop hears.
pub enum MMsg {
    Key(KeyEvent),
    Paste(String),
    Redraw,
    /// something under the state folder changed
    Changed,
    /// a repo's board said something (a command finished, a prompt was read)
    Board(PathBuf, Msg),
    /// which coordinators' panes are alive: (repo's state folder, coordinator, alive)
    Alive(Vec<(PathBuf, String, bool)>),
    /// the image look's: the key reader stopped for the terminal's answers, or read
    /// them; an image drawn ahead is ready (the board's `Msg::Parked`, `Answered`, `Ready`)
    Look(Msg),
}

/// The selected task: its repo's state folder and its ID (IDs repeat across repos).
pub type Sel = (PathBuf, String);

pub struct Mission {
    pub state: PathBuf,
    pub bullpen: PathBuf,
    pub fleet: Fleet,
    reader: Reader,
    /// the last answer about each coordinator's pane, by (repo's state folder, ID)
    pub alive: HashMap<(PathBuf, String), bool>,
    /// each repo's board, by state folder: the keys on its tasks go there
    pub boards: HashMap<PathBuf, App>,
    pub sel: Option<Sel>,
    /// where the selection was in the list: a task that leaves it hands the selection
    /// to the one that takes its place
    sel_at: usize,
    pub msg: Note,
    /// q asks to be sure
    pub quitting: bool,
    /// the first row shown, and how many tasks the list showed (a page)
    pub top: usize,
    pub page: usize,
    pub height: u16,
    pub started: Instant,
    /// the Rex session this runs in: a diff opens over it
    pub session: Option<String>,
    /// the global settings (the board's look, type and motion are mission control's too)
    pub cfg: Config,
    /// drawn as text or images: the board_look setting, until the images can't be drawn
    pub look: Look,
    /// the board_look setting as last read: a change to it changes `look`
    look_set: Look,
    /// drawn as images this frame (the terminal draws them), and the frame's view for them
    pub image: bool,
    pub view: Option<crate::image::View>,
    tx: Sender<MMsg>,
}

impl Mission {
    pub fn new(state: PathBuf, bullpen: PathBuf, select: Option<Sel>, tx: Sender<MMsg>) -> Mission {
        let cfg = global();
        Mission {
            state,
            bullpen,
            fleet: Fleet::default(),
            reader: Reader::default(),
            alive: HashMap::new(),
            boards: HashMap::new(),
            sel: select,
            sel_at: 0,
            msg: Note::default(),
            quitting: false,
            top: 0,
            page: 1,
            height: 0,
            started: Instant::now(),
            session: std::env::var("REX_SESSION").ok().filter(|s| !s.is_empty()),
            look: cfg.look,
            look_set: cfg.look,
            cfg,
            image: false,
            view: None,
            tx,
        }
    }

    /// Read the snapshot again (only the files that changed). One that can't be read
    /// (a broken global config) keeps the last, and says why.
    pub fn reload(&mut self) {
        self.cfg = global();
        if self.cfg.look != self.look_set {
            (self.look, self.look_set) = (self.cfg.look, self.cfg.look);
        }
        match self.reader.read(&self.state) {
            Ok(fleet) => self.take(fleet),
            Err(e) => self.msg = Note::failed(e.0),
        }
    }

    /// Show this snapshot: the coordinators get the last word on their panes, and each
    /// repo's board its tasks and settings.
    pub fn take(&mut self, mut fleet: Fleet) {
        for r in &mut fleet.repos {
            for c in &mut r.coordinators {
                c.alive = self.alive.get(&(r.repo.dir.clone(), c.id.clone())).copied();
            }
        }
        for r in &fleet.repos {
            let dir = r.repo.dir.clone();
            if !self.boards.contains_key(&dir) {
                let app = self.board_for(&dir, &r.repo.root);
                self.boards.insert(dir.clone(), app);
            }
            let app = self.boards.get_mut(&dir).expect("just made");
            app.cfg = Config::from_obj(r.repo.cfg.clone());
            app.tasks = model::tasks_of(r.all.clone());
        }
        // a repo gone from the snapshot takes its board along, unless it's busy
        self.boards.retain(|d, app| app.busy() || fleet.repos.iter().any(|r| r.repo.dir == *d));
        self.fleet = fleet;
        self.settle();
    }

    /// A repo's board, whose messages come back as MMsg::Board.
    fn board_for(&self, dir: &Path, root: &Path) -> App {
        let (tx, rx) = mpsc::channel();
        let (up, d) = (self.tx.clone(), dir.to_path_buf());
        std::thread::spawn(move || {
            while let Ok(m) = rx.recv() {
                if up.send(MMsg::Board(d.clone(), m)).is_err() {
                    return;
                }
            }
        });
        let mut app = App::for_repo(dir.to_path_buf(), root.to_path_buf(), self.bullpen.clone(), tx);
        app.pane_session = self.session.clone();
        // the pane can't come over to mission control's session: ⏎ goes to its own
        app.go_switch = true;
        app
    }

    pub fn rows(&self) -> Vec<Row> {
        items::rows(&self.fleet)
    }

    pub fn order(&self) -> Vec<Item> {
        items::order(&self.rows())
    }

    fn sel_of(&self, i: Item) -> Sel {
        (self.fleet.repos[i.repo].repo.dir.clone(), items::id(items::task(&self.fleet, i)).to_string())
    }

    /// The selected task, or the first when none is.
    pub fn current(&self) -> Option<Item> {
        let order = self.order();
        let found = order.iter().copied().find(|i| self.sel.as_ref() == Some(&self.sel_of(*i)));
        found.or(order.first().copied())
    }

    /// Keep the selection on a task that's listed: one that left hands it to the task
    /// now where it was.
    pub fn settle(&mut self) {
        let order = self.order();
        match order.iter().position(|i| self.sel.as_ref() == Some(&self.sel_of(*i))) {
            Some(at) => self.sel_at = at,
            None => {
                let at = self.sel_at.min(order.len().saturating_sub(1));
                self.sel = order.get(at).map(|i| self.sel_of(*i));
            }
        }
    }

    /// The board of the selected task's repo.
    pub fn active(&self) -> Option<&App> {
        self.boards.get(&self.sel.as_ref()?.0)
    }

    fn active_mut(&mut self) -> Option<&mut App> {
        let dir = self.sel.as_ref()?.0.clone();
        self.boards.get_mut(&dir)
    }

    /// Is a board asking or being typed into: then every key goes to it.
    fn board_open(&self) -> bool {
        self.active().is_some_and(|a| a.pending.is_some() || !matches!(a.mode, Mode::Normal))
    }

    /// How long the loop may wait before drawing again: the marks move while agents
    /// work, a command's spinner faster.
    pub fn tick(&self) -> Duration {
        let moving = self.fleet.repos.iter().flat_map(|r| &r.open).any(|t| matches!(items::status(t), "working" | "review"));
        if self.boards.values().any(|a| !a.running.is_empty()) {
            Duration::from_millis(50)
        } else if moving {
            Duration::from_millis(100)
        } else if self.board_open() {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(1)
        }
    }

    /// Would a reload lose something: a board busy, or q asked.
    pub fn busy(&self) -> bool {
        self.quitting || self.boards.values().any(App::busy)
    }

    /// Read a prompt being answered again, on every board that has one open.
    pub fn peek(&mut self) {
        let h = self.height;
        for app in self.boards.values_mut() {
            app.height = h;
            app.peek();
        }
    }

    /// Handle one message. False when mission control should quit.
    pub fn handle(&mut self, m: MMsg) -> bool {
        match m {
            MMsg::Key(k) => return self.key(k),
            MMsg::Paste(text) => {
                if let Some(app) = self.active_mut() {
                    app.handle(Msg::Paste(text));
                }
            }
            MMsg::Board(dir, m) => {
                let Some(app) = self.boards.get_mut(&dir) else { return true };
                let before = app.msg.clone();
                app.handle(m);
                if app.msg != before {
                    self.msg = app.msg.clone();
                }
            }
            MMsg::Alive(list) => {
                self.alive = list.into_iter().map(|(d, c, a)| ((d, c), a)).collect();
                let fleet = std::mem::take(&mut self.fleet);
                self.take(fleet);
            }
            MMsg::Redraw | MMsg::Changed | MMsg::Look(_) => {}
        }
        true
    }

    /// Handle a key. False when mission control should quit.
    pub fn key(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && k.code == KeyCode::Char('c') {
            return false;
        }
        if self.quitting {
            if matches!(k.code, KeyCode::Char('y' | 'Y')) {
                return false;
            }
            self.quitting = false;
            self.msg = Note::info("Cancelled.");
            return true;
        }
        if self.board_open() {
            self.to_board(k);
            return true;
        }
        let ch = match k.code {
            KeyCode::Char(c) if !ctrl => Some(c),
            _ => None,
        };
        let order = self.order();
        let i = self.current().and_then(|c| order.iter().position(|x| *x == c)).unwrap_or(0);
        let page = self.page.max(1);
        let to = |n: usize| order.get(n.min(order.len().saturating_sub(1))).copied();
        let moved = match (k.code, ch) {
            (KeyCode::Char('d'), _) if ctrl => Some(to(i + (page / 2).max(1))),
            (KeyCode::Char('u'), _) if ctrl => Some(to(i.saturating_sub((page / 2).max(1)))),
            (KeyCode::Down, _) | (_, Some('j')) => Some(to(i + 1)),
            (KeyCode::Up, _) | (_, Some('k')) => Some(to(i.saturating_sub(1))),
            (KeyCode::PageDown, _) => Some(to(i + page)),
            (KeyCode::PageUp, _) => Some(to(i.saturating_sub(page))),
            (KeyCode::End, _) | (_, Some('G')) => Some(to(usize::MAX)),
            (KeyCode::Home, _) => Some(to(0)),
            _ => None,
        };
        if let Some(to) = moved {
            if self.msg.kind != crate::app::Kind::Failed {
                self.msg.clear();
            }
            if let Some(to) = to {
                self.select(to);
            }
            return true;
        }
        match (k.code, ch) {
            (_, Some('q')) => self.quitting = true,
            (_, Some('n')) => self.next(i),
            (_, Some('/')) => self.pick(),
            (KeyCode::Esc, _) => self.msg.clear(),
            (KeyCode::Enter, _) | (_, Some('i' | 'a' | 'd' | 't' | 's' | 'r' | 'g' | 'v' | 'p')) => self.to_board(k),
            _ => {}
        }
        true
    }

    fn select(&mut self, i: Item) {
        self.sel = Some(self.sel_of(i));
        self.settle();
    }

    /// The key on the selected task, in its repo's board: as the board does it there.
    fn to_board(&mut self, k: KeyEvent) {
        let Some(item) = self.current() else {
            self.msg = Note::info("Nothing open in any repository.");
            return;
        };
        let (dir, id) = self.sel_of(item);
        self.sel = Some((dir.clone(), id.clone()));
        let msg = self.msg.clone();
        let Some(app) = self.boards.get_mut(&dir) else { return };
        if app.pending.is_none() && matches!(app.mode, Mode::Normal) {
            app.sel = Some(id);
        }
        app.msg = msg;
        app.key(k);
        self.msg = app.msg.clone();
    }

    /// n: the next task that needs you after position i, in any repo (wrapping round);
    /// once nothing does, the next that's ready or finished.
    fn next(&mut self, i: usize) {
        let order = self.order();
        let after = |part: fn(&str) -> bool| {
            let js: Vec<usize> = (0..order.len()).filter(|&j| part(items::status(items::task(&self.fleet, order[j])))).collect();
            js.iter().find(|&&j| j > i).or(js.first()).map(|&j| order[j])
        };
        let (needs, ready) = (items::INBOX[0].1, items::INBOX[1].1);
        if let Some(to) = after(needs) {
            self.msg.clear();
            self.select(to);
        } else if let Some(to) = after(ready) {
            let t = items::task(&self.fleet, to);
            let what = if items::status(t) == "ready" { "ready" } else { "finished" };
            self.msg = Note::info(format!("Nothing needs you: {} is {what}.", items::id(t)));
            self.select(to);
        } else {
            self.msg = Note::info("Nothing needs you.");
        }
    }

    /// /: the picker across repos, in a floating pane over mission control.
    fn pick(&mut self) {
        let (ok, out) = crate::app::run_bullpen(&self.bullpen, &self.state, &["pick", "--pane"]);
        self.msg = if ok { Note::default() } else { Note::failed(out) };
    }

    /// The selected task's repo's board and the task in it, for drawing its detail.
    pub fn current_task(&self) -> Option<(&App, &Task, Item)> {
        let item = self.current()?;
        let r = &self.fleet.repos[item.repo];
        let app = self.boards.get(&r.repo.dir)?;
        let id = items::id(items::task(&self.fleet, item));
        Some((app, app.tasks.iter().find(|t| t.id == id)?, item))
    }
}

#[cfg(test)]
#[path = "tests/test_mission.rs"]
mod tests;
