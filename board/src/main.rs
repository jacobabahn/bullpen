//! bullpen's task board: a live list of a repo's tasks, grouped by what you do
//! next, from which you open, accept, drop, tell or start them.
//!
//! It reads the repo's state directory (config.json and tasks/*.json) and redraws
//! when anything there changes. Everything it does goes through the `bullpen`
//! command, so the board itself never changes a task.
//!
//! Usage: bullpen-board --dir <repo state dir> --root <repo> --bullpen <bullpen program>

mod model;
mod ui;

use model::{Action, Config, Task};
use notify::{RecursiveMode, Watcher};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

pub const BOARD_KEYS: &str = "\
Moving
  j k  ↑ ↓     select a task
  n            select the next task that needs you
  ⏎            open the worker's pane (the reviewer's, in review)
  c            back to the coordinator, panes put away

Acting on the selected task
  a            accept: merge its PR and clean up
  d            drop: close its PR, pane and worktree
  t            tell its worker something
  p            peek at the end of its worker's screen
  s            start it
  r            resume a stopped worker

The board
  l            switch layout (grouped, compact)
  f            show finished tasks too
  ?            this list
  q            quit the board";

/// What the bottom of the board is doing besides showing keys.
pub enum Mode {
    Normal,
    /// a full-screen text (keys, a peek) until any key
    Overlay { title: String, lines: Vec<String> },
    /// typing a line to tell a task's worker
    Tell { id: String, text: String },
}

pub struct App {
    pub dir: PathBuf,
    pub root: PathBuf,
    pub bullpen: PathBuf,
    pub repo_name: String,
    pub cfg: Config,
    pub tasks: Vec<Task>,
    pub sel: Option<String>,
    pub finished: bool,
    pub msg: String,
    /// the first row shown, when the tasks don't all fit
    pub top: usize,
    /// a y/n question shown on the task's row
    pub pending: Option<(Action, String)>,
    /// said on the bottom line while something slow runs
    pub busy: Option<String>,
    pub mode: Mode,
    /// screen height, for how much a peek shows
    pub height: u16,
}

enum Msg {
    Key(KeyEvent),
    Redraw,
    Changed,
}

impl App {
    fn reload(&mut self) {
        self.cfg = Config::load(&self.dir);
        self.tasks = model::load_tasks(&self.dir);
    }

    /// The tasks in the order the board shows them.
    pub fn order(&self) -> Vec<&Task> {
        model::board_groups(&self.tasks, self.finished).into_iter().flat_map(|(_, ts)| ts).collect()
    }

    pub fn current(&self) -> Option<&Task> {
        let order = self.order();
        let sel = self.sel.as_deref();
        order.iter().find(|t| Some(t.id.as_str()) == sel).or(order.first()).copied()
    }

    /// Keep the selection on a task that's still shown.
    fn settle(&mut self) {
        let first = self.order().first().map(|t| t.id.clone());
        if !self.order().iter().any(|t| Some(&t.id) == self.sel.as_ref()) {
            self.sel = first;
        }
    }

    /// Run `bullpen <args>`: did it work, and the last line it said.
    fn bp(&self, args: &[&str]) -> (bool, String) {
        let out = Command::new(&self.bullpen)
            .args(args)
            .current_dir(&self.root)
            // never let it read our terminal: it would wait there for keys meant for the board
            .stdin(Stdio::null())
            .output();
        match out {
            Ok(o) => {
                let text = if o.status.success() { o.stdout } else { o.stderr };
                let text = String::from_utf8_lossy(&text);
                let last = text.trim().lines().last().unwrap_or("");
                (o.status.success(), last.strip_prefix("bullpen: ").unwrap_or(last).to_string())
            }
            Err(e) => (false, format!("can't run {}: {e}", self.bullpen.display())),
        }
    }

    /// Say what's happening, draw, then do the slow thing.
    fn slow<T>(&mut self, term: &mut ratatui::DefaultTerminal, text: String, f: impl FnOnce(&Self) -> T) -> T {
        self.busy = Some(text);
        let _ = term.draw(|f| ui::draw(f, self));
        let out = f(self);
        self.busy = None;
        out
    }

