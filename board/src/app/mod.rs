//! The board's state and what it does with a key or a message: `App`, its modes, and
//! the selection. The keys (the Later group's apart), the background commands and
//! reloading the board's program are child modules, so they share `App`'s private fields.

mod jobs;
mod keys;
mod later;
mod restart;
#[cfg(test)]
pub(crate) mod testing;

pub use jobs::Job;

/// Run `bullpen <args>` in cwd and wait: did it work, and the last line it said.
pub fn run_bullpen(bullpen: &std::path::Path, cwd: &std::path::Path, args: &[&str]) -> (bool, String) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (ok, out) = jobs::run(bullpen, cwd, &args);
    (ok, jobs::last_line(&out))
}
pub use restart::{parse_args, Args, ExeId};

use crate::edit::LineEdit;
use crate::model::{self, Action, Config, Item, Prompt, Task};
use ratatui::crossterm::event::KeyEvent;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// What the bottom of the board is doing besides showing keys.
pub enum Mode {
    Normal,
    /// a full-screen text until any key; a peek at a task keeps updating until esc or
    /// q, and t ⏎ i a d v go back to the board and act on the task
    Overlay { title: String, lines: Vec<String>, peek: Option<String> },
    /// the `?` list, scrolled to its top line; scroll keys scroll, any other key closes
    Help { top: usize },
    /// typing a line to tell a task's worker, or with reply, to answer its coordinator's question
    Tell { id: String, text: LineEdit, reply: bool },
    /// the prompt a task's agent is at, read again every second (None until first read),
    /// until an option is pressed or esc
    Answer { id: String, title: String, prompt: Option<Prompt> },
    /// g on a task whose tour is stale: n writes a new one, o opens the old one
    Stale { id: String },
    /// q: y quits the board, any other key stays
    Quit,
}

/// What a message on the board is: a failure (red, and kept until the next action on
/// a task), something that worked (green), or just information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Info,
    Done,
    Failed,
}

/// The board's message line: what just happened. It wraps to a few rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Note {
    pub text: String,
    pub kind: Kind,
}

impl Note {
    fn new(kind: Kind, text: impl Into<String>) -> Note {
        Note { text: text.into(), kind }
    }
    pub fn info(text: impl Into<String>) -> Note {
        Note::new(Kind::Info, text)
    }
    pub fn done(text: impl Into<String>) -> Note {
        Note::new(Kind::Done, text)
    }
    pub fn failed(text: impl Into<String>) -> Note {
        Note::new(Kind::Failed, text)
    }
    /// What a command said: the line given when it worked, else its error.
    fn of(ok: bool, worked: impl Into<String>, err: String) -> Note {
        if ok { Note::done(worked) } else { Note::failed(err) }
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
    pub fn clear(&mut self) {
        *self = Note::default();
    }
}

impl PartialEq<&str> for Note {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

pub struct App {
    pub dir: PathBuf,
    pub root: PathBuf,
    pub bullpen: PathBuf,
    /// the coordinator this board sits beside (`--coordinator`): other ones' tasks are
    /// hidden, or dimmed with `o`
    pub me: Option<String>,
    pub repo_name: String,
    pub cfg: Config,
    pub tasks: Vec<Task>,
    /// the backlog's open items, in list order: the Later group
    pub later: Vec<Item>,
    /// `L`: the Later group shows a row per item, not its one line
    pub later_open: bool,
    /// what needs you or is ready in other repositories, looked at every few seconds
    pub others: model::OtherRepos,
    pub sel: Option<String>,
    /// `f`: closed tasks shown too
    pub closed: bool,
    pub msg: Note,
    /// the first row shown, when the tasks don't all fit
    pub top: usize,
    /// a y/n question shown on the task's row
    pub pending: Option<(Action, String)>,
    /// slow commands still running, by task, oldest first
    pub running: Vec<(String, Job)>,
    pub mode: Mode,
    /// screen height, for how much a peek shows
    pub height: u16,
    /// how many tasks the list showed last time it was drawn: a page, for PgDn and PgUp
    pub page: usize,
    /// when the board started: spinners turn by it
    pub started: Instant,
    tx: Sender<Msg>,
    /// a peek read is in flight
    peeking: bool,
    /// the peek's last read failed: it shows why, and isn't live
    pub peek_failed: bool,
    /// when the peek reads the screen again
    peek_due: Instant,
    /// the board's own program, as it was when it started: replaced, the board reloads
    pub exe: Option<(PathBuf, ExeId)>,
    /// the Rex session `v` opens a diff over (`diff --session`); None: the command's own
    /// choice, the coordinator's. Mission control sets its own, so the diff opens over it.
    pub pane_session: Option<String>,
}

pub enum Msg {
    Key(KeyEvent),
    /// pasted text (bracketed paste): it goes into a line being typed, and is never keys
    Paste(String),
    Redraw,
    Changed,
    Done { id: String, job: Job, ok: bool, out: String },
    Peeked { id: String, text: String, ok: bool },
    Prompted { id: String, prompt: Option<Prompt>, out: String },
}

impl App {
    /// The board as the command line starts it; `exe` is its own program, to reload
    /// when that's replaced.
    pub fn new(args: Args, exe: Option<PathBuf>, tx: Sender<Msg>) -> App {
        let Args { dir, root, bullpen, me, select, closed, reloaded } = args;
        let repo_name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        App {
            cfg: Config::load(&dir),
            tasks: Vec::new(),
            later: Vec::new(),
            later_open: false,
            others: Default::default(),
            dir,
            root,
            bullpen,
            me,
            repo_name,
            sel: select,
            closed,
            msg: if reloaded { Note::info("Reloaded: the board was rebuilt.") } else { Note::default() },
            top: 0,
            pending: None,
            running: Vec::new(),
            mode: Mode::Normal,
            height: 0,
            page: 1,
            started: Instant::now(),
            tx,
            peeking: false,
            peek_failed: false,
            peek_due: Instant::now(),
            exe: exe.and_then(|p| ExeId::of(&p).map(|id| (p, id))),
            pane_session: None,
        }
    }

