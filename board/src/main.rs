//! bullpen's task board: a live list of a repo's tasks, grouped by what you do
//! next, from which you open, accept, drop, tell or start them.
//!
//! It reads the repo's state directory (config.json and tasks/*.json) and redraws
//! when anything there changes. Everything it does goes through the `bullpen`
//! command, so the board itself never changes a task. Slow commands (accept, drop,
//! start, resume) run in the background: the board stays usable meanwhile.
//!
//! Usage: bullpen-board --dir <repo state dir> --root <repo> --bullpen <bullpen program>

mod model;
mod ui;

use model::{Action, Config, Task};
use notify::{RecursiveMode, Watcher};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

pub const BOARD_KEYS: &str = "\
Moving
  j k  ↑ ↓     select a task
  n            select the next task that needs you
  ⏎            open the worker's pane (the reviewer's, in review)
  c            back to the coordinator, panes put away

Acting on the selected task
  a            accept: merge its PR (or local branch) and clean up
  d            drop: close its PR, pane and worktree
  t            tell its worker something
  p            peek at the end of its worker's screen (live)
  v            view its diff against its base (q closes it)
  s            start it
  r            resume a stopped worker

The board
  l            switch layout (grouped, compact)
  f            show finished tasks too
  ?            this list
  q            quit the board";

/// How often a peek reads the worker's screen again.
const PEEK_EVERY: Duration = Duration::from_secs(1);

/// What the bottom of the board is doing besides showing keys.
pub enum Mode {
    Normal,
    /// a full-screen text (keys, a peek) until any key; a peek at a task keeps updating
    Overlay { title: String, lines: Vec<String>, peek: Option<String> },
    /// typing a line to tell a task's worker
    Tell { id: String, text: String },
}

/// A slow command, run in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Accept,
    Drop,
    DropForce,
    Start,
    Resume,
}

impl Job {
    fn args(self, id: &str) -> Vec<String> {
        let a: &[&str] = match self {
            Job::Accept => &["accept", id],
            Job::Drop => &["drop", id],
            Job::DropForce => &["drop", id, "--force"],
            Job::Start => &["start", id],
            Job::Resume => &["resume", id],
        };
        a.iter().map(|s| s.to_string()).collect()
    }

    /// While it runs: "Accepting".
    pub fn doing(self) -> &'static str {
        match self {
            Job::Accept => "Accepting",
            Job::Drop | Job::DropForce => "Dropping",
            Job::Start => "Starting",
            Job::Resume => "Resuming",
        }
    }

    /// Once it worked: "Accepted".
    fn done(self) -> &'static str {
        match self {
            Job::Accept => "Accepted",
            Job::Drop | Job::DropForce => "Dropped",
            Job::Start => "Started",
            Job::Resume => "Resumed",
        }
    }
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
    /// slow commands still running, by task, oldest first
    pub running: Vec<(String, Job)>,
    pub mode: Mode,
    /// screen height, for how much a peek shows
    pub height: u16,
    /// when the board started: spinners turn by it
    pub started: Instant,
    tx: Sender<Msg>,
    /// a peek read is in flight
    peeking: bool,
    /// when the peek reads the screen again
    peek_due: Instant,
}

enum Msg {
    Key(KeyEvent),
    Redraw,
    Changed,
    Done { id: String, job: Job, ok: bool, out: String },
    Peeked { id: String, text: String },
}

/// Run `bullpen <args>`: did it work, and what it said (stdout, else stderr).
fn run(bullpen: &Path, root: &Path, args: &[String]) -> (bool, String) {
    let out = Command::new(bullpen)
        .args(args)
        .current_dir(root)
        // never let it read our terminal: it would wait there for keys meant for the board
        .stdin(Stdio::null())
        .output();
    match out {
        Ok(o) => {
            let text = if o.status.success() { o.stdout } else { o.stderr };
            (o.status.success(), String::from_utf8_lossy(&text).into_owned())
        }
        Err(e) => (false, format!("can't run {}: {e}", bullpen.display())),
    }
}

