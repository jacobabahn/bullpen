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
- `accept` checks only in bullpen's own repository. Accepting a task elsewhere never
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
  doesn't try that source again by itself; `bullpen build` does. A command whose
  Python is gone but which an old build still hands to Python says to rebuild.
- cargo can be a mise shim with no Rust version set for bullpen's folder. Then
  bullpen tries rustup's `~/.cargo/bin/cargo`, and failing that says to set a
  version. It never changes your mise config.

With `BULLPEN_BOARD` set (a board program of your own), bullpen doesn't build the
board by itself. Boards started from an older install's path
(`board/target/bullpen-board`, from before the workspace) don't reload themselves:
`bullpen doctor` names them, and `bullpen up` restarts them. `bullpen build-board`
is the old name of `bullpen build`, until the port is done.
