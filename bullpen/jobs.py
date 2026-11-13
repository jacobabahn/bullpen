"""bullpen's background jobs (the push, the build, an auto-accept, a tour, a gone
coordinator's look): started so that they outlive the command and the pane it runs in,
with their output in a log and their failure in the news."""

import os
import shlex
import subprocess
import sys
from pathlib import Path

from .core import now

# A command often runs in an agent's pane (Claude Code's Bash tool), and may close that
# pane last (a verdict closes its reviewer's). When a pane's program ends, it kills the
# whole tree of the command it ran, by parent: a new session isn't enough, a job that
# is still the command's child dies with it (BP-118: an auto-accept's push and build
# did). So a job is started by a supervisor that leaves the command's tree at once (a
# double fork: its parent exits, and init adopts it), runs the job with its output in
# the log, and says a failure the job didn't say itself.

LOG_MAX = 256 * 1024  # the log is cut to its last half past this

# What says a failed job, in the order registered: fn(cwd, task_id, line). news.py
# registers news; a command that never imported it only logs.
FAILED = []


def background(argv, cwd, log, what, task=None, pass_fds=(), wait=False):
    """Start argv as a job of its own, in cwd: it outlives this command and its pane.
    Its output goes to log (a file); if it fails without saying so (it was killed, or
    it ended non-zero with output, a traceback say), "<what> failed: ..." goes to
    FAILED. A job that says its own failure (in the news) prints nothing and exits 1.
    wait (the tests): run it here and wait for it. Returns the job's pid, or None."""
    if wait:
        return supervise(argv, cwd, log, what, task, pass_fds)
    sys.stdout.flush()  # what this command printed is its own, not the supervisor's
    sys.stderr.flush()
    r, w = os.pipe()
    pid = os.fork()
    if pid == 0:
        try:
            os.close(r)
            os.setsid()
            if os.fork():
                os._exit(0)  # the supervisor's parent ends now: it leaves this command's tree
            quiet()
            supervise(argv, cwd, log, what, task, pass_fds, w)
        except BaseException:  # noqa: BLE001 - never back into the command's code
            pass
        os._exit(0)
    os.close(w)
    os.waitpid(pid, 0)
    with os.fdopen(r, "rb") as f:
        job = f.read().strip()
    return int(job) if job.isdigit() else None


def quiet():
    """The supervisor's stdin and stdout away from the pane's terminal."""
    null = os.open(os.devnull, os.O_RDWR)
    for fd in (0, 1, 2):
        os.dup2(null, fd)
    os.close(null)


def supervise(argv, cwd, log, what, task, pass_fds=(), tell=None):
    """Run the job, its output appended to log; say its failure. tell: a pipe to write its pid to."""
    log = Path(log)
    log.parent.mkdir(parents=True, exist_ok=True)
    trim(log)
    with log.open("a") as f:
        f.write(f"{now()} {shlex.join(str(a) for a in argv)}\n")
        f.flush()
        start = f.tell()
        try:
            p = subprocess.Popen(argv, cwd=cwd, stdin=subprocess.DEVNULL, stdout=f, stderr=f, pass_fds=pass_fds)
        except OSError as e:
            p, code, why = None, None, f"it didn't start: {e}"
        if tell is not None:
            os.write(tell, str(p.pid if p else "").encode())
            os.close(tell)
        if p:
            code = p.wait()
            why = reason(log, start, code)
        if tell is not None:  # the supervisor's copy of a lock the job held: free once it's done
            for fd in pass_fds:
                os.close(fd)
        if why:
            f.write(f"{now()} failed: {why}\n")
    if why:
        line = f"{what} failed: {why}. Log: {log}"
        for fn in FAILED:
            try:
                fn(cwd, task, line)
            except Exception as e:  # noqa: BLE001 - the log has it
                with log.open("a") as f:
                    f.write(f"{now()} and saying so failed: {e}\n")
    return p.pid if p else None


def reason(log, start, code):
    """Why the job failed, from its exit code and what it wrote, or None when it didn't
    (or said so itself: non-zero with no output)."""
    if code == 0:
        return None
    if code < 0:
        return f"it was killed (signal {-code})"
    with log.open("rb") as f:
        f.seek(start)
        out = f.read().decode(errors="replace")
    lines = [l.strip() for l in out.splitlines() if l.strip()]
    if not lines:
        return None
    return f"{lines[-1]} (exit {code})"


def trim(log):
    """Keep the log small: past LOG_MAX, only its last half (from a line's start)."""
    try:
        if log.stat().st_size <= LOG_MAX:
            return
        data = log.read_bytes()[-LOG_MAX // 2:]
        log.write_bytes(data[data.find(b"\n") + 1:])
    except OSError:
        pass
