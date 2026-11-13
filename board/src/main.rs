//! bullpen's task board: a live list of a repo's tasks, grouped by what you do
//! next, from which you open, accept, drop, tell or start them.
//!
//! It reads the repo's state directory (config.json and tasks/*.json) and redraws
//! when anything there changes. Everything it does goes through the `bullpen`
//! command, so the board itself never changes a task. Slow commands (accept, drop,
//! start, resume) run in the background: the board stays usable meanwhile.
//!
//! When its own program is replaced (bullpen rebuilds the board when its source
//! changes), it execs the new one in place, keeping its selection and `f`.
//!
//! Usage: bullpen-board --dir <repo state dir> --root <repo> --bullpen <bullpen program>

mod model;
mod ui;

use model::{Action, Config, Layout, Prompt, Question, Task, TourShow};
use notify::{RecursiveMode, Watcher};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

/// The `?` list: a heading, then one short line per key. The board wraps it to its
/// width; the long explanations are in the README.
pub const BOARD_KEYS: &[(&str, &[(&str, &str)])] = &[
    ("Moving", &[
        ("j k", "select a task (↑ ↓ too)"),
        ("PgDn", "a page down (ctrl+d: half)"),
        ("PgUp", "a page up (ctrl+u: half)"),
        ("G", "the last task (End; Home: first)"),
        ("n", "next that needs you, else ready"),
        ("⏎", "open its pane, or its reviewer's"),
        ("c", "back to the coordinator"),
        ("/", "pick any open task, anywhere"),
    ]),
    ("The selected task", &[
        ("a", "accept and merge, or close"),
        ("d", "drop it"),
        ("i", "answer its question"),
        ("t", "tell its worker something"),
        ("p", "peek at its worker's screen"),
        ("v", "view its diff"),
        ("g", "guided tour of its change"),
        ("s", "start it"),
        ("r", "resume a stopped worker"),
    ]),
    ("The board", &[
        ("l", "switch layout"),
        ("f", "show closed tasks too"),
        ("o", "other coordinators' tasks too"),
        ("esc", "put a red message away"),
        ("?", "this list"),
        ("q", "quit the board (y to be sure)"),
    ]),
    ("In a tour", &[
        ("n p", "next, previous step"),
        ("j k", "scroll (space b too)"),
        ("c", "comment on the step"),
        ("s", "at the end: send the comments"),
        ("a", "accept (before the end: a twice)"),
        ("t", "tell its worker something"),
        ("⏎ v", "close it, open its pane or diff"),
        ("esc", "close it"),
    ]),
];

/// How often a peek reads the worker's screen again.
const PEEK_EVERY: Duration = Duration::from_secs(1);

/// What the bottom of the board is doing besides showing keys.
pub enum Mode {
    Normal,
    /// a full-screen text until any key; a peek at a task keeps updating until esc or
    /// q, and t ⏎ i a d v go back to the board and act on the task
    Overlay { title: String, lines: Vec<String>, peek: Option<String> },
    /// the `?` list, scrolled to its top line; scroll keys scroll, any other key closes
    Help { top: usize },
    /// typing a line to tell a task's worker, or with reply, to answer its coordinator's question
    Tell { id: String, text: String, reply: bool },
    /// the prompt a task's agent is at, read again every second (None until first read),
    /// until an option is pressed or esc
    Answer { id: String, title: String, prompt: Option<Prompt> },
    /// g on a task whose tour is stale: n writes a new one, o opens the old one
    Stale { id: String },
    /// q: y quits the board, any other key stays
    Quit,
}

/// A slow command, run in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Accept,
    Close,
    Drop,
    DropForce,
    Start,
    Resume,
}

impl Job {
    fn args(self, id: &str) -> Vec<String> {
        let a: &[&str] = match self {
            Job::Accept => &["accept", id],
            Job::Close => &["accept", id], // accept closes a finished task
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
            Job::Close => "Closing",
            Job::Drop | Job::DropForce => "Dropping",
            Job::Start => "Starting",
            Job::Resume => "Resuming",
        }
    }

    /// Once it worked: "Accepted".
    fn done(self) -> &'static str {
        match self {
            Job::Accept => "Accepted",
            Job::Close => "Closed",
            Job::Drop | Job::DropForce => "Dropped",
            Job::Start => "Started",
            Job::Resume => "Resumed",
        }
    }
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
}

/// What tells one program file from another: a rebuild renames a new file into place,
/// so its inode (and size or mtime) changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExeId {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: (i64, i64),
}

impl ExeId {
    /// The file at path, if it's a whole program (executable, not empty).
    pub fn of(path: &Path) -> Option<ExeId> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let m = std::fs::metadata(path).ok()?;
        if !m.is_file() || m.len() == 0 || m.permissions().mode() & 0o111 == 0 {
            return None;
        }
        Some(ExeId { dev: m.dev(), ino: m.ino(), len: m.len(), mtime: (m.mtime(), m.mtime_nsec()) })
    }
}

enum Msg {
    Key(KeyEvent),
    Redraw,
    Changed,
    Done { id: String, job: Job, ok: bool, out: String },
    Peeked { id: String, text: String, ok: bool },
    Prompted { id: String, prompt: Option<Prompt>, out: String },
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

/// An accept that changed bullpen's keys.lua ends by saying so: that, not "Accepted",
/// is the line to show, since Rex runs the old keys until its config is reloaded.
fn keys_changed(job: Job, out: &str) -> bool {
    job == Job::Accept && out.starts_with("Keys changed")
}

/// Does this key do something to a task (or leave the board for the coordinator)?
/// Esc counts too: it puts a failure away.
fn acts_on_task(code: KeyCode, ch: Option<char>) -> bool {
    matches!(code, KeyCode::Enter | KeyCode::Esc) || ch.is_some_and(|c| "adsrpvtigc".contains(c))
}

/// A key in a peek: None stays in it, Some(false) goes back to the board, Some(true)
/// goes back and does what the key does there.
fn peek_key(code: KeyCode) -> Option<bool> {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => Some(false),
        KeyCode::Enter | KeyCode::Char('t' | 'i' | 'a' | 'd' | 'v') => Some(true),
        _ => None,
    }
}

/// What a command failed with, as a sentence: "BP-3's agent isn't running."
fn said(line: &str) -> String {
    match line.chars().last() {
        Some('.' | '!' | '?') | None => line.to_string(),
        _ => format!("{line}."),
    }
}

/// The last line a command said, without its "bullpen: ".
fn last_line(text: &str) -> String {
    let last = text.trim().lines().last().unwrap_or("");
    last.strip_prefix("bullpen: ").unwrap_or(last).to_string()
}

impl App {
    /// The program to exec in place of this one, when the board's program was replaced
    /// and nothing would be lost: no command running, no y/n asked, nothing being typed
    /// or answered. (An overlay just closes.)
    pub fn replaced(&self) -> Option<PathBuf> {
        let (path, was) = self.exe.as_ref()?;
        let busy = !self.running.is_empty()
            || self.pending.is_some()
            || matches!(self.mode, Mode::Tell { .. } | Mode::Answer { .. } | Mode::Stale { .. } | Mode::Quit);
        match ExeId::of(path) {
            Some(now) if now != *was && !busy => Some(path.clone()),
            _ => None,
        }
    }

