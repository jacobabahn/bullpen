//! Claude Code's scratch folder for a working directory: `<temp>/claude-<uid>/<the
//! directory's path, each character but a letter or digit turned into ->` (as the folders
//! on a machine show it: /home/me/.cache/x is `-home-me--cache-x`). A task's worker and
//! reviewer run in its worktree, so the worktree's folder is theirs. It goes with the
//! worktree (`names`, `forget`, from hooks.rs's remove_worktree); doctor's `temp` line
//! (`check`) counts those of closed tasks left from before, and `doctor --fix` removes
//! them (`fix`, `leftovers`).
//!
//! Only a folder named for the exact path, right in `<temp>/claude-<uid>/`, is ever
//! removed, and never through a symlink: neither `claude-<uid>` nor the folder may be one,
//! and a link inside the folder goes as a link (remove_dir_all doesn't follow them).

use bullpen_core::fleet;
use bullpen_core::json::{s, strs};
use bullpen_core::paths::state;
use bullpen_core::settings::CLOSED;
use bullpen_core::task::is_claude;
use bullpen_core::Repo;
use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The temp folders Claude Code may keep its scratch in: BULLPEN_CLAUDE_TMP alone when
/// set (the tests'), else CLAUDE_CODE_TMPDIR, TMPDIR and /tmp, each once.
pub fn bases() -> Vec<PathBuf> {
    if let Some(own) = std::env::var_os("BULLPEN_CLAUDE_TMP").filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(own)];
    }
    let mut out: Vec<PathBuf> = Vec::new();
    let set = ["CLAUDE_CODE_TMPDIR", "TMPDIR"].map(std::env::var_os);
    for b in set.into_iter().flatten().filter(|v| !v.is_empty()).map(PathBuf::from) {
        if !out.contains(&b) {
            out.push(b);
        }
    }
    if !out.iter().any(|b| b == Path::new("/tmp")) {
        out.push("/tmp".into());
    }
    out
}

/// `<base>/claude-<uid>`, this user's.
pub fn root(base: &Path) -> PathBuf {
    base.join(format!("claude-{}", unsafe { libc::getuid() }))
}

/// The name Claude Code gives the scratch folder of the working directory path.
pub fn folder_name(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The names of a worktree's scratch folders: for its path as bullpen has it and, while
/// it's there, as the system resolves it. Empty when neither agent is Claude Code.
pub fn names(repo: &Repo, wt: &str) -> Vec<String> {
    let claude = ["worker", "reviewer"].iter().any(|k| is_claude(&strs(&repo.cfg, k)));
    if wt.is_empty() || !claude {
        return Vec::new();
    }
    let mut out = vec![folder_name(wt)];
    if let Ok(real) = std::fs::canonicalize(wt) {
        let real = folder_name(&real.display().to_string());
        if !out.contains(&real) {
            out.push(real);
        }
    }
    out
}

/// The scratch folder name in root, if it's a real folder (not a link) in a real root.
fn folder(root: &Path, name: &str) -> Option<PathBuf> {
    let real_dir = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir());
    if name.is_empty() || name.contains('/') || name == "." || name == ".." || !real_dir(root) {
        return None;
    }
    let dir = root.join(name);
    real_dir(&dir).then_some(dir)
}

/// Remove the scratch folders named names, in each temp folder. Quiet: a folder that
/// can't go stays. Returns those removed.
pub fn forget(names: &[String]) -> Vec<PathBuf> {
    let roots: Vec<PathBuf> = bases().iter().map(|b| root(b)).collect();
    forget_in(&roots, names)
}

/// forget, in the given roots (`<temp>/claude-<uid>` each).
fn forget_in(roots: &[PathBuf], names: &[String]) -> Vec<PathBuf> {
    let mut gone = Vec::new();
    for root in roots {
        for name in names {
            if let Some(dir) = folder(root, name) {
                if std::fs::remove_dir_all(&dir).is_ok() {
                    gone.push(dir);
                }
            }
        }
    }
    gone
}