    /// A repo's board with nothing read yet and no program to reload, which mission
    /// control drives: it fills in `tasks` and `cfg` from its snapshot, and sends it the
    /// keys it acts on a task with. Its background commands answer on tx.
    pub fn for_repo(dir: PathBuf, root: PathBuf, bullpen: PathBuf, tx: Sender<Msg>) -> App {
        let args = Args { dir, root, bullpen, ..Default::default() };
        App::new(args, None, tx)
    }

    pub fn reload(&mut self) {
        self.cfg = Config::load(&self.dir);
        self.tasks = model::load_tasks(&self.dir);
        self.later = model::load_later(&self.dir);
        // the backlog emptied: the next item added shows as the folded line again
        if self.later.is_empty() {
            self.later_open = false;
        }
        // a drop asked about an item that has gone meanwhile
        if let Some((Action::DropItem, id)) = &self.pending {
            if !self.later.iter().any(|i| &i.id == id) {
                self.pending = None;
            }
        }
        if self.cfg.other_repos {
            self.others.refresh(&self.dir);
        } else {
            self.others = Default::default();
        }
        self.take_select();
    }

    /// Select the task `bullpen go <id> --board` asked this board to (the picker does,
    /// for a task with no pane): select/<this board's coordinator> holds its ID. One
    /// older than 30 seconds is left over (no board read it then) and is dropped.
    fn take_select(&mut self) {
        let path = self.dir.join("select").join(self.me.as_deref().unwrap_or("board"));
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(30));
        let _ = std::fs::remove_file(&path);
        let id = text.trim();
        if !fresh || self.sel.as_deref() == Some(id) {
            return;
        }
        if self.order().iter().any(|t| t.id == id) {
            self.sel = Some(id.to_string());
            self.pending = None; // a question about the task that was selected
        } else if let Some(t) = self.tasks.iter().find(|t| t.id == id && self.cfg.hides(t, self.me.as_deref())) {
            self.msg = Note::info(format!("{id} is {}'s: o shows it.", t.coordinator.as_deref().unwrap_or("another coordinator")));
        }
    }

    /// The groups the board shows: without the tasks it hides.
    pub fn groups(&self) -> Vec<(&'static str, Vec<&Task>)> {
        model::shown_groups(&self.tasks, &self.cfg, self.me.as_deref(), self.closed)
    }

