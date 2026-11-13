"""Rebuilding the board when its source changes."""

import fcntl
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from .core import (
    BOARD, BOARD_LOCK, BOARD_LOG, BOARD_SRC, BOARD_STAMP, CARGO_BOARD, Fail, HOME, Repo, SELF,
    TARGET, command, file_hash, git, news, now, read_json, repo_root, write_json,
)


# bullpen's own checkout (HOME) builds the board. Its source has a hash; the build
# records the hash it was built from (BOARD_STAMP). `up`, and an accept in HOME's own
# repo, start a build in the background when the source no longer matches. A running
# board sees its program replaced and execs the new one. A build that fails, or one
# skipped because board/ has uncommitted changes, is one news line per source hash, and
# the old board keeps running.

def board_sources():
    d = BOARD_SRC
    return [d / "Cargo.toml", d / "Cargo.lock", *(p for p in (d / "src").rglob("*") if p.is_file())]


def board_source_hash():
    """A hash of the board's source: Cargo.toml, Cargo.lock and src/, by path."""
    parts = {str(p.relative_to(BOARD_SRC)): file_hash(p) for p in board_sources() if p.is_file()}
    return hashlib.sha256(json.dumps(parts, sort_keys=True).encode()).hexdigest()


def board_dirty():
    """Does HOME have uncommitted changes to the board's source (new files too)?"""
    p = git(HOME, "status", "--porcelain", "--", "board/Cargo.toml", "board/Cargo.lock", "board/src",
            check=False)
    return p.returncode == 0 and bool(p.stdout.strip())


def lock_board(wait):
    """The build lock's open file, locked, or None when another build holds it (wait=False)."""
    TARGET.mkdir(parents=True, exist_ok=True)
    f = BOARD_LOCK.open("a")
    try:
        fcntl.flock(f, fcntl.LOCK_EX | (0 if wait else fcntl.LOCK_NB))
    except BlockingIOError:
        f.close()
        return None
    return f


def board_building():
    if not BOARD_LOCK.exists():
        return False
    f = lock_board(wait=False)
    if f is None:
        return True
    f.close()
    return False


def set_board_stamp(**kw):
    st = read_json(BOARD_STAMP, {})
    st.update(kw)
    write_json(BOARD_STAMP, {k: v for k, v in st.items() if v})


BOARD_NEWS = {"id": "board"}  # the news log's task column for these lines


def rebuild_board_if_stale(repo):
    """`up`, and an accept in HOME's own repo: when the board's source isn't what it
    was built from, build it in the background. Says what it started; never fails."""
    try:
        if os.environ.get("BULLPEN_BOARD") or not (BOARD_SRC / "Cargo.toml").exists():
            return None  # a board of the person's own choosing, or no source to build
        src, st = board_source_hash(), read_json(BOARD_STAMP, {})
        if src in (st.get("source"), st.get("failed")):
            return None  # current, or its build failed already: doctor says so
        if board_dirty():
            if st.get("skipped") != src:
                set_board_stamp(skipped=src)
                news(repo, BOARD_NEWS, "board", (
                    f"The board wasn't rebuilt: {BOARD_SRC} has uncommitted changes, and a build would take "
                    f"them in. Boards keep running the old one. Commit or drop them, then run `bullpen up` "
                    f"(or `bullpen build-board` to build them as they are)."), everyone=True)
            return None
        lock = lock_board(wait=False)
        if lock is None:
            return None  # a build is running: it checks the source again when it's done
        with lock:
            subprocess.Popen([SELF, "_build_board", "--lock-fd", str(lock.fileno())], cwd=repo.root,
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                             start_new_session=True, pass_fds=(lock.fileno(),))
        return "rebuilding the board in the background (its source changed); running boards reload when it's done"
    except (Fail, OSError) as e:
        print(f"bullpen: couldn't check the board: {e}", file=sys.stderr)
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


