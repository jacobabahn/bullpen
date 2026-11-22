//! Running mission control: its command line, the loop, and what keeps it current.
//!
//! One `notify` watch on the whole state folder (every repo's tasks, doing/, a new
//! repo appearing) reads the snapshot again; so does a check once a second, for an
//! event missed. Rex is asked about the coordinators' panes on a slower clock, off the
//! drawing thread: `bullpen repos --json` every ALIVE_EVERY. When its program is
//! replaced (a rebuild), it execs the new one in place, keeping its selection.
//!
//! With the image look (`board_look image`) it reads keys as the board does, stopping
//! while it asks the terminal what the images need (image/term.rs), and draws what a j
//! or k would show next while it waits for a key.
//!
//! Usage: bullpen-board --mission --state <state folder> --bullpen <bullpen program>
//! [--render FILE --cols N --theme dark|light]

use super::{MMsg, Mission, Sel};
use crate::app::{ExeId, Msg, Note};
use crate::image::{Picture, Theme};
use notify::{RecursiveMode, Watcher};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often Rex is asked whether each coordinator's pane is alive.
pub const ALIVE_EVERY: Duration = Duration::from_secs(5);

/// The command line.
#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub state: PathBuf,
    pub bullpen: PathBuf,
    pub select: Option<Sel>,
    pub reloaded: bool,
    /// `--render FILE`: write mission control as the image look draws it, and exit
    pub render: Option<PathBuf>,
    pub cols: Option<u16>,
    pub theme: Option<Theme>,
}

/// The arguments after `--mission`.
pub fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut it = argv.into_iter();
    let mut out = Args::default();
    let (mut repo, mut task) = (None, None);
    while let Some(a) = it.next() {
        let v = it.next().ok_or(format!("{a} needs a value"))?;
        match a.as_str() {
            "--state" => out.state = PathBuf::from(v),
            "--bullpen" => out.bullpen = PathBuf::from(v),
            "--select-repo" => repo = Some(PathBuf::from(v)),
            "--select" => task = Some(v),
            "--reloaded" => out.reloaded = v == "on",
            "--render" => out.render = Some(PathBuf::from(v)),
            "--cols" => out.cols = Some(v.parse().map_err(|_| format!("--cols needs a number, not {v}"))?),
            "--theme" => out.theme = Some(Theme::named(&v).ok_or(format!("--theme is dark or light, not {v}"))?),
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    if out.state.as_os_str().is_empty() || out.bullpen.as_os_str().is_empty() {
        return Err("usage: bullpen-board --mission --state <state folder> --bullpen <program> (`bullpen mission` runs it)".into());
    }
    out.select = repo.zip(task);
    Ok(out)
}

/// What a reload starts the new program with: these, and the selection.
pub fn restart_args(m: &Mission) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "--mission".into(),
        "--state".into(),
        m.state.display().to_string(),
        "--bullpen".into(),
        m.bullpen.display().to_string(),
    ];
    if let Some((dir, id)) = &m.sel {
        a.extend(["--select-repo".into(), dir.display().to_string(), "--select".into(), id.clone()]);
    }
    a.extend(["--reloaded".into(), "on".into()]);
    a
}

/// Each coordinator's pane, alive or not, as `bullpen repos --json` says: (repo's state
/// folder, coordinator, alive). None when it can't say.
pub fn alive_from(json: &str) -> Option<Vec<(PathBuf, String, bool)>> {
    let v: Value = serde_json::from_str(json.trim()).ok()?;
    let mut out = Vec::new();
    for r in v.get("repos")?.as_array()? {
        let dir = PathBuf::from(r.get("dir")?.as_str()?);
        for c in r.get("coordinators").and_then(Value::as_array).into_iter().flatten() {
            let (Some(id), Some(alive)) = (c.get("id").and_then(Value::as_str), c.get("alive").and_then(Value::as_bool)) else {
                continue;
            };
            out.push((dir.clone(), id.to_string(), alive));
        }
    }
    Some(out)
}

