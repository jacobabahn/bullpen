# The code

bullpen's command line is moving from Python to Rust, one group of commands at a time
(the plan: BP-101's `rust-port-plan.md`). Until the last of it moves, both are here:

- `bin/bullpen` is the entry everything runs: people, `install.sh`'s link, Lua, the key
  bindings, the board and bullpen's own background jobs. It's a small `sh` script that
  runs the Rust build, `target/bullpen`, or before the first build the Python
  (`bin/bullpen-py`). It forks nothing on its usual path.
- `target/bullpen` (`cli/src/main.rs`) looks the command up in its table,
  `cli/src/commands.rs`: every command, its group in `--help`, its help line, and who
  owns it. No command or `--help` prints the help and a name bullpen doesn't have is one
  line ("Did you mean"), both from Rust (`cli/src/help.rs`). A Rust command runs there.
  A Python one is handed over whole, with `exec` of `bin/bullpen-py` before anything is
  read or written, so its arguments, help and errors are argparse's. A command is
  always one side's, never some calls each: `tests/test_cli.py` checks that the
  Python-owned names are exactly the ones `cli.COMMANDS` declares (`bullpen _commands`
  prints the table). Moving a command changes its row's owner and deletes its Python.
- `bin/bullpen-py` is the Python entry: it finds the `bullpen/` package beside it and
  runs `bullpen.cli.main`. Python caches a package's bytecode, but compiles a script it
  runs on every call, so it stays small. Python given a command it doesn't have says
  it isn't in this build: the build is older than its source. First it waits for the
  build that's running, or builds it, and runs the call again with the new build
  (`board_build.catch_up`); only when it can't does it say to run `bullpen build`.
- The Rust is one cargo workspace (`Cargo.toml`, one `Cargo.lock`, built into
  `target/`): `core/` (`bullpen-core`: tasks, config, wording, what the command line
  and the board share), `cli/` (`bullpen-cli`, the program `bullpen`) and `board/`.
  `core` never depends on `cli`. Rust tests sit beside each module as
  `test_<module>.rs`. rustfmt with `max_width = 120` (`rustfmt.toml`) for `core` and
  `cli`; the board isn't formatted yet.
- Python code that still calls code that moved keeps its own copy, a twin: library
  code, frozen, deleted with its last Python caller. `bullpen/core.py` is `core/`'s
  twin, and `task_line.py` is `core/src/words.rs`' and `cli/src/task_line.rs`' (for
  `pick` and `config`); `news.py` (`news`, `tell_coordinator`, `ready_news`, the outboxes),
  `notifications.py` (`notify`), `stops.py`, `agents.py`, `coordinators.py`, `modes.py`,
  `rex.py` and `panes.py` (for launch, review and `up`) are twins of `cli/src/`'s for the
  Python commands, and so are `go.py` (only `relink`, for `up`) and `board.py`
  (`ensure_board`, `old_board` and the board's paths, for `up`, doctor and the build). There is one bridge,
  the run queue: a Rust save that frees a slot runs `bullpen _queue` (Python's
  `start_queued`, reading the tasks fresh) once the command is done, until T6 moves the
  queue to Rust.
- `cli/check/compare.sh` runs each case of `cli/check/cases-*.txt` with the Python and
  the Rust build in a fresh fixture, and compares stdout, stderr, exit code and the
  state folder, byte for byte, and what the fake Rex was asked and its windows after. Each port adds cases for what it moves, and runs them
  against main's Python (`BULLPEN_PY=<main checkout>/bin/bullpen-py`).

## The Rust

`core/src/`:

- `fail.rs`: `Fail`, a command's error ("bullpen: ...", exit 1), and `Res`
- `paths.rs`: bullpen's checkout (from the program's own resolved path), the state
  folder, Rex, `bin/bullpen` and `bin/bullpen-py`
- `settings.rs`: the settings table, in `config`'s order
- `config.rs`: the defaults, the global config, a repo's, and the migrations
- `repo.rs`: `Repo`: its config and tasks, `save_task` (which returns the task as it
  was: the cli runs what a save sets off), the repo a command runs in
- `task.rs`: a task's status (`set_status`), stops, and the board's words for it
- `json.rs`: JSON as Python's `json` writes it (indent 2, ASCII only, keys in order)
- `text.rs`, `widths.rs`: widths and cuts as Python's, and textwrap
- `words.rs`: what the board says about a task, for `ls`, `show` and the board: its
  mark, the board's order, its age, and the board's sentence (`board_detail`)
- `timeutil.rs`: ISO stamps as Python's `isoformat`, ages, the clock
- `proc.rs`: running a program, `git`, `file_hash` (`sha256.rs`)
- `lock.rs`: flock, the same lock as Python's `fcntl.flock`

`cli/src/`:

- `main.rs`: the entry: look the command up, run it or hand it to Python
- `commands.rs`: the table, in `--help`'s order, one line per command
- `args.rs`: a command's arguments by argparse's rules, and its usage and help
- `help.rs`: the top-level help and the unknown-command line, as the Python wrote them
- `show.rs`: `ls` and `show`
- `task_line.rs`: one task line, as the board says it (or its rows, on a narrow
  terminal), cut to the terminal's width and coloured there
- `saves.rs`: saving a task and what it sets off (`save`): the run queue (through
  `bullpen _queue`), the person's notification, the PR to follow; `later` and `run_later`
- `rex.rs`: Rex: running its commands, finding (`find_pane`, by ID or label), typing
  into, focusing and closing panes, a pane's terminal and program, the end of its screen
  (`screen_end`)
- `panes/`: where a worker's, reviewer's or diff's pane lives (see "Where a pane lives"):
  `mod.rs` (`place`, `show`, `hide`, `find`, `gather`, `placement_at`, `for_new`), and one
  file per placement that does more than focus: `float.rs`, `tab.rs`, `background.rs`
- `go/`: `mod.rs`: `go`, `hide` and `adopt`; `relink.rs`: finding a repo's panes again
  after a Rex restart
- `board.rs`: `board`, and a coordinator's board pane beside it (`ensure_board`)
- `diff.rs`: `diff` (the pager, `--stat`, `--pane`)
- `agents.rs`: a task's worker and reviewer, and the one it waits on
- `coordinators.rs`: a repo's coordinators, who sees to a task, whose board shows it
  (`on_board`), and the session to work in (`session_for`)
- `news/`: `mod.rs`: a news line (`news`, `tell_coordinator`), the outboxes and typing
  them into coordinators' panes (`deliver`, `has_draft`); `read.rs`: `news` and
  `news --follow`
- `notifications.rs`: the person's notifications (the save hook) and `_flush`
- `stops.rs`: a worker or reviewer stopping (its callers move from T6 on)
- `modes.rs`: the task's base ref (the adapters move in T8)
- `start/mod.rs`: `add`, `after`, `model`, `review-mode`, what a task waits on, and
  which saves move the run queue (`slot_freed`); `peek.rs`: `peek`
- `triage.rs`: `ask`, `ok` and `note`

## The Python

The modules, each importing only the ones above it in this list (and `cli.py`
importing the one with the command being run, or all of them for `--help`):

- `core.py`: the base: paths, the settings table (`SETTINGS`, at the top), small
  helpers, a repo's config and tasks (`Repo`), a task's status (`set_status`) and the
  panes bullpen knows. It imports none of the others.
- `jobs.py`: starting a background job so it outlives the command and its pane (a
  double fork), with its output in `jobs.log` and its failure in the news
- `modes.py`: local mode and remote mode, as two adapters (`mode(repo, t)`), the
  task's base ref, and the one way to push (`quiet_git`)
- `rex.py`: Rex: running its commands, finding, typing into and closing panes
- `agents.py`: a task's worker and reviewer: where each one's pane is (`pane`), and
  the one the task waits on (`waits_on`, `waited_on`). Code that needs either asks it,
  rather than reading the task's `block` or its review's `session`
- `coordinators.py`: a repo's coordinators, and who sees to a task (`sees_to`)
- `panes.py`: where a worker's or reviewer's pane lives: making, showing, hiding and
  finding it again, and keeping it there (`gather`; a twin, for launch, review and `up`)
- `news.py`: news lines, and the outboxes typed into coordinators' panes
- `notifications.py`: the person's notifications
- `guide.py`: the coordinator's guide, and telling running coordinators it changed
- `limit.py`: the usage limit
- `stops.py`: a worker or reviewer stopping: triage, or the person
- `hooks.py`: a worktree's setup and teardown hooks
- `rules.py`: the rules every worker and reviewer gets
- `start.py`: starting tasks: what a task waits on, the run queue (which moves by
  itself, from a status change; `_queue` for a Rust save), an agent's pane, handing a
  worker a message
- `review.py`: what a review covers, starting a reviewer
- `answer.py`: answering a prompt from the board, `reply`
- `triage.py`: the coordinator's `review`
- `watcher.py`: starting the watcher, and replacing it when the code changes
- `close.py`: the one close step (`close`) that accept, close, drop and auto-accept
  share, and the `close` and `drop` commands
- `task_line.py`: one task line, as the board says it (a twin, for `pick` and `config`)
- `line_edit.py`: typing on a terminal, for the picker and the stepper
- `tours.py`: guided review tours and the person's comments on them
- `stepper.py`: reading a tour on the board, and the `tour` command that opens it
- `ready.py`: `ready` and `finish`
- `board.py`: the board's program, and its pane beside a coordinator (a twin, for `up`,
  doctor and board_build)
