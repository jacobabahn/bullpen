"""Building bullpen's two programs from its own checkout: the command line and the board."""

import fcntl
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time

from .core import (
    Fail, HOME, Repo, SELF, file_hash, git, now, read_json, repo_root, write_json,
)
from .jobs import background
from .news import news
from .board import BOARD, CARGO_BOARD, TARGET
from .watcher import restart_watcher


# bullpen's own checkout (HOME) is one cargo workspace (Cargo.toml): core/, cli/ (the
# command line, bin/bullpen runs it) and board/. Each program's source has a hash, and
# its build records the hash it was built from (its stamp). `up`, and an accept in
# HOME's own repo, start a build in the background when a source no longer matches. A
# running board sees its program replaced and execs the new one; a command runs the new
# build from its next call. A build that fails, or one skipped because the source has
# uncommitted changes, is one news line per source hash, and the old program keeps running.

BUILD_LOCK = TARGET / "build.lock"
BUILD_LOG = TARGET / "build.log"
SHARED = ("Cargo.toml", "Cargo.lock")  # the workspace's: both programs are built from them
# How long a call waits for a build that's running before it says it's stuck (the tests set it)
BUILD_WAIT = float(os.environ.get("BULLPEN_BUILD_WAIT") or 600)
# The commands that build: Rust's since T5. An older build hands them to the Python (BP-119)
BUILD_ENTRIES = ("build", "build-board", "_build_board")
# A test's or a compare's fixture (tests/support.py, cli/check/): bullpen never builds,
# or takes the build lock in, the checkout it runs from (cli/src/build.rs's twin)
NO_BUILD = "bullpen: builds are off here (BULLPEN_NO_BUILD is set: a test's or a compare's fixture)"


def no_build():
    return bool(os.environ.get("BULLPEN_NO_BUILD"))


class Program:
    """One of the workspace's programs: its crates, and where its build goes. What runs
    is a copy of cargo's output (installed), renamed into place once it's whole, never
    cargo's own, which it rewrites in place."""

    def __init__(self, name, package, crates, what, keeps, news_as):
        self.name, self.package, self.crates, self.what, self.keeps = name, package, crates, what, keeps
        self.news_as = news_as  # (the news log's task column, its kind)
        self.installed = TARGET / name
        self.cargo_out = TARGET / "release" / name
        # {"source": the source hash it was built from, "failed": a hash whose build failed,
        #  "skipped": one not built because the source had uncommitted changes}
        self.stamp = TARGET / f"{name}.json"

    def paths(self):
        """What its source is, relative to HOME: each crate's Cargo.toml and src/, and the workspace's files."""
        return [*SHARED, *(f"{c}/{p}" for c in self.crates for p in ("Cargo.toml", "src"))]

    def files(self):
        out = []
        for rel in self.paths():
            p = HOME / rel
            out += sorted(f for f in p.rglob("*") if f.is_file()) if p.is_dir() else [p]
        return [f for f in out if f.is_file()]

    def source_hash(self):
        """A hash of its source, by path."""
        parts = {str(p.relative_to(HOME)): file_hash(p) for p in self.files()}
        return hashlib.sha256(json.dumps(parts, sort_keys=True).encode()).hexdigest()

    def dirty(self):
        """Does HOME have uncommitted changes to its source (new files too)?"""
        p = git(HOME, "status", "--porcelain", "--", *self.paths(), check=False)
        return p.returncode == 0 and bool(p.stdout.strip())

    def subject(self):
        """Its name to start a sentence with: "The board", "bullpen's command line"."""
        return self.what[0].upper() + self.what[1:] if self.what.startswith("the ") else self.what

    def set_stamp(self, **kw):
        st = read_json(self.stamp, {})
        st.update(kw)
        write_json(self.stamp, {k: v for k, v in st.items() if v})

    def stale(self):
        """Its source is neither what it was built from nor a source whose build failed."""
        st = read_json(self.stamp, {})
        return self.source_hash() not in (st.get("source"), st.get("failed"))


