# BP-92: bullpen in Mojo, and how it compares

> The code is on the `mojo` branch, not on main. This page is a copy of `mojo/REPORT.md` from
> that branch (at 4e1e083), written on 2026-11-12. Every `mojo/...` path below is a file on the
> `mojo` branch.

**In short.** The read-only commands (`ls`, `show`, `news`, `peek`, `diff --stat`, `check`) and six
state-changing ones (`add`, `after`, `model`, `review-mode`, `ok`, `note`) are ported. Their output is
byte for byte the Python's in 132 checked cases, with no Python interop anywhere. The Mojo build runs
them 4 to 6 times faster: about 11–18 ms against 63–73 ms. Nearly all of that is start-up, and Python
can win most of it back without a new language. **My recommendation is not to move bullpen to Mojo.**
The reasons are at the end.

Mojo 1.2.0 (2016d0cd), installed in this worktree only (`uv venv .venv && uv pip install --python .venv
mojo`). Python 3.14.4. The Python measured is main's `bin/bullpen` at 7c1ee24. Machine: 16 cores,
Linux, shared with other agents (load about 1.2 during the timings).

## How far the port got, and why it stops there

| Stage | Commands | State |
|---|---|---|
| 1. read-only | loading config (defaults, then the global config, then the repo's) and tasks, `ls` (plain, `--all`, `--all-repos`, `--json`; piped and on a terminal, in colour, cut to its width), `show` (text and `--json`), `news` (one-off) | ported |
| 2. | `peek` (`--lines`, `--vt`), `diff --stat`, `check` (the report; `--quiet`, `--all-repos`, IDs) | ported |
| 3. state-changing | `add`, `after`, `model`, `review-mode`, `ok`, `note` | ported |
| 3. the rest | `ask`, `tell`, `start`, `ready`, `accept`, `drop`, `close`, `resume`, `check --fix`, `news --follow`, `diff` with its pager or pane | not ported |

Any command not ported is handed on to `bin/bullpen` with `execv`, so `mojo/build/bullpen` can stand
in for the Python one. Those six state-changing commands are the ones that only write a task's files. The next
command along, `ask`, sets a task to *needs*, and saving that change sends the person a desktop
notification (an OSC escape written to a pane's terminal, after asking Rex which pane they're
looking at) and types news to the coordinators. From there every remaining command reaches into
notifications, news typed into panes, Rex panes and layers, git worktrees, `gh`, the setup and
teardown hooks, and the run queue, which starts the next task by itself after an accept or drop.
Porting any one of them means porting most of the 9,000 lines. It also means building the
two-implementation lock-step that the recommendation below advises against. So I stopped where a
command's effects stay inside its own files. `save_task` in the port refuses a change that would
notify, rather than skip the notification silently.

What the port does differently, all outside what bullpen's own files hold:
- A float in JSON keeps the text it was read with. Python's json writes floats as their repr, so
  files Python wrote round-trip exactly, but `1e5` would come back as `1e5`, not `100000.0`.
- Usage errors (a missing argument, a bad `--review` choice) exit 2 as argparse does, but don't
  copy argparse's usage text.
- The width tables (CJK, emoji, combining marks) come from Python 3.14's unicodedata (Unicode
  16.0). Regex `\w` outside ASCII, and `re.I`'s Unicode case folds, are approximations, used only to
  read Claude Code's screen in `check`.
- A config.json too old to have been migrated (the triage, stops and layers migrations) is
  refused with a message; Python migrates it.
- On a broken pipe, `news` is killed by SIGPIPE instead of catching it. Its cursor still moves
  only after each line is printed, as in the Python.

## Checking it: byte for byte

`mojo/check/compare.sh` runs each case in `mojo/check/cases*.txt` with the Python and then with the Mojo
build. Each run gets a fresh fixture state from `mojo/check/fixture.py`: 41 tasks covering every status,
stuck, usage-limited, asked, GitHub states, review kinds, wide and Unicode titles, real git
worktrees with commits, uncommitted and untracked files, two coordinators, a news log with a
multi-line record and cursors, and the tests' fake `rex` with live panes, screens and status
records. It compares stdout, stderr, the exit code, and every file in the state folder afterwards.
Stamps written at run time (`now()`) read as `<now>`. Strict mode (`BULLPEN_MOJO_STRICT=1`) makes
an unported command fail instead of falling back to Python, so a "same" is a real match.

