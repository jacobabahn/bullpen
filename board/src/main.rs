//! bullpen's task board: a live list of a repo's tasks, grouped by what you do
//! next, from which you open, accept, drop, tell or start them.
//!
//! It reads the repo's state directory (config.json and tasks/*.json, through
//! bullpen-core) and redraws
//! when anything there changes. Everything it does goes through the `bullpen`
//! command, so the board itself never changes a task. Slow commands (accept, drop,
//! start, resume) run in the background: the board stays usable meanwhile.
//!
//! When its own program is replaced (bullpen rebuilds the board when its source
//! changes), it execs the new one in place, keeping its selection and `f`.
//!
//! Usage: bullpen-board --dir <repo state dir> --root <repo> --bullpen <bullpen program>
//!
//! With `--mission` first it's mission control instead, every repo on one screen
//! (mission/; `bullpen mission` runs it). With `--render FILE` it writes its list as the
//! image look draws it to FILE (a PNG) and quits.

mod app;
mod edit;
mod image;
mod mission;
mod model;
mod ui;

// ui reads App, Kind and Mode from here
use app::{parse_args, App, Kind, Mode, Msg, Note};
use notify::{RecursiveMode, Watcher};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

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
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("--mission") {
        return mission::run::main(argv[1..].to_vec());
    }
    let args = match parse_args(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("bullpen-board: {e}");
            std::process::exit(2);
        }
    };
    if let Some(path) = args.render.clone() {
        return render(args, &path);
    }
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

    // the key reader; it stops while the image look asks the terminal something
    let gate = Arc::new(image::term::Gate::default());
    read_keys(tx, gate.clone());
    let mut picture = image::Picture::new(gate);

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
                picture.clear(term.backend_mut());
                term_restore();
                let err = Command::new(&exe).args(app.restart_args()).exec(); // returns only if it failed
                term = term_init();
                app.exe = None; // don't try again
                app.msg = Note::failed(format!("Couldn't reload the rebuilt board: {err}"));
            }
        }
        app.settle();
        app.peek();
        app.image = picture.drawing(&app);
        if term.draw(|f| ui::draw(f, &mut app)).is_err() {
            break;
        }
        // the image after the frame, through the same output, never in the middle of one
        if picture.show(&mut app, term.backend_mut()) {
            continue; // the list is text again: draw it now
        }
        let m = match rx.recv_timeout(app.tick()) {
            Ok(m) => m,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        stale = !matches!(m, Msg::Parked); // a key may have changed a task, and a file event says one did
        if !handle(&mut app, &mut picture, term.backend_mut(), m) {
            break;
        }
        // a burst of file events is one redraw
        while let Ok(m) = rx.try_recv() {
            if !handle(&mut app, &mut picture, term.backend_mut(), m) {
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
    picture.clear(term.backend_mut());
    term_restore();
    status("state=clear");
}

/// One message: the image look's own (the terminal's answers), else the board's.
fn handle(app: &mut App, picture: &mut image::Picture, out: &mut impl Write, m: Msg) -> bool {
    match m {
        Msg::Parked => picture.parked(out),
        Msg::Redraw => {
            picture.resized();
            return app.handle(m);
        }
        Msg::Answered(a) => {
            picture.answered(&a);
            // what was typed while the board read the answers
            for typed in image::term::keys(&a.rest) {
                let m = match typed {
                    image::term::Typed::Key(k) => Msg::Key(k),
                    image::term::Typed::Paste(text) => Msg::Paste(text),
                };
                if !app.handle(m) {
                    return false;
                }
            }
        }
        m => return app.handle(m),
    }
    true
}

/// Read keys on a thread of their own, sending them to the board. While the image look
/// asks the terminal something the reader stops, and reads the answers instead (image/term.rs).
fn read_keys(keys: mpsc::Sender<Msg>, gate: Arc<image::term::Gate>) {
    std::thread::spawn(move || loop {
        if gate.wanted() {
            if keys.send(Msg::Parked).is_err() {
                return;
            }
            gate.park();
            let (answers, left) = image::term::read_answers(&mut image::term::Stdin, Some(image::PROBE));
            let late = !answers.done;
            if keys.send(Msg::Answered(answers)).is_err() {
                return;
            }
            // what didn't come in time mustn't reach crossterm as keys: read it here
            if late {
                image::term::drain(&mut image::term::Stdin, left, Some(image::PROBE), |typed| {
                    let _ = keys.send(match typed {
                        image::term::Typed::Key(k) => Msg::Key(k),
                        image::term::Typed::Paste(text) => Msg::Paste(text),
                    });
                });
            }
            gate.done();
            continue;
        }
        // look for a key a tenth of a second at a time, so a wish to ask is seen soon
        match event::poll(Duration::from_millis(100)) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => return,
        }
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
}

/// `--render FILE`: the board's list as the image look draws it, written to FILE.
fn render(args: app::Args, path: &std::path::Path) {
    let (tx, _rx) = mpsc::channel();
    let cols = args.cols.unwrap_or(60).max(20);
    let mut app = App::new(args, None, tx);
    app.read();
    app.settle();
    let rows = ui::rows::build(&app, cols + 2, cols + 2 < ui::NARROW, model::now());
    if let Err(e) = image::render_file(&rows, cols, (16, 34), app.cfg.font.as_deref(), path) {
        eprintln!("bullpen-board: {e}");
        std::process::exit(1);
    }
}
