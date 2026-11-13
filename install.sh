#!/bin/sh
# Build bullpen (it needs cargo) and link it into ~/.local/bin (or $BIN_DIR).
set -eu
here=$(cd "$(dirname "$0")" && pwd)

# bullpen's command line (cli/, with the commands not moved yet in Python, bullpen/)
# and its task board (board/) are Rust programs, built in place: bin/bullpen runs
# target/bullpen, which runs target/bullpen-board. bullpen rebuilds them by itself when
# their source changes (`bullpen up`, and an accept in this checkout).
# cargo as bullpen's build finds it (board_build.py's cargo_command and run_cargo): the
# one on PATH, else rustup's; a mise shim with no Rust version set here falls back to
# rustup's, or says to set one.
rustup=$HOME/.cargo/bin/cargo
cargo=$(command -v cargo || echo "$rustup")
if ! [ -x "$cargo" ]; then
    echo "install.sh: bullpen needs cargo, and it isn't on PATH: install Rust (https://rustup.rs), then run this again" >&2
    exit 1
fi
mkdir -p "$here/target"
log=$here/target/install.log

# Build bullpen's command line with cargo, its output shown and kept in $log.
build_cli() {
    { "$1" build --release --manifest-path "$here/Cargo.toml" -p bullpen-cli 2>&1; echo $? >"$log.status"; } |
        tee "$log"
    [ "$(cat "$log.status")" = 0 ]
}

# `bullpen build` is the command line's own, so the command line is built here first,
# every time (cargo does nothing when it's current): whatever state the build is in
# (missing, older than its source, a build that failed, one whose commands moved since),
# this puts a command line built from the source as it is in place.
if ! build_cli "$cargo"; then
    if ! grep -q "No version is set for shim" "$log"; then
        echo "install.sh: bullpen wasn't built (see above; log: $log)" >&2
        exit 1
    fi
    if ! [ -x "$rustup" ] || [ "$(readlink -f "$rustup")" = "$(readlink -f "$cargo")" ]; then
        echo "install.sh: $cargo is a mise shim with no Rust version set for $here: set one (for example \`mise use -g rust\`, or a mise.toml in $here), or install rustup's cargo" >&2
        exit 1
    fi
    echo "install.sh: $cargo is a mise shim with no version set here: trying $rustup" >&2
    build_cli "$rustup" || { echo "install.sh: bullpen wasn't built (see above; log: $log)" >&2; exit 1; }
fi
rm -f "$log.status"
cp "$here/target/release/bullpen" "$here/target/.bullpen.install"
mv "$here/target/.bullpen.install" "$here/target/bullpen"
# then both programs, with their stamps
"$here/bin/bullpen" build || { echo "install.sh: bullpen wasn't built (see above)" >&2; exit 1; }

dir=${BIN_DIR:-$HOME/.local/bin}
mkdir -p "$dir"
ln -sf "$here/bin/bullpen" "$dir/bullpen"
echo "linked $dir/bullpen -> $here/bin/bullpen"
command -v rex >/dev/null || echo "note: rex isn't on PATH"
command -v gh >/dev/null || echo "note: gh isn't on PATH (needed to open pull requests)"
