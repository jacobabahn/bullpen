//! Reloading the board when its program is replaced: telling one program file from
//! another, and the command line the new one starts with.

use super::{App, Mode};
use std::path::{Path, PathBuf};

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

impl App {
    /// The program to exec in place of this one, when the board's program was replaced
    /// and nothing would be lost: no command running, no y/n asked, nothing being typed
    /// or answered. (An overlay just closes.)
    pub fn replaced(&self) -> Option<PathBuf> {
        let (path, was) = self.exe.as_ref()?;
        match ExeId::of(path) {
            Some(now) if now != *was && !self.busy() => Some(path.clone()),
            _ => None,
        }
    }

    /// Would a reload lose something: a command running, a y/n asked, something being
    /// typed or answered?
    pub fn busy(&self) -> bool {
        !self.running.is_empty()
            || self.pending.is_some()
            || matches!(self.mode, Mode::Tell { .. } | Mode::Answer { .. } | Mode::Stale { .. } | Mode::Quit)
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
}

/// The command line: where to look, and (from a reload) what the board showed.
#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub(super) dir: PathBuf,
    pub(super) root: PathBuf,
    pub(super) bullpen: PathBuf,
    pub(super) me: Option<String>,
    pub(super) select: Option<String>,
    pub(super) closed: bool,
    pub(super) reloaded: bool,
}

pub fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<Args, String> {
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

#[cfg(test)]
#[path = "test_restart.rs"]
mod tests;