    /// The arguments the new program starts with: these, plus what the board shows
    /// that isn't saved anywhere else (the selection and `f`; `l` and `o` are settings).
    pub fn restart_args(&self) -> Vec<String> {
        let mut a: Vec<String> = vec![
            "--dir".into(),
            self.dir.display().to_string(),
            "--root".into(),
            self.root.display().to_string(),
            "--bullpen".into(),
            self.bullpen.display().to_string(),
        ];
        if let Some(me) = &self.me {
            a.extend(["--coordinator".into(), me.clone()]);
        }
        if let Some(sel) = &self.sel {
            a.extend(["--select".into(), sel.clone()]);
        }
        if self.closed {
            a.extend(["--closed".into(), "on".into()]);
        }
        a.extend(["--reloaded".into(), "on".into()]);
        a
    }

    fn reload(&mut self) {
        self.cfg = Config::load(&self.dir);
        self.tasks = model::load_tasks(&self.dir);
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
                self.msg = Note::info(format!("{id} is still {}.", j.doing().to_lowercase()));
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

    fn done(&mut self, id: String, job: Job, ok: bool, out: String) {
        self.running.retain(|(x, _)| *x != id);
        // accepted or closed: on to the next task that needs you, to run down a queue
        // (unless the selection already moved off it)
        if ok && matches!(job, Job::Accept | Job::Close) && self.sel.as_deref() == Some(id.as_str()) {
            let i = self.order().iter().position(|t| t.id == id).unwrap_or(0);
            if let Some((next, _)) = self.next_up(i, Some(&id)) {
                self.sel = Some(next);
            }
        }
        if job == Job::Drop && !ok && out.contains("uncommitted") && self.pending.is_none() {
            // ask again on its row, about losing the changes this time
            self.sel = Some(id.clone());
            self.pending = Some((Action::DropForce, id));
            self.msg.clear();
        } else {
            self.msg = if keys_changed(job, &out) { Note::done(out) } else { Note::of(ok, format!("{} {id}.", job.done()), out) };
        }
    }

    /// Read a task's screen again in the background, for a peek or an answer that's open.
    fn peek(&mut self) {
        let (id, answer) = match &self.mode {
            Mode::Overlay { peek: Some(id), .. } => (id.clone(), false),
            Mode::Answer { id, .. } => (id.clone(), true),
            _ => return,
        };
        if self.peeking || Instant::now() < self.peek_due {
            return;
        }
        self.peeking = true;
        let (bullpen, root, tx) = (self.bullpen.clone(), self.root.clone(), self.tx.clone());
        if answer {
            let lines = self.height.saturating_sub(6).max(1).to_string();
            let args = vec!["prompt".into(), id.clone(), "--lines".into(), lines];
            std::thread::spawn(move || {
                let (ok, out) = run(&bullpen, &root, &args);
                let prompt = if ok { Prompt::parse(&out) } else { None };
                let _ = tx.send(Msg::Prompted { id, prompt, out: last_line(&out) });
            });
            return;
        }
        let lines = self.height.saturating_sub(4).max(1).to_string();
        let args = vec!["peek".into(), id.clone(), "--lines".into(), lines, "--vt".into()];
        std::thread::spawn(move || {
            let (ok, text) = run(&bullpen, &root, &args);
            // a screen that can't be read: why, in the board's words
            let text = if ok { text } else { said(&last_line(&text)) };
            let _ = tx.send(Msg::Peeked { id, text, ok });
        });
    }

    fn peeked(&mut self, id: String, text: String, ok: bool) {
        self.peeking = false;
        self.peek_due = Instant::now() + PEEK_EVERY;
        if let Mode::Overlay { peek: Some(p), lines, .. } = &mut self.mode {
            if *p == id {
                *lines = text.lines().map(String::from).collect();
                self.peek_failed = !ok;
            }
        }
    }

    /// A prompt read for the answer that's open. One that can't be read (its agent
    /// went on, its pane is gone) closes it, saying why.
    fn prompted(&mut self, id: String, prompt: Option<Prompt>, out: String) {
        self.peeking = false;
        self.peek_due = Instant::now() + PEEK_EVERY;
        let Mode::Answer { id: open, prompt: shown, .. } = &mut self.mode else { return };
        if *open != id {
            return;
        }
        match prompt {
            Some(p) => *shown = Some(p),
            None => {
                self.mode = Mode::Normal;
                self.msg = Note::failed(out);
            }
        }
    }

    /// Open what `i` answers on the task: its agent's prompt, or a line for the
    /// coordinator's question.
    fn answer(&mut self, t: &Task) {
        match t.question() {
            Some(Question::Prompt) => {
                self.mode = Mode::Answer { id: t.id.clone(), title: format!("{}  {}", t.id, t.title), prompt: None };
                self.peek_due = Instant::now();
                self.peek();
            }
            Some(Question::Ask) => self.mode = Mode::Tell { id: t.id.clone(), text: String::new(), reply: true },
            None if t.needs_you() => self.msg = Note::info(format!("{} asks nothing here: ⏎ opens it.", t.id)),
            None => self.msg = Note::info(format!("{} doesn't need you.", t.id)),
        }
    }

    /// A key while a prompt is shown: an option's key presses it (if the prompt is
    /// still on its screen), ⏎ opens the pane, esc or q goes back.
    fn answer_key(&mut self, code: KeyCode) {
        let Mode::Answer { id, prompt, .. } = &self.mode else { return };
        let (id, prompt) = (id.clone(), prompt.clone());
        let reviewer = prompt.as_ref().is_some_and(|p| p.who == "reviewer");
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                let (ok, out) = if reviewer { self.bp(&["go", &id, "--review"]) } else { self.bp(&["go", &id]) };
                self.msg = Note::of(ok, format!("Opened {id}."), out);
            }
            KeyCode::Char(c) => {
                let Some(p) = prompt else { return };
                let (Some((key, label)), Some(pid)) = (p.option(c), p.id.as_deref()) else { return };
                self.mode = Mode::Normal;
                let (ok, out) = self.bp(&["answer", &id, key, "--prompt", pid]);
                self.msg = Note::of(ok, format!("Answered {id}: {label}."), out);
            }
            _ => {}
        }
    }

    /// g: the task's guided tour. A current one opens; with none, one is written and
    /// opens when it's done; a stale one asks first.
    fn tour(&mut self, t: &Task) {
        if !t.tourable() {
            self.msg = Note::info(match t.status.as_str() {
                "finished" => format!("{} has no code to tour.", t.id),
                _ => format!("{} gets a tour once it's in review.", t.id),
            });
            return;
        }
        if !t.tour_writing() {
            let (bullpen, root) = (self.bullpen.clone(), self.root.clone());
            let (ok, out) = run(&bullpen, &root, &["tour".into(), t.id.clone(), "--show".into(), "--json".into()]);
            if !ok {
                self.msg = Note::failed(last_line(&out));
                return;
            }
            if TourShow::parse(&out).is_some_and(|s| s.state == "stale") {
                self.mode = Mode::Stale { id: t.id.clone() };
                self.msg = Note::info(format!("{}'s tour is stale: the code changed.", t.id));
                return;
            }
        }
        self.open_tour(&t.id, false);
    }

    /// `bullpen tour <id> --pane`: open the stepper, or start writing a tour (new) that opens when written.
    fn open_tour(&mut self, id: &str, new: bool) {
        let mut args = vec!["tour", id, "--pane"];
        if new {
            args.push("--new");
        }
        let (ok, out) = self.bp(&args);
        self.msg = Note::of(ok, format!("{}.", model::capitalize(&out)), out);
    }

    /// A key while g asks about a stale tour.
    fn stale_key(&mut self, ch: Option<char>) {
        let Mode::Stale { id } = &self.mode else { return };
        let id = id.clone();
        self.mode = Mode::Normal;
        match ch {
            Some('n') => self.open_tour(&id, true),
            Some('o') => self.open_tour(&id, false),
            _ => self.msg = Note::info("Cancelled."),
        }
    }

    /// Handle a key. False when the board should quit.
    fn key(&mut self, k: KeyEvent) -> bool {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return false;
        }
        match &mut self.mode {
            Mode::Overlay { peek: None, .. } => {
                self.mode = Mode::Normal;
                return true;
            }
            Mode::Overlay { peek: Some(id), .. } => {
                let id = id.clone();
                if let Some(back) = peek_key(k.code) {
                    self.mode = Mode::Normal;
                    if !back {
                        return true;
                    }
                    // back on the board with the peeked task selected, the key acts on it
                    self.sel = Some(id);
                } else {
                    return true;
                }
            }
            Mode::Quit => {
                if matches!(k.code, KeyCode::Char('y' | 'Y')) {
                    return false;
                }
                self.mode = Mode::Normal;
                self.msg = Note::info("Cancelled.");
                return true;
            }
            Mode::Help { top } => {
                // the draw keeps it in range: it knows how many lines the list wraps to
                let page = usize::from(self.height.saturating_sub(4)).max(1);
                match k.code {
                    KeyCode::Down | KeyCode::Char('j') => *top += 1,
                    KeyCode::Up | KeyCode::Char('k') => *top = top.saturating_sub(1),
                    KeyCode::PageDown | KeyCode::Char(' ') => *top += page,
                    KeyCode::PageUp | KeyCode::Char('b') => *top = top.saturating_sub(page),
                    _ => self.mode = Mode::Normal,
                }
                return true;
            }
            Mode::Answer { .. } => {
                self.answer_key(k.code);
                return true;
            }
            Mode::Stale { .. } => {
                self.stale_key(if let KeyCode::Char(c) = k.code { Some(c) } else { None });
                return true;
            }
            Mode::Tell { id, text, reply } => {
                match k.code {
                    KeyCode::Enter => {
                        let (id, text, reply) = (id.clone(), text.trim().to_string(), *reply);
                        self.mode = Mode::Normal;
                        if !text.is_empty() {
                            let (ok, out) = self.bp(&[if reply { "reply" } else { "tell" }, &id, &text]);
                            let sent = if reply { format!("Answer sent to {id}'s coordinator.") } else { format!("Sent to {id}.") };
                            self.msg = Note::of(ok, sent, out);
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
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // a failure stays while you look around, until the next action on a task (or esc)
        if self.msg.kind != Kind::Failed || self.pending.is_some() || acts_on_task(k.code, ch.filter(|_| !ctrl)) {
            self.msg.clear();
        }

        if let Some((action, tid)) = self.pending.take() {
            if !matches!(ch, Some('y' | 'Y')) {
                self.msg = Note::info("Cancelled.");
                return true;
            }
            let job = match action {
                Action::Accept => Job::Accept,
                Action::Close => Job::Close,
                Action::Drop => Job::Drop,
                Action::DropForce => Job::DropForce,
            };
            self.spawn(&tid, job);
            return true;
        }

        let ids: Vec<String> = self.order().iter().map(|t| t.id.clone()).collect();
        let i = ids.iter().position(|x| Some(x) == self.sel.as_ref()).unwrap_or(0);
        // the selection moves; the list scrolls to keep it in view
        let page = self.page.max(1);
        let to = |n: usize| ids.get(n.min(ids.len().saturating_sub(1))).cloned();
        match (k.code, ch) {
            // before the letters: ctrl+d isn't d (drop)
            (_, Some('d')) if ctrl => self.sel = to(i + (page / 2).max(1)),
            (_, Some('u')) if ctrl => self.sel = to(i.saturating_sub((page / 2).max(1))),
            (_, Some(_)) if ctrl => {}
            (_, Some('q')) => self.mode = Mode::Quit,
            (KeyCode::Down, _) | (_, Some('j')) => self.sel = to(i + 1),
            (KeyCode::Up, _) | (_, Some('k')) => self.sel = to(i.saturating_sub(1)),
            (KeyCode::PageDown, _) => self.sel = to(i + page),
            (KeyCode::PageUp, _) => self.sel = to(i.saturating_sub(page)),
            (KeyCode::End, _) | (_, Some('G')) => self.sel = to(usize::MAX),
            (KeyCode::Home, _) => self.sel = to(0),
            (_, Some('/')) => {
                // a floating pane over this window; it goes where you choose, or esc
                let (ok, out) = self.bp(&["pick", "--pane"]);
                if !ok {
                    self.msg = Note::failed(out);
                }
            }
            (_, Some('?')) => {
                self.mode = Mode::Help { top: 0 }
            }
            (_, Some('l')) => {
                let layout = self.cfg.layout.other();
                self.cfg.layout = layout;
                self.top = 0;
                let (ok, out) = self.bp(&["config", "--board-layout", layout.name()]);
                let said = match layout {
                    Layout::Grouped => "Grouped layout.",
                    Layout::Compact => "Compact layout.",
                };
                self.msg = if ok { Note::info(said) } else { Note::failed(out) };
            }
            (_, Some('f')) => {
                self.closed = !self.closed;
                self.top = 0;
                // a failure stays while you look around
                if self.msg.kind != Kind::Failed {
                    self.msg = Note::info(if self.closed { "Showing closed tasks too." } else { "Hiding closed tasks." });
                }
            }
            (_, Some('o')) => self.toggle_others(),
            (_, Some('c')) => {
                let (ok, out) = self.bp(&["go"]);
                self.msg = Note::of(ok, "Opened the coordinator.", out);
            }
            (_, Some('n')) => match self.next_up(i, None) {
                Some((id, true)) => self.sel = Some(id),
                Some((id, false)) => {
                    let st = self.tasks.iter().find(|t| t.id == id).map_or("", |t| t.status.as_str());
                    self.msg = Note::info(format!("Nothing needs you: {id} is {}.", if st == "ready" { "ready" } else { "finished" }));
                    self.sel = Some(id);
                }
                None => self.msg = Note::info("Nothing needs you."),
            },
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
                let reviewer = cur.status == "review" || cur.on_reviewer();
                // a pane that opens in another tab shows nothing here: say it worked
                self.msg = if reviewer {
                    let (ok, out) = self.bp(&["go", sel, "--review"]);
                    Note::of(ok, format!("Opened {sel}'s reviewer."), out)
                } else if cur.block.is_some() {
                    let (ok, out) = self.bp(&["go", sel]);
                    Note::of(ok, format!("Opened {sel}."), out)
                } else {
                    Note::info(match cur.status.as_str() {
                        "todo" => "Not started yet: s starts it.".into(),
                        "stopped" => "Its agent exited: r resumes it.".into(),
                        _ => format!("{sel} has no pane open."),
                    })
                };
            }
            (_, Some('a')) => {
                let action = match cur.status.as_str() {
                    "ready" => Action::Accept,
                    "finished" => Action::Close,
                    _ => {
                        self.msg = Note::info(if cur.closed() { format!("{sel} is closed already.") } else { format!("{sel} isn't ready yet.") });
                        return;
                    }
                };
                if !self.still(sel) {
                    self.pending = Some((action, sel.into()));
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
                self.peek_failed = false;
                self.peek_due = Instant::now();
                self.peek();
            }
            (_, Some('v')) => {
                let (ok, out) = self.bp(&["diff", sel, "--pane"]);
                self.msg = Note::of(ok, format!("Opened {sel}'s diff."), out);
            }
            (_, Some('t')) => self.mode = Mode::Tell { id: sel.into(), text: String::new(), reply: false },
            (_, Some('i')) => self.answer(&cur),
            (_, Some('g')) => self.tour(&cur),
            _ => {}
        }
    }

    /// How long the loop may wait for something to happen before redrawing.
    fn tick(&self) -> Duration {
        let moving = |t: &Task| matches!(t.status.as_str(), "working" | "review");
        if !self.running.is_empty() || self.tasks.iter().any(|t| t.tour_writing()) {
            Duration::from_millis(50) // a command's spinner turns fast
        } else if self.tasks.iter().any(moving) {
            Duration::from_millis(100) // the marks of working tasks turn twice a second
        } else if matches!(self.mode, Mode::Overlay { peek: Some(_), .. } | Mode::Answer { .. }) {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(1) // ages move on by the minute
        }
    }

    /// Handle one message. False when the board should quit.
    fn handle(&mut self, m: Msg) -> bool {
        match m {
            Msg::Key(k) => return self.key(k),
            Msg::Done { id, job, ok, out } => self.done(id, job, ok, out),
            Msg::Peeked { id, text, ok } => self.peeked(id, text, ok),
            Msg::Prompted { id, prompt, out } => self.prompted(id, prompt, out),
            Msg::Redraw | Msg::Changed => {}
        }
        true
    }
}

/// Report this program's status to the terminal (the Program Status Protocol).
fn status(report: &str) {
    use std::io::Write;
    print!("\x1b]7501;{report}\x1b\\");
    let _ = std::io::stdout().flush();
}

/// The command line: where to look, and (from a reload) what the board showed.
#[derive(Debug, Default, PartialEq)]
pub struct Args {
    dir: PathBuf,
    root: PathBuf,
    bullpen: PathBuf,
    me: Option<String>,
    select: Option<String>,
    closed: bool,
    reloaded: bool,
}

fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut it = argv.into_iter();
    let (mut dir, mut root, mut bullpen) = (None, None, None);
    let mut out = Args::default();
    while let Some(a) = it.next() {
        let v = it.next().ok_or(format!("{a} needs a value"))?;
        match a.as_str() {
            "--dir" => dir = Some(PathBuf::from(v)),
            "--root" => root = Some(PathBuf::from(v)),
            "--bullpen" => bullpen = Some(PathBuf::from(v)),
            "--coordinator" => out.me = Some(v),
            "--select" => out.select = Some(v),
            // --finished: what a board from before it was renamed restarts with
            "--closed" | "--finished" => out.closed = v == "on",
            "--reloaded" => out.reloaded = v == "on",
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    match (dir, root, bullpen) {
        (Some(d), Some(r), Some(b)) => Ok(Args { dir: d, root: r, bullpen: b, ..out }),
        _ => Err("usage: bullpen-board --dir <state dir> --root <repo> --bullpen <program> [--coordinator <id>] \
                  (`bullpen board` runs it)"
            .into()),
    }
}

fn main() {
    let Args { dir, root, bullpen, me, select, closed, reloaded } = match parse_args(std::env::args().skip(1)) {
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
    // The program as `bullpen board` named it: a rebuild replaces that file.
    let exe = std::env::args_os().next().map(PathBuf::from).filter(|p| p.is_absolute());
    let mut app = App {
        cfg: Config::load(&dir),
        tasks: Vec::new(),
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
    };

    // Rex picks a pane's icon by the program in it and doesn't know this one. A
    // program can say what it is (OSC 7501's app): "rust" gets Rust's icon.
    // bullpen ignores status from the board's pane, and other terminals ignore this.
    status("state=idle:app=rust");
    let mut term = ratatui::init();
    // Read the tasks again when their files change, and once a second in case a
    // change was missed; frames drawn in between only move the animations.
    let (mut stale, mut read) = (true, Instant::now());
    'main: loop {
        if stale || read.elapsed() >= Duration::from_secs(1) {
            app.reload();
            (stale, read) = (false, Instant::now());
            if let Some(exe) = app.replaced() {
                use std::os::unix::process::CommandExt;
                ratatui::restore();
                let err = Command::new(&exe).args(app.restart_args()).exec(); // returns only if it failed
                term = ratatui::init();
                app.exe = None; // don't try again
                app.msg = Note::failed(format!("Couldn't reload the rebuilt board: {err}"));
            }
        }
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
        stale = true; // a key may have changed a task, and a file event says one did
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
        app.msg = Note::info(format!("Waiting for {} to finish…", app.running[0].0));
        let _ = term.draw(|f| ui::draw(f, &mut app));
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Msg::Done { id, .. }) => app.running.retain(|(x, _)| *x != id),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            _ => {}
        }
    }
    ratatui::restore();
    status("state=clear");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(tasks: Vec<Task>) -> App {
        let (tx, _rx) = mpsc::channel();
        let dir = PathBuf::from("/nonexistent/bullpen-board-test");
        App {
            cfg: Config::load(&dir),
            tasks,
            others: Default::default(),
            root: dir.clone(),
            bullpen: dir.join("no-bullpen"),
            dir,
            me: None,
            repo_name: "repo".into(),
            sel: None,
            closed: false,
            msg: Note::default(),
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
            exe: None,
        }
    }

    fn task(id: &str, status: &str) -> Task {
        Task { id: id.into(), status: status.into(), ..Default::default() }
    }

    fn press(a: &mut App, c: char) {
        a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }

    fn owned(id: &str, status: &str, c: &str) -> Task {
        Task { coordinator: Some(c.into()), ..task(id, status) }
    }

    #[test]
    fn selection_and_n_skip_the_other_coordinators_tasks() {
        let mut a = app(vec![owned("BP-1", "needs", "c1"), owned("BP-2", "needs", "c2"), owned("BP-3", "needs", "c1")]);
        a.cfg.coordinators = vec!["c1".into(), "c2".into()];
        a.me = Some("c1".into());
        a.sel = Some("BP-2".into());
        a.settle();
        assert_eq!(a.sel.as_deref(), Some("BP-1")); // BP-2 isn't shown: off it
        press(&mut a, 'j');
        assert_eq!(a.sel.as_deref(), Some("BP-3"));
        press(&mut a, 'n');
        assert_eq!(a.sel.as_deref(), Some("BP-1"));
        press(&mut a, 'n');
        assert_eq!(a.sel.as_deref(), Some("BP-3"));
        // o shows them: BP-2 comes between (the setting can't be saved here; it still toggles)
        press(&mut a, 'o');
        assert!(a.cfg.shows_others(Some("c1")));
        assert_eq!(a.order().iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["BP-1", "BP-2", "BP-3"]);
        press(&mut a, 'k');
        assert_eq!(a.sel.as_deref(), Some("BP-2"));
        // and o again hides them
        press(&mut a, 'o');
        assert!(!a.cfg.shows_others(Some("c1")));
        a.settle();
        assert_eq!(a.sel.as_deref(), Some("BP-1"));
    }

    #[test]
    fn o_with_one_coordinator_says_so() {
        let mut a = app(vec![owned("BP-1", "needs", "c1")]);
        a.cfg.coordinators = vec!["c1".into()];
        a.me = Some("c1".into());
        press(&mut a, 'o');
        assert!(a.cfg.board_others.is_empty());
        assert_eq!(a.msg, "One coordinator: every task is shown.");
    }

    #[test]
    fn a_closes_a_finished_task_after_y() {
        let mut a = app(vec![task("BP-1", "finished")]);
        a.settle();
        press(&mut a, 'a');
        assert_eq!(a.pending, Some((Action::Close, "BP-1".into())));
        press(&mut a, 'y');
        assert_eq!(a.job("BP-1"), Some(Job::Close));
        assert_eq!(Job::Close.args("BP-1"), vec!["accept", "BP-1"]);
        // n cancels
        let mut a = app(vec![task("BP-1", "finished")]);
        a.settle();
        press(&mut a, 'a');
        press(&mut a, 'n');
        assert!(a.running.is_empty() && a.pending.is_none());
        assert_eq!(a.msg, "Cancelled.");
    }

    #[test]
    fn a_accepts_a_ready_task_after_y() {
        let mut a = app(vec![task("BP-1", "ready")]);
        a.settle();
        press(&mut a, 'a');
        assert_eq!(a.pending, Some((Action::Accept, "BP-1".into())));
        press(&mut a, 'y');
        assert_eq!(a.job("BP-1"), Some(Job::Accept));
        assert_eq!(Job::Accept.args("BP-1"), vec!["accept", "BP-1"]);
    }

    #[test]
    fn a_on_a_task_that_isnt_done_says_so() {
        let mut a = app(vec![task("BP-1", "working")]);
        a.settle();
        press(&mut a, 'a');
        assert!(a.pending.is_none());
        assert_eq!(a.msg, "BP-1 isn't ready yet.");
        let mut a = app(vec![task("BP-1", "done")]);
        a.closed = true;
        a.settle();
        press(&mut a, 'a');
        assert!(a.pending.is_none());
        assert_eq!(a.msg, "BP-1 is closed already.");
    }

    #[test]
    fn c_always_goes_to_the_coordinator() {
        for status in ["ready", "finished", "needs"] {
            let mut a = app(vec![task("BP-1", status)]);
            a.settle();
            press(&mut a, 'c');
            assert!(a.pending.is_none() && a.running.is_empty(), "{status}");
            assert!(a.msg.text.contains("can't run"), "{status}: {}", a.msg.text); // it tried `bullpen go`
        }
    }

    fn needs(id: &str, ask: bool, kind: &str) -> Task {
        let stop = Some(model::Stop { by: "worker".into(), kind: kind.into() });
        Task { ask: ask.then_some(true), stop, ..task(id, "needs") }
    }

    fn prompt(options: &[(&str, &str)]) -> Prompt {
        Prompt {
            who: "worker".into(),
            question: Some("Do you want to proceed?".into()),
            options: options.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect(),
            id: Some("ab12".into()),
            screen: vec!["touch hello.txt".into()],
        }
    }

    #[test]
    fn the_help_scrolls_and_any_other_key_closes_it_without_quitting() {
        let mut a = app(vec![task("BP-1", "ready")]);
        a.height = 14;
        press(&mut a, '?');
        assert!(matches!(a.mode, Mode::Help { top: 0 }));
        press(&mut a, 'j');
        press(&mut a, 'j');
        press(&mut a, 'k');
        assert!(matches!(a.mode, Mode::Help { top: 1 }));
        a.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(matches!(a.mode, Mode::Help { top: 2 }));
        press(&mut a, ' '); // a page: the height less the title, gap, foot and gap
        assert!(matches!(a.mode, Mode::Help { top: 12 }));
        press(&mut a, 'b');
        press(&mut a, 'b');
        assert!(matches!(a.mode, Mode::Help { top: 0 }));
        // q closes the list; it doesn't quit the board
        assert!(a.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)));
        assert!(matches!(a.mode, Mode::Normal));
        for close in [KeyCode::Esc, KeyCode::Char('?'), KeyCode::Char('a')] {
            press(&mut a, '?');
            a.key(KeyEvent::new(close, KeyModifiers::NONE));
            assert!(matches!(a.mode, Mode::Normal), "{close:?}");
        }
        assert!(a.pending.is_none(), "the key that closes it does nothing else");
    }

    #[test]
    fn i_on_a_prompt_shows_it_and_a_number_answers_it() {
        let mut a = app(vec![needs("BP-1", false, "prompt")]);
        a.settle();
        press(&mut a, 'i');
        assert!(matches!(&a.mode, Mode::Answer { id, prompt: None, .. } if id == "BP-1"));
        a.peeking = false; // the read it started can't reach this test
        a.prompted("BP-1".into(), Some(prompt(&[("1", "Yes"), ("2", "No")])), String::new());
        // a key that's no option does nothing
        press(&mut a, '7');
        assert!(matches!(a.mode, Mode::Answer { .. }));
        // an option runs `bullpen answer BP-1 2 --prompt ab12` (there's no bullpen here)
        press(&mut a, '2');
        assert!(matches!(a.mode, Mode::Normal));
        assert!(a.msg.text.contains("can't run"), "{}", a.msg.text);
        // esc goes back without answering
        a.answer(&needs("BP-1", false, "prompt"));
        a.answer_key(KeyCode::Esc);
        assert!(matches!(a.mode, Mode::Normal));
    }

    #[test]
    fn a_prompt_that_cant_be_read_closes_the_answer() {
        let mut a = app(vec![needs("BP-1", false, "prompt")]);
        a.settle();
        press(&mut a, 'i');
        a.prompted("BP-2".into(), None, "other".into()); // another task's: ignored
        assert!(matches!(a.mode, Mode::Answer { .. }));
        a.prompted("BP-1".into(), None, "BP-1 isn't waiting at a prompt".into());
        assert!(matches!(a.mode, Mode::Normal));
        assert_eq!(a.msg, "BP-1 isn't waiting at a prompt");
        // a screen with no prompt now stays open, with nothing to press
        press(&mut a, 'i');
        a.prompted("BP-1".into(), Some(Prompt { question: None, ..prompt(&[]) }), String::new());
        press(&mut a, '1');
        assert!(matches!(a.mode, Mode::Answer { .. }));
    }

    #[test]
    fn i_on_an_ask_types_an_answer_for_the_coordinator() {
        let mut a = app(vec![needs("BP-1", true, "ended")]);
        a.settle();
        press(&mut a, 'i');
        assert!(matches!(&a.mode, Mode::Tell { id, reply: true, .. } if id == "BP-1"));
        for c in "Postgres".chars() {
            press(&mut a, c);
        }
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(a.mode, Mode::Normal));
        assert!(a.msg.text.contains("can't run"), "{}", a.msg.text); // it tried `bullpen reply`
    }

    #[test]
    fn i_with_nothing_to_answer_says_so() {
        let mut a = app(vec![needs("BP-1", false, "ended"), task("BP-2", "working")]);
        a.settle();
        press(&mut a, 'i');
        assert!(matches!(a.mode, Mode::Normal));
        assert_eq!(a.msg, "BP-1 asks nothing here: ⏎ opens it.");
        press(&mut a, 'j');
        press(&mut a, 'i');
        assert_eq!(a.msg, "BP-2 doesn't need you.");
    }

    #[test]
    fn n_goes_to_what_needs_you_then_to_what_is_ready() {
        let mut a = app(vec![task("BP-1", "working"), task("BP-2", "triage"), task("BP-3", "ready"), task("BP-4", "finished"), task("BP-5", "needs")]);
        a.settle();
        assert_eq!(a.sel.as_deref(), Some("BP-5")); // the board's first: needs you
        press(&mut a, 'n');
        assert_eq!((a.sel.as_deref(), a.msg.text.as_str()), (Some("BP-5"), ""));
        // nothing needs you: ready, then finished, round again, saying why
        a.tasks.retain(|t| t.id != "BP-5");
        a.settle();
        press(&mut a, 'n');
        assert_eq!((a.sel.as_deref(), a.msg.text.as_str()), (Some("BP-3"), "Nothing needs you: BP-3 is ready."));
        press(&mut a, 'n');
        assert_eq!((a.sel.as_deref(), a.msg.text.as_str()), (Some("BP-4"), "Nothing needs you: BP-4 is finished."));
        press(&mut a, 'n');
        assert_eq!(a.sel.as_deref(), Some("BP-3"));
        // a stopped task needs you too
        a.tasks.push(task("BP-6", "stopped"));
        press(&mut a, 'n');
        assert_eq!((a.sel.as_deref(), a.msg.text.as_str()), (Some("BP-6"), ""));
        // nothing to do at all
        let mut a = app(vec![task("BP-1", "working"), task("BP-2", "triage")]);
        a.settle();
        press(&mut a, 'n');
        assert_eq!(a.msg, "Nothing needs you.");
    }

    #[test]
    fn after_an_accept_the_next_task_that_needs_you_is_selected() {
        let mut a = app(vec![task("BP-1", "ready"), task("BP-2", "needs"), task("BP-3", "ready"), task("BP-4", "finished")]);
        a.sel = Some("BP-3".into());
        a.settle();
        a.done("BP-3".into(), Job::Accept, true, String::new());
        assert_eq!(a.msg, "Accepted BP-3.");
        assert_eq!(a.sel.as_deref(), Some("BP-2"));
        // with nothing needing you, the next one ready to accept, then a finished one,
        // even past "Coordinator on it" and a task in review
        let mut a = app(vec![task("BP-1", "ready"), task("BP-2", "triage"), task("BP-3", "ready"), task("BP-4", "finished"), task("BP-5", "review")]);
        a.sel = Some("BP-1".into());
        a.settle();
        a.done("BP-1".into(), Job::Accept, true, String::new());
        assert_eq!(a.sel.as_deref(), Some("BP-3"));
        a.done("BP-3".into(), Job::Accept, true, String::new());
        assert_eq!(a.sel.as_deref(), Some("BP-4")); // BP-1's still listed until the board reads again
        // a closed one too; with nothing else to accept or close the selection stays
        let mut a = app(vec![task("BP-4", "finished")]);
        a.sel = Some("BP-4".into());
        a.done("BP-4".into(), Job::Close, true, String::new());
        assert_eq!((a.msg.text.as_str(), a.sel.as_deref()), ("Closed BP-4.", Some("BP-4")));
        // a failed accept, or a selection already moved elsewhere, stays put
        let mut a = app(vec![task("BP-1", "ready"), task("BP-2", "needs"), task("BP-3", "ready")]);
        a.sel = Some("BP-1".into());
        a.done("BP-1".into(), Job::Accept, false, "merge refused".into());
        assert_eq!(a.sel.as_deref(), Some("BP-1"));
        a.done("BP-3".into(), Job::Accept, true, String::new());
        assert_eq!(a.sel.as_deref(), Some("BP-1"));
    }

    #[test]
    fn an_accept_that_changed_the_keys_says_so_in_place_of_accepted() {
        let mut a = app(vec![task("BP-1", "ready")]);
        a.done("BP-1".into(), Job::Accept, true, "Keys changed: run `rex config reload`".into());
        assert_eq!(a.msg, "Keys changed: run `rex config reload`");
        // any other last line is still "Accepted"
        let mut a = app(vec![task("BP-1", "ready")]);
        a.done("BP-1".into(), Job::Accept, true, "BP-1 done (merged into main: 0123456789ab)".into());
        assert_eq!(a.msg, "Accepted BP-1.");
    }

    #[test]
    fn go_board_selects_a_task_through_the_select_file() {
        let d = Scratch::new("select");
        let mut a = app(vec![owned("BP-1", "needs", "c1"), owned("BP-2", "todo", "c1")]);
        (a.dir, a.me) = (d.0.clone(), Some("c1".into()));
        a.settle();
        a.pending = Some((Action::Accept, "BP-1".into()));
        let file = d.0.join("select").join("c1");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let ask = |id: &str| std::fs::write(&file, format!("{id}\n")).unwrap();
        ask("BP-2");
        a.take_select();
        assert_eq!(a.sel.as_deref(), Some("BP-2"));
        assert!(a.pending.is_none()); // the question was about BP-1
        assert!(!file.exists()); // read once
        ask("BP-9"); // a task it doesn't have: stays put
        a.take_select();
        assert_eq!(a.sel.as_deref(), Some("BP-2"));
        ask("BP-1"); // left over from long ago: dropped
        let old = std::time::SystemTime::now() - Duration::from_secs(60);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(old).unwrap();
        a.take_select();
        assert_eq!(a.sel.as_deref(), Some("BP-2"));
        assert!(!file.exists());
        a.cfg.coordinators = vec!["c1".into(), "c2".into()];
        a.tasks.push(owned("BP-3", "todo", "c2"));
        ask("BP-3"); // shown only with o: say so, and stay put
        a.take_select();
        assert_eq!(a.sel.as_deref(), Some("BP-2"));
        assert_eq!(a.msg, "BP-3 is c2's: o shows it.");
        a.cfg.board_others = vec!["c1".into()];
        ask("BP-3");
        a.take_select();
        assert_eq!(a.sel.as_deref(), Some("BP-3"));
        std::fs::write(d.0.join("select").join("c2"), "BP-1").unwrap(); // another coordinator's board's
        a.take_select();
        assert_eq!(a.sel.as_deref(), Some("BP-3"));
    }

    /// A scratch folder of its own for a test, removed at the end.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let d = std::env::temp_dir().join(format!("bullpen-board-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Scratch(d)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn program(path: &Path, text: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, text).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        // A program just written can't run while another test's fork still holds it
        // open for writing (ETXTBSY): wait until it runs.
        for _ in 0..200 {
            let run = Command::new(path).arg("--ready").stdout(Stdio::null()).stderr(Stdio::null()).status();
            match run {
                Err(e) if e.raw_os_error() == Some(26) => std::thread::sleep(Duration::from_millis(10)),
                _ => break,
            }
        }
    }

    /// An app whose bullpen is this script.
    fn with_bullpen(d: &Scratch, tasks: Vec<Task>, script: &str) -> App {
        let exe = d.0.join("bullpen");
        program(&exe, script);
        let mut a = app(tasks);
        (a.bullpen, a.root) = (exe, d.0.clone());
        a.settle();
        a
    }

    fn key(a: &mut App, code: KeyCode) {
        a.key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn a_failure_stays_until_the_next_action_on_a_task() {
        let d = Scratch::new("failure");
        let fails = "#!/bin/sh\necho 'bullpen: the coordinator has no pane' >&2\nexit 1\n";
        let mut a = with_bullpen(&d, vec![task("BP-1", "needs"), task("BP-2", "ready")], fails);
        press(&mut a, 'c');
        assert_eq!(a.msg, Note::failed("the coordinator has no pane"));
        // looking around keeps it: moving, n, f, the help and back
        for c in ['j', 'k', 'n', 'f', 'f', 'x', '?', 'q'] {
            press(&mut a, c);
            assert_eq!(a.msg, "the coordinator has no pane", "after {c}");
        }
        key(&mut a, KeyCode::Down);
        assert_eq!(a.msg.kind, Kind::Failed);
        // esc puts it away
        key(&mut a, KeyCode::Esc);
        assert!(a.msg.is_empty());
        // an action on a task does too, saying its own thing
        press(&mut a, 'c');
        a.sel = Some("BP-1".into());
        press(&mut a, 'a');
        assert_eq!(a.msg, Note::info("BP-1 isn't ready yet."));
        // which, not being a failure, goes with the next key
        press(&mut a, 'j');
        assert!(a.msg.is_empty());

        // a background command's result: green when it worked, red and kept when not
        a.done("BP-2".into(), Job::Accept, true, String::new());
        assert_eq!(a.msg, Note::done("Accepted BP-2."));
        press(&mut a, 'k');
        assert!(a.msg.is_empty());
        a.done("BP-2".into(), Job::Accept, false, "BP-2's branch doesn't merge cleanly".into());
        assert_eq!(a.msg, Note::failed("BP-2's branch doesn't merge cleanly"));
        press(&mut a, 'j');
        assert_eq!(a.msg.kind, Kind::Failed);
        // a y/n answered is an action
        press(&mut a, 'a');
        assert!(a.msg.is_empty() && a.pending.is_some());
    }

    #[test]
    fn slash_opens_the_picker_or_says_why_not() {
        let d = Scratch::new("slash");
        let mut a = with_bullpen(&d, vec![task("BP-1", "ready")], "#!/bin/sh\n");
        a.bullpen = fake_bullpen(&d, "current");
        press(&mut a, '/');
        assert_eq!(calls(&d), ["pick --pane"]);
        assert!(a.msg.is_empty() && matches!(a.mode, Mode::Normal));
        let d = Scratch::new("slash-fails");
        let mut a = with_bullpen(&d, vec![], "#!/bin/sh\necho 'bullpen: no Rex session here' >&2\nexit 1\n");
        press(&mut a, '/');
        assert_eq!(a.msg, Note::failed("no Rex session here"));
    }

    #[test]
    fn ctrl_letters_dont_act_as_the_letters() {
        let mut a = app(vec![task("BP-1", "ready"), task("BP-2", "ready")]);
        a.settle();
        for c in ['a', 'q', 'l', 'k'] {
            a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
        }
        assert!(a.pending.is_none() && matches!(a.mode, Mode::Normal) && a.msg.is_empty());
        a.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert_eq!(a.sel.as_deref(), Some("BP-2"));
        assert!(a.pending.is_none());
    }

    #[test]
    fn l_and_o_say_what_they_did() {
        let d = Scratch::new("say");
        let c = |id: &str, who: &str| owned(id, "needs", who);
        let mut a = with_bullpen(&d, vec![c("BP-1", "c1"), c("BP-2", "c2")], "#!/bin/sh\nexit 0\n");
        a.cfg.coordinators = vec!["c1".into(), "c2".into()];
        a.me = Some("c1".into());
        press(&mut a, 'o');
        assert_eq!(a.msg, Note::info("Showing c2's tasks too."));
        press(&mut a, 'o');
        assert_eq!(a.msg, "Hiding c2's tasks.");
        a.cfg.coordinators.push("c3".into());
        press(&mut a, 'o');
        assert_eq!(a.msg, "Showing other coordinators' tasks too.");
        press(&mut a, 'l');
        assert_eq!(a.msg, Note::info("Compact layout."));
        press(&mut a, 'l');
        assert_eq!(a.msg, "Grouped layout.");
        // a setting that can't be saved says why, in red
        let d = Scratch::new("say-fails");
        let mut a = with_bullpen(&d, vec![], "#!/bin/sh\necho 'bullpen: config.json is read-only' >&2\nexit 1\n");
        press(&mut a, 'l');
        assert_eq!(a.msg, Note::failed("config.json is read-only"));
    }

    fn peek_at(a: &mut App, id: &str) {
        a.sel = Some(id.into());
        a.peeking = true; // no read in the background: it can't reach this test
        press(a, 'p');
        assert!(matches!(&a.mode, Mode::Overlay { peek: Some(p), .. } if p == id));
    }

    #[test]
    fn a_peek_returns_on_esc_or_q_and_stays_on_other_keys() {
        let mut a = app(vec![task("BP-1", "working"), task("BP-2", "ready")]);
        for k in [KeyCode::Esc, KeyCode::Char('q')] {
            peek_at(&mut a, "BP-1");
            for c in "jkn?lsrpgc".chars() {
                assert!(a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
                assert!(matches!(a.mode, Mode::Overlay { .. }), "{c} left the peek");
            }
            assert!(a.key(KeyEvent::new(k, KeyModifiers::NONE)));
            assert!(matches!(a.mode, Mode::Normal));
            assert_eq!(a.sel.as_deref(), Some("BP-1"));
            assert!(a.pending.is_none()); // q went back; it didn't ask to quit
        }
    }

    #[test]
    fn t_i_a_d_in_a_peek_go_back_and_act_on_the_peeked_task() {
        let mut a = app(vec![task("BP-1", "working"), task("BP-2", "ready"), needs("BP-3", true, "worker")]);
        peek_at(&mut a, "BP-1");
        a.sel = Some("BP-2".into()); // the selection moved under the peek (a select from cmd+;)
        press(&mut a, 't');
        assert!(matches!(&a.mode, Mode::Tell { id, reply: false, .. } if id == "BP-1"));
        a.mode = Mode::Normal;
        peek_at(&mut a, "BP-2");
        press(&mut a, 'a');
        assert!(matches!(a.mode, Mode::Normal));
        assert_eq!(a.pending, Some((Action::Accept, "BP-2".into())));
        press(&mut a, 'n');
        peek_at(&mut a, "BP-2");
        press(&mut a, 'd');
        assert_eq!(a.pending, Some((Action::Drop, "BP-2".into())));
        press(&mut a, 'n');
        peek_at(&mut a, "BP-3");
        press(&mut a, 'i');
        assert!(matches!(&a.mode, Mode::Tell { id, reply: true, .. } if id == "BP-3"));
    }

    #[test]
    fn enter_and_v_in_a_peek_go_back_and_open_the_pane_or_diff() {
        let d = Scratch::new("peek-acts");
        let exe = fake_bullpen(&d, "current");
        let mut a = app(vec![Task { block: Some("b1".into()), ..task("BP-1", "working") }]);
        (a.bullpen, a.root) = (exe, d.0.clone());
        peek_at(&mut a, "BP-1");
        a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(a.mode, Mode::Normal));
        assert_eq!(a.msg, Note::done("Opened BP-1."));
        peek_at(&mut a, "BP-1");
        press(&mut a, 'v');
        assert!(matches!(a.mode, Mode::Normal));
        assert_eq!(a.msg, Note::done("Opened BP-1's diff."));
        assert_eq!(calls(&d), ["go BP-1", "diff BP-1 --pane"]);
    }

    #[test]
    fn q_asks_before_quitting_the_board_and_ctrl_c_doesnt() {
        let mut a = app(vec![task("BP-1", "ready")]);
        assert!(a.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)));
        assert!(matches!(a.mode, Mode::Quit));
        assert!(a.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))); // a second q doesn't quit either
        assert!(matches!(a.mode, Mode::Normal));
        assert_eq!(a.msg, "Cancelled.");
        press(&mut a, 'q');
        assert!(!a.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)));
        let mut a = app(vec![task("BP-1", "ready")]);
        assert!(!a.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        // a board asking whether to quit isn't replaced under the question
        press(&mut a, 'q');
        assert!(a.replaced().is_none());
    }

    /// A bullpen that logs its arguments, says the tour's state is `state` to
    /// `tour --show --json`, and answers `tour --pane` as the real one does.
    fn fake_bullpen(d: &Scratch, state: &str) -> PathBuf {
        let exe = d.0.join("bullpen");
        let log = d.0.join("log");
        program(&exe, &format!(
            "#!/bin/sh\n[ \"$1\" = --ready ] && exit 0\necho \"$*\" >> {log}\ncase \"$*\" in\n\
             *--show*) echo '{{\"id\": \"'$2'\", \"state\": \"{state}\", \"why\": null, \"steps\": []}}' ;;\n\
             *--new*) echo \"writing a tour of $2: it opens when it's done\" ;;\n\
             *) echo \"opened $2's tour\" ;;\nesac\n",
            log = log.display()
        ));
        exe
    }

    fn calls(d: &Scratch) -> Vec<String> {
        std::fs::read_to_string(d.0.join("log")).unwrap_or_default().lines().map(String::from).collect()
    }

    #[test]
    fn g_on_a_task_that_isnt_ready_or_in_review_says_why_not() {
        for status in ["working", "todo", "needs", "finished"] {
            let mut a = app(vec![task("BP-1", status)]);
            a.settle();
            press(&mut a, 'g');
            assert!(matches!(a.mode, Mode::Normal));
            let said = if status == "finished" { "BP-1 has no code to tour." } else { "BP-1 gets a tour once it's in review." };
            assert_eq!(a.msg, said);
        }
    }

    #[test]
    fn g_opens_a_current_tour_or_has_one_written() {
        for (state, said) in [("current", "Opened BP-1's tour."), ("none", "Opened BP-1's tour."), ("failed", "Opened BP-1's tour.")] {
            let d = Scratch::new(&format!("g-{state}"));
            let mut a = app(vec![task("BP-1", "ready")]);
            (a.bullpen, a.root) = (fake_bullpen(&d, state), d.0.clone());
            a.settle();
            press(&mut a, 'g');
            assert!(matches!(a.mode, Mode::Normal), "{state}");
            assert_eq!(calls(&d), ["tour BP-1 --show --json", "tour BP-1 --pane"], "{state}");
            assert_eq!(a.msg, said, "{state}");
        }
        // being written: no need to look, `--pane` opens it once it's done
        let d = Scratch::new("g-writing");
        let writing = Some(model::TourRun { state: "writing".into(), ..Default::default() });
        let mut a = app(vec![Task { tour: writing, ..task("BP-1", "review") }]);
        (a.bullpen, a.root) = (fake_bullpen(&d, "writing"), d.0.clone());
        a.settle();
        press(&mut a, 'g');
        assert_eq!(calls(&d), ["tour BP-1 --pane"]);
    }

    #[test]
    fn g_on_a_stale_tour_asks_new_or_old() {
        let d = Scratch::new("g-stale");
        let mut a = app(vec![task("BP-1", "ready")]);
        (a.bullpen, a.root) = (fake_bullpen(&d, "stale"), d.0.clone());
        a.settle();
        press(&mut a, 'g');
        assert!(matches!(&a.mode, Mode::Stale { id } if id == "BP-1"));
        assert_eq!(a.msg, "BP-1's tour is stale: the code changed.");
        press(&mut a, 'n');
        assert!(matches!(a.mode, Mode::Normal));
        assert_eq!(calls(&d).last().unwrap(), "tour BP-1 --pane --new");
        assert_eq!(a.msg, "Writing a tour of BP-1: it opens when it's done.");
        press(&mut a, 'g');
        press(&mut a, 'o');
        assert_eq!(calls(&d).last().unwrap(), "tour BP-1 --pane");
        assert_eq!(a.msg, "Opened BP-1's tour.");
        press(&mut a, 'g');
        press(&mut a, 'x');
        assert_eq!(a.msg, "Cancelled.");
        assert_eq!(calls(&d).len(), 5); // three looks, two opens
    }

    #[test]
    fn a_board_reloads_once_its_program_is_renamed_into_place_and_its_idle() {
        let d = Scratch::new("reexec");
        let exe = d.0.join("bullpen-board");
        program(&exe, "#!/bin/sh\n");
        let mut a = app(vec![task("BP-1", "ready")]);
        a.exe = Some((exe.clone(), ExeId::of(&exe).unwrap()));
        assert_eq!(a.replaced(), None); // the same file
        // a build writes a copy, then renames it over the program
        let tmp = d.0.join(".bullpen-board.1");
        program(&tmp, "#!/bin/sh\necho new\n");
        std::fs::rename(&tmp, &exe).unwrap();
        a.running.push(("BP-1".into(), Job::Accept));
        assert_eq!(a.replaced(), None); // not while a command runs
        a.running.clear();
        a.mode = Mode::Tell { id: "BP-1".into(), text: "half a".into(), reply: false };
        assert_eq!(a.replaced(), None); // nor while you type
        a.mode = Mode::Stale { id: "BP-1".into() };
        assert_eq!(a.replaced(), None); // nor while it asks about a stale tour
        a.mode = Mode::Normal;
        a.pending = Some((Action::Accept, "BP-1".into()));
        assert_eq!(a.replaced(), None); // nor while it asks y/n
        a.pending = None;
        assert_eq!(a.replaced(), Some(exe.clone()));
    }

    #[test]
    fn a_missing_or_empty_program_isnt_reloaded() {
        let d = Scratch::new("missing");
        let exe = d.0.join("bullpen-board");
        program(&exe, "#!/bin/sh\n");
        let mut a = app(vec![]);
        a.exe = Some((exe.clone(), ExeId::of(&exe).unwrap()));
        std::fs::remove_file(&exe).unwrap();
        assert_eq!(a.replaced(), None);
        std::fs::write(&exe, "").unwrap();
        assert_eq!(a.replaced(), None);
        a.exe = None; // started from a relative path, or reloading failed
        program(&exe, "#!/bin/sh\necho new\n");
        assert_eq!(a.replaced(), None);
    }

    #[test]
    fn a_reload_keeps_the_selection_f_and_the_coordinator() {
        let mut a = app(vec![task("BP-1", "ready"), task("BP-2", "done")]);
        a.me = Some("c2".into());
        a.sel = Some("BP-2".into());
        press(&mut a, 'f');
        let args = parse_args(a.restart_args()).unwrap();
        assert_eq!(
            args,
            Args {
                dir: a.dir.clone(),
                root: a.root.clone(),
                bullpen: a.bullpen.clone(),
                me: Some("c2".into()),
                select: Some("BP-2".into()),
                closed: true,
                reloaded: true,
            }
        );
        a.closed = false;
        a.me = None;
        a.sel = None;
        let args = parse_args(a.restart_args()).unwrap();
        assert_eq!((args.me, args.select, args.closed), (None, None, false));
    }

    #[test]
    fn f_says_whether_closed_tasks_show_and_an_old_boards_finished_still_works() {
        let mut a = app(vec![task("BP-1", "ready"), task("BP-2", "done")]);
        press(&mut a, 'f');
        assert!(a.closed);
        assert_eq!(a.msg, "Showing closed tasks too.");
        press(&mut a, 'f');
        assert_eq!((a.closed, a.msg.text.as_str()), (false, "Hiding closed tasks."));
        // a board from before --closed restarts the new one with --finished
        let argv = ["--dir", "d", "--root", "r", "--bullpen", "b", "--finished", "on"].map(String::from);
        assert!(parse_args(argv).unwrap().closed);
    }

    #[test]
    fn a_failed_peek_says_why_without_bullpens_prefix() {
        let mut a = app(vec![task("BP-3", "stopped")]);
        a.mode = Mode::Overlay { title: "BP-3".into(), lines: vec!["…".into()], peek: Some("BP-3".into()) };
        a.peeked("BP-3".into(), said(&last_line("bullpen: BP-3's agent isn't running\n")), false);
        assert!(a.peek_failed);
        assert!(matches!(&a.mode, Mode::Overlay { lines, .. } if lines == &["BP-3's agent isn't running."]));
        // once it reads the screen again, it's live
        a.peeked("BP-3".into(), "$ cargo test".into(), true);
        assert!(!a.peek_failed);
    }
}