/// The scratch folders of closed tasks of every repo bullpen knows: named for a closed
/// task's worktree that isn't there, and for no open task's.
pub fn leftovers() -> Vec<PathBuf> {
    let repos = fleet::repos(&state()).unwrap_or_default();
    let (mut open, mut closed) = (HashSet::new(), Vec::new());
    for repo in &repos {
        for t in repo.tasks().unwrap_or_default() {
            let wt = repo.worktree_dir(&t);
            if CLOSED.contains(&s(&t, "status")) {
                closed.push(wt);
            } else {
                open.insert(folder_name(&wt.display().to_string()));
                open.insert(folder_name(s(&t, "worktree")));
            }
        }
    }
    let mut out = Vec::new();
    for root in bases().iter().map(|b| root(b)) {
        for wt in &closed {
            let name = folder_name(&wt.display().to_string());
            if open.contains(&name) || wt.exists() {
                continue;
            }
            if let Some(dir) = folder(&root, &name).filter(|d| !out.contains(d)) {
                out.push(dir);
            }
        }
    }
    out
}

/// The space a folder takes on disk, its links not followed.
pub fn size(dir: &Path) -> u64 {
    let Ok(m) = std::fs::symlink_metadata(dir) else {
        return 0;
    };
    let own = m.blocks() * 512;
    if !m.file_type().is_dir() {
        return own;
    }
    let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
    own + entries.map(|e| size(&e.path())).sum::<u64>()
}

/// How full the filesystem at path is: (percent used, bytes free), as `df` says it.
pub fn fullness(path: &Path) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let frsize = st.f_frsize as u64;
    let used = (st.f_blocks as u64 - st.f_bfree as u64) * frsize;
    let avail = st.f_bavail as u64 * frsize;
    if used + avail == 0 {
        return None;
    }
    Some(((used * 100).div_ceil(used + avail), avail))
}

/// A temp folder this full or fuller makes doctor's `temp` line a warning.
pub const NEARLY_FULL: u64 = 90;

/// doctor's `temp` line: how full each temp folder with Claude scratch in it is (the
/// first, with none), and the scratch folders closed tasks left. "warn" when one is at
/// least NEARLY_FULL percent full, "skip" when none can be told.
pub fn check() -> (&'static str, String) {
    let bases = bases();
    let with: Vec<&PathBuf> = bases.iter().filter(|b| root(b).is_dir()).collect();
    let shown = if with.is_empty() { vec![&bases[0]] } else { with };
    let (mut full, mut told) = (false, false);
    let mut said: Vec<String> = Vec::new();
    for b in shown {
        match fullness(b) {
            Some((pct, free)) => {
                full |= pct >= NEARLY_FULL;
                told = true;
                said.push(format!("{} is {pct}% full ({} free)", b.display(), human(free)));
            }
            None => said.push(format!("can't tell how full {} is", b.display())),
        }
    }
    let left = leftovers();
    let tail = if left.is_empty() {
        "no Claude scratch folders left by closed tasks".to_string()
    } else {
        let total: u64 = left.iter().map(|d| size(d)).sum();
        format!(
            "{} Claude scratch folder{} of closed tasks hold {}: `bullpen doctor --fix` removes them",
            left.len(),
            if left.len() == 1 { "" } else { "s" },
            human(total)
        )
    };
    let st = match (full, told) {
        (true, _) => "warn",
        (false, true) => "ok",
        (false, false) => "skip",
    };
    (st, format!("{}; {tail}", said.join("; ")))
}

/// `doctor --fix`'s part: remove the scratch folders closed tasks left. What it did, or None.
pub fn fix() -> Option<String> {
    let left = leftovers();
    if left.is_empty() {
        return None;
    }
    let (mut n, mut freed) = (0, 0);
    for d in &left {
        let took = size(d);
        if std::fs::remove_dir_all(d).is_ok() {
            n += 1;
            freed += took;
        }
    }
    let mut did = format!(
        "removed {n} Claude scratch folder{} of closed tasks ({})",
        if n == 1 { "" } else { "s" },
        human(freed)
    );
    if n < left.len() {
        did.push_str(&format!("; {} couldn't go", left.len() - n));
    }
    Some(did)
}

/// A size as people read it: "9.8 GB", "120 MB", "4 KB".
pub fn human(bytes: u64) -> String {
    let units = ["bytes", "KB", "MB", "GB", "TB"];
    let mut n = bytes as f64;
    let mut i = 0;
    while n >= 1000.0 && i + 1 < units.len() {
        n /= 1000.0;
        i += 1;
    }
    if i == 0 || n >= 10.0 {
        format!("{} {}", n.round() as u64, units[i])
    } else {
        format!("{n:.1} {}", units[i])
    }
}

#[cfg(test)]
#[path = "test_claude_tmp.rs"]
mod tests;
