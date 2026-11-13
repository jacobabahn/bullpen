#!/bin/sh
# Build bullpen (it needs cargo) and link it into ~/.local/bin (or $BIN_DIR).
set -eu
here=$(cd "$(dirname "$0")" && pwd)

# bullpen's command line (cli/, with the commands not moved yet in Python, bullpen/)
# and its task board (board/) are Rust programs, built in place: bin/bullpen runs
# target/bullpen, which runs target/bullpen-board. bullpen rebuilds them by itself when
# their source changes (`bullpen up`, and an accept in this checkout).
if ! command -v cargo >/dev/null && ! [ -x "$HOME/.cargo/bin/cargo" ]; then
    echo "install.sh: bullpen needs cargo, and it isn't on PATH: install Rust (https://rustup.rs), then run this again" >&2
    exit 1
fi
# `bullpen build` is the command line's own: the first time, build it with cargo here.
if ! [ -x "$here/target/bullpen" ]; then
    cargo=cargo
    command -v cargo >/dev/null || cargo=$HOME/.cargo/bin/cargo
    "$cargo" build --release --manifest-path "$here/Cargo.toml" -p bullpen-cli &&
        cp "$here/target/release/bullpen" "$here/target/.bullpen.install" &&
        mv "$here/target/.bullpen.install" "$here/target/bullpen" ||
        { echo "install.sh: bullpen wasn't built (see above)" >&2; exit 1; }
fi
"$here/bin/bullpen" build || { echo "install.sh: bullpen wasn't built (see above)" >&2; exit 1; }

dir=${BIN_DIR:-$HOME/.local/bin}
mkdir -p "$dir"
ln -sf "$here/bin/bullpen" "$dir/bullpen"
echo "linked $dir/bullpen -> $here/bin/bullpen"
command -v rex >/dev/null || echo "note: rex isn't on PATH"
command -v gh >/dev/null || echo "note: gh isn't on PATH (needed to open pull requests)"