/// Ask `bullpen repos --json` every ALIVE_EVERY, in the background.
fn ask_alive(bullpen: PathBuf, state: PathBuf, tx: Sender<MMsg>) {
    std::thread::spawn(move || loop {
        let out = Command::new(&bullpen).args(["repos", "--json"]).current_dir(&state).stdin(Stdio::null()).output();
        if let Some(list) = out.ok().filter(|o| o.status.success()).and_then(|o| alive_from(&String::from_utf8_lossy(&o.stdout))) {
            if tx.send(MMsg::Alive(list)).is_err() {
                return;
            }
        }
        std::thread::sleep(ALIVE_EVERY);
    });
}

/// Watch the state folder: any change reads the snapshot again.
fn watch(state: &Path, tx: Sender<MMsg>) -> Option<notify::RecommendedWatcher> {
    let mut w = notify::recommended_watcher(move |r: notify::Result<notify::Event>| {
        if r.is_ok() {
            let _ = tx.send(MMsg::Changed);
        }
    })
    .ok()?;
    let _ = std::fs::create_dir_all(state);
    w.watch(state, RecursiveMode::Recursive).ok()?;
    Some(w)
}

pub fn main(argv: Vec<String>) {
    let args = match parse_args(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("bullpen-board: {e}");
            std::process::exit(2);
        }
    };
    let (tx, rx) = mpsc::channel();
    let exe = std::env::args_os().next().map(PathBuf::from).filter(|p| p.is_absolute());
    let mut exe = exe.and_then(|p| ExeId::of(&p).map(|id| (p, id)));
    let mut m = Mission::new(args.state, args.bullpen, args.select, tx.clone());
    if args.reloaded {
        m.msg = Note::info("Reloaded: mission control was rebuilt.");
    }
    if let Some(path) = args.render {
        return render(&mut m, &path, args.cols, args.theme);
    }
    let _watcher = watch(&m.state, tx.clone());
    ask_alive(m.bullpen.clone(), m.state.clone(), tx.clone());
    // the key reader (the board's: it stops while the image look asks the terminal
    // something) and drawing ahead's thread speak the board's messages
    let gate = Arc::new(crate::image::term::Gate::default());
    let board = forward(tx);
    crate::read_keys(board.clone(), gate.clone());
    let mut picture = Picture::new(gate);
    picture.use_thread(board);

    crate::status("state=idle:app=rust");
    let mut term = crate::term_init();
    let (mut stale, mut read) = (true, Instant::now());
    'main: loop {
        if stale || read.elapsed() >= Duration::from_secs(1) {
            m.reload();
            (stale, read) = (false, Instant::now());
            // not while the image look asks the terminal: the answers would be the new one's keys
            if let Some((path, was)) = exe.as_ref().filter(|_| picture.idle()) {
                if ExeId::of(path).is_some_and(|now| now != *was) && !m.busy() {
                    use std::os::unix::process::CommandExt;
                    picture.clear(term.backend_mut());
                    crate::term_restore();
                    let err = Command::new(path).args(restart_args(&m)).exec(); // returns only if it failed
                    term = crate::term_init();
                    exe = None;
                    m.msg = Note::failed(format!("Couldn't reload the rebuilt mission control: {err}"));
                }
            }
        }
        m.peek();
        match draw(&mut term, &mut m, &mut picture) {
            Err(_) => break,
            Ok(true) => continue, // the images couldn't be drawn: text now
            Ok(false) => {}
        }
        let tick = picture.tick().map_or(m.tick(), |t| t.min(m.tick()));
        // nothing waiting: draw what a j or a k would want next, looking for a key between
        let area = term.get_frame().area();
        let got = loop {
            match rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) if picture.ahead(&mut m, area, term.backend_mut()) => continue,
                Ok(msg) => break Ok(msg),
                Err(mpsc::TryRecvError::Disconnected) => break Err(mpsc::RecvTimeoutError::Disconnected),
                Err(mpsc::TryRecvError::Empty) => break rx.recv_timeout(tick),
            }
        };
        let msg = match got {
            Ok(msg) => msg,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        stale |= matches!(msg, MMsg::Changed | MMsg::Key(_) | MMsg::Board(..));
        if !handle(&mut m, &mut picture, term.backend_mut(), msg) {
            break;
        }
        // a burst of file events is one read
        while let Ok(msg) = rx.try_recv() {
            stale |= matches!(msg, MMsg::Changed | MMsg::Key(_) | MMsg::Board(..));
            if !handle(&mut m, &mut picture, term.backend_mut(), msg) {
                break 'main;
            }
        }
    }
    // let a command that's still running finish
    while m.boards.values().any(|a| !a.running.is_empty()) {
        m.msg = Note::info("Waiting for a command to finish…");
        let _ = draw(&mut term, &mut m, &mut picture);
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(msg @ MMsg::Board(..)) => {
                m.handle(msg);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            _ => {}
        }
    }
    picture.clear(term.backend_mut());
    crate::term_restore();
    crate::status("state=clear");
}

