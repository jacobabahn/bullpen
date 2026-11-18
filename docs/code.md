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
  ratatui's); ask before adding one. The board has ratatui's and its own besides, the
  image look's among them: `tiny-skia` (drawing), `ttf-parser` (fonts) and
  `miniz_oxide` (its PNGs' zlib). A module's Rust tests are in a `tests/` folder
  beside it, as `test_<module>.rs`. rustfmt with `max_width = 120` (`rustfmt.toml`)
  for `core`, `cli` and `tests/fakes`; the board isn't formatted yet.
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
(cli/src/accept/mod.rs). Its docs are a line in `docs/commands.md` at least (see
[Tests](tests.md#the-tables)); a setting is a row in `SETTINGS` (`core/src/settings.rs`)
and its words in `docs/settings.md`.

A doctor check is a `DoctorCheck` value in the module of `cli/src/doctor/` for its kind
(`tools.rs`, `builds.rs`, `files.rs`, ...), in that module's `CHECKS`: its name, whether
it is about the setup rather than a repo (`shared`: `--all-repos` runs it once), whether
`up` leaves it out (`slow`), how it runs, and its `--fix`, with the fix's words in the
flag's help, if it has one. `doctor/mod.rs` lists the modules in doctor's order and
knows nothing of any one check. Its docs are a line in `docs/doctor.md` naming it.

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
A module's tests (`tests/test_<module>.rs` in its folder) aren't on the map.

## The board's drawing

The board (`board/src/`) draws with ratatui. `ui/rows.rs` works out the list's rows
from the board's state once a frame, as values: each row's text, its mark, whether
it's selected, asked about, dim, its risk and age. The text list (`ui/list.rs`,
`ui/later.rs`) draws them as cells. With the image look (docs/board.md) the list
leaves its cells blank and hands the same rows to `image/`, which draws them as one
image (`render.rs`, given rows and a size in pixels, with no terminal) and, after
ratatui's frame and through the same output, sends and places it with the kitty
graphics protocol (`kitty.rs`, `mod.rs`). Asking the terminal (`term.rs`) needs its
input: the key reader (`main.rs`) stops while the board asks, and reads the answers
itself. The rest of the board stays text.

## Closing a task

Accept, close, drop, finish's auto-accept and auto-accept end a task through one close
step, `close` in `cli/src/close.rs`, in one order: the reviewer's pane, the PR
(withdrawn, unless it was merged), the worktree (and its Claude scratch folder) and branch, the status and the save,
`bullpen: started` taken off an issue's task's issue when nothing was merged (issues/write.rs),
then what the caller says and does (`then`: news, and after an accept the rebase of
the other tasks), and the worker's pane last, since the step may run in it. Each
caller says only what to tell and how to merge. Nothing calls the run queue: the save
moves it.

## Making a task ready

`ready` (review off, or a branch with no commits), an approval kept at `ready`, the
verdict (an approval, or findings after the last round) and a worker's turn that leaves
a ready task's code as approved (`ready_again`) make the task ready through one step, `make_ready` in `cli/src/become_ready.rs`, told how it
became ready (`How`): the risk label, the status, the save, then auto-accept (only for
an approval, as a job started one way, `start_auto_accept`) or the ready news. A branch
with no commits is recorded there as `nothing_to_merge`, the field core's
`nothing_to_merge` reads, and it goes when the worker is back at work (`back_to_work`)
or runs `ready` again; a task made ready before that field is read by its why. A
verdict's approval is checked against its base first when accepts left it in review
(`left_in_review::after_approval`), before auto-accept or the news; the verdict waits
for its auto-accept, the others don't. A ready task rebased after an accept with its
approval kept (`rebase_task`) goes through it too, as `Rebased`: labelled for its code
now, with neither auto-accept nor ready news (the accept's `After BP-3: ...` line says
it). No other code sets a task ready (`test_become_ready.rs` holds that); `answer`
only puts back the status a question interrupted.

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
never lets git wait on a prompt. It runs git through core's `proc::quiet` (no terminal,
a session of its own, a timeout), as `issues/repo.rs`'s `gh_quiet` runs gh for every
issue read and write.

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
