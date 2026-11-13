#!/bin/sh
# Build the Mojo bullpen into mojo/build/bullpen (or $BUILD_OUT), with the Mojo in the
# worktree's .venv.
set -e
here=$(cd "$(dirname "$0")" && pwd)
out=${BUILD_OUT:-$here/build/bullpen}
mkdir -p "$(dirname "$out")"
rm -f "$out"
"$here/../.venv/bin/mojo" build -I "$here" "$here/main.mojo" -o "$out" "$@" 2>&1 | grep -v Crashpad || true
test -x "$out"