/// Draw a frame: the text, then the images after it, through the same output. True when
/// it's to be drawn again at once (the images couldn't be drawn: text now).
fn draw<B: ratatui::backend::Backend + Write>(term: &mut ratatui::Terminal<B>, m: &mut Mission, picture: &mut Picture) -> std::io::Result<bool> {
    m.image = picture.drawing(m);
    // each repository's board says its tasks' lines as the image look does
    for app in m.boards.values_mut() {
        app.image = m.image;
    }
    term.draw(|f| super::draw::draw(f, m))?;
    Ok(picture.show(m, term.backend_mut()))
}

/// One message: the image look's own (the terminal's answers, an image drawn ahead, a
/// resize), else mission control's.
fn handle(m: &mut Mission, picture: &mut Picture, out: &mut impl Write, msg: MMsg) -> bool {
    match msg {
        MMsg::Look(Msg::Parked) => picture.parked(out),
        MMsg::Look(Msg::Ready(r)) => picture.ready(r, out),
        MMsg::Look(Msg::Answered(a)) => {
            picture.answered(&a);
            // what was typed while it read the answers
            for typed in crate::image::term::keys(&a.rest) {
                let msg = match typed {
                    crate::image::term::Typed::Key(k) => MMsg::Key(k),
                    crate::image::term::Typed::Paste(text) => MMsg::Paste(text),
                };
                if !m.handle(msg) {
                    return false;
                }
            }
        }
        MMsg::Redraw => {
            picture.resized();
            return m.handle(MMsg::Redraw);
        }
        msg => return m.handle(msg),
    }
    true
}

/// A sender of the board's messages (the key reader's, drawing ahead's) that hands them
/// on to mission control's loop as its own.
fn forward(to: Sender<MMsg>) -> Sender<Msg> {
    let (tx, rx) = mpsc::channel::<Msg>();
    std::thread::spawn(move || {
        for msg in rx {
            let msg = match msg {
                Msg::Key(k) => MMsg::Key(k),
                Msg::Paste(text) => MMsg::Paste(text),
                Msg::Redraw => MMsg::Redraw,
                other => MMsg::Look(other),
            };
            if to.send(msg).is_err() {
                return;
            }
        }
    });
    tx
}

/// `--render FILE`: mission control as the image look draws it, to FILE, and exit.
fn render(m: &mut Mission, path: &Path, cols: Option<u16>, theme: Option<Theme>) {
    m.reload();
    if let Err(e) = super::look::render(m, cols.unwrap_or(160).max(40), (16, 34), theme.unwrap_or_default(), path) {
        eprintln!("bullpen-board: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "tests/test_run.rs"]
mod tests;