    /// Handle a key. False when the board should quit.
    fn key(&mut self, k: KeyEvent, term: &mut ratatui::DefaultTerminal) -> bool {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return false;
        }
        match &mut self.mode {
            Mode::Overlay { .. } => {
                self.mode = Mode::Normal;
                return true;
            }
            Mode::Tell { id, text } => {
                match k.code {
                    KeyCode::Enter => {
                        let (id, text) = (id.clone(), text.trim().to_string());
                        self.mode = Mode::Normal;
                        if !text.is_empty() {
                            let (ok, out) = self.bp(&["tell", &id, &text]);
                            self.msg = if ok { format!("Sent to {id}.") } else { out };
                        }
                    }
                    KeyCode::Esc => self.mode = Mode::Normal,
                    KeyCode::Backspace => {
                        text.pop();
                    }
                    KeyCode::Char(c) if text.chars().count() < 500 => text.push(c),
                    _ => {}
                }
                return true;
            }
            Mode::Normal => {}
        }

        let ch = match k.code {
            KeyCode::Char(c) => Some(c),
            _ => None,
        };
        self.msg.clear();

        if let Some((action, tid)) = self.pending.take() {
            if !matches!(ch, Some('y' | 'Y')) {
                self.msg = "Cancelled.".into();
                return true;
            }
            if action == Action::Accept {
                let (ok, out) = self.slow(term, format!("Accepting {tid}…"), |a| a.bp(&["accept", &tid]));
                self.msg = if ok { format!("Accepted {tid}.") } else { out };
            } else {
                let mut args = vec!["drop", tid.as_str()];
                if action == Action::DropForce {
                    args.push("--force");
                }
                let (ok, out) = self.slow(term, format!("Dropping {tid}…"), |a| a.bp(&args));
                if !ok && out.contains("uncommitted") {
                    self.pending = Some((Action::DropForce, tid));
                } else {
                    self.msg = if ok { format!("Dropped {tid}.") } else { out };
                }
            }
            return true;
        }

        let ids: Vec<String> = self.order().iter().map(|t| t.id.clone()).collect();
        let i = ids.iter().position(|x| Some(x) == self.sel.as_ref()).unwrap_or(0);
        match (k.code, ch) {
            (_, Some('q')) => return false,
            (KeyCode::Down, _) | (_, Some('j')) => {
                self.sel = ids.get((i + 1).min(ids.len().saturating_sub(1))).cloned();
            }
            (KeyCode::Up, _) | (_, Some('k')) => {
                self.sel = ids.get(i.saturating_sub(1)).cloned();
            }
            (_, Some('?')) => {
                self.mode = Mode::Overlay {
                    title: "Board keys".into(),
                    lines: BOARD_KEYS.lines().map(String::from).collect(),
                }
            }
            (_, Some('l')) => {
                let layout = self.cfg.layout.other();
                self.cfg.layout = layout;
                self.top = 0;
                let (ok, out) = self.bp(&["config", "--board-layout", layout.name()]);
                if !ok {
                    self.msg = out;
                }
            }
            (_, Some('f')) => self.finished = !self.finished,
            (_, Some('c')) => self.msg = self.bp(&["go"]).1,
            (_, Some('n')) => {
                let order = self.order();
                let needs: Vec<usize> = (0..order.len()).filter(|&j| order[j].needs_you()).collect();
                match needs.iter().find(|&&j| j > i).or(needs.first()) {
                    Some(&j) => self.sel = Some(ids[j].clone()),
                    None => self.msg = "Nothing needs you.".into(),
                }
            }
            _ => self.task_key(k.code, ch, term),
        }
        true
    }

    /// Keys about the selected task.
    fn task_key(&mut self, code: KeyCode, ch: Option<char>, term: &mut ratatui::DefaultTerminal) {
        let Some(cur) = self.current().cloned() else { return };
        let sel = cur.id.as_str();
        match (code, ch) {
            (KeyCode::Enter, _) => {
                let reviewer = cur.status == "review"
                    || (cur.status == "needs" && cur.why.as_deref().unwrap_or("").starts_with("Reviewer"));
                self.msg = if reviewer {
                    self.bp(&["go", sel, "--review"]).1
                } else if cur.block.is_some() {
                    self.bp(&["go", sel]).1
                } else {
                    match cur.status.as_str() {
                        "todo" => "Not started yet: s starts it.".into(),
                        "stopped" => "Its agent exited: r resumes it.".into(),
                        _ => format!("{sel} has no pane open."),
                    }
                };
            }
            (_, Some('a')) => {
                if cur.status != "ready" {
                    self.msg = "Only a task that's ready to accept can be accepted.".into();
                } else {
                    self.pending = Some((Action::Accept, sel.into()));
                }
            }
            (_, Some('d')) if !cur.closed() => self.pending = Some((Action::Drop, sel.into())),
            (_, Some('s')) => {
                let (ok, out) = self.slow(term, format!("Starting {sel}…"), |a| a.bp(&["start", sel]));
                self.msg = if ok { format!("Started {sel}.") } else { out };
            }
            (_, Some('r')) => {
                let (ok, out) = self.slow(term, format!("Resuming {sel}…"), |a| a.bp(&["resume", sel]));
                self.msg = if ok { format!("Resumed {sel}.") } else { out };
            }
            (_, Some('p')) => {
                let lines = self.height.saturating_sub(4).max(1).to_string();
                let out = Command::new(&self.bullpen)
                    .args(["peek", sel, "--lines", &lines])
                    .current_dir(&self.root)
                    .stdin(Stdio::null())
                    .output();
                let text = match out {
                    Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
                    Ok(o) => String::from_utf8_lossy(&o.stderr).into_owned(),
                    Err(e) => e.to_string(),
                };
                self.mode = Mode::Overlay {
                    title: format!("{sel}  {}", cur.title),
                    lines: text.lines().map(String::from).collect(),
                };
            }
            (_, Some('t')) => self.mode = Mode::Tell { id: sel.into(), text: String::new() },
            _ => {}
        }
    }
}

