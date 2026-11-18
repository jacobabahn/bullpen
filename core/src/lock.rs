//! Lock files, as fcntl.flock takes them (std's File::lock is flock(2) on Linux and
//! macOS, so the Python bullpen and this one exclude each other): the news readers,
//! notifications, the push, the build and tours' writes to a task. A lock is held while
//! its File is open.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// Open the lock file at path (made, with its folder, when missing; never emptied).
pub fn open(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// The lock at path, waiting while another holds it.
pub fn lock(path: &Path) -> io::Result<File> {
    let f = open(path)?;
    f.lock()?;
    Ok(f)
}

/// The lock at path, or None when another holds it now.
pub fn try_lock(path: &Path) -> io::Result<Option<File>> {
    let f = open(path)?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// The lock at path: waiting for it (wait), or None when another holds it.
pub fn take(path: &Path, wait: bool) -> io::Result<Option<File>> {
    if wait {
        lock(path).map(Some)
    } else {
        try_lock(path)
    }
}

#[cfg(test)]
#[path = "tests/test_lock.rs"]
mod tests;
