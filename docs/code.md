# The code

bullpen's Python is the `bullpen/` package, beside `bin/`. `bin/bullpen` is only the
entry script: it finds the package from its own resolved path (so `install.sh`'s link
to it works) and runs `bullpen.cli.main`. Python caches a package's bytecode, but
compiles a script it runs on every call, so keeping the script small makes every call
(the board's, the watcher's, the keys') about twice as fast. Lua, the key bindings
and bullpen's own background jobs still run `bin/bullpen`.

The modules, each importing only the ones above it in this list (and `cli.py`
importing the one with the command being run, or all of them for `--help`):

- `core.py`: the base: paths, the settings table (`SETTINGS`, at the top), small
  helpers, a repo's config and tasks (`Repo`), a task's status (`set_status`) and the
  panes bullpen knows. It imports none of the others.
- `modes.py`: local mode and remote mode, as two adapters (`mode(repo, t)`), the
  task's base ref, and the one way to push (`quiet_git`)
- `rex.py`: Rex: running its commands, finding, typing into and closing panes
- `agents.py`: a task's worker and reviewer: where each one's pane is (`pane`), and
  the one the task waits on (`waits_on`, `waited_on`). Code that needs either asks it,
  rather than reading the task's `block` or its review's `session`
- `coordinators.py`: a repo's coordinators, and who sees to a task (`sees_to`)
- `panes.py`: where a worker's, reviewer's or diff's pane lives: making, showing,
  hiding and finding it again, and keeping it there (`gather`)
- `news.py`: news lines, the outboxes typed into coordinators' panes, `bullpen news`
- `notifications.py`: the person's notifications
- `guide.py`: the coordinator's guide, and telling running coordinators it changed
- `limit.py`: the usage limit
- `stops.py`: a worker or reviewer stopping: triage, or the person
- `hooks.py`: a worktree's setup and teardown hooks
- `rules.py`: the rules every worker and reviewer gets
- `start.py`: adding and starting tasks: what a task waits on, the run queue (which
  moves by itself, from a status change), an agent's pane, handing a worker a message
- `review.py`: what a review covers, starting a reviewer
- `answer.py`: answering a prompt from the board, `reply`
- `triage.py`: the coordinator's `ask`, `ok`, `review` and `note`
- `watcher.py`: starting the watcher, and replacing it when the code changes
- `close.py`: the one close step (`close`) that accept, close, drop and auto-accept
  share, and the `close` and `drop` commands
- `task_line.py`: one task line, as the board says it
- `line_edit.py`: typing on a terminal, for the picker and the stepper
- `tours.py`: guided review tours and the person's comments on them
- `stepper.py`: reading a tour on the board, and the `tour` command that opens it
- `ready.py`: `ready` and `finish`
- `board.py`: the board's program, and its pane beside a coordinator
- `board_build.py`: rebuilding the board when its source changes
- `push_remote.py`: the push remote's background push and its checks, in local mode
- `keys.py`: bullpen's key bindings in the Rex config
- `accept.py`: accepting a task, and rebasing the others after it
- `auto_accept.py`: a reviewer's verdict, and auto-accept
- `prs.py`: following a task's pull request after ready
- `config.py`: `config`
- `go.py`: `go`, `hide`, `adopt`, and finding a repo's panes again after a Rex restart
  (`relink`)
- `show.py`: `ls`, `show` and `diff`
- `doctor.py`: `bullpen doctor`
- `pick.py`: the jump picker
- `up.py`: `up`
- `events.py`: what the watcher reports: an agent's events, a pane that closed, no
  progress, the usage limit's reset
- `check.py`: `check`, which applies the events the watcher missed
- `cli.py`: the parser and `main`. It keeps only the `--help` groups' titles and
  their order (`GROUPS`), the modules with commands (`MODULES`), and which module
  declares each command (`COMMANDS`).

Each module with commands declares them in its own `commands(sub)`, with
`core.command(sub, name, fn, group, help=...)`; the group is a title in `cli.GROUPS`,
and internal commands (the watcher's and bullpen's own, named `_...`) have none.
`bullpen --help` lists a group's commands in `MODULES` order, then in the order each
module declares them. A new command touches its own module, and `cli.COMMANDS`
(a test checks that it says what the modules declare).

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

A worker's, reviewer's or diff's pane goes where its placement in `panes.py` puts it:
`FLOAT` (a capsule in its coordinator's tab, the default), `TABS` (a tab of its own) or
`BACKGROUND` (a detached Rex block, in no tab), picked by the `workers` and `reviewers`
settings (`for_new`; a diff never goes to the background). Each placement has the same
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
class there, added to `PLACEMENTS`.

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
  and builds only its parser. `--help` and no command build them all.
- `cli.Formatter` loads argparse's colours (Python 3.14's `_colorize`, about 13 ms)
  and the terminal's width (`shutil`) only when it formats help or usage.
- `bin/bullpen` runs `python3 -S`: `site` adds nothing to a standard-library program.
- A standard module that takes over a millisecond to import and that most calls never
  use (`shutil`, `hashlib`, `tempfile`, `textwrap`) is imported in the function that
  needs it, with a comment saying why. A module compiles its patterns on first use
  (re caches them), not on import. `python3 -X importtime bin/bullpen ls` shows what
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
(`tests/test_cli.py`) holds every module to it.
