"""The board's program, and a coordinator's board pane beside it: what `up`, doctor and
board_build use. The `board` command is Rust's (cli/src/board.rs), with its own copy."""

import json
import os
import sys
from pathlib import Path

from .core import HOME, REX, SELF, index_block, run, unindex_block
from .rex import board_label, rex_alive, rex_close, with_state


def ensure_board(repo, session, c, out=sys.stdout, said=None):
    """Coordinator c's task board, in a narrow pane to its right. A board running an
    older build than `bullpen board` runs now (old_board) is replaced: it never reloads."""
    b = c.get("board")
    old = None
    if b and rex_alive(b.get("session"), b.get("block")):
        if not old_board(b):
            return
        old = b
    p = run([REX, "split", "--json", "--focus=false", "-s", session, "-b", c["block"],
             "--split=right", "--ratio", "35", "--label", board_label(repo),
             "-c", str(repo.root), "--", *with_state([SELF, "board", "--coordinator", c["id"]])])
    block = json.loads(p.stdout)["block_ids"][0]
    if b:
        unindex_block(b.get("block"))
    c["board"] = {"block": block, "session": session}
    index_block(block, repo, "board")
    repo.save()
    if old:  # only once the new one is up
        rex_close(old.get("session"), old["block"])
    what = "restarted (it ran an older build)" if old else "started"
    if said is None:
        print(f"board {what}: {block}", file=out)
    else:
        said.append((what, "board", block))


def board_program(b):
    """The program a board's pane runs, as it was started (the path a board reloads
    from), or None when Rex can't say or it isn't the board yet."""
    p = run([REX, "block", "call", *(["-s", b["session"]] if b.get("session") else []), "-b", b["block"],
             "process"], check=False)
    if p.returncode != 0:
        return None
    try:
        child = json.loads(p.stdout).get("child") or {}
    except (ValueError, AttributeError):
        return None
    prog = child.get("invoked_path") or child.get("argv0") or ""
    if not os.path.isabs(prog):
        prog = child.get("path") or ""
    if not prog or Path(prog).name not in ("bullpen-board", board_exe().name):
        return None  # still `bullpen board`, about to exec it, or Rex doesn't say
    return prog


def old_board(b):
    """A running board's program, when it isn't the one `bullpen board` runs now: an
    older install's path, which a rebuild never replaces, so the board never reloads.
    (A board on the right path whose file was rebuilt reloads by itself.) Else None."""
    prog = board_program(b)
    if prog and os.path.realpath(prog) != os.path.realpath(board_exe()):
        return prog
    return None


TARGET = HOME / "target"  # the cargo workspace's (board_build.py builds it)
# What boards run: a copy of cargo's output, renamed into place once it's whole, so a
# board never runs a half-written file (cargo's own output is a hard link it rewrites).
BOARD = TARGET / "bullpen-board"
CARGO_BOARD = TARGET / "release" / "bullpen-board"


def board_exe():
    """The board program `bullpen board` runs: BULLPEN_BOARD, else the copy bullpen
    builds, else cargo's own output."""
    if os.environ.get("BULLPEN_BOARD"):
        return Path(os.environ["BULLPEN_BOARD"])
    return BOARD if BOARD.exists() or not CARGO_BOARD.exists() else CARGO_BOARD
