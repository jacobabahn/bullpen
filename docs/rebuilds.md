# Board rebuilds

Every board runs the program `bullpen build-board` (and `install.sh`) builds from
bullpen's own checkout, the one `bin/bullpen` is in. The build records a hash of
the board's source (`board/Cargo.toml`, `Cargo.lock`, `src/`). When the source no
longer matches, as after accepting a task that changes `board/` or a `git pull`,
bullpen builds it again in the background:

- `bullpen up` checks, in any repository.
- `accept` checks only in bullpen's own repository. Accepting a task elsewhere never
  builds anything.
- The build copies cargo's output and renames the copy into place, so a board never
  runs a half-written file.
- Each running board checks its program every second. Once it's replaced, the board
  runs the new one in its own pane, keeping the selected task and `f` (`l` and `o`
  are settings already), and says "Reloaded: the board was rebuilt." It waits while
  a command it started is running, a y/n question is open, or you're typing or
  answering. No pane is reopened.
- If `board/` has uncommitted changes, there's no build, so half-finished work never
  goes out. One news line says so.
- If the build fails (no cargo, a compile error), the old board keeps running. One
  news line gives the reason and the log, `board/target/bullpen-board.log`.
  bullpen doesn't try that source again by itself; `bullpen build-board` does.
- cargo can be a mise shim with no Rust version set for bullpen's folder. Then
  bullpen tries rustup's `~/.cargo/bin/cargo`, and failing that says to set a
  version. It never changes your mise config.

With `BULLPEN_BOARD` set (a board program of your own), bullpen builds nothing.
Boards started from an older install's path (`board/target/release/bullpen-board`)
don't reload themselves: `bullpen doctor` names them, and `bullpen up` restarts them.