def run_cargo(cargo, log):
    """Build the board with cargo, its output appended to log: (ok, why not)."""
    argv = [*cargo, "build", "--release", "--manifest-path", str(BOARD_SRC / "Cargo.toml")]
    p = subprocess.run(argv, cwd=BOARD_SRC, stdin=subprocess.DEVNULL, text=True, capture_output=True)
    log.write(f"$ {' '.join(argv)}\n{p.stdout}{p.stderr}")
    log.flush()
    if p.returncode == 0:
        return True, None
    out = p.stdout + p.stderr
    if MISE_NO_VERSION in out:
        rustup = Path.home() / ".cargo" / "bin" / "cargo"
        if os.access(rustup, os.X_OK) and rustup.resolve() != Path(cargo[0]).resolve():
            log.write(f"{cargo[0]} is a mise shim with no version set here: trying {rustup}\n")
            return run_cargo([str(rustup)], log)
        return False, (f"{cargo[0]} is a mise shim with no Rust version set for {BOARD_SRC}: set one "
                       f"(for example `mise use -g rust`, or a mise.toml in {HOME}), or install rustup's cargo")
    lines = [l for l in out.splitlines() if l.strip()]
    err = next((l for l in lines if l.startswith("error")), lines[-1] if lines else f"exit {p.returncode}")
    return False, f"cargo failed: {err.strip()}"


def build_board(background):
    """Build the board and put it in place: (ok, what happened). In the background, it
    first checks that the source still needs it."""
    src = board_source_hash()
    st = read_json(BOARD_STAMP, {})
    if background and src in (st.get("source"), st.get("failed")):
        return True, None
    with BOARD_LOG.open("w") as log:
        log.write(f"{now()} building the board from {BOARD_SRC}\n")
        cargo, why = cargo_command()
        ok = False
        if cargo:
            ok, why = run_cargo(cargo, log)
        if ok and not os.access(CARGO_BOARD, os.X_OK):
            ok, why = False, f"cargo made no {CARGO_BOARD}"
        if not ok:
            log.write(f"failed: {why}\n")
            set_board_stamp(failed=src)
            return False, why
        # A copy, renamed into place: a running board never sees a half-written file.
        tmp = TARGET / f".bullpen-board.{os.getpid()}"
        shutil.copy2(CARGO_BOARD, tmp)
        os.replace(tmp, BOARD)
        write_json(BOARD_STAMP, {"source": src})
        log.write(f"built {BOARD}\n")
    return True, f"built the board: {BOARD}"


def cmd_build_board(args):
    """Build the board now, whatever board/ holds (install.sh runs this)."""
    lock = lock_board(wait=False)
    if lock is None:
        print("another build of the board is running: waiting for it")
        lock = lock_board(wait=True)
    with lock:
        ok, said = build_board(background=False)
    if not ok:
        raise Fail(f"the board wasn't built: {said} (log: {BOARD_LOG})")
    print(said)


def cmd_build_board_bg(args):
    """`up` or accept started this, holding the build lock for it (--lock-fd), which
    it keeps until it's done, news included."""
    with open(args.lock_fd, "a", closefd=True):
        try:
            ok, said = build_board(background=True)
        except Exception as e:  # noqa: BLE001 - say it once, whatever it was
            ok, said = False, f"{type(e).__name__}: {e}"
            set_board_stamp(failed=board_source_hash())
        if ok:
            return 0
        try:
            repo = Repo(repo_root())
        except (Fail, ValueError):
            return 1
        news(repo, BOARD_NEWS, "board", f"The board wasn't rebuilt: {said}. Boards keep running the old one. "
             f"Log: {BOARD_LOG}. Fix it, then run `bullpen build-board`.", everyone=True)
        return 1


def commands(sub):
    """This module's commands, for bullpen's parser."""
    command(sub, "build-board", cmd_build_board, "Setup",
            help="build the board from board/ now and put it in place (install.sh runs it)")

    p = command(sub, "_build_board", cmd_build_board_bg)
    p.add_argument("--lock-fd", type=int, required=True)
