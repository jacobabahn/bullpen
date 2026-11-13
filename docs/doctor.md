# Doctor

`bullpen doctor` checks the setup and prints one line per check: `ok`, `FAIL` with
the exact fix, or `--` when a check doesn't apply or can't be told. It exits
non-zero when anything fails. It checks:

- `rex` is on PATH and its server is preview.13 or later (`rex server status`)
- `gh` is installed and signed in (`gh auth status`), unless the repo is in
  [local mode](local-mode.md)
- the programs in the coordinator, worker and reviewer commands (Claude Code's
  `claude`, unless you changed them) are on PATH
- the board is built from `board/`'s source as it is now (`Cargo.toml`, `Cargo.lock`
  and `src/`, by hash), or being rebuilt. Else it says why: its last build failed
  (with the log), `board/` has uncommitted changes, or it just hasn't been rebuilt
  yet; `bullpen build-board` fixes each
- the keys are in your Rex config: Rex lists the `bullpen_next`, `bullpen_board`,
  `bullpen_coordinator` and `bullpen_jump` actions (`rex actions`). With some of them
  listed and one missing, the line is there but Rex loaded an older `keys.lua`: the
  fix is `rex config reload`. With none listed, it's `bullpen keys --install`
- the watcher is running, from the current `watch.lua` and bullpen code (`bin/bullpen`
  and `bullpen/*.py`; the hashes in `watcher.json`)
- the state folder is writable, and this repo's `config.json` parses
- the global `config.json` (see [Settings](settings.md)), if there is one, parses
  and sets nothing only a repository can have
- the setup and teardown hooks, if there are any: `.bullpen/setup.sh` as committed
  on the base (its `#!` interpreter exists, and an `sh` or `bash` script parses), or
  the `config --setup` command (it parses, and its program is found). A
  `.bullpen/setup.sh` that is there but not committed on the base is a failure: it
  wouldn't run.
- the push remote, if one is set (see [Local mode](local-mode.md)): the remote is
  there, answers without a prompt, and has the base branch as it is here. Else, for
  one behind: "github is 2 commits behind main: `git push github main`"
- the coordinators in `config.json` still have their panes and boards, and each
  running board runs the program `bullpen board` runs now (Rex says which program a
  pane runs). One from an older install's path never reloads by itself: "c1's board
  is from an older build: `bullpen up` restarts it". `up` opens a new board in its
  place and closes the old one

`bullpen up` runs the quick checks (not `gh` or the push remote, which need the
network, nor the watcher and coordinators, which `up` sees to itself) and, if any fail, prints one
line: "2 problems: run `bullpen doctor`". Doctor itself ends with the failed checks'
names ("2 problems: keys, watcher."), and writes paths in your home folder with `~`.
