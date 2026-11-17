//! bullpen's background jobs: the push, the build, an auto-accept, a
//! tour, a gone coordinator's look. Each is started so that it outlives the command and
//! the pane it runs in, with its output in a log (the repo's jobs.log) and its failure
//! in the news.
//!
//! A command often runs in an agent's pane (Claude Code's Bash tool), and may close that
//! pane last. When a pane's program ends, it kills the whole tree of the command it ran,
//! by parent: a new session isn't enough, a job that is still the command's child dies
//! with it (BP-118). So a job is started by a supervisor that leaves the command's tree
//! at once (a double fork: its parent exits, and init adopts it), runs the job with its
//! output in the log, and says a failure the job didn't say itself. Never start a job
//! with a bare detached spawn.

use crate::news::{news, Opts};
use bullpen_core::json::Obj;
use bullpen_core::repo::repo_root_of;
use bullpen_core::text::{shell_join, splitlines, strip};
use bullpen_core::timeutil::iso_now;
use bullpen_core::{Repo, Res};
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};

/// The log is cut to its last half past this.
const LOG_MAX: u64 = 256 * 1024;

/// A job to start: what it runs, where, where its output goes, and what it is in words.
pub struct Job<'a> {
    pub argv: Vec<String>,
    pub cwd: &'a Path,
    pub log: &'a Path,
    /// "Pushing main to backup": its failure is "<what> failed: …"
    pub what: String,
    /// the task it is for: its failure is news to that task's coordinator
    pub task: Option<&'a str>,
    /// open fds the job keeps (a lock it holds for the command)
    pub pass_fds: Vec<RawFd>,
}

/// Start the job: it outlives this command and its pane. Its output goes to its log;
/// if it fails without saying so (it was killed, or it ended non-zero with output),
/// "<what> failed: …" is news. A job that says its own failure (in the news) prints
/// nothing and exits 1. wait (the tests): run it here and wait for it. Returns the
/// job's pid, or None.
pub fn background(job: &Job, wait: bool) -> Option<u32> {
    if wait {
        return supervise(job, None);
    }
    // what this command printed is its own, not the supervisor's
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    let mut fds = [0 as RawFd; 2];
    // SAFETY: fds has room for the two ends
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return None;
    }
    let (r, w) = (fds[0], fds[1]);
    // Neither end may reach the job (as os.pipe's can't): EOF on r must come when the
    // supervisor writes the pid, not when the job and all it starts have ended.
    for fd in fds {
        // SAFETY: plain fcntl on fds this process just made (pipe2 isn't on macOS)
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    // SAFETY: this program is single-threaded; the child only runs Rust code it owns
    // and leaves by _exit, never back into the command's code.
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        let _ = std::panic::catch_unwind(|| unsafe {
            libc::close(r);
            libc::setsid();
            if libc::fork() != 0 {
                libc::_exit(0); // the supervisor's parent ends now: it leaves this command's tree
            }
            quiet();
            supervise(job, Some(w));
        });
        unsafe { libc::_exit(0) };
    }
    // SAFETY: closing our copy of the write end, then reading the pipe as a File we own
    unsafe { libc::close(w) };
    if pid > 0 {
        let mut status = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
    }
    let mut said = String::new();
    let _ = unsafe { File::from_raw_fd(r) }.read_to_string(&mut said);
    strip(&said).parse().ok()
}

/// The supervisor's stdin, stdout and stderr away from the pane's terminal.
fn quiet() {
    // SAFETY: plain fd calls on fds this process owns
    unsafe {
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if null >= 0 {
            for fd in 0..3 {
                libc::dup2(null, fd);
            }
            libc::close(null);
        }
    }
}

