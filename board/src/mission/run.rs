//! Running mission control: its command line, the loop, and what keeps it current.
//!
//! One `notify` watch on the whole state folder (every repo's tasks, doing/, a new
//! repo appearing) reads the snapshot again; so does a check once a second, for an
//! event missed. Rex is asked about the coordinators' panes on a slower clock, off the
//! drawing thread: `bullpen repos --json` every ALIVE_EVERY. When its program is
//! replaced (a rebuild), it execs the new one in place, keeping its selection.
//!
//! Usage: bullpen-board --mission --state <state folder> --bullpen <bullpen program>

use super::{MMsg, Mission, Sel};
use crate::app::{ExeId, Note};
use notify::{RecursiveMode, Watcher};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
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
    let _watcher = watch(&m.state, tx.clone());
    ask_alive(m.bullpen.clone(), m.state.clone(), tx.clone());
    let keys = tx;
    std::thread::spawn(move || loop {
        let msg = match event::read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => MMsg::Key(k),
            Ok(Event::Paste(text)) => MMsg::Paste(text),
            Ok(Event::Resize(..)) => MMsg::Redraw,
            Ok(_) => continue,
            Err(_) => return,
        };
        if keys.send(msg).is_err() {
            return;
        }
    });

    crate::status("state=idle:app=rust");
    let mut term = crate::term_init();
    let (mut stale, mut read) = (true, Instant::now());
    'main: loop {
        if stale || read.elapsed() >= Duration::from_secs(1) {
            m.reload();
            (stale, read) = (false, Instant::now());
            if let Some((path, was)) = &exe {
                if ExeId::of(path).is_some_and(|now| now != *was) && !m.busy() {
                    use std::os::unix::process::CommandExt;
                    crate::term_restore();
                    let err = Command::new(path).args(restart_args(&m)).exec(); // returns only if it failed
                    term = crate::term_init();
                    exe = None;
                    m.msg = Note::failed(format!("Couldn't reload the rebuilt mission control: {err}"));
                }
            }
        }
        m.peek();
        if term.draw(|f| super::draw::draw(f, &mut m)).is_err() {
            break;
        }
        let msg = match rx.recv_timeout(m.tick()) {
            Ok(msg) => msg,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        stale |= matches!(msg, MMsg::Changed | MMsg::Key(_) | MMsg::Board(..));
        if !m.handle(msg) {
            break;
        }
        // a burst of file events is one read
        while let Ok(msg) = rx.try_recv() {
            stale |= matches!(msg, MMsg::Changed | MMsg::Key(_) | MMsg::Board(..));
            if !m.handle(msg) {
                break 'main;
            }
        }
    }
    // let a command that's still running finish
    while m.boards.values().any(|a| !a.running.is_empty()) {
        m.msg = Note::info("Waiting for a command to finish…");
        let _ = term.draw(|f| super::draw::draw(f, &mut m));
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(msg @ MMsg::Board(..)) => {
                m.handle(msg);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            _ => {}
        }
    }
    crate::term_restore();
    crate::status("state=clear");
}

#[cfg(test)]
#[path = "test_run.rs"]
mod tests;
