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
- `accept` checks only in bullpen's own repository, right after the merge (before the
  other tasks are rebased and the queued ones start). Accepting a task elsewhere never
  builds anything.
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
- A new command line replaces the running watcher, so its calls run the new build.
- Every build runs cargo through one lookup, `bin/bullpen-cargo` (see below), so a
  build finds the same working cargo whether it starts in your shell, an agent's pane,
  the watcher, a background job or a test's fixture.

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
