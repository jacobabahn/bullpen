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

mod app;
mod edit;
mod model;
mod ui;

// ui reads App, Kind and Mode from here
use app::{parse_args, App, Kind, Mode, Msg, Note};
use notify::{RecursiveMode, Watcher};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The `?` list: a heading, then one short line per key. The board wraps it to its
/// width; the long explanations are in docs/board.md.
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

/// Take the terminal, with bracketed paste on: a paste arrives whole, so a newline
/// in it doesn't send the line, and pasted text outside a line isn't taken as keys.
fn term_init() -> ratatui::DefaultTerminal {
    let term = ratatui::init();
    let _ = ratatui::crossterm::execute!(std::io::stdout(), ratatui::crossterm::event::EnableBracketedPaste);
    term
}

fn term_restore() {
    let _ = ratatui::crossterm::execute!(std::io::stdout(), ratatui::crossterm::event::DisableBracketedPaste);
    ratatui::restore();
}

/// Report this program's status to the terminal (the Program Status Protocol).
fn status(report: &str) {
    use std::io::Write;
    print!("\x1b]7501;{report}\x1b\\");
    let _ = std::io::stdout().flush();
}

fn main() {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("bullpen-board: {e}");
            std::process::exit(2);
        }
    };
    let (tx, rx) = mpsc::channel();
    // The program as `bullpen board` named it: a rebuild replaces that file.
    let exe = std::env::args_os().next().map(PathBuf::from).filter(|p| p.is_absolute());
    let mut app = App::new(args, exe, tx.clone());

    // redraw when the repo's tasks or config change
    let changed = tx.clone();
    let mut watcher = notify::recommended_watcher(move |r: notify::Result<notify::Event>| {
        if r.is_ok() {
            let _ = changed.send(Msg::Changed);
        }
    })
    .ok();
    if let Some(w) = watcher.as_mut() {
        let _ = std::fs::create_dir_all(&app.dir);
        let _ = w.watch(&app.dir, RecursiveMode::Recursive);
    }

    let keys = tx;
    std::thread::spawn(move || loop {
        let m = match event::read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => Msg::Key(k),
            Ok(Event::Paste(text)) => Msg::Paste(text),
            Ok(Event::Resize(..)) => Msg::Redraw,
            Ok(_) => continue,
            Err(_) => return,
        };
        if keys.send(m).is_err() {
            return;
        }
    });

    // Rex picks a pane's icon by the program in it and doesn't know this one. A
    // program can say what it is (OSC 7501's app): "rust" gets Rust's icon.
    // bullpen ignores status from the board's pane, and other terminals ignore this.
    status("state=idle:app=rust");
    let mut term = term_init();
    // Read the tasks again when their files change, and once a second in case a
    // change was missed; frames drawn in between only move the animations.
    let (mut stale, mut read) = (true, Instant::now());
    'main: loop {
        if stale || read.elapsed() >= Duration::from_secs(1) {
            app.reload();
            (stale, read) = (false, Instant::now());
            if let Some(exe) = app.replaced() {
                use std::os::unix::process::CommandExt;
                term_restore();
                let err = Command::new(&exe).args(app.restart_args()).exec(); // returns only if it failed
                term = term_init();
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
    term_restore();
    status("state=clear");
}