- `board_build.py`: building bullpen's command line and the board, and rebuilding them
  when their source changes
- `push_remote.py`: the push remote's background push and its checks, in local mode
- `keys.py`: bullpen's key bindings in the Rex config
- `accept.py`: accepting a task, and rebasing the others after it
- `auto_accept.py`: a reviewer's verdict, and auto-accept
- `prs.py`: following a task's pull request after ready
- `config.py`: `config`
- `go.py`: finding a repo's panes again after a Rex restart (`relink`; a twin, for `up`)
- `doctor.py`: `bullpen doctor`
- `pick.py`: the jump picker
- `up.py`: `up`
- `events.py`: what the watcher reports: an agent's events, a pane that closed, no
  progress, the usage limit's reset
- `check.py`: `check`, which applies the events the watcher missed
- `cli.py`: the parser and `main`. It keeps only the groups' titles (`GROUPS`), the
  modules with commands (`MODULES`), and which module declares each command Python
  owns (`COMMANDS`). The help comes from the Rust entry.

Each module with commands declares them in its own `commands(sub)`, with
`core.command(sub, name, fn, group, help=...)`; the group is a title in `cli.GROUPS`,
and internal commands (the watcher's and bullpen's own, named `_...`) have none. A
new command touches its own module, `cli.COMMANDS` (a test checks that it says what
the modules declare) and its row in `cli/src/commands.rs`, which gives `bullpen --help`
its order.

