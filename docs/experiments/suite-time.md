# BP-126: where the test suite's time goes

**In short.** The suite is limited by CPU, not by waits. Nearly two thirds of its time goes on
starting the fake `rex`, a Python script that bullpen runs about 10,600 times per run, at
about 23 ms each. Rewriting only the fake `rex` and `gh` in Rust should take the full run from
**about 44 s to about 13–16 s** on this machine, with a change of about 500 lines. Moving all
the tests to Rust would save under 1 s more than that, for 10,000+ lines of new code.
**Recommendation: rewrite the fakes in Rust (option 2), and leave the tests in Python.**

Measured on 2026-11-13 at 0453a6b: Python 3.14.4, git 2.53.0, a release build of the CLI.
Machine: AMD Ryzen 7 PRO 6850U, 8 cores with 2 threads each (16 CPUs), shared with other agents.

## Method

- **Quiet machine.** Each timed run waited for the 1-minute load average to drop below 2.5
  before it started. The CPU that other processes used during a run was counted from
  `/proc/stat` (busy time, minus the run's own `/usr/bin/time` user + sys): about 30 CPU-s
  during a 44 s run (0.7 of a CPU), and about 0.3 of a CPU during the serial runs.
- **Repeats.** `-j 16` ran three times, `-j 8` and `-j 4` twice each, and `-j 1` once. The
  repeated runs agreed within 1–2%.
- **Attribution.** A temporary build, in a scratch worktree, put a small C wrapper in front of
  `git` (first on `PATH`), the fake `rex` and `gh`, and the tests' `bin/bullpen`. The wrapper
  forks the real program, waits for it with `wait4`, and logs its PID, its parent's PID, its
  wall time and CPU (including the children it waited for), its first argument, and the test
  that ran it. A process's own time is its time minus that of the logged children it waited
  for. `run.py` also logged each test's own CPU (`RUSAGE_SELF`) and the time it spent in
  `time.sleep` and `select.select`. None of this is left in the suite.
- **Instrumentation side effects.** The wrapper adds about 1.8 ms per call: the serial run took
  427 s with it and 352 s without. 10 of the 518 tests fail under it, because they check
  `bullpen`'s own path, replace the fake `gh`, or send ctrl-c. Their times are left out of the
  per-test table. The breakdown below takes the overhead out: the uninstrumented run's total is
  shared out by the wrapped run's counts and per-call times.
- **Per-call costs** come from a separate micro-benchmark: 200 spawns of each program from
  Python, timed with `wait4`, on a quiet machine.

## Breakdown

One process (`-j 1`) has no contention, so its time is the suite's real work. Uninstrumented,
the run took **352 s wall and 332 CPU-s**, about 60% user and 40% system.

| Part | Calls per run | Time per call | Total (serial) | Share |
|---|---|---|---|---|
| fake `rex` (Python) | 10,588 | 23 ms | ~240 s | ~68% |
| `git` (19,370 from bullpen, 6,780 from the tests' fixtures) | 26,144 | 2.4 ms | ~62 s | ~18% |
| `bin/bullpen` (its own time: `sh` entry + Rust) | 4,343 | ~5 ms | ~22 s | ~6% |
| waits in the tests (`time.sleep` 12.7 s, `select` 9.0 s) | | | ~22 s | ~6% |
| the tests' own Python (fixture set-up, assertions) | | | ~6–10 s | ~2% |
| fake `gh` (Python) | 253 | 21 ms | ~5 s | ~1.5% |
| `run.py`'s build check (hashing the source; nothing to build) | 1 | 75 ms | 0.08 s | 0% |

Wall and CPU differ by only 20 s, and that is the tests' sleeps and selects. bullpen itself
doesn't wait in the tests: its polls (`build`'s lock, the tour's stepper) only show up in the
few tests about them. The fake `rex` and `gh` are leaves, so their time is all their own. A
`rex` call is a Python start (about 11 ms, or 7 ms with `-S`), plus `import json` and `pathlib`
(about 9 ms; `json` pulls in `re`), plus reading and writing `reg.json`.

Under load the same work costs more CPU. At `-j 16` the suite uses 594 CPU-s, against 332
serial, because two threads share each core and the clock drops. In the instrumented `-j 16`
run, `rex` took 44 ms of CPU per call (470 CPU-s, 72% of the total), `git` 2.8 ms and `gh`
41 ms. Its share is about the same as in the serial run.

The busiest callers of `rex` are `up` (6.3 calls each, run 498 times, mostly from the
`start_task` helper), `start` (3.8 each) and `_event` (5.0). By subcommand: `block` 3,441,
`api call` 2,995, `capture` 800, `send` and `send-key` 1,240, `server status` and `actions` 565
each, `split` 481 and `run` 464.

### Per-call cost of a fake

| Program, started from Python | Wall | CPU |
|---|---|---|
| `/bin/true` | 0.52 ms | 0.47 ms |
| trivial C (`puts`) | 0.54 ms | 0.49 ms |
| trivial Rust (`println!`) | 0.83 ms | 0.77 ms |
| `git --version` | 1.1 ms | 1.05 ms |
| `python3 -c pass` | 10.8 ms | 10.7 ms |
| `python3 -S -c pass` | 7.2 ms | 7.1 ms |
| `python3 -c 'import json, pathlib'` | 19.9 ms | 19.8 ms |
| fake `rex capture` | 23.9 ms | 23.7 ms |
| fake `rex api call session.view` | 23.3 ms | 23.2 ms |
| fake `gh pr view` | 21.5 ms | 21.4 ms |

A Rust fake that reads and writes a small JSON file should cost about 1 ms per call.
Any Python fake that imports `json` costs at least 13–15 ms.

## How the wall time changes with `-j`

| `-j` | Wall (runs) | CPU-s | Speed-up over `-j 1` |
|---|---|---|---|
| 1 | 352 s | 332 | 1.0 |
| 4 | 94.1, 96.2 s | 350 | 3.7 |
| 8 | 55.5, 55.4 s | 394 | 6.4 |
| 16 | 44.5, 44.2, 44.3 s | 594 | 8.0 |

The speed-up is close to linear up to the 8 physical cores, and the 8 extra hardware threads
add another 20%. A suite limited by waits would keep scaling past the CPUs and wouldn't use
more CPU as `-j` grows. This one uses 13.4 of 16 CPUs on average at `-j 16`, so it is limited
by CPU.

## The slowest 20 tests

From the serial run (no contention). Calls are from the instrumented serial run; "waits" is
`time.sleep` + `select` in the test.

| Test | Wall | `rex` calls / s | `git` calls / s | `bullpen` calls | Waits |
|---|---|---|---|---|---|
| Triage: several tasks that need you at once get one notification more | 4.1 | 62 / 1.5 | 92 / 0.2 | 21 | 2.4 |
| BoardBuild: up leaves the build running in the background | 3.4 | 10 / 0.3 | 25 / 0.1 | 0 | 3.2 |
| Tour: the stepper steps, takes comments, resumes and sends | 2.8 | 25 / 0.6 | 86 / 0.2 | 14 | 2.2 |
| News: go next falls through to a ready task on the board | 2.2 | 92 / 2.1 | 55 / 0.1 | 17 | 0 |
| Tour: the stepper tells the worker and v closes it for the diff | 2.0 | 29 / 0.7 | 79 / 0.2 | 12 | 1.3 |
| Coordinators: a gone coordinator's tasks fall back to the others | 2.0 | 80 / 1.9 | 78 / 0.2 | 16 | 0 |
| Queue: accept starts queued tasks in ID order within max running | 1.9 | 65 / 1.5 | 137 / 0.3 | 21 | 0 |
| News: a guide change is told to each running coordinator alone | 1.9 | 68 / 1.5 | 97 / 0.2 | 13 | 0 |
| Triage: a task that needs you sends a notification from its pane | 1.8 | 37 / 0.9 | 37 / 0.1 | 10 | 0.8 |
| SelfUpdate: an auto-accept from a reviewer whose pane closes rebuilds and pushes | 1.8 | 22 / 0.5 | 115 / 1.3 | 0 | 1.1 |
| Review: a rebase check that asks for changes sends it back without a round | 1.8 | 50 / 1.1 | 159 / 0.4 | 27 | 0 |
| Review: after accept, a conflict resolved by hand gets a rebase check | 1.7 | 51 / 1.2 | 143 / 0.4 | 24 | 0 |
| Review: after accept, a conflict resolved by keeping both sides keeps the approval | 1.6 | 49 / 1.1 | 139 / 0.4 | 23 | 0 |
| Flow: accept restarts a stale watcher and leaves a current one | 1.6 | 55 / 1.3 | 114 / 0.3 | 15 | 0 |
| Review: after accept, a worker that changed more than the conflict gets a follow-up | 1.6 | 45 / 1.0 | 141 / 0.4 | 23 | 0 |
| Config: doctor checks the setup hook | 1.6 | 42 / 1.0 | 107 / 0.3 | 7 | 0 |
| Pick: pick goes to the task you choose | 1.6 | 30 / 0.7 | 62 / 0.1 | 11 | 1.1 |
| News: go next cycles through tasks needing you | 1.6 | 59 / 1.3 | 82 / 0.2 | 20 | 0 |
| Triage: a task that needs you later is its own notification | 1.5 | 41 / 1.0 | 64 / 0.1 | 12 | 0.4 |
| News: go next, all repos, goes across repos | 1.5 | 59 / 1.3 | 66 / 0.1 | 22 | 0 |

Two kinds of test are slow:

- **Many `rex` calls** (40–90 per test, which is 1–2 s). This is most of them, with the review,
  queue, news and coordinator tests at the top. The median test makes 20 `rex` calls and takes
  0.75 s.
- **Waits:**
  - The triage tests read each pane's terminal with `terminal()`'s reader, which waits
    0.2 s in `select` every time there's nothing to read. That's 12 empty reads, and 2.4 s, in
    the slowest test.
  - The tour stepper tests poll every 0.1 s for background work.
  - `up leaves the build running in the background` sleeps 3.2 s in its slow fake cargo.

The SelfUpdate tests spend their time in `git` (a copy of bullpen's own checkout).

## Options, with the expected saving

The savings are for the full run at `-j 16`: 44 s now.

### 1. Leave it as it is: saves 0 s

The full suite runs once per task, right before `ready`, so 44 s isn't a crisis. But the same
mix holds for targeted runs (`tests/run.py review` and the like), which a worker runs many
times per task. There, too, about two thirds of the time is the fake `rex`.

### 2. Rewrite only the fakes in Rust: saves about 28–31 s (44 s → about 13–16 s)

At about 1 ms a call instead of 23 ms, the fakes' ~245 s of the serial 352 s comes down to
about 11 s. That's roughly 70% less work. The suite is limited by CPU, so the wall time should
fall about as much:

- The remaining serial work (about 110 s) over the same 8× parallel speed-up gives about 14 s.
- Some waits don't shrink with CPU (the slowest tests' 2–3 s), and the run's tail grows in
  share.
- So 13–16 s.

**Check on the model.** Changing only the fakes' shebang to `#!/usr/bin/python3 -S` (no
`site`) saves 3.6 ms per call by the micro-benchmark, so the model predicts 39 s off the serial
run and 11% off `-j 16`. Measured: 352 → 314 s serial, and 44.3 → 39.4 s at `-j 16` (three runs
each way, 39.1–39.6 s). The prediction held.

**Size:**

- A small crate in the workspace (say `tests/fakes/`, with bin targets `fake-rex` and
  `fake-gh`), using only `serde_json`, which is already allowed.
- The fake `rex` is about 175 lines of Python: its block, layer and window registry in
  `reg.json`, the `api call` methods, and the switch files tests drop in `FAKE_DIR`. In Rust
  that's about 350–450 lines. The fake `gh` is about 15 lines, so 40–60 in Rust.
- `run.py` builds the crate with the CLI (`-p bullpen-fakes`), and its build stamp covers the
  crate's source.
- `support.py` copies or links the two binaries into each test's `fake/` folder instead of
  writing scripts there.
- Three tests in `test_accept.py` add Python to the fake `gh` (a refused merge, a slow merge).
  They'd use switch files instead, as the fake `rex` already does (`send_fails_<block>`,
  `server_down`).
