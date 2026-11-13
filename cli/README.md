# bullpen in Rust (experiment, BP-95)

A port of part of bullpen's Python CLI to Rust, to compare it with the Python and with
BP-92's Mojo port. See REPORT.md for how far it got and what it measured. Nothing here is
used by bullpen itself.

- Build: `cd cli && cargo build --release` (Rust 1.99). The binary is `cli/target/release/bullpen`.
- Run: `cli/target/release/bullpen ls`, in a repo, with the same `BULLPEN_STATE` as
  `bin/bullpen`. A command it hasn't ported, and any call it can't do as Python would (help,
  a usage error, a config to migrate, a change that notifies), is handed to `bin/bullpen`
  (exec), so it can stand in for it.
- Check: `cli/check/compare.sh [CASE...]` runs each case of `check/cases*.txt` with Python and
  Rust in a fresh fixture state (`check/fixture.py`) and compares stdout, stderr, exit code and
  the state folder after, byte for byte. `BULLPEN_RUST_STRICT=1` (which it sets) makes a
  hand-over to Python fail instead.
- Benchmark: `cli/bench/bench.py` (Python and Rust interleaved; `BENCH_PY` is the Python to
  time, by default the main checkout's). Results in `bench/results.{md,json}`.
- `tools/widths.py` writes `src/widths.rs` from Python's unicodedata.

## The code

`src/main.rs` dispatches a command to its module; `args.rs` reads each command's arguments as
argparse does for the common cases and hands the rest to Python. The modules mirror the Python
package's, with the same function names where it can:

- `core.rs`: paths, running programs, config layering, `Repo`, tasks (`Obj`: the task's JSON
  object in file order), `save_task`, status words. `Fail::Msg` is a command's error
  ("bullpen: ...", exit 1); `Fail::Python` hands the whole call to Python, only before anything
  is printed or written.
- `json.rs`: `dumps` as Python's `json.dumps(indent=2)` writes (ensure_ascii, float repr), and
  Python's truthiness and str() of values (`truthy`, `get`, `s`, `obj`, `py_str`).
- `text.rs` (width, fit, textwrap, Python's strip/split/splitlines), `widths.rs` (generated),
  `timeutil.rs` (ISO stamps, ages, the clock).
- the commands: `show.rs` and `task_line.rs` (`ls`, `show`), `news.rs`, `peek.rs`, `diff.rs`
  (`--stat`), `check.rs` (with `events.rs`, `stops.rs`, `agents.rs`, `modes.rs`), `add.rs`
  (`add`, `after`, `model`, `review-mode`), `triage.rs` (`ok`, `note`); `rex.rs` and
  `coordinators.rs` hold what of those the others need.