CLI = Program("bullpen", "bullpen-cli", ("core", "cli"), "bullpen's command line",
              "Commands keep running the old build", ("bullpen", "build"))
BOARD_PROGRAM = Program(BOARD.name, "bullpen-board", ("core", "board"), "the board",
                        "Boards keep running the old one", ("board", "board"))
assert BOARD_PROGRAM.installed == BOARD and BOARD_PROGRAM.cargo_out == CARGO_BOARD


def programs(background=False):
    """The programs HOME has the source of. In the background (`up`, accept), not the board
    when BULLPEN_BOARD names a board of the person's own choosing."""
    out = [p for p in (CLI, BOARD_PROGRAM) if (HOME / p.crates[-1] / "Cargo.toml").exists()]
    if background and os.environ.get("BULLPEN_BOARD"):
        out = [p for p in out if p is not BOARD_PROGRAM]
    return out if (HOME / "Cargo.toml").exists() else []


def lock_build(wait):
    """The build lock's open file, locked, or None when another build holds it (wait=False)."""
    TARGET.mkdir(parents=True, exist_ok=True)
    f = BUILD_LOCK.open("a")
    try:
        fcntl.flock(f, fcntl.LOCK_EX | (0 if wait else fcntl.LOCK_NB))
    except BlockingIOError:
        f.close()
        return None
    return f


def mark(lock, pid):
    """Write the build's pid in the lock file it holds, for a call that waits on it to name."""
    lock.truncate(0)
    lock.write(f"{pid}\n")
    lock.flush()


def holder():
    """The pid in the lock file, when that process is running: the build that holds it."""
    try:
        pid = int(BUILD_LOCK.read_text().strip())
        os.kill(pid, 0)
        return pid
    except (OSError, ValueError):
        return None


def wait_for_build():
    """The build lock, once the build that's running is done: never longer than BUILD_WAIT.
    Past it, the build is stuck (or its holder isn't building): say so and exit."""
    deadline = time.monotonic() + BUILD_WAIT
    while (lock := lock_build(wait=False)) is None:
        if time.monotonic() > deadline:
            pid = holder()
            who, stop = (f" (pid {pid})", f"stop it (`kill {pid}`)") if pid else ("", "stop it")
            print(f"bullpen: the build that's running{who} still isn't done after {BUILD_WAIT:g}s: if it's "
                  f"stuck, {stop} and run this again (log: {BUILD_LOG})", file=sys.stderr)
            sys.exit(1)
        time.sleep(0.1)
    return lock


def names(progs):
    return " and ".join(p.what for p in progs)


def rebuild_if_stale(repo):
    """`up`, and an accept in HOME's own repo: when a program's source isn't what it was
    built from, build it in the background. Says what it started; never fails."""
    if no_build():
        return None
    try:
        stale = []
        for p in programs(background=True):
            if not p.stale():
                continue  # current, or its build failed already: doctor says so
            if p.dirty():
                src = p.source_hash()
                if read_json(p.stamp, {}).get("skipped") != src:
                    p.set_stamp(skipped=src)
                    tid, kind = p.news_as
                    news(repo, {"id": tid}, kind, (
                        f"{p.subject()} wasn't rebuilt: {HOME} has uncommitted changes to "
                        f"{', '.join(p.crates)}, and a build would take them in. {p.keeps}. Commit or drop them, "
                        f"then run `bullpen up` (or `bullpen build` to build them as they are)."), everyone=True)
                continue
            stale.append(p)
        if not stale:
            return None
        lock = lock_build(wait=False)
        if lock is None:
            return None  # a build is running: it checks the sources again when it's done
        with lock:  # the job keeps it; this command's copy closes here
            mark(lock, os.getpid())
            job = background([SELF, "_build_board", "--lock-fd", str(lock.fileno())], repo.root,
                             repo.dir / "jobs.log", f"Rebuilding {names(stale)}", pass_fds=(lock.fileno(),))
            if job:
                mark(lock, job)
        their = "their source" if len(stale) > 1 else "its source"
        boards = "; running boards reload when it's done" if BOARD_PROGRAM in stale else ""
        return f"rebuilding {names(stale)} in the background ({their} changed){boards}"
    except (Fail, OSError) as e:
        print(f"bullpen: couldn't check bullpen's build: {e}", file=sys.stderr)
        return None