- One branch of the fake `rex` (`kills_tree_<block>`) walks the process tree with `ps`. It
  would read `/proc` or keep calling `ps`.
- In all, about 500 lines of Rust and about 50 lines changed in the tests: one task.

The fakes' behaviour is the tests' contract with Rex and GitHub, so the port should be checked
by running the full suite against both versions before the Python ones go.

### 3. Other cheap wins

| Change | Saving at `-j 16` now | After option 2 |
|---|---|---|
| `#!/usr/bin/python3 -S` in the two fakes (one line each; measured) | 4.9 s | n/a (replaced by 2) |
| Drop `pathlib` from the fakes too (fake `rex` → about 15 ms) | about 4 s more, est. | n/a |
| A fixture project made once per process and copied per test, instead of 9 `git` calls per `setUp` (6,780 calls, about 16 s serial) | about 1.5–2 s | about 1.5–2 s |
| Shorter waits in the slowest tests: `terminal()`'s 0.2 s `select` per empty read down to 0.05 s, and a shorter slow cargo in `up leaves the build running` | under 1 s (they overlap with other work) | about 1–2 s: these tests become the run's tail |
| Start the slowest test classes first in `run.py`, so the tail is short tests | up to about 1 s, est. | about 1–2 s |
| The build check | nothing: 75 ms when nothing changed | |