    /// The tasks in the order the board shows them.
    pub fn order(&self) -> Vec<&Task> {
        self.groups().into_iter().flat_map(|(_, ts)| ts).collect()
    }

    /// Show or hide the other coordinators' tasks, and remember it for this board.
    fn toggle_others(&mut self) {
        let Some(me) = self.me.clone().filter(|_| self.cfg.coordinators.len() > 1) else {
            self.msg = Note::info("One coordinator: every task is shown.");
            return;
        };
        let on = !self.cfg.shows_others(Some(&me));
        self.cfg.board_others.retain(|c| *c != me);
        if on {
            self.cfg.board_others.push(me.clone());
        }
        self.top = 0;
        let (ok, out) = self.bp(&["config", "--board-others", if on { "on" } else { "off" }, "--coordinator", &me]);
        let theirs = match self.cfg.coordinators.iter().filter(|c| **c != me).collect::<Vec<_>>()[..] {
            [one] => format!("{one}'s tasks"),
            _ => "other coordinators' tasks".into(),
        };
        self.msg = if !ok {
            Note::failed(out)
        } else if on {
            Note::info(format!("Showing {theirs} too."))
        } else {
            Note::info(format!("Hiding {theirs}."))
        };
    }

    /// The selected task: none while an item of the Later group is selected.
    pub fn current(&self) -> Option<&Task> {
        if self.current_item().is_some() {
            return None;
        }
        let order = self.order();
        let sel = self.sel.as_deref();
        order.iter().find(|t| Some(t.id.as_str()) == sel).or(order.first()).copied()
    }

    /// Keep the selection on a task (or item) that's still shown.
    pub fn settle(&mut self) {
        let ids = self.nav_ids();
        if !ids.iter().any(|id| Some(id) == self.sel.as_ref()) {
            self.sel = ids.first().cloned();
        }
    }

    /// The next task after position `i` that needs you (wrapping round), other than
    /// `skip`; once nothing needs you, the next that's ready to accept or finished
    /// (to close). The bool says it needs you.
    fn next_up(&self, i: usize, skip: Option<&str>) -> Option<(String, bool)> {
        let order = self.order();
        let after = |pick: &dyn Fn(&Task) -> bool| {
            let js: Vec<usize> =
                (0..order.len()).filter(|&j| pick(order[j]) && Some(order[j].id.as_str()) != skip).collect();
            js.iter().find(|&&j| j > i).or(js.first()).map(|&j| order[j].id.clone())
        };
        let yours = |t: &Task| matches!(t.status.as_str(), "ready" | "finished");
        after(&|t: &Task| t.needs_you()).map(|id| (id, true)).or_else(|| after(&yours).map(|id| (id, false)))
    }

    /// How long the loop may wait for something to happen before redrawing.
    pub fn tick(&self) -> Duration {
        let moving = |t: &Task| matches!(t.status.as_str(), "working" | "review");
        if !self.running.is_empty() || self.tasks.iter().any(|t| t.tour_writing()) {
            Duration::from_millis(50) // a command's spinner turns fast
        } else if self.tasks.iter().any(moving) {
            Duration::from_millis(100) // under half of MARK_MS, so the marks show every frame
        } else if matches!(self.mode, Mode::Overlay { peek: Some(_), .. } | Mode::Answer { .. }) {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(1) // ages move on by the minute
        }
    }

    /// Handle one message. False when the board should quit.
    pub fn handle(&mut self, m: Msg) -> bool {
        match m {
            Msg::Key(k) => return self.key(k),
            Msg::Paste(text) => {
                if let Mode::Tell { text: line, .. } = &mut self.mode {
                    line.paste(&text);
                }
            }
            Msg::Done { id, job, ok, out } => self.done(id, job, ok, out),
            Msg::Peeked { id, text, ok } => self.peeked(id, text, ok),
            Msg::Prompted { id, prompt, out } => self.prompted(id, prompt, out),
            Msg::Redraw | Msg::Changed => {}
        }
        true
    }
}

#[cfg(test)]
#[path = "test_app.rs"]
mod tests;
