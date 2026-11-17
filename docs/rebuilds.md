# Rebuilds

bullpen's command line (`target/bullpen`, which `bin/bullpen` runs) and every board
(`target/bullpen-board`) are the programs `bullpen build` (and `install.sh`) builds
from bullpen's own checkout, the one `bin/bullpen` is in: one cargo workspace, built
in one cargo run. Each build records a hash of the program's source (its stamp,
`target/bullpen.json` and `target/bullpen-board.json`): the command line's is `core/`
and `cli/` (each crate's `Cargo.toml` and `src/`), the board's `core/` and `board/`,
and both take in the workspace's `Cargo.toml` and `Cargo.lock`. When a source no
longer matches, as after accepting a task that changes it or a `git pull`, bullpen
builds that program again in the background:

- `bullpen up` checks, in any repository.
- The watcher checks its own copy, the one it runs, every 3 minutes (with its
  `check --fix`): see [The watcher's check](#the-watchers-check).
- `accept` checks only when the repository it merged into is a copy of bullpen that
  runs from its own build, right after the merge (before the other tasks are rebased
  and the queued ones start). Accepting a task elsewhere never builds anything. Which
  copy of bullpen ran the accept doesn't matter: see [An accept from another
  copy](#an-accept-from-another-copy).
- The build copies cargo's output and renames the copy into place, so nothing ever
  runs a half-written file. A command that's running keeps its old program; the next
  call runs the new one.
- Each running board checks its program every second. Once it's replaced, the board
  runs the new one in its own pane, keeping the selected task and `f` (`l` and `o`
  are settings already), and says "Reloaded: the board was rebuilt." It waits while
  a command it started is running, a y/n question is open, or you're typing or
  answering. No pane is reopened.
- If a program's source has uncommitted changes, it isn't built, so half-finished
  work never goes out. One news line says so.
- If the build fails (no cargo, a compile error), the old programs keep running. One
  news line per program gives the reason and the log, `target/build.log`. bullpen
  doesn't try that source again by itself; `bullpen build` does.
- An internal command a merge adds (a job's, or one a newer `watch.lua` calls) isn't
  in the old build until the new one is in place. A call to it in that window waits
  for the build that's running, or builds the command line itself when none is, then
  runs again with the new build, once: `` bullpen: `_x` is in the build that's
  running: waiting for it `` on stderr (a job's goes to `jobs.log`). When it can't (the
  source has uncommitted changes, or its build failed), it says to rebuild, with the
  log. A command people type never sets off a build: one the build doesn't have gets
  the unknown-command line, and `bullpen build` puts the new one in place.
- A job never catches up holding the build lock: `_build_board`, the background build,
  holds it (accept passed it on) and would wait for itself. So the build commands keep
  their names.
- Nothing waits for a build for ever. The program that holds the build lock writes its
  pid in `target/build.lock`, and a call that waits for it (a new internal command,
  `bullpen build`) gives up after 10 minutes: `` bullpen: the build that's running (pid 1234)
  still isn't done after 600s: if it's stuck, stop it (`kill 1234`) and run this again ``,
  exiting 1.
- A new command line replaces the running watcher of its own copy, so its calls run
  the new build. Another copy's watcher is left alone: see [One watcher, from the
  coordinator's copy](#one-watcher-from-the-coordinators-copy).
- Every build runs cargo through one lookup, `bin/bullpen-cargo` (see below), so a
  build finds the same working cargo whether it starts in your shell, an agent's pane,
  the watcher, a background job or a test's fixture.

## The watcher's check

A merge made by a copy of bullpen from before BP-165 (which doesn't rebuild the
checkout it merged into), or a plain `git pull`, leaves a copy's build older than its
source, and the watcher and the boards would run old code until the next `up`. So the
watcher's periodic `check --fix --all-repos` also compares its own copy's source with
its build (the test doctor's `bullpen` and `board` lines use) and, when the source is
newer, starts the same background build an accept starts: the same lock, log and
stamps, the watcher restarted and the boards reloaded when it's done. It never builds
another copy, and a `check` you run yourself (without `--all-repos`, or without
`--fix`) never builds.

It builds only a settled tree. It skips, saying why in one news line (once, while the
reason stands; its note is `target/watch-build.json`), when:

- the checkout has uncommitted changes under `core/`, `cli/`, `board/` or `lua/`:
  `` bullpen wasn't rebuilt: ~/dev/bullpen has uncommitted changes under core/, cli/,
  board/, lua/ ... once they are committed or dropped, the watcher builds bullpen at
  its next check (or `bullpen build`, as they are) ``
- a merge or a rebase is in progress there: it builds once it's finished or aborted
- a build is running: left alone, and said only when the same build is still running
  a check later, with its pid, to stop it if it's stuck

A build that fails is the usual news line, once: the same source isn't tried again (a
new commit is). A build that works says nothing, unless the pull changed a
coordinator's guide: the next check, on the new build, tells the coordinators what
changed, as an accept does.

## Which cargo

A build starts wherever the command that set it off runs, and their environments
differ: an agent's pane may have mise's shims on PATH and no `~/.cargo/bin`, and a
test's fixture sets HOME to a folder of its own. rustup's
cargo is a proxy that reads its toolchain from `$RUSTUP_HOME`, by default
`$HOME/.rustup`, so with HOME pointing elsewhere it says "no default is configured"
(BP-120: a fixture's call, right after an accept, set off the catch-up build of main
with the fixture's HOME). `bin/bullpen-cargo` runs the first
cargo that works, trying in order:

1. the one that worked last time, with what it needed (`target/cargo.env`)
2. each cargo on PATH (a mise shim often comes before rustup's)
3. rustup's: `$CARGO_HOME/bin/cargo`, `$HOME/.cargo/bin/cargo`, and the account's own
   `~/.cargo/bin/cargo` (from the password database, whatever HOME says)
4. mise's Rust (`mise where rust`), never `mise exec`, which would install one

Each is checked with `cargo --version` in bullpen's folder first (with
`RUSTUP_AUTO_INSTALL=0`: a check never downloads). A rustup cargo with no default
toolchain is tried again with the account's own rustup (`RUSTUP_HOME`, `CARGO_HOME`),
then with a toolchain rustup has installed (`RUSTUP_TOOLCHAIN`), which the build log
says: `` rustup has no default toolchain: using 1.99.1-x86_64-unknown-linux-gnu (set
one with `rustup default ...`) ``. It never changes your rustup or mise config. When
none works, the build fails with what it tried and why each failed, in the news line
and from `bullpen build`: `` no cargo works here: tried cargo on PATH
(~/.local/share/mise/shims/cargo): a mise shim with no Rust version set for ...; ...
``. `bullpen doctor` checks it (`cargo`), from its own environment, which is the one a
rebuild it set off would have.

From any state of the build (none, older than its source, a build that failed, a stuck
one stopped by hand), `bullpen build` builds both programs again, and so does
`./install.sh`, which builds the command line with cargo itself first, every time
(cargo does nothing when it's current), so it never depends on the build in place. Both
find cargo the same way (`bin/bullpen-cargo`), and so does `tests/run.py`.

With `BULLPEN_BOARD` set (a board program of your own), bullpen doesn't build the
board by itself. Boards started from an older install's path
(`board/target/bullpen-board`, from before the workspace) don't reload themselves:
`bullpen doctor` names them, and `bullpen up` restarts them.

An installed build from before the Python was removed (BP-116) that hands a command to
`bin/bullpen-py` finds no such file: run `./install.sh` once. Builds from after BP-114
hand nothing over.

## An accept from another copy

There can be several copies of bullpen on one machine: the installed checkout, and a
worktree or clone of it on another branch, each with its own `bin/bullpen` and
`target/`, all sharing one state folder. A board opened from one copy runs that copy's
commands, in whichever repository its tasks are in.

When an accept merges into a checkout of bullpen that runs from its own build, that
checkout's programs are checked and rebuilt, whichever copy ran the accept. A checkout
counts when it has all three of: the entry `bin/bullpen`, the workspace's source
(`Cargo.toml` and `cli/Cargo.toml`), and a command line built there before
(`target/bullpen`, which `install.sh` or `bullpen build` made). A clone of bullpen's
source that was never built there isn't one: nothing runs from it, so nothing is built.

The copy that ran the accept checks the merged checkout's source against that
checkout's stamps, takes that checkout's build lock, and starts that checkout's own
`bin/bullpen _build_board` in the background. So the build is the same as when the
checkout's own copy runs the accept: its own source, lock, stamps and `build.log`,
never uncommitted source (the same one news line), the same news lines when it fails,
and its own watcher replaced when its command line is new. The accept says where:
`` rebuilding bullpen's command line in the background (its source changed):
~/dev/bullpen, this repository's own copy of bullpen ``. The copy that ran the accept
isn't rebuilt for it: its own source didn't change.

This is in the copy that runs the accept: a copy from before this (BP-165) still
builds nothing in another checkout, until it has this change itself.

## One watcher, from the coordinator's copy

There is one watcher for the whole state folder: it forwards every repository's panes
to one `bin/bullpen` (`watcher.json` records its pane, its stamp and the copy it runs,
`home`). Which copy that is: the one the repository's coordinator was started from
(with several coordinators, the first one's: the watcher follows that copy).
`up` records it in each coordinator it starts or registers (`bullpen` in the
coordinator's entry in `config.json`); the coordinator's agent was told that copy's
`bin/bullpen` and given its guide, so that is the copy the repository's agents
already work with.

- `up` and `accept` see to the watcher of the repository's coordinator's copy,
  whichever copy runs them: its `watch.lua`, its `bin/bullpen`, its build's stamp. A
  command run by another copy never puts its own watcher in its place, and when none
  runs it starts the coordinator's copy's.
- A build replaces a running watcher only when the watcher runs the copy that was
  built.
- When the coordinator's copy is gone (its folder has no `bin/bullpen` and
  `lua/watch.lua`), no watcher is started from a guess: `doctor` says so (`copy`), with
  the fix, `bullpen up --here` in the coordinator's pane, which makes the copy that
  runs it the coordinator's.
- A coordinator started before copies were recorded is taken to use the copy that next
  runs `up` in its repository; a watcher from before, whose stamp is that copy's, is
  recorded as that copy's without a restart. Until then `accept` and a build treat an
  unrecorded watcher as their own copy's, as they always did.
- With repositories whose coordinators use different copies, the one watcher follows
  the last `up` or `accept`; `doctor` in the other repository says it runs from another
  copy.
- The guide: an accept tells running coordinators when their guide changed. A
  coordinator another copy started has that copy's guide, so only its own copy tells it.

A command run in a repository by a copy other than its coordinator's does its work
with its own code, which may be older (steps a newer accept has aren't in it). It
warns of nothing as it runs. A command that saves a task notes that it ran
(`other-copy.json` in the repository's state folder: the copy, the command, when), and
`doctor` reports it while the two copies' source differs: see [Doctor](doctor.md).
A copy that any of the repository's coordinators was started from is never "another
copy" there: a second coordinator made with another copy's `up --here` runs that
copy's commands, nothing is noted, and `doctor` names its copy beside the first's.

## Fixtures never build

A test's fixture runs bullpen from a real checkout (the one being tested), with a
HOME and a state of its own. A build set off there would build that checkout in the fixture's
environment, and hold its build lock against the person's own builds (BP-120: a
fixture's call caught up, built main's checkout, failed, and the accept's rebuild found
its lock held and skipped). So `tests/support.py` sets `BULLPEN_NO_BUILD=1` for every
bullpen it runs. With it set, bullpen never builds, or takes the build lock in, its
checkout: no catch-up (a command the build doesn't have gets the plain unknown-command
line), no background rebuild from `up` or accept, and `bullpen build` says `builds are
off here`. The tests that are about building (`tests/test_board_build.py` and the
modules that use its setup) build a copy of bullpen, without it.