**All 132 cases are the same.** They include `ls` on 100- and 40-column terminals, `show` for
every task, `news` with missing, stale, past-the-end and mid-record cursors, a busy lock and bad
UTF-8, `check` over two dozen more panes in different states (blocked, failed, idle, a long-running shell, compacting…), every error path of `add`/`after`/`model`, and `ok` on a
reviewer's stop (which also marks the PR for the watcher). Beyond the fixture, `ls --json`,
`ls --json --all-repos --all` and `show --json` match on a copy of the real state (92 tasks).

## Benchmark

`mojo/bench/bench.py`: 40 runs each after 3 warm-up runs, Python and Mojo interleaved (one order,
then the other), output to /dev/null. Wall time median, with the 25th–75th percentile. Raw numbers
are in `mojo/bench/results.json`.

| command | Python | Mojo | Python / Mojo |
|---|---|---|---|
| start-up (Python: an unknown command; Mojo: `_nop`) | 61.1 ms (60.1–62.5) | 9.4 ms (9.2–10.0) | 6.5× |
| `ls --json` | 64.8 ms (64.0–65.9) | 12.3 ms (11.8–12.7) | 5.3× |
| `ls` | 67.2 ms (66.5–68.2) | 12.9 ms (12.3–13.3) | 5.2× |
| `ls --all` | 67.4 ms (66.3–68.9) | 12.7 ms (12.3–13.2) | 5.3× |
| `ls --all`, real state, 92 tasks | 72.8 ms (72.1–73.5) | 17.7 ms (17.4–18.2) | 4.1× |
| `ls --all-repos` | 64.0 ms (63.3–64.8) | 11.0 ms (10.7–11.5) | 5.8× |
| `show BP-17` | 66.9 ms (66.0–67.9) | 12.3 ms (11.4–12.5) | 5.5× |
| `show BP-17 --json` | 63.4 ms (62.2–64.2) | 11.1 ms (10.7–11.5) | 5.7× |
| `news` | 63.8 ms (63.0–65.8) | 11.2 ms (10.7–11.6) | 5.7× |
| `peek BP-3` (fake rex) | 111.7 ms (109.7–113.2) | 56.4 ms (55.3–57.7) | 2.0× |
| `diff BP-3 --stat` | 72.6 ms (71.0–73.9) | 18.7 ms (17.3–19.2) | 3.9× |
| `check`, fixture (fake rex, 24 panes) | 823.9 ms (820.0–833.5) | 763.0 ms (753.2–767.6) | 1.1× |
| `check`, real state and real rex | 156.7 ms (154.5–159.2) | 101.1 ms (99.3–102.9) | 1.6× |
| `note BP-3 …` (writes the task) | 65.2 ms (64.2–66.0) | 11.2 ms (10.7–11.6) | 5.8× |
| `model BP-1 opus` (writes the task) | 64.5 ms (63.5–65.7) | 11.2 ms (10.8–11.6) | 5.8× |

### Start-up against the work

Subtract each program's start-up from its times and the work itself is about the same:

| | Python | Mojo |
|---|---|---|
| start-up | 61 ms | 9.4 ms |
| work, `ls --json` (fixture) | ~4 ms | ~3 ms |
| work, `ls --all` (92 tasks) | ~12 ms | ~8 ms |
| work, `diff --stat` (3 git calls) | ~12 ms | ~9 ms |
| work, `check` (real rex) | ~96 ms | ~92 ms |

The work is mostly other programs: `git rev-parse` to find the repo (every command), git for
diffs, and `rex` for panes. Those cost the same from either language. The fake rex in the fixture
is a Python script (about 30 ms a call), which is why `peek` and the fixture's `check` gain little.

Where Python's 61 ms of start-up goes:

| | median |
|---|---|
| `python3 -c pass` (the interpreter) | 10.9 ms |
| `import bullpen.cli` (every module: `cli` imports all 27 with commands) | 42.0 ms |
| the same plus `cli.parser()` (argparse for every command) | 59.3 ms |
| `import bullpen.core` alone | 30.1 ms |

So about 50 of Python's 61 ms is bullpen's own imports and its parser, not the interpreter. A
command that imported only its own module and built only its own subparser would start in
something like 25–35 ms. That is a guess from these parts, not a measurement.

Where Mojo's 9.4 ms of start-up goes: it isn't bullpen's code. An empty `hello world` Mojo binary
takes the same 9–10 ms, against 1.0 ms for `/bin/true`. strace shows the Mojo runtime starting a
thread pool of one thread per core (16 `clone3`), about 3 ms of initialisation before it, and a 2 ms
`clock_nanosleep` that calibrates the CPU clock (this machine has no
`/sys/devices/system/cpu/cpu0/tsc_freq_khz`). None of the runtime's environment variables turned
that off. Pinning to one CPU (`taskset -c 0`) saved about 1 ms.

### Build and binary

- Build: 2.9–3.1 s wall each time (`mojo build`, default optimisation), with about 240 MB of peak
  memory. There's no incremental build: every build compiles everything.
- Binary: 655 KB (563 KB stripped). It needs three of Mojo's runtime libraries (2.0 MB together:
  `libKGENCompilerRTShared`, `libAsyncRTRuntimeGlobals`, `libMSupportGlobals`). Its RUNPATH is this
  worktree's `.venv`, so it stops working once the venv is gone. Installing it anywhere means
  shipping those libraries beside it. The toolchain in `.venv` is 672 MB, plus 42 MB in
  `~/.cache/modular`. The Python package is 1.2 MB and needs nothing beyond python3.

### Python interop

**The port uses no Python interop.** Everything that's missing from Mojo's standard library was
written in Mojo or done through libc. To measure what one interop use would cost, there's a probe,
`mojo/bench/interop_probe.mojo`, that imports Python's `json` once:

| | median |
|---|---|
| Mojo probe, no interop | 9.2 ms |
| the same probe, importing Python's `json` | 47.3 ms |

So the first use of interop costs about 38 ms of start-up: it loads libpython and starts an
interpreter. A command that needed interop even once would give back most of its lead over the
Python.

## What was hard or missing in Mojo

Missing from the standard library, and what replaced it (`file:` what holds it, in `mojo/bullpen/` on the `mojo` branch):
- **JSON**: none. A parser and a `dumps` that writes what Python's `json.dumps(indent=2)` does,
  `ensure_ascii` escapes included (`json.mojo`, 440 lines). A struct can't hold a `List` of
  itself, so a document is an arena: nodes in one list, children by index.
- **argparse**: none. A small per-command spec reader (`args.mojo`).
- **Running a program**: `std.subprocess.run` takes one shell string and returns stdout only, with
  no exit code, and stderr goes to the terminal. bullpen needs the exit code and the error text,
  so `proc.mojo` calls `posix_spawnp` over FFI, with the child's output in memfds.
- **datetime, time zones**: none. ISO stamps are parsed by hand. Local time and offsets come from
  `localtime_r` over FFI (`timeutil.mojo`), and `clock_gettime` gives now.
- **regex**: none. The few patterns (`\w` boundaries, Claude Code's shell and compacting lines,
  escape codes) are matched by hand in `events.mojo`, `peek.mojo` and `check.mojo`.
- **unicodedata, textwrap**: none. The width and combining tables were generated from Python's
  unicodedata, and textwrap was rewritten (`text.mojo`).
- **flock, sigprocmask, ioctl(TIOCGWINSZ), rename, execv, readlink**: all over FFI.
- **str methods**: Mojo's `strip()` and `splitlines()` don't use Python's whitespace and line
  breaks (`\xa0`, `　`, `\x1c`…), so `text.mojo` has Python's.

