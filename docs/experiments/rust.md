# BP-95: bullpen's CLI in Rust, and how it compares

> The experiment's code is on the `rust` branch, not on main. This page is a copy of
> `cli/REPORT.md` from that branch (at 0c7d862), written on 2026-11-12. Every path the report
> names (`cli/...`, `check/...`, `bench/...`, `tools/...`, `src/*.rs`) is a file on the `rust`
> branch, under `cli/`; read one with `git fetch github rust` and
> `git show FETCH_HEAD:cli/<path>`.

> **What happened next.** The report advised against a half port and said that, if a native CLI
> were ever wanted, it should replace the Python whole, with this branch's harness as the spec.
> That's what followed: the full port (BP-103 to BP-117, merged 2026-11-13) replaced the Python
> command line with Rust. So this page describes the experiment, not the code on main now
> (docs/code.md has that). On main today:
> - The Rust is one cargo workspace, `core/`, `cli/` and `board/`, and the board reads tasks and
>   settings through `core`. The report's "Reusing the board's code" weighed that and decided
>   against it for a partial port.
> - There's no Python to hand over to, so there's no `Fail::Python`, no strict mode and no
>   `check/compare.sh`: the byte-for-byte comparison ended with the Python.
> - Text width is the `unicode-width` crate's, as the board's; `tools/widths.py` and
>   `src/widths.rs` are gone.
> - `cli/src/` is a module per command or group, not the port's flat set of files.
>
> Measured since the port: a command takes about 4 ms (`_nop`, which times start-up), and the
> `sh` entry `bin/bullpen` adds about 0.5 ms (docs/code.md, "Start-up"). The full test suite,
> which runs `bin/bullpen` as a person does, takes about 43 s on 16 CPUs (docs/tests.md). The
> tables under "Benchmark" are the experiment's, against the Python of the time; the last
> all-Python commit against the full port is in "After the full port", right after them.

**In short.** The same commands as BP-92's Mojo port are ported. That's the read-only
commands (`ls`, `show`, `news`, `peek`, `diff --stat`, `check`) and the six state-changing ones
that write only a task's files (`add`, `after`, `model`, `review-mode`, `ok`, `note`). Their
output is byte for byte the Python's in 225 checked cases (Mojo: 132), and in spot checks on a copy
of the real state. Rust starts in 1.2 ms, against 9.4 ms for Mojo and 27 ms for Python since
BP-94. Typical commands take 3–4 ms in Rust against 30–37 ms in Python. **My recommendation is
still not to move bullpen's CLI to Rust now.** The 30 ms a call saves is real, but nobody calls
these commands often enough to feel it, and a half port means two implementations to keep in
step. If a native CLI is ever wanted, Rust is the right language for it, not Mojo; the reasons
are at the end.

Rust 1.99.1, release build. The Python measured is **main's `bin/bullpen` at 90de8a4**, with
BP-94's start-up work. BP-92's Mojo numbers were measured against an older main (7c1ee24, before
BP-94), so this report gives them beside the Python of that time. Python 3.14.4. Machine: 16
cores, Linux, shared with other agents (load about 1.5–2.5 during the final timings).

## How far the port got, and why it stops there

