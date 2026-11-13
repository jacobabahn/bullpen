#!/bin/sh
# Build the Mojo bullpen into mojo/build/bullpen, with the Mojo in the worktree's .venv.
set -e
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$here/build"
"$here/../.venv/bin/mojo" build -I "$here" "$here/main.mojo" -o "$here/build/bullpen" "$@" 2>&1 | grep -v Crashpad || true
test -x "$here/build/bullpen"
