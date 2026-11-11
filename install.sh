#!/bin/sh
# Link bullpen into ~/.local/bin (or $BIN_DIR).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dir=${BIN_DIR:-$HOME/.local/bin}
mkdir -p "$dir"
ln -sf "$here/bin/bullpen" "$dir/bullpen"
echo "linked $dir/bullpen -> $here/bin/bullpen"

# The task board is a Rust program, built in place: bin/bullpen runs it from there.
if command -v cargo >/dev/null; then
  cargo build --release --quiet --manifest-path "$here/board/Cargo.toml"
  echo "built the board: $here/board/target/release/bullpen-board"
else
  echo "note: cargo isn't on PATH, so the board isn't built (https://rustup.rs)"
fi
command -v rex >/dev/null || echo "note: rex isn't on PATH"
command -v gh >/dev/null || echo "note: gh isn't on PATH (needed to open pull requests)"