# The one lookup of cargo every build uses (cli/src/build.rs's twin): it runs the first
# cargo that works here, with what it needs, and when none does says what it tried.
BULLPEN_CARGO = HOME / "bin" / "bullpen-cargo"
NO_CARGO = "bullpen-cargo: no cargo works here"


def run_cargo(log, progs, nice=False):
    """Build these programs with cargo (bin/bullpen-cargo), its output appended to log: (ok, why not)."""
    argv = [str(BULLPEN_CARGO), "build", "--release", "--manifest-path", str(HOME / "Cargo.toml")]
    for p in progs:
        argv += ["-p", p.package]
    proc = subprocess.run(["nice", *argv] if nice else argv, cwd=HOME, stdin=subprocess.DEVNULL, text=True,
                          capture_output=True)
    log.write(f"$ {' '.join(argv)}\n{proc.stdout}{proc.stderr}")
    log.flush()
    if proc.returncode == 0:
        return True, None
    lines = [l for l in (proc.stdout + proc.stderr).splitlines() if l.strip()]
    none = next((l for l in lines if l.startswith(NO_CARGO)), None)
    if none:
        return False, none[len("bullpen-cargo: "):].strip()
    err = next((l for l in lines if l.startswith("error")), lines[-1] if lines else f"exit {proc.returncode}")
    return False, f"cargo failed: {err.strip()}"


def build(progs, background=False, nice=False):
    """Build these programs and put them in place: (the ones that failed, why, what was
    built). In the background, it builds only the ones whose source still needs it and
    has no uncommitted changes."""
    if background:
        progs = [p for p in progs if p.stale() and not p.dirty()]
    if not progs:
        return [], None, []
    srcs = {p.name: p.source_hash() for p in progs}
    with BUILD_LOG.open("w") as log:
        log.write(f"{now()} building {names(progs)} from {HOME}\n")
        ok, why = run_cargo(log, progs, nice)
        missing = [p for p in progs if not os.access(p.cargo_out, os.X_OK)]
        if ok and missing:
            ok, why = False, f"cargo made no {missing[0].cargo_out}"
        if not ok:
            log.write(f"failed: {why}\n")
            for p in progs:
                p.set_stamp(failed=srcs[p.name])
            return progs, why, []
        for p in progs:
            # A copy, renamed into place: a running program never sees a half-written file.
            tmp = TARGET / f".{p.name}.{os.getpid()}"
            shutil.copy2(p.cargo_out, tmp)
            os.replace(tmp, p.installed)
            write_json(p.stamp, {"source": srcs[p.name]})
            log.write(f"built {p.installed}\n")
    return [], None, progs


def watcher_after(built):
    """A new command line: a running watcher is replaced, so it runs the new build."""
    if CLI in built:
        restart_watcher()


def catch_up(argv):
    """The build handed over a command the Python doesn't have: an accept moved it, and
    the build is older than its source (the window between the merge and the new build,
    or a build that never ran). Wait for the build that's running (never for ever), or
    build what's stale now, then run the call again with the new build. Returns only
    when it can't: no build, a source with uncommitted changes, a build that failed, no
    new build, or a second try. Never for a build entry (build_entry): this process may
    be the build, and would wait for itself."""
    # this very call, run again (the build and the Python exec, so the pid stays): not the
    # jobs a call that caught up starts, which may need to catch up themselves
    if no_build():
        return  # a fixture: the call fails with not_in_build's line
    if os.environ.get("BULLPEN_CAUGHT_UP") == str(os.getpid()) or CLI not in programs() or not os.access(CLI.installed, os.X_OK):
        return
    if read_json(CLI.stamp, {}).get("failed") == CLI.source_hash():
        return
    was = os.stat(CLI.installed).st_ino  # a new build is a new file (renamed into place)
    lock = lock_build(wait=False)
    if lock is None:
        print(f"bullpen: `{argv[0]}` is in the build that's running: waiting for it", file=sys.stderr)
        lock = wait_for_build()
    with lock:
        if CLI.stale() and not CLI.dirty():
            print(f"bullpen: `{argv[0]}` is newer than this build: building it first (log: {BUILD_LOG})",
                  file=sys.stderr)
            mark(lock, os.getpid())
            # the board too: an accept's rebuild that finds this build running leaves it to it
            failed, _, built = build(programs(background=True), background=True)
            watcher_after(built)
            if CLI in failed:
                return
    if os.stat(CLI.installed).st_ino == was or read_json(CLI.stamp, {}).get("source") != CLI.source_hash():
        return  # no new build: it would hand the call over again
    sys.stderr.flush()
    os.execve(SELF, [SELF, *argv], {**os.environ, "BULLPEN_CAUGHT_UP": str(os.getpid())})


