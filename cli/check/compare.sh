#!/bin/bash
# Check the Rust bullpen against the Python one, byte for byte (BP-92's mojo/check/compare.sh, for Rust).
#
#   compare.sh [CASES...]     (default: every line of check/cases*.txt)
#   BULLPEN_PY=<main checkout>/bin/bullpen-py compare.sh   (main's Python, as each port task runs it)
#
# Each case is a command line for bullpen, like `show BP-3` or `ls --all`. For each,
# a fresh fixture (fixture.py) is made, the Python bullpen ($BULLPEN_PY, default this
# checkout's bin/bullpen-py) runs in it, then a fresh one again for the Rust build
# (target/release/bullpen): their stdout, stderr, exit code and the state
# folder afterwards (every file but locks) must be the same. A case starting `tty `
# runs on a terminal 100 columns wide (`tty40 `: 40 wide). Leading `env:NAME=VALUE ` words
# set NAME for the run, and for fixture.py (FIXTURE=a,b picks its extras). RUST_BIN: the binary to
# check (default target/release/bullpen: run `cargo build --release -p bullpen-cli` first). Prints one line per case,
# a diff for each that differs, and exits 1 if any did. What the fake rex was asked (rex.log) and
# its windows after (reg.json) are compared too; a checkout's own path reads as TOP. Stamps from the run's own
# time (a command's now()) read as <now>. A case starting `usage ` runs without strict mode (a
# command handed to the Python then runs there), and all of it is compared.
here=$(cd "$(dirname "$0")" && pwd)
top=$(cd "$here/../.." && pwd)
py=${BULLPEN_PY:-$top/bin/bullpen-py}
rs=${RUST_BIN:-$top/target/release/bullpen}
pytop=$(cd "$(dirname "$py")/.." && pwd)
export BULLPEN_RUST_STRICT=1  # an unported command fails, rather than run the Python
work=${COMPARE_DIR:-$(mktemp -d)}
fail=0

if [ $# -gt 0 ]; then cases=("$@"); else mapfile -t cases < <(cat "$here"/cases*.txt | grep -v '^\s*\(#\|$\)'); fi

run_one() {  # run_one PROGRAM CASE OUTDIR
    local prog=$1 case=$2 out=$3 cols="" usage=""
    mkdir -p "$out"
    local envs sets=()
    while [[ $case == env:* ]]; do
        sets+=("${case%% *}"); sets[-1]=${sets[-1]#env:}; case=${case#* }
    done
    envs=$(env "${sets[@]}" python3 "$here/fixture.py" "$work/fx" "$now") || { echo "fixture.py failed" >&2; exit 2; }
    eval "$envs"
    case "$case" in usage\ *) usage=1; case=${case#usage } ;; esac
    [ -n "$usage" ] && sets+=("BULLPEN_RUST_STRICT=")
    case "$case" in
        tty40\ *) cols=40; case=${case#tty40 } ;;
        tty\ *) cols=100; case=${case#tty } ;;
    esac
    local t0
    t0=$(date +%s.%N)
    (
        cd "$FIXTURE_ROOT" || { echo "no fixture" >&2; exit 2; }
        for kv in "${sets[@]}"; do export "$kv"; done
        if [ -n "$cols" ]; then
            script -qec "stty cols $cols rows 50; $prog $case; echo \"[exit \$?]\"" /dev/null >"$out/stdout" 2>&1 </dev/null
            echo 0 >"$out/code"
        else
            eval "\"$prog\" $case" >"$out/stdout" 2>"$out/stderr" </dev/null
            echo $? >"$out/code"
        fi
    )
    (cd "$work/fx" && find state -type f ! -name '*.lock' | sort | while read -r f; do echo "== $f"; cat "$f"; done) >"$out/state"
    for f in rex.log reg.json; do [ -f "$work/fx/fake/$f" ] && cp "$work/fx/fake/$f" "$out/$f"; done
    sed -i "s#$work/fx#FX#g; s#$pytop#TOP#g; s#$top#TOP#g" "$out"/*
    python3 "$here/now_stamps.py" "$t0" "$(date +%s.%N)" "$out"/*
    return 0
}

for c in "${cases[@]}"; do
    rm -rf "$work/py" "$work/rust"
    now=$(date +%s.%N)  # per case: both runs of it see the same ages
    run_one "$py" "$c" "$work/py" || exit 2
    run_one "$rs" "$c" "$work/rust" || exit 2
    if diff -q "$work/py" "$work/rust" >/dev/null; then
        echo "same     $c"
    else
        echo "DIFFERS  $c"
        diff -r "$work/py" "$work/rust" | head -${DIFF_LINES:-40}
        fail=1
    fi
done
[ -z "$COMPARE_DIR" ] && rm -rf "$work"
exit $fail
