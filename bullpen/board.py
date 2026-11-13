"""Where the board's program is built: what board_build uses. The `board` command,
ensure_board and old_board are Rust's (cli/src/board.rs)."""

from .core import HOME

TARGET = HOME / "target"  # the cargo workspace's (board_build.py builds it)
# What boards run: a copy of cargo's output, renamed into place once it's whole, so a
# board never runs a half-written file (cargo's own output is a hard link it rewrites).
BOARD = TARGET / "bullpen-board"
CARGO_BOARD = TARGET / "release" / "bullpen-board"