def build_entry(argv):
    """A build entry an older build handed over (it's Rust's since T5, and the merge
    took the Python's away): build with this twin, so the new build is in place
    whichever side the build that's installed thinks owns it. `_build_board` holds the
    build lock already (--lock-fd): it builds, never waiting for the build that's running."""
    if argv[0] != "_build_board":
        return build_now()
    import argparse
    ap = argparse.ArgumentParser(prog="bullpen _build_board")
    ap.add_argument("--lock-fd", type=int, required=True)
    return build_bg(ap.parse_args(argv[1:]).lock_fd)


def build_now():
    """`build`: build bullpen's command line and the board now, whatever their source holds."""
    if no_build():
        print(NO_BUILD, file=sys.stderr)
        return 1
    progs = programs()
    if not progs:
        print(f"bullpen: {HOME} has no Cargo.toml: nothing to build", file=sys.stderr)
        return 1
    lock = lock_build(wait=False)
    if lock is None:
        print("another build of bullpen is running: waiting for it")
        sys.stdout.flush()
        lock = wait_for_build()
    with lock:
        mark(lock, os.getpid())
        failed, why, built = build(progs)
    watcher_after(built)
    if failed:
        were = "weren't" if len(failed) > 1 else "wasn't"
        print(f"bullpen: {names(failed)} {were} built: {why} (log: {BUILD_LOG})", file=sys.stderr)
        return 1
    for p in built:
        print(f"built {p.what}: {p.installed}")
    return 0


def build_bg(lock_fd):
    """`_build_board`: `up` or accept started this, holding the build lock for it
    (lock_fd), which it keeps until it's done, news included."""
    with open(lock_fd, "a", closefd=True):
        progs = programs(background=True)
        try:
            failed, why, built = build(progs, background=True)
        except Exception as e:  # noqa: BLE001 - say it once, whatever it was
            failed, why, built = [p for p in progs if p.stale()], f"{type(e).__name__}: {e}", []
            for p in failed:
                p.set_stamp(failed=p.source_hash())
        watcher_after(built)
        if not failed:
            return 0
        try:
            repo = Repo(repo_root())
        except (Fail, ValueError):
            return 1
        for p in failed:
            tid, kind = p.news_as
            news(repo, {"id": tid}, kind, f"{p.subject()} wasn't rebuilt: {why}. {p.keeps}. "
                 f"Log: {BUILD_LOG}. Fix it, then run `bullpen build`.", everyone=True)
        return 1


def not_in_build(name):
    """What the Python says of a call it has no command for. The Rust entry prints the
    help and the unknown-command line, so this is a build older than its source (it
    handed over a command whose Python is gone), or no build at all."""
    name = name or "--help"
    if not os.access(CLI.installed, os.X_OK):
        return f"bullpen: bullpen isn't built: run {HOME / 'install.sh'} (it needs cargo)"
    failed = read_json(CLI.stamp, {}).get("failed")
    log = f" (its last build failed: {BUILD_LOG})" if failed else ""
    return f"bullpen: `{name}` isn't in this build of bullpen: rebuild it with `bullpen build`{log}"