What was hard:
- **The language moves fast.** 1.2 removed `fn` (the brief asked for `fn`; typed `def` replaces
  it), and in the same release deprecated `UnsafePointer` and `alloc` without a layout.
  Code samples from a few months ago don't compile. No stdlib documentation ships with the
  package: the API was learned from compiler errors.
- **FFI lifetimes**: Mojo destroys a value right after its last use. Passing a buffer's address to
  libc counts as that last use, so the buffer can be freed *before* libc reads it. It showed up
  as wrong local times, and as zero results that looked plausible. The fix is a `_ = buf^` after
  every such call. Nothing warns about it.
- **Name clashes with libc**: `external_call["mkdir"]` and `["open"]` fail at link time because
  the stdlib already declares them with other signatures.
- **Strings**: `len()` of a String is an error (bytes, code points or graphemes?), slicing is by
  byte, many methods return a `StringSlice` that has to be wrapped again, and there is no negative
  indexing. These are all reasonable, but they make code that is short in Python long here.
- **Errors**: a mistake in a generic or unused function shows up only once something calls it.
  One type error can bring misleading follow-on errors with it.

## Size and clarity

| | lines |
|---|---|
| Python functions this port covers (by name, in `bullpen/`) | about 1,210 |
| The Mojo port (`mojo/main.mojo`, `mojo/bullpen/*.mojo`) | 3,729 |
| of which stand in for the standard library (json, args, proc, timeutil, text, textutil) | about 1,100 |
| The compare harness and fixture (`mojo/check/`), and its 132 cases | about 440, and 140 |

The Mojo is about three times as long. A third of that is the standard library it lacks. The rest
is the same logic, spelled out more: optional values (Python's `t.get("x") or {}`) become a node
index and a getter, a dict lookup becomes a loop or a `Dict` with explicit types, and an f-string
becomes concatenation. Read side by side, each Mojo function follows its Python one line for line,
but you'd rather read the Python.

The two halves came from parallel sub-agents, each in its own worktree. Each checked its own
commands against the Python. Merging them took one conflict in the dispatch and one duplicated
helper module.

## Recommendation

**Don't port bullpen to Mojo.** The speed-up is real (5× for a typical command, 50 ms saved per
call), but it comes from start-up, not from Mojo:
- Python can get most of it. Lazy imports per command (the entry script imports only the module
  for argv[1]) and building one subparser instead of all of them would take start-up from 61 ms
  to something like 25–35 ms. That's worth its own task, measured the same way with this
  benchmark.
- Mojo's own start-up floor (9 ms, a thread pool per core and a clock calibration) is about the same as
  a bare Python interpreter's (11 ms).
- What it costs: a 670 MB toolchain to build, runtime libraries to ship next to the binary, a
  language that broke its function syntax this release, about 1,100 lines of home-made standard
  library (JSON, argparse, subprocess, datetime, regex) to own, and FFI with a lifetime trap that
  gives wrong answers silently.
- Commands that do real work spend it in git and rex, which no language change touches.
- A half port means two implementations to keep in step byte for byte, and the parts left
  (start, ready, accept and the watcher's paths) are where the bugs live.

If a native bullpen is ever wanted for the hot callers (the board, the watcher, the key
bindings), the board is already Rust. Rust has a stable toolchain, real JSON (serde), argument
parsing (clap) and process handling, and it is already in the repository. It would be the natural
place to look, not Mojo.

What's worth keeping from this branch, even without the port: `mojo/bench/bench.py`'s interleaved
timing approach, and the measurement that bullpen's own imports and parser cost about 50 ms per
call.

## Files

All on the `mojo` branch, under `mojo/`:

- `REPORT.md`: this report (the original of this page). `README.md`: how to build, run and check the port.
- `main.mojo`, `bullpen/*.mojo`: the port (every file under 1,000 lines; the largest, `core.mojo`,
  is 628).
- `check/compare.sh`, `check/fixture.py`, `check/now_stamps.py`, `check/cases*.txt`: the
  byte-for-byte check.
- `bench/bench.py`, `bench/results.{md,json}`, `bench/interop_probe.mojo`: the benchmark.
