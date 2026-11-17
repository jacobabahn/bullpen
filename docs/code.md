# The code

bullpen's command line is Rust (it was Python until the port of BP-101's
`rust-port-plan.md`, BP-103 to BP-116). The tests are still Python (`tests/`,
docs/tests.md); their fake Rex and fake gh are Rust (`tests/fakes/`, BP-138).

- `bin/bullpen` is the entry everything runs: people, `install.sh`'s link, Lua, the key
  bindings, the board and bullpen's own background jobs. It's a small `sh` script that
  runs the build, `target/bullpen`, or with no build says to run `install.sh`. It forks
  nothing on its usual path, and resolves a link (install.sh's) only when it's run
  through one.
- `target/bullpen` (`cli/src/main.rs`) looks the command up in its table,
  `cli/src/commands.rs`: every command, its group in `--help`, its help line, and the
  function that runs it (`bullpen _commands` prints the table). No command or `--help`
  prints the help, and a name bullpen doesn't have is one line ("Did you mean"), both
  in `cli/src/help.rs`.
- A call for an internal command (`_...`: a job's, or the watcher's from a newer
  `watch.lua`) the build doesn't have, while the build is older than its source (the
  window between an accept's merge and its new build), catches up first
  (`build::catch_up`): it waits for the build that's running (at most
  `BULLPEN_BUILD_WAIT`, 600 s), or builds what's stale, and runs the call again with
  the new build, once. Only when it can't does it say to run `bullpen build`. A
  person's command never sets off a build: a typo gets the unknown-command line.