| Stage | Commands | State |
|---|---|---|
| 1. read-only | config (defaults, then the global config, then the repo's) and tasks, `ls` (plain, `--all`, `--all-repos`, `--json`; piped and on a terminal, in colour, cut to its width), `show` (text and `--json`), `news` (one-off) | ported |
| 2. | `peek` (`--lines`, `--vt`), `diff --stat`, `check` (the report; `--quiet`, `--all-repos`, IDs) | ported |
| 3. state-changing | `add`, `after`, `model`, `review-mode`, `ok`, `note` | ported |
| 3. the rest | `ask`, `tell`, `start`, `ready`, `accept`, `drop`, `close`, `resume`, `check --fix`, `news --follow`, `diff` with its pager or pane, `config`, ... | handed to Python |

I stopped where BP-92 stopped, for the same reason. The next command, `ask`, notifies the
person and types news into coordinators' panes. Every command after it reaches into
notifications, Rex panes and layers, git worktrees, `gh`, the hooks and the run queue. Porting
any one of them means porting most of the 9,000 lines, and the two implementations would then
have to be kept in step. Going further would only make the recommendation below more expensive
to act on, not more informed.

**Handing over.** Anything the port doesn't do, it hands to Python with `exec`, before it has
printed or written anything (`Fail::Python` in `core.rs`). That covers an unported command,
`--help`, every usage error (so argparse's exact usage text, which Mojo didn't copy), a
config.json that still needs a migration (Mojo refused those), and a save that would notify the
person or move the run queue. It also covers a `--brief-file` that is a pipe, since a pipe can't
be read twice, and a few malformed inputs where Python fails in its own words. So the Rust
binary can stand in for `bin/bullpen` for every call. A call it doesn't do costs about 1 ms more
than Python alone.

**What is still different from the Python.** All of these are edge cases outside what bullpen
writes:
- A stamp without a UTC offset is read as local time; Python would fail to subtract it.
- Output from git or rex that isn't UTF-8 is decoded leniently; Python's strict decode would
  crash.
- Reading Claude Code's screen in `check`: `\d` is ASCII digits only, and `\w` is
  `char::is_alphanumeric`. re.I's special folds (ſ, the Kelvin sign) aren't handled.
- `check` with several unknown IDs reports the first of them in the order given. Python goes in
  set order, which changes from run to run.
- `repo.base()` with an origin/HEAD without a slash keeps the whole name (Python raises
  IndexError). A `coordinators` entry that isn't an object is skipped.
- On a closed pipe the process dies by SIGPIPE, as a shell tool does, rather than print
  Python's BrokenPipeError.
- The width tables come from Python 3.14's unicodedata (Unicode 16.0), by
  `tools/widths.py`. A Python with newer Unicode would need them made again.

## Checking it: byte for byte

`check/compare.sh` and `check/fixture.py` are BP-92's harness, pointed at the Rust binary. The
fixture has 41 tasks in every status, real git worktrees, a news log with cursors, two
coordinators, and the tests' fake `rex` with live panes and screens. Each case runs in a fresh
copy of it, with the Python and then with Rust, and compares stdout, stderr, the exit code and
every file of the state folder afterwards. Strict mode (`BULLPEN_RUST_STRICT=1`) makes a hand-over
fail, so a "same" is the port's own. Usage-error cases (`usage ...`) run without strict mode and
are compared whole.

**All 225 cases are the same.** That's BP-92's 132 cases plus 93 new ones: option prefixes, the
order of `--auto-accept`/`--no-auto-accept`, every error path of `add`/`after`/`model`, `ok` and
`note` in every status, `news --every`, `peek --lines +2/999`, a missing worktree, `REX_BLOCK`
set to a coordinator's pane, a board's pane or an unknown one, and the usage errors.

On a copy of the real state (99 tasks), these also match:
- `ls` (plain, `--all`, `--all-repos --all`, `--json --all-repos --all`)
- `show` for every task, text and `--json` (190 runs)
- `check`, `check --all-repos`, `diff --stat` on two tasks, and `peek`

## Benchmark

`bench/bench.py` is BP-92's runner, for Rust. Each case gets 40 runs after 3 warm-up runs, Python
and Rust interleaved (one order, then the other), with output to /dev/null. Times are wall-time
medians, with the 25th–75th percentile in brackets. Raw numbers are in `bench/results.json`.
hyperfine (2.0, installed in ~/.cargo) doesn't interleave two programs, so it served as a cross-check
of start-up instead (below). A first full run was taken during a burst of load (8.5) from other
agents. Its rex-heavy rows spread widely (`check` 805–1648 ms), so the table is from a second run
at load 1.5–2.5.

| command | Python, main 90de8a4 | Rust | Python / Rust | Mojo (BP-92) | Python then (7c1ee24) |
|---|---|---|---|---|---|
| start-up (Python: an unknown command; Rust, Mojo: `_nop`) | 26.8 ms (25.9–36.9) | 1.4 ms (1.1–1.5) | 19.6× | 9.4 ms | 61.1 ms |
| `ls --json` | 33.6 ms (32.7–34.7) | 3.6 ms (3.1–3.9) | 9.4× | 12.3 ms | 64.8 ms |
| `ls` | 34.0 ms (33.6–34.7) | 4.2 ms (3.8–4.9) | 8.0× | 12.9 ms | 67.2 ms |
| `ls --all` | 34.4 ms (33.7–35.5) | 4.3 ms (3.5–4.8) | 7.9× | 12.7 ms | 67.4 ms |
| `ls --all`, real state (99 tasks; Mojo's: 92) | 37.7 ms (37.1–38.4) | 8.0 ms (7.4–8.6) | 4.7× | 17.7 ms | 72.8 ms |
| `ls --all-repos` | 31.8 ms (30.1–32.7) | 2.6 ms (2.4–2.7) | 12.3× | 11.0 ms | 64.0 ms |
| `show BP-17` | 34.4 ms (33.8–35.5) | 3.6 ms (3.1–3.9) | 9.5× | 12.3 ms | 66.9 ms |
| `show BP-17 --json` | 32.1 ms (31.2–32.6) | 2.9 ms (2.5–3.3) | 11.0× | 11.1 ms | 63.4 ms |
| `news` | 30.0 ms (29.4–30.7) | 2.8 ms (2.5–3.2) | 10.8× | 11.2 ms | 63.8 ms |
| `peek BP-3` (fake rex) | 78.4 ms (76.9–79.5) | 47.4 ms (46.7–48.4) | 1.7× | 56.4 ms | 111.7 ms |
| `diff BP-3 --stat` | 40.7 ms (39.8–41.6) | 10.5 ms (9.6–10.9) | 3.9× | 18.7 ms | 72.6 ms |
| `check`, fixture (fake rex, 24 panes) | 796.3 ms (787.9–807.2) | 751.7 ms (743.9–767.5) | 1.1× | 763.0 ms | 823.9 ms |
| `check`, real state and real rex | 147.0 ms (144.3–150.1) | 112.1 ms (109.9–114.3) | 1.3× | 101.1 ms | 156.7 ms |
| `note BP-3 …` (writes the task) | 35.4 ms (34.9–36.2) | 2.7 ms (2.4–3.3) | 13.2× | 11.2 ms | 65.2 ms |
| `model BP-1 opus` (writes the task) | 37.0 ms (35.9–37.7) | 3.1 ms (2.4–3.5) | 11.9× | 11.2 ms | 64.5 ms |

Since BP-94, Python's "start-up" row means less than it did in BP-92. An unknown command now
exits before any parser is built. It measures only the interpreter plus importing
`bullpen.cli`, which is a floor, not what a command pays before its work. The command rows are
the honest comparison. Real-rex `check` depends on the live panes at the time, so compare it
only within its row. BP-92 measured it at another time, with other panes.

hyperfine (`-N`, 40 runs, 3 warm-up), as a cross-check:

| | mean ± σ |
|---|---|
| Rust `_nop` | 1.3 ± 0.3 ms |
| `/usr/bin/true` | 0.7 ± 0.2 ms |
| `python3 -S -c pass` | 7.3 ± 0.8 ms |
| `python3 -c pass` | 10.7 ± 0.6 ms |
| Python `bin/bullpen _no_such_command` | 29.6 ± 6.2 ms |

### Start-up against the work

Subtracting each program's start-up gives roughly what the command itself costs:

| | Python (90de8a4) | Rust | Mojo (BP-92) |
|---|---|---|---|
| start-up | 27 ms (a floor, see above) | 1.4 ms | 9.4 ms |
| work, `ls --json` (fixture) | ~7 ms | ~2 ms | ~3 ms |
| work, `ls --all` (real state) | ~11 ms | ~7 ms | ~8 ms |
| work, `diff --stat` (3 git calls) | ~14 ms | ~9 ms | ~9 ms |
| work, `check` (real rex) | ~120 ms | ~111 ms | ~92 ms |

Rust's 1.4 ms of start-up is mostly the kernel's: `/usr/bin/true` takes 0.7 ms. Most of Rust's
`ls` is the one `git rev-parse` every command makes to find its repository: 1.7 ms on its own,
measured with hyperfine. Commands that do real work spend it in git and rex, which no language
change touches. That's why `check` gains little in any of the three. The fixture's fake rex is a
Python script, about 30 ms a call.

### Build and binary

- **Clean build:** 3.8 s wall (17.8 s CPU, 306 MB peak), `nice -n 10 cargo build --release`. It
  compiles 16 crates: the crate and its dependencies serde_json (with indexmap), serde, chrono
  (clock only) and libc.
- **Incremental build** after editing one file: 1.4 s. With nothing changed: 0.03 s. A debug build
  from clean: 2.9 s. Mojo had no incremental build (2.9–3.1 s every time).
- **Binary:** 1.38 MB, 1.10 MB stripped. It is self-contained (only libc and libgcc_s, as any
  Linux program has). Mojo's was 655 KB but needed 2.0 MB of runtime libraries beside it, with a
  RUNPATH into its venv.
- **Toolchain:** rustup's (1.7 GB on this machine, already there for the board). Mojo needed 672 MB
  in the worktree's venv, plus 42 MB of cache.

## After the full port

Measured on 2026-11-13, once the full port was on main. The "Benchmark" above compares the
experiment's partial port with the Python of its time, on BP-92's fixture; this compares the
last all-Python commit with the whole Rust command line, on a smaller fixture. The two agree:
a typical command goes from about 33 ms to about 4 ms.

**Method.**
- Python: the last all-Python commit, `e55c950` (2026-11-12), in a throwaway detached
  worktree. Rust: main at `da76c47` (2026-11-13). Python 3.13.15. 16-core Linux server, no
  workers running (load about 0.7).
- Commands: a fresh git repo with 30 unstarted tasks made by the Python version, in its own
  `BULLPEN_STATE` (a copy each, so `add` doesn't affect the other). Each command ran 60 times
  after 5 warm-ups, the two versions alternating, timed around the whole process (spawn to
  exit) with stdout and stderr discarded. `ls` and `show BP-5` gave identical output from both.
- Suite: `python3 tests/run.py`, 3 runs each, alternating, 16 processes at nice 10.

Commands, in ms:

| Command | Python median | Python min | Rust median | Rust min | Faster by |
| --- | --- | --- | --- | --- | --- |
| `--help` | 44.8 | 42.1 | 2.0 | 1.3 | 22.8x |
| `ls` | 32.8 | 30.5 | 4.2 | 3.3 | 7.8x |
| `ls --all` | 32.3 | 30.5 | 4.4 | 2.9 | 7.3x |
| `show BP-5` | 35.1 | 31.8 | 4.1 | 2.9 | 8.5x |
| `config` | 35.4 | 32.7 | 4.1 | 2.9 | 8.6x |
| `news` | 29.9 | 27.7 | 3.5 | 2.3 | 8.6x |
| `add` | 44.7 | 42.0 | 7.6 | 6.0 | 5.8x |
| `doctor` | 98.1 | 92.8 | 95.8 | 89.9 | 1.0x |

`doctor` exited 1 in both (the fixture has no Rex session); its time goes to the outside
programs it runs, so it's unchanged.

The suite:

| | Tests | Run 1 | Run 2 | Run 3 |
| --- | --- | --- | --- | --- |
| Python (`e55c950`) | 524 | 60.4 s | 60.4 s | 60.4 s |
| Rust (`da76c47`) | 518 | 42.8 s | 42.8 s | 42.9 s |

The two commits don't have the same tests (the port removed Python-only tests and added
others), so this is close, not exact. The tests are still Python driving the built command
line.

Earlier informal figures (`ls` 138 ms to 14 ms, the suite 74 s to about 45 s) were taken with
workers running and against the real repo's larger task list. These controlled numbers replace
them.

Binary sizes: `target/bullpen` 3.9 MB, `target/bullpen-board` 2.2 MB.

The timing script, to repeat the run. It sits in a folder with `py/` (a detached worktree at
the Python commit), `rs/` (a checkout of the Rust, built), `repo/` (the fixture repo) and
`state-py/`, `state-rs/` (the two copies of its state):

```python
import os, subprocess, sys, time, statistics
S = os.path.dirname(os.path.abspath(__file__))
impl = {"python": (S + "/py/bin/bullpen", S + "/state-py"), "rust": (S + "/rs/bin/bullpen", S + "/state-rs")}
cases = [["--help"], ["ls"], ["ls", "--all"], ["show", "BP-5"], ["config"], ["news"], ["doctor"],
         ["add", "Bench task", "--brief", "A brief."]]
N, WARM = 60, 5
def run(name, args):
    exe, state = impl[name]
    env = dict(os.environ, BULLPEN_STATE=state)
    t = time.perf_counter()
    p = subprocess.run([exe, *args], cwd=S + "/repo", env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return (time.perf_counter() - t) * 1000, p.returncode
print(f"{'command':<12} {'python med':>10} {'min':>7} {'rust med':>9} {'min':>7} {'ratio':>6}  exit")
for args in cases:
    times = {"python": [], "rust": []}; rc = {}
    for i in range(N + WARM):
        for name in ("python", "rust") if i % 2 else ("rust", "python"):
            ms, code = run(name, args); rc[name] = code
            if i >= WARM: times[name].append(ms)
    p, r = times["python"], times["rust"]
    print(f"{' '.join(args[:2]):<12} {statistics.median(p):>10.1f} {min(p):>7.1f} {statistics.median(r):>9.1f} {min(r):>7.1f} {statistics.median(p)/statistics.median(r):>5.1f}x  {rc['python']}/{rc['rust']}")
```

## How it was built, and what was easy or hard compared with Mojo

I wrote the crate, its core, `ls`/`show` and the harness. Two sub-agents in their own worktrees
then ported `news`/`peek`/`diff`/`check` and the six state-changing commands from that base; I
reviewed their code and merged it. Merging took two conflicts, both in additive code: `main.rs`'s
dispatch, and one helper written twice (`coordinators::calling`, which `coordinator_at` already
does). `ls` and `show` matched on the first build in all 55 cases. Each sub-agent's commands
matched on its first full run too, after 8 and 12 minutes.

**Easy, where Mojo was hard.** Rust's standard library and three crates cover what BP-92 had to
write by hand:
- `std::process` gives exit code, stdout and stderr; Mojo needed `posix_spawnp` over FFI with
  memfds.
- `File::try_lock` replaces flock over FFI; `fs::rename` and `IsTerminal` are std; `libc` covers
  `ioctl(TIOCGWINSZ)` and the signal mask.
- serde_json with `preserve_order` reads a task and writes it back with its keys in their order,
  untouched. Mojo needed a 440-line JSON arena.
- chrono does local time and offsets; Mojo used `localtime_r` over FFI.
- None of Mojo's traps showed up: no FFI lifetimes freeing a buffer early, no clashes with libc
  names, no syntax that changed this release. Compiler errors pointed at the real mistake, and
  the borrow checker caught one real slip (reading a task while it was borrowed for writing).

**Still hand-written, in both.** Being byte for byte the Python's is what costs, whatever the
language:
- A JSON `Formatter` (about 60 lines). It escapes non-ASCII as `\uXXXX` and writes floats as
  Python's repr: `1e+16`, `1.5e-05`.
- Python's truthiness and `str()` of a value (`truthy`, `py_str`, `py_repr`).
- textwrap's algorithm, Python's `strip`/`split`/`splitlines` with its own whitespace and line
  breaks, and the width tables made from unicodedata.
- argparse's prefix matching. Rust hands every usage error to Python, so argparse's text comes
  out exactly; Mojo couldn't do that cheaply, because a Python call costs it 38 ms of interop.
- What took thought was Python behaviour that isn't in bullpen's code:
  - subprocess's text mode and `read_text()` turn `\r\n` and `\r` into `\n`.
  - argparse lets the last of `--auto-accept`/`--no-auto-accept` win.
  - `lines[-n:]` has its own meaning for 0 and for negative n.
  - `news_records` drops the last whole record when an unfinished line follows it.
  - An empty dict is false.
  - Once the port has read a pipe, Python can't read it again: hand-over has to come before any
    read that can't be repeated, not just before any write.

**Reusing the board's code.** I looked at it and didn't reuse it. `cli/` is its own crate, not a
cargo workspace with the board:
- The board's `model` reads a task into typed structs that drop unknown fields. The CLI must
  write a task back exactly as it was, so it keeps the JSON object itself.
- The board's sentences differ from `ls`'s on purpose. It adds "Yours in 7 min." and the tour,
  and its "Finished" line lacks "a closes it.".
- The board measures width with the unicode-width crate, which disagrees with Python's
  unicodedata for zero-width characters (U+200D is 0 columns to it, 1 to Python). Its `wrap` is
  not textwrap's.
- What would carry over unchanged is about 60 lines (`pr_short`, `ago`, `clock`,
  `global_config_path`). That isn't worth moving the board's `model` into a shared crate, which
  would also mean changing `board_build.py`. It hashes `board/src` to know when to rebuild the
  board.
- A workspace would also unify serde_json's features, so `preserve_order` would change the
  board's build too.

**Clap or hand-rolled.** Hand-rolled (`args.rs`, 120 lines). clap's errors, help and
abbreviation rules aren't argparse's. The port must accept exactly what argparse accepts and
hand the rest to Python for argparse's own message, which is simpler to do with a small reader
than to fight clap into.

## Size and clarity

| | lines |
|---|---|
| Python functions this port covers (BP-92's count, same scope) | about 1,210 |
| The Rust port, `cli/src/*.rs`, without tests and the generated `widths.rs` | 3,172 |
| of which stand in for Python's standard library (json, text and textwrap, args, timeutil) | about 610 |
| `widths.rs`, made by `tools/widths.py` | 327 |
| unit tests, in the same files | 158 |
| The Mojo port (BP-92) | 3,729, of which about 1,100 stand in for the standard library |

The Rust is about 2.6 times the Python's length; Mojo's was 3.1 times. The rest of the
difference is the same logic spelled out. Python's `t.get("x") or {}` becomes
`obj(t, "x")`, `f"{t['round']}"` becomes `py_str(...)`, and every dict lookup has to say what
missing, null or the wrong type means. That is clearer about edge cases and longer to read. The
largest file is `core.rs`, at 534 lines. Lines run up to about 120 columns: the code isn't
rustfmt'd, to keep it as dense as the Python.

## Recommendation

**Don't move bullpen's CLI to Rust now. If a native CLI is ever wanted, Rust is the language for
it, not Mojo.**

- **What it would buy.** About 30 ms per call (34 ms → 4 ms for `ls`, `show`, `news`, `note`).
  BP-94 already took Python from 61–73 ms to 30–37 ms, the larger half of the gap, without a new
  language. Commands that do real work (`check`, `peek`, `diff`) gain 5–35 ms, because their
  time is in git and rex.
- **Who would notice.** Not a person typing a command: 35 ms is below what anyone feels. The
  frequent callers would: the watcher's calls, the board's commands, the key bindings. But the
  board already reads tasks and config itself, in Rust, and the watcher's calls are the ones
  that start agents and type into panes. Those are the unported 9,000 lines.
- **What a half port costs.** Two implementations to keep in step byte for byte. Every change
  to a ported command's wording, or to how tasks are stored, would need the same change in Rust
  and a compare case. That doubles the cost of the most often edited code, the sentences and
  the commands. A full port is a rewrite of the parts where the bugs live (start, ready,
  accept, the watcher's paths), with the risk that brings.
- **Why Rust and not Mojo, if ever.**
  - It starts 7 times faster than Mojo: 1.4 ms against 9.4.
  - Its toolchain is stable and already in the repository for the board.
  - It builds in under 4 s clean and 1.4 s incrementally.
  - Its binary is self-contained.
  - It needed a third less hand-written standard library, and none of the FFI traps.
  - This port took under an hour of wall time, with two sub-agents.
- **The way to do it, if the 30 ms ever matters.** Replace the CLI whole, not in halves, using
  this branch's harness as the spec, and drop "byte for byte the Python's" once the Python is
  gone. A native CLI that didn't have to mimic Python's `str()`, textwrap and argparse would
  be shorter than this port.

What's worth keeping from this branch, even without the port:
- the hand-over design (`Fail::Python`: a fast path that runs only when it's sure, falling back
  to the full program otherwise);
- the 93 new compare cases;
- the measurement that, after BP-94, a typical command still spends about 27 of its 34 ms
  before it starts its work.

## Files

All on the `rust` branch, under `cli/`:
- `REPORT.md` is this report (the original of this page); `README.md` says how to build, run and check the port.
- `Cargo.toml`, `src/*.rs` hold the port. Every file is under 1,000 lines; the largest, `core.rs`, is 534.
- `check/compare.sh`, `check/fixture.py`, `check/now_stamps.py` and `check/cases*.txt` are the
  byte-for-byte check.
- `bench/bench.py` and `bench/results.{md,json}` are the benchmark.
- `tools/widths.py` makes `src/widths.rs`.