The `-S` shebang is the cheapest step on its own and is measured. But it's worth doing only as
a stopgap if option 2 waits. Once the fakes are native, the waits and the scheduling become
the tail of a 13–16 s run, and they're worth another 2–3 s together.

One flaky failure turned up in the runs for this report: `test_review`'s `tearDownModule` found a temp
folder left behind, once, in a `-S` run. It looks like a background job racing the clean-up,
not something the shebang caused.

### 4. Move all the tests to Rust: saves about 29–32 s, under 1 s more than option 2

- **Size.** The tests are 25 modules, about 8,900 lines with `support.py`. Rust tests run
  longer for the same checks (process set-up, JSON, `pty` and `select` handling), so that's
  roughly 10,000–13,000 lines. That's weeks of agent work and review, and every test would be
  rewritten, so coverage could slip in places.
- **What it would save:**
  - the fakes' start-up, which option 2 already saves;
  - the tests' own Python (about 6–10 s serial, under 1 s at `-j 16`);
  - the Python workers' start-up (negligible).
- **What it wouldn't save:**
  - the 4,343 `bullpen` and 26,144 `git` processes, which are most of what remains after
    option 2;
  - the waits.

  Calling bullpen in-process instead of as `bin/bullpen` would cut the bullpen processes. But
  the tests would then stop checking the program a person runs, and bullpen's state (its
  environment variables and working folder) is per-process, so in-process tests couldn't run
  side by side.

**Not worth it for speed.** Option 2 gets almost all of the saving for about a twentieth of
the code.