/// The last line a command said, without its "bullpen: ".
fn last_line(text: &str) -> String {
    let last = text.trim().lines().last().unwrap_or("");
    last.strip_prefix("bullpen: ").unwrap_or(last).to_string()
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

    /// What's running for a task, if anything.
    pub fn job(&self, id: &str) -> Option<Job> {
        self.running.iter().find(|(x, _)| x == id).map(|(_, j)| *j)
    }

    /// Keep the selection on a task that's still shown.
    fn settle(&mut self) {
        let first = self.order().first().map(|t| t.id.clone());
        if !self.order().iter().any(|t| Some(&t.id) == self.sel.as_ref()) {
            self.sel = first;
        }
    }

    /// Run `bullpen <args>` and wait: for quick commands.
    fn bp(&self, args: &[&str]) -> (bool, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (ok, out) = run(&self.bullpen, &self.root, &args);
        (ok, last_line(&out))
    }

    fn still(&mut self, id: &str) -> bool {
        match self.job(id) {
            Some(j) => {
                self.msg = format!("{id} is still {}.", j.doing().to_lowercase());
                true
            }
            None => false,
        }
    }

    /// Start a slow command in the background; its result comes back as Msg::Done.
    fn spawn(&mut self, id: &str, job: Job) {
        if self.still(id) {
            return;
        }
        self.running.push((id.to_string(), job));
        let (bullpen, root, tx, id) = (self.bullpen.clone(), self.root.clone(), self.tx.clone(), id.to_string());
        std::thread::spawn(move || {
            let (ok, out) = run(&bullpen, &root, &job.args(&id));
            let _ = tx.send(Msg::Done { id, job, ok, out: last_line(&out) });
        });
    }

    fn done(&mut self, id: String, job: Job, ok: bool, out: String) {
        self.running.retain(|(x, _)| *x != id);
        if job == Job::Drop && !ok && out.contains("uncommitted") && self.pending.is_none() {
            // ask again on its row, about losing the changes this time
            self.sel = Some(id.clone());
            self.pending = Some((Action::DropForce, id));
            self.msg.clear();
        } else {
            self.msg = if ok { format!("{} {id}.", job.done()) } else { out };
        }
    }

    /// Read a task's screen again in the background, for a peek that's open.
    fn peek(&mut self) {
        let Mode::Overlay { peek: Some(id), .. } = &self.mode else { return };
        if self.peeking || Instant::now() < self.peek_due {
            return;
        }
        self.peeking = true;
        let lines = self.height.saturating_sub(4).max(1).to_string();
        let args = vec!["peek".into(), id.clone(), "--lines".into(), lines, "--vt".into()];
        let (bullpen, root, tx, id) = (self.bullpen.clone(), self.root.clone(), self.tx.clone(), id.clone());
        std::thread::spawn(move || {
            let (_, text) = run(&bullpen, &root, &args);
            let _ = tx.send(Msg::Peeked { id, text });
        });
    }

    fn peeked(&mut self, id: String, text: String) {
        self.peeking = false;
        self.peek_due = Instant::now() + PEEK_EVERY;
        if let Mode::Overlay { peek: Some(p), lines, .. } = &mut self.mode {
            if *p == id {
                *lines = text.lines().map(String::from).collect();
            }
        }
    }

    /// Handle a key. False when the board should quit.
    fn key(&mut self, k: KeyEvent) -> bool {
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
            let job = match action {
                Action::Accept => Job::Accept,
                Action::Drop => Job::Drop,
                Action::DropForce => Job::DropForce,
            };
            self.spawn(&tid, job);
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
                    peek: None,
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
            (_, Some('c')) => {
                let (ok, out) = self.bp(&["go"]);
                self.msg = if ok { "Opened the coordinator.".into() } else { out };
            }
            (_, Some('n')) => {
                let order = self.order();
                let needs: Vec<usize> = (0..order.len()).filter(|&j| order[j].needs_you()).collect();
                match needs.iter().find(|&&j| j > i).or(needs.first()) {
                    Some(&j) => self.sel = Some(ids[j].clone()),
                    None => self.msg = "Nothing needs you.".into(),
                }
            }
            _ => self.task_key(k.code, ch),
        }
        true
    }

    /// Keys about the selected task.
    fn task_key(&mut self, code: KeyCode, ch: Option<char>) {
        let Some(cur) = self.current().cloned() else { return };
        let sel = cur.id.as_str();
        match (code, ch) {
            (KeyCode::Enter, _) => {
                let reviewer = cur.status == "review"
                    || (cur.status == "needs" && cur.why.as_deref().unwrap_or("").starts_with("Reviewer"));
                // a pane that opens in another tab shows nothing here: say it worked
                self.msg = if reviewer {
                    let (ok, out) = self.bp(&["go", sel, "--review"]);
                    if ok { format!("Opened {sel}'s reviewer.") } else { out }
                } else if cur.block.is_some() {
                    let (ok, out) = self.bp(&["go", sel]);
                    if ok { format!("Opened {sel}.") } else { out }
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
                } else if !self.still(sel) {
                    self.pending = Some((Action::Accept, sel.into()));
                }
            }
            (_, Some('d')) if !cur.closed() => {
                if !self.still(sel) {
                    self.pending = Some((Action::Drop, sel.into()));
                }
            }
            (_, Some('s')) => self.spawn(sel, Job::Start),
            (_, Some('r')) => self.spawn(sel, Job::Resume),
            (_, Some('p')) => {
                self.mode = Mode::Overlay {
                    title: format!("{sel}  {}", cur.title),
                    lines: vec!["…".into()],
                    peek: Some(sel.into()),
                };
                self.peek_due = Instant::now();
                self.peek();
            }
            (_, Some('v')) => self.msg = self.bp(&["diff", sel, "--pane"]).1,
            (_, Some('t')) => self.mode = Mode::Tell { id: sel.into(), text: String::new() },
            _ => {}
        }
    }

    /// How long the loop may wait for something to happen before redrawing.
    fn tick(&self) -> Duration {
        if !self.running.is_empty() {
            Duration::from_millis(100) // spinners turn
        } else if matches!(self.mode, Mode::Overlay { peek: Some(_), .. })
            || self.tasks.iter().any(|t| t.status == "working")
        {
            Duration::from_millis(200) // a peek reads again; working marks move
        } else {
            Duration::from_secs(1) // ages move on by the minute
        }
    }

    /// Handle one message. False when the board should quit.
    fn handle(&mut self, m: Msg) -> bool {
        match m {
            Msg::Key(k) => return self.key(k),
            Msg::Done { id, job, ok, out } => self.done(id, job, ok, out),
            Msg::Peeked { id, text } => self.peeked(id, text),
            Msg::Redraw | Msg::Changed => {}
        }
        true
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

    let keys = tx.clone();
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
        running: Vec::new(),
        mode: Mode::Normal,
        height: 0,
        started: Instant::now(),
        tx,
        peeking: false,
        peek_due: Instant::now(),
    };

    let mut term = ratatui::init();
    'main: loop {
        app.reload();
        app.settle();
        app.peek();
        if term.draw(|f| ui::draw(f, &mut app)).is_err() {
            break;
        }
        let m = match rx.recv_timeout(app.tick()) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if !app.handle(m) {
            break;
        }
        // a burst of file events is one redraw
        while let Ok(m) = rx.try_recv() {
            if !app.handle(m) {
                break 'main;
            }
        }
    }
    // Let a command that's still running finish: it writes to the board as it goes.
    while !app.running.is_empty() {
        app.msg = format!("Waiting for {} to finish…", app.running[0].0);
        let _ = term.draw(|f| ui::draw(f, &mut app));
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Msg::Done { id, .. }) => app.running.retain(|(x, _)| *x != id),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            _ => {}
        }
    }
    ratatui::restore();
}