fn args() -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let mut it = std::env::args().skip(1);
    let (mut dir, mut root, mut bullpen) = (None, None, None);
    while let Some(a) = it.next() {
        let v = it.next().ok_or(format!("{a} needs a value"))?;
        match a.as_str() {
            "--dir" => dir = Some(PathBuf::from(v)),
            "--root" => root = Some(PathBuf::from(v)),
            "--bullpen" => bullpen = Some(PathBuf::from(v)),
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    match (dir, root, bullpen) {
        (Some(d), Some(r), Some(b)) => Ok((d, r, b)),
        _ => Err("usage: bullpen-board --dir <state dir> --root <repo> --bullpen <program> (`bullpen board` runs it)".into()),
    }
}

fn main() {
    let (dir, root, bullpen) = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("bullpen-board: {e}");
            std::process::exit(2);
        }
    };
    let (tx, rx) = mpsc::channel();

    // redraw when the repo's tasks or config change
    let changed = tx.clone();
    let mut watcher = notify::recommended_watcher(move |r: notify::Result<notify::Event>| {
        if r.is_ok() {
            let _ = changed.send(Msg::Changed);
        }
    })
    .ok();
    if let Some(w) = watcher.as_mut() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = w.watch(&dir, RecursiveMode::Recursive);
    }

    let keys = tx;
    std::thread::spawn(move || loop {
        let m = match event::read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => Msg::Key(k),
            Ok(Event::Resize(..)) => Msg::Redraw,
            Ok(_) => continue,
            Err(_) => return,
        };
        if keys.send(m).is_err() {
            return;
        }
    });

    let repo_name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut app = App {
        cfg: Config::load(&dir),
        tasks: Vec::new(),
        dir,
        root,
        bullpen,
        repo_name,
        sel: None,
        finished: false,
        msg: String::new(),
        top: 0,
        pending: None,
        busy: None,
        mode: Mode::Normal,
        height: 0,
    };

    let mut term = ratatui::init();
    loop {
        app.reload();
        app.settle();
        if term.draw(|f| ui::draw(f, &mut app)).is_err() {
            break;
        }
        // ages move on by the minute: redraw every second anyway
        let m = match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if let Msg::Key(k) = m {
            if !app.key(k, &mut term) {
                break;
            }
        }
        // a burst of file events is one redraw
        while let Ok(m) = rx.try_recv() {
            if let Msg::Key(k) = m {
                if !app.key(k, &mut term) {
                    ratatui::restore();
                    return;
                }
            }
        }
    }
    ratatui::restore();
}
