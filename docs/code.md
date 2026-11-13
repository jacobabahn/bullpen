# The code

bullpen's Python is the `bullpen/` package, beside `bin/`. `bin/bullpen` is only the
entry script: it finds the package from its own resolved path (so `install.sh`'s link
to it works) and runs `bullpen.cli.main`. Python caches a package's bytecode, but
compiles a script it runs on every call, so keeping the script small makes every call
(the board's, the watcher's, the keys') about twice as fast. Lua, the key bindings
and bullpen's own background jobs still run `bin/bullpen`.

The modules, each importing only the ones above it in this list (and `cli.py`
importing all of them):

- `core.py`: the base: paths, the settings table (`SETTINGS`, at the top), small
  helpers, a repo's config and tasks (`Repo`), a task's status (`set_status`) and the
  panes bullpen knows. It imports none of the others.
- `rex.py`: Rex: running its commands, finding, typing into and closing panes
- `coordinators.py`: a repo's coordinators, and who sees to a task (`sees_to`)
- `news.py`: news lines, the outboxes typed into coordinators' panes, `bullpen news`
- `notifications.py`: the person's notifications
- `guide.py`: the coordinator's guide, and telling running coordinators it changed
- `limit.py`: the usage limit
- `stops.py`: a worker or reviewer stopping: triage, or the person
- `hooks.py`: a worktree's setup and teardown hooks
- `rules.py`: the rules every worker and reviewer gets
- `start.py`: adding and starting tasks: what a task waits on, the run queue, an
  agent's pane, handing a worker a message
- `review.py`: what a review covers, starting a reviewer
- `answer.py`: answering a prompt from the board, `reply`
- `triage.py`: the coordinator's `ask`, `ok`, `review` and `note`
- `watcher.py`: starting the watcher, and replacing it when the code changes
- `close.py`: closing a finished task, and `drop`
- `task_line.py`: one task line, as the board says it
- `line_edit.py`: typing on a terminal, for the picker and the stepper
- `tours.py`: guided review tours and the person's comments on them
- `stepper.py`: reading a tour on the board, and the `tour` command that opens it
- `ready.py`: `ready` and `finish`
- `board.py`: the board's program, and its pane beside a coordinator
- `board_build.py`: rebuilding the board when its source changes
- `push_remote.py`: the push remote, in local mode
- `keys.py`: bullpen's key bindings in the Rex config
- `accept.py`: accepting a task, and rebasing the others after it
- `auto_accept.py`: a reviewer's verdict, and auto-accept
- `prs.py`: following a task's pull request after ready
- `config.py`: `config`
- `go.py`: `go`, `hide`, `adopt`, and putting a repo's panes back where they belong
- `show.py`: `ls`, `show` and `diff`
- `doctor.py`: `bullpen doctor`
- `pick.py`: the jump picker
- `up.py`: `up`
- `events.py`: what the watcher reports: an agent's events, a pane that closed, no
  progress, the usage limit's reset
- `check.py`: `check`, which applies the events the watcher missed
- `cli.py`: the parser and `main`. It keeps only the `--help` groups' titles and
  their order (`GROUPS`), and the modules with commands (`MODULES`).

Each module with commands declares them in its own `commands(sub)`, with
`core.command(sub, name, fn, group, help=...)`; the group is a title in `cli.GROUPS`,
and internal commands (the watcher's and bullpen's own, named `_...`) have none.
`bullpen --help` lists a group's commands in `MODULES` order, then in the order each
module declares them. A new command touches only its own module.

A command function (`cmd_...`) only reads its arguments and calls plain functions;
no other code calls it. Auto-accept calls `accept.accept`, and `check --fix` calls
`events.event` and `events.gone`, as the watcher's commands do.

## No import loops

A module imports the ones it needs at the top, never inside a function, and the
imports never loop back: `tests/test_cli.py` checks both. Where a lower module has
to set something off in a higher one, the higher one registers with the lower:

- Saving a task (`Repo.save_task`) calls each function registered with
  `core.on_save(fn)`, as `fn(repo, t, old)`, `old` the task as it was saved before.
  `notifications.py` registers the person's notification, and `prs.py` marks a PR
  for the watcher to follow. `cli.py` imports every module, so they're all
  registered before a command runs. This is how the run queue hears that a task left
  `working`, too, once it starts the next task by itself (BP-75's task E).
- A tour written in the background opens in the stepper through the `on_written`
  function the stepper hands `tours.write_tour`.

A file stays under 1,000 lines unless there's a good reason; a test
(`tests/test_cli.py`) holds every module to it.