/// Run the job, its output appended to its log; say its failure. tell: a pipe to write
/// its pid to.
fn supervise(job: &Job, tell: Option<RawFd>) -> Option<u32> {
    let log = job.log;
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    trim(log);
    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log) else {
        if let Some(w) = tell {
            unsafe { libc::close(w) };
        }
        return None;
    };
    let _ = writeln!(f, "{} {}", iso_now(), shell_join(&job.argv));
    let start = f.metadata().map(|m| m.len()).unwrap_or(0);
    let child = start_job(job, &f);
    let pid = child.as_ref().ok().map(|c| c.id());
    if let Some(w) = tell {
        let text = pid.map(|p| p.to_string()).unwrap_or_default();
        // SAFETY: w is the pipe's write end, ours to write and close
        unsafe {
            libc::write(w, text.as_ptr().cast(), text.len());
            libc::close(w);
        }
    }
    let why = match child {
        Err(e) => Some(format!("it didn't start: {e}")),
        Ok(mut c) => match c.wait() {
            Ok(st) => reason(log, start, st.code().unwrap_or_else(|| -st.signal().unwrap_or(0))),
            Err(e) => Some(format!("it couldn't be waited for: {e}")),
        },
    };
    if tell.is_some() {
        // the supervisor's copy of a lock the job held: free once it's done
        for &fd in &job.pass_fds {
            unsafe { libc::close(fd) };
        }
    }
    if let Some(why) = &why {
        let _ = writeln!(f, "{} failed: {why}", iso_now());
        drop(f);
        let line = format!(
            "{} failed: {why}. Log: {}. Tell the person if it isn't yours to settle",
            job.what,
            log.display()
        );
        if let Err(e) = job_failed(job.cwd, job.task, &line) {
            if let Ok(mut f) = OpenOptions::new().append(true).open(log) {
                let _ = writeln!(f, "{} and saying so failed: {e}", iso_now());
            }
        }
    }
    pid
}

/// Start the job's program, its output into the log, keeping the fds it is passed.
fn start_job(job: &Job, log: &File) -> Result<std::process::Child, String> {
    let (prog, args) = job.argv.split_first().ok_or("no command")?;
    let out = log.try_clone().map_err(|e| e.to_string())?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .current_dir(job.cwd)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    let keep = job.pass_fds.clone();
    // SAFETY: only fcntl between fork and exec
    unsafe {
        cmd.pre_exec(move || {
            for &fd in &keep {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    cmd.spawn().map_err(|e| match e.raw_os_error() {
        Some(n) => format!("[Errno {n}] {}: '{prog}'", strerror(&e)),
        None => e.to_string(),
    })
}

/// The OS's words for an error, without Rust's " (os error N)".
fn strerror(e: &std::io::Error) -> String {
    let s = e.to_string();
    s.split(" (os error").next().unwrap_or(&s).to_string()
}

/// Why the job failed, from its exit code (minus the signal that killed it) and what it
/// wrote, or None when it didn't (or said so itself: non-zero with no output).
pub fn reason(log: &Path, start: u64, code: i32) -> Option<String> {
    if code == 0 {
        return None;
    }
    if code < 0 {
        return Some(format!("it was killed (signal {})", -code));
    }
    let mut out = Vec::new();
    if let Ok(mut f) = File::open(log) {
        let _ = f.seek(SeekFrom::Start(start));
        let _ = f.read_to_end(&mut out);
    }
    let out = String::from_utf8_lossy(&out);
    let last = splitlines(&out).into_iter().map(strip).rfind(|l| !l.is_empty())?;
    Some(format!("{last} (exit {code})"))
}

/// Keep the log small: past LOG_MAX, only its last half (from a line's start).
pub fn trim(log: &Path) {
    let Ok(meta) = std::fs::metadata(log) else {
        return;
    };
    if meta.len() <= LOG_MAX {
        return;
    }
    let Ok(data) = std::fs::read(log) else {
        return;
    };
    let tail = &data[data.len() - (LOG_MAX / 2) as usize..];
    let from = tail.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1);
    let _ = std::fs::write(log, &tail[from..]);
}

/// A background job failed without saying so: news to the task's coordinator, or to
/// everyone (news.py's job_failed).
fn job_failed(cwd: &Path, tid: Option<&str>, line: &str) -> Res<()> {
    let repo = Repo::new(repo_root_of(cwd)?)?;
    let job: Obj = json!({"id": "job"}).as_object().cloned().unwrap_or_default();
    let task = tid.filter(|t| !t.is_empty()).and_then(|t| repo.task(t).ok());
    let everyone = task.is_none();
    news(
        &repo,
        task.as_ref().unwrap_or(&job),
        "job",
        line,
        Opts {
            everyone,
            ..Opts::default()
        },
    )
}

#[cfg(test)]
#[path = "test_jobs.rs"]
mod tests;
