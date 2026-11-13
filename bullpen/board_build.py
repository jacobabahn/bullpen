"""Building bullpen's two programs from its own checkout: the command line and the board."""

import fcntl
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from .core import (
    Fail, HOME, Repo, SELF, command, file_hash, git, now, read_json, repo_root, write_json,
)
from .news import news
from .board import BOARD, CARGO_BOARD, TARGET


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


def building():
    if not BUILD_LOCK.exists():
        return False
    f = lock_build(wait=False)
    if f is None:
        return True
    f.close()
    return False


def names(progs):
    return " and ".join(p.what for p in progs)


def rebuild_if_stale(repo):
    """`up`, and an accept in HOME's own repo: when a program's source isn't what it was
    built from, build it in the background. Says what it started; never fails."""
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
        with lock:
            subprocess.Popen([SELF, "_build_board", "--lock-fd", str(lock.fileno())], cwd=repo.root,
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                             start_new_session=True, pass_fds=(lock.fileno(),))
        their = "their source" if len(stale) > 1 else "its source"
        boards = "; running boards reload when it's done" if BOARD_PROGRAM in stale else ""
        return f"rebuilding {names(stale)} in the background ({their} changed){boards}"
    except (Fail, OSError) as e:
        print(f"bullpen: couldn't check bullpen's build: {e}", file=sys.stderr)
        return None


MISE_NO_VERSION = "No version is set for shim"


def cargo_command():
    """[cargo] to build with, or (None, why not)."""
    cargo = shutil.which("cargo")
    if not cargo:
        rustup = Path.home() / ".cargo" / "bin" / "cargo"
        if os.access(rustup, os.X_OK):
            return [str(rustup)], None
        return None, "cargo isn't on PATH (install Rust: https://rustup.rs)"
    return [cargo], None


def run_cargo(cargo, log, progs, nice=False):
    """Build these programs with cargo, its output appended to log: (ok, why not)."""
    argv = [*cargo, "build", "--release", "--manifest-path", str(HOME / "Cargo.toml")]
    for p in progs:
        argv += ["-p", p.package]
    proc = subprocess.run(["nice", *argv] if nice else argv, cwd=HOME, stdin=subprocess.DEVNULL, text=True,
                          capture_output=True)
    log.write(f"$ {' '.join(argv)}\n{proc.stdout}{proc.stderr}")
    log.flush()
    if proc.returncode == 0:
        return True, None
    out = proc.stdout + proc.stderr
    if MISE_NO_VERSION in out:
        rustup = Path.home() / ".cargo" / "bin" / "cargo"
        if os.access(rustup, os.X_OK) and rustup.resolve() != Path(cargo[0]).resolve():
            log.write(f"{cargo[0]} is a mise shim with no version set here: trying {rustup}\n")
            return run_cargo([str(rustup)], log, progs, nice)
        return False, (f"{cargo[0]} is a mise shim with no Rust version set for {HOME}: set one "
                       f"(for example `mise use -g rust`, or a mise.toml in {HOME}), or install rustup's cargo")
    lines = [l for l in out.splitlines() if l.strip()]
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
        cargo, why = cargo_command()
        ok = False
        if cargo:
            ok, why = run_cargo(cargo, log, progs, nice)
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


def cmd_build(args):
    """Build bullpen's command line and the board now, whatever their source holds
    (install.sh runs this)."""
    progs = programs()
    if not progs:
        raise Fail(f"{HOME} has no Cargo.toml: nothing to build")
    lock = lock_build(wait=False)
    if lock is None:
        print("another build of bullpen is running: waiting for it")
        lock = lock_build(wait=True)
    with lock:
        failed, why, built = build(progs)
    if failed:
        raise Fail(f"{names(failed)} weren't built: {why} (log: {BUILD_LOG})" if len(failed) > 1 else
                   f"{names(failed)} wasn't built: {why} (log: {BUILD_LOG})")
    for p in built:
        print(f"built {p.what}: {p.installed}")


def cmd_build_bg(args):
    """`up` or accept started this, holding the build lock for it (--lock-fd), which
    it keeps until it's done, news included."""
    with open(args.lock_fd, "a", closefd=True):
        progs = programs(background=True)
        try:
            failed, why, _ = build(progs, background=True)
        except Exception as e:  # noqa: BLE001 - say it once, whatever it was
            failed, why = [p for p in progs if p.stale()], f"{type(e).__name__}: {e}"
            for p in failed:
                p.set_stamp(failed=p.source_hash())
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
        return f"bullpen: bullpen isn't built: run `{SELF} build` (it needs cargo)"
    failed = read_json(CLI.stamp, {}).get("failed")
    log = f" (its last build failed: {BUILD_LOG})" if failed else ""
    return f"bullpen: `{name}` isn't in this build of bullpen: rebuild it with `bullpen build`{log}"


def commands(sub):
    """This module's commands, for bullpen's parser."""
    command(sub, "build", cmd_build, "Setup",
            help="build bullpen and the board now and put them in place (install.sh runs it)")

    p = command(sub, "_build_board", cmd_build_bg)
    p.add_argument("--lock-fd", type=int, required=True)
