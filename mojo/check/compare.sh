#!/bin/bash
# Check the Mojo bullpen against the Python one, byte for byte.
#
#   compare.sh [CASES...]     (default: every line of cases.txt)
#
# Each case is a command line for bullpen, like `show BP-3` or `ls --all`. For each,
# a fresh fixture (fixture.py) is made, Python's bin/bullpen runs in it, then a fresh
# one again for mojo/build/bullpen: their stdout, stderr, exit code and the state
# folder afterwards (every file but locks) must be the same. A case starting `tty `
# runs on a terminal 100 columns wide (`tty40 `: 40 wide). MOJO_BIN: the binary to
# check (default mojo/build/bullpen). Prints one line per case,
# a diff for each that differs, and exits 1 if any did.
here=$(cd "$(dirname "$0")" && pwd)
top=$(cd "$here/../.." && pwd)
py="$top/bin/bullpen"
mj=${MOJO_BIN:-$top/mojo/build/bullpen}
export BULLPEN_MOJO_STRICT=1  # an unported command fails, rather than run the Python
work=${COMPARE_DIR:-$(mktemp -d)}
now=$(python3 -c 'import time; print(time.time())')
fail=0

if [ $# -gt 0 ]; then cases=("$@"); else mapfile -t cases < <(grep -v '^\s*\(#\|$\)' "$here/cases.txt"); fi

run_one() {  # run_one PROGRAM CASE OUTDIR
    local prog=$1 case=$2 out=$3 cols=""
    mkdir -p "$out"
    local envs
    envs=$(python3 "$here/fixture.py" "$work/fx" "$now") || { echo "fixture.py failed" >&2; exit 2; }
    eval "$envs"
    case "$case" in
        tty40\ *) cols=40; case=${case#tty40 } ;;
        tty\ *) cols=100; case=${case#tty } ;;
    esac
    (
        cd "$FIXTURE_ROOT" || { echo "no fixture" >&2; exit 2; }
        if [ -n "$cols" ]; then
            script -qec "stty cols $cols rows 50; $prog $case; echo \"[exit \$?]\"" /dev/null >"$out/stdout" 2>&1 </dev/null
            echo 0 >"$out/code"
        else
            eval "\"$prog\" $case" >"$out/stdout" 2>"$out/stderr" </dev/null
            echo $? >"$out/code"
        fi
    )
    (cd "$work/fx" && find state -type f ! -name '*.lock' | sort | while read -r f; do echo "== $f"; cat "$f"; done) >"$out/state"
    sed -i "s#$work/fx#FX#g" "$out"/*
}

for c in "${cases[@]}"; do
    rm -rf "$work/py" "$work/mojo"
    run_one "$py" "$c" "$work/py" || exit 2
    run_one "$mj" "$c" "$work/mojo" || exit 2
    if diff -q "$work/py" "$work/mojo" >/dev/null; then
        echo "same     $c"
    else
        echo "DIFFERS  $c"
        diff -r "$work/py" "$work/mojo" | head -${DIFF_LINES:-40}
        fail=1
    fi
done
[ -z "$COMPARE_DIR" ] && rm -rf "$work"
exit $fail