- `install.sh` builds the command line with cargo itself, every time, before
  `bullpen build` (the command line's own). Every place that runs cargo on bullpen
  runs it through `bin/bullpen-cargo`, the one lookup of a working cargo
  (docs/rebuilds.md).
- The Rust is one cargo workspace (`Cargo.toml`, one `Cargo.lock`, built into
  `target/`): `core/` (`bullpen-core`: tasks, config, wording, what the command line
  and the board share), `cli/` (`bullpen-cli`, the program `bullpen`), `board/` and
  `tests/fakes/` (`bullpen-fakes`, the tests' fake `rex` and `gh`; only `tests/run.py`
  builds it).
  `core` never depends on `cli`; the board depends on `core`. The crates are `serde_json`
  (with `preserve_order`), `chrono`, `libc` and `unicode-width` (the board's too, and
  ratatui's); ask before adding one. Rust tests sit beside each module as
  `test_<module>.rs`. rustfmt with `max_width = 120` (`rustfmt.toml`) for `core`,
  `cli` and `tests/fakes`; the board isn't formatted yet.
- The files bullpen writes (tasks, config, news, stamps) are as the Python wrote them:
  JSON as Python's `json` (indent 2, ASCII only, keys in order), stamps as `isoformat`,
  cuts as Python's. Comments that say "as Python's" name what's kept. The width of text
  on screen is `unicode-width`'s, as the board and ratatui measure it; it differs from
  Python's `unicodedata` only on zero-width characters, which take no column.

## Commands

A command is a row in `cli/src/commands.rs` (its group is a title in `help::GROUPS`;
internal commands, the watcher's and bullpen's own, named `_...`, have none) and a
function `cmd_...(argv)` in its module, which reads its arguments (`args.rs`, by
argparse's rules: prefixes, `--x=y`, `--`, the last of a repeated option) and calls
plain functions. No other code calls a `cmd_...`. Auto-accept calls `accept::accept`
(cli/src/accept/mod.rs).

## Where things are

Each module says what it's for in its own header: the opening `//!` comment of a Rust
file, the opening `--` comment of a Lua one. `bin/code-map` prints them as the code map,
grouped by crate and folder, a line per file (each header's first paragraph):

    bin/code-map                  # the whole map
    bin/code-map cli/src/news     # only the files under these paths
    bin/code-map --full board     # each file's whole header

It reads the checkout it's in, so a worktree maps its own code, built or not. There's no
list to keep: a new module writes its header, and a test (`tests/test_cli.py`) holds
every module in `core/`, `cli/`, `board/`, `tests/fakes/` and `lua/` to having one.
A module's tests (`test_<module>.rs`) sit beside it and aren't on the map.

## Closing a task

Accept, close, drop, finish's auto-accept and auto-accept end a task through one close
step, `close` in `cli/src/close.rs`, in one order: the reviewer's pane, the PR
(withdrawn, unless it was merged), the worktree (and its Claude scratch folder) and branch, the status and the save,
then what the caller says and does (`then`: news, and after an accept the rebase of
the other tasks), and the worker's pane last, since the step may run in it. Each
caller says only what to tell and how to merge. Nothing calls the run queue: the save
moves it.

## Where a pane lives

A worker's, reviewer's or diff's pane goes where its placement in `cli/src/panes/` puts it:
`FLOAT` (a capsule in its coordinator's tab, the default), `TAB` (a capsule in its
coordinator's workers tab, found by the tab's label), `TABS` (a tab of its own) or
`BACKGROUND` (a detached Rex block, in no tab), picked by the task's own `placement`
(`start --pane`) over the `workers` setting, and the `reviewers` setting (`for_new`; a
diff never goes to the background). Rex places a block only beside another, and one
placed beside a floating block joins its layer, so `gather` moves a pane into a workers
tab beside a stand-in on a layer (or in a tab) of its own, then closes the stand-in. Each placement has the same
steps: `place` (make the pane; one the hide key puts away gets its hint in its label),
`show`, `hide` (False when it has no away, so `hide` exits 3), `find` (by ID, else
label, after a Rex restart), and what `gather` does to keep it home. Code that makes,
shows, hides or finds one of these panes calls `panes.place`, `show`, `hide`, `find`
and `gather`, and never reads the settings. `place` returns the placement's name with
the block, and the caller records it with the pane (the task's `pane`, its review's
`pane`): a background pane that's up floats like a capsule, so the record is what
says to detach it rather than minimize it. Otherwise showing and hiding go by where the
pane is now (`placement_at`), so a pane started before the settings changed keeps
working. Rex's `session.view` lists only placed blocks: `BACKGROUND.find` asks
`session.list_blocks` for the detached ones, once per view. A new placement is a new
`Placement` there.

## Local mode and remote mode

The steps that differ between the modes go through one of two adapters in
`cli/src/modes.rs`, `REMOTE` (origin and gh) or `LOCAL` (the local base branch and the push
remote), with the same verbs: `publish` (at ready), `merge`, `after_merge`, `withdraw`
(close and drop), `refresh` (push a branch bullpen rebased) and `base_ref`.
`mode(repo, t)` picks one: the mode recorded on the task when it was published (its
`local`, true or false), else the repo's as it is now. Code doesn't ask
`repo.has_origin()` about a task; that's only for the repo itself (doctor, the push
remote, the base new work starts from). Every push goes through `quiet_git`, which
never lets git wait on a prompt.

## Start-up

Agents, hooks, the board and the watcher call bullpen all the time, so a call stays
cheap: the `sh` entry adds about 0.5 ms, and a command takes about 4 ms (`_nop`
times start-up). A command reads only the files it needs.

## What a save sets off

Saving a task goes through `saves::save`: core writes it (`Repo::save_task`, which
returns the task as it was), then, in one order, the run queue hears of the status
change (`move_queue`: a task that leaves `working` or closes frees a slot), the
person's notification (`notifications::notify`), and a PR the watcher should follow
(`follow_saved`). No caller has to remember any of them.

What a save sets off that should wait for the command's own work (the run queue,
whose news follows the command's) goes to `saves::later(key, fn)`. `main` runs it when
the command returns (`run_later`), and a command that closes the pane it runs in runs
it first: `rex::close_last` closes a pane after it.

A file stays under 1,000 lines unless there's a good reason; a test
(`tests/test_cli.py`) holds every `.rs` file in `core/` and `cli/` to it.