A command function (`cmd_...`) only reads its arguments and calls plain functions;
no other code calls it. Auto-accept calls `accept.accept`, and `check --fix` calls
`events.event` and `events.gone`, as the watcher's commands do.

## Closing a task

Accept, close, drop and auto-accept end a task through one close step,
`close.close(repo, t, status, then)`, in one order: the reviewer's pane, the PR
(withdrawn, unless it was merged), the worktree and branch, the status and the save,
then what the caller says and does (`then`: news, and after an accept the rebase of
the other tasks), and the worker's pane last, since the step may run in it. Each
caller says only what to tell and how to merge. Nothing calls the run queue: the save
moves it.

## Where a pane lives

A worker's, reviewer's or diff's pane goes where its placement in `cli/src/panes/` (and
its Python twin, `panes.py`) puts it:
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
`Placement` there (a class in the twin, added to `PLACEMENTS`).

## Local mode and remote mode

The steps that differ between the modes go through one of two adapters in
`modes.py`, `REMOTE` (origin and gh) or `LOCAL` (the local base branch and the push
remote), with the same six verbs: `publish` (at ready), `merge`, `after_merge`,
`withdraw`, `refresh` (push a branch bullpen rebased) and `base_ref`.
`mode(repo, t)` picks one: the mode recorded on the task when it was published (its
`local`, true or false), else the repo's as it is now. Code doesn't ask
`repo.has_origin()` about a task; that's only for the repo itself (doctor, the push
remote, the base new work starts from). Every push goes through `quiet_git`, which
never lets git wait on a prompt.

## Start-up

Most of a call's time is Python starting up and importing, and agents, hooks, the
board and the watcher call bullpen all the time, so a call imports only what it
needs (BP-94 took `ls` from about 72 ms to 37 ms):

- `cli.main` imports only the module with the command being run (`cli.COMMANDS`)
  and builds only its parser.
- `cli.Formatter` loads argparse's colours (Python 3.14's `_colorize`, about 13 ms)
  and the terminal's width (`shutil`) only when it formats help or usage.
- `bin/bullpen-py` runs `python3 -S`: `site` adds nothing to a standard-library program.
- The `sh` entry adds about 0.5 ms to a call and the Rust hand-over about 1 ms; a
  command in Rust takes about 4 ms.
- A standard module that takes over a millisecond to import and that most calls never
  use (`shutil`, `hashlib`, `tempfile`, `textwrap`) is imported in the function that
  needs it, with a comment saying why. A module compiles its patterns on first use
  (re caches them), not on import. `python3 -X importtime bin/bullpen-py tell BP-1 x` shows what
  a call imports.

## No import loops

A module imports the bullpen modules it needs at the top, never inside a function
(`cli.py` imports a command's module by name), and the imports never loop back:
`tests/test_cli.py` checks both. Where a lower module has to set something off in a
higher one, the higher one registers with the lower:

- Saving a task (`Repo.save_task`) calls each function registered with
  `core.on_save(fn)`, as `fn(repo, t, old)`, `old` the task as it was saved before.
  `notifications.py` registers the person's notification, `prs.py` marks a PR
  for the watcher to follow, and `start.py` moves the run queue when a task leaves
  `working` or closes (`move_queue`). A command imports only its own module, so
  `cli.main` has the first save import these three (`core.LOAD_SAVERS`), and their
  functions run in one order (`cli.SAVERS`) whichever the command imported first.
  A command that can replace bullpen's own code under itself imports them first
  (`core.load_savers`): `accept`, before it merges into the checkout bullpen runs from,
  and the tour stepper, which stays open while others accept. Otherwise a module
  imported after that would be the new code, beside the old the command already runs.
- What a save sets off that should wait for the command's own work (the run queue,
  whose news follows the command's) goes to `core.later(key, fn)`. `cli.main` runs
  it when the command returns (`core.run_later`), and a command that closes the pane
  it runs in runs it first: `rex.close_last` closes a pane after it.
- A tour written in the background opens in the stepper through the `on_written`
  function the stepper hands `tours.write_tour`.

A file stays under 1,000 lines unless there's a good reason; a test
(`tests/test_cli.py`) holds every Python module and every `.rs` file in `core/` and
`cli/` to it.
