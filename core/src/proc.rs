//! Running programs as core.run does: stdin from /dev/null (or the input given),
//! output read as text, \r\n and \r read as \n (Python's text=True); and git,
//! finding a program on PATH, and a file's hash.

use crate::fail::{fail, Res};
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// A program's run: its exit code (minus the signal, when one killed it, as Python's
/// returncode) and its output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Done {
    /// Did it exit 0?
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

/// core.run: run argv and wait for it. Its stdin is /dev/null, so it never reads our
/// terminal (from the board, it would wait there for keys meant for the board). With
/// check, a failure is a Fail that quotes the command and what it said.
pub fn run<S: AsRef<str>>(argv: &[S], check: bool) -> Res<Done> {
    run_with(argv, None, None, check)
}

/// run, in the folder cwd.
pub fn run_in<S: AsRef<str>>(argv: &[S], cwd: &Path, check: bool) -> Res<Done> {
    run_with(argv, Some(cwd), None, check)
}

/// run, with input as its stdin.
pub fn run_input<S: AsRef<str>>(argv: &[S], input: &str, check: bool) -> Res<Done> {
    run_with(argv, None, Some(input), check)
}

/// run, in cwd when given, with input as its stdin when given.
pub fn run_with<S: AsRef<str>>(argv: &[S], cwd: Option<&Path>, input: Option<&str>, check: bool) -> Res<Done> {
    let argv: Vec<&str> = argv.iter().map(AsRef::as_ref).collect();
    let Some((prog, args)) = argv.split_first() else {
        return fail("no command to run");
    };
    let mut cmd = Command::new(prog);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = match spawn(cmd, input) {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return fail(format!("[Errno 2] No such file or directory: '{prog}'"));
        }
        Err(e) => return Err(e.into()),
    };
    let done = Done {
        code: out.status.code().unwrap_or_else(|| -out.status.signal().unwrap_or(1)),
        stdout: text(&out.stdout),
        stderr: text(&out.stderr),
    };
    if check && !done.ok() {
        let said = if done.stderr.is_empty() {
            &done.stdout
        } else {
            &done.stderr
        };
        return fail(format!("`{}` failed: {}", argv.join(" "), crate::text::strip(said)));
    }
    Ok(done)
}

/// Start the command, give it input (from a thread, so a program that writes before
/// it has read everything can't stall us both), and wait for its output.
fn spawn(mut cmd: Command, input: Option<&str>) -> std::io::Result<Output> {
    let mut child = cmd.spawn()?;
    let writer = match (input, child.stdin.take()) {
        (Some(text), Some(mut stdin)) => {
            let bytes = text.as_bytes().to_vec();
            Some(std::thread::spawn(move || {
                let _ = stdin.write_all(&bytes); // a program that exits without reading it is fine
            }))
        }
        _ => None,
    };
    let out = child.wait_with_output();
    if let Some(w) = writer {
        let _ = w.join();
    }
    out
}

/// Output as Python's text mode reads it: UTF-8 (a bad byte as U+FFFD), with \r\n
/// and \r as \n.
pub fn text(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    if !s.contains('\r') {
        return s.into_owned();
    }
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// core.git: git -C repo, then args.
pub fn git(repo: impl AsRef<Path>, args: &[&str], check: bool) -> Res<Done> {
    let repo = repo.as_ref().display().to_string();
    let mut argv = vec!["git", "-C", &repo];
    argv.extend_from_slice(args);
    run(&argv, check)
}

/// The commit a worktree has checked out.
pub fn head(wt: impl AsRef<Path>) -> Res<String> {
    Ok(crate::text::strip(&git(wt, &["rev-parse", "HEAD"], true)?.stdout).into())
}

/// Where HEAD left base_ref; None when git can't say.
pub fn merge_base(wt: impl AsRef<Path>, base_ref: &str) -> Res<Option<String>> {
    let out = git(wt, &["merge-base", base_ref, "HEAD"], false)?;
    let sha = crate::text::strip(&out.stdout);
    Ok((!sha.is_empty()).then(|| sha.into()))
}

/// core.resolve: the command with its program's absolute path, so Rex panes don't
/// depend on PATH (as it is, when it isn't found).
pub fn resolve(cmd: &[String]) -> Vec<String> {
    let mut out = cmd.to_vec();
    if let Some(found) = cmd.first().and_then(|p| which(p)) {
        out[0] = found;
    }
    out
}

/// shutil.which: a name with a / is itself when it can be run; any other is looked for
/// in each $PATH folder (os.defpath when PATH isn't set).
pub fn which(name: &str) -> Option<String> {
    if name.contains('/') {
        return runnable(Path::new(name)).then(|| name.into());
    }
    let path = std::env::var("PATH").unwrap_or_else(|_| "/bin:/usr/bin".into());
    path.split(':')
        .map(|dir| {
            if dir.is_empty() {
                name.to_string()
            } else {
                format!("{dir}/{name}")
            }
        })
        .find(|p| {
            // an empty entry is the current folder, which os.path.join("", name) names
            runnable(Path::new(p))
        })
}

/// A file (not a folder) we may run.
fn runnable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    if !path.exists() || path.is_dir() {
        return false;
    }
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: c is a valid NUL-terminated path for the length of the call
    unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 }
}

/// core.file_hash: the file's SHA-256, in hex.
pub fn file_hash(path: impl AsRef<Path>) -> Res<String> {
    Ok(crate::sha256::hex(&std::fs::read(path)?))
}

#[cfg(test)]
#[path = "test_proc.rs"]
mod tests;
