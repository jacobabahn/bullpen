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
  (`board_build.catch_up`, which waits at most `BUILD_WAIT`); only when it can't does
  it say to run `bullpen build`. The build entries (`build`, `_build_board`) never go
  to the catch-up, which would wait for the lock the background build holds itself:
  `board_build.build_entry` builds with the Python's twin (BP-119).
  With no build at all, it says to run `install.sh`, which builds the command line with
  cargo itself, every time, before `bullpen build` (the command line's own). Every place
  that runs cargo on bullpen runs it through `bin/bullpen-cargo`, the one lookup of a
  working cargo (docs/rebuilds.md).
- The Rust is one cargo workspace (`Cargo.toml`, one `Cargo.lock`, built into
  `target/`): `core/` (`bullpen-core`: tasks, config, wording, what the command line
  and the board share), `cli/` (`bullpen-cli`, the program `bullpen`) and `board/`.
  `core` never depends on `cli`. Rust tests sit beside each module as
  `test_<module>.rs`. rustfmt with `max_width = 120` (`rustfmt.toml`) for `core` and
  `cli`; the board isn't formatted yet.
- Python code that still calls code that moved keeps its own copy, a twin: library
  code, frozen, deleted with its last Python caller. `bullpen/core.py` is `core/`'s
  twin, and `line_edit.py` is `cli/src/line_edit.rs`' (for the stepper); `news.py` (`news`, `tell_coordinator`, `ready_news`, the outboxes),
  `notifications.py` (`notify`), `stops.py`, `agents.py`, `coordinators.py`, `modes.py`,
  `rex.py` and `panes.py` (for launch and review) are twins of `cli/src/`'s for the
  Python commands, and so are, for accept: `guide.py` (`tell_guide_changes`),
  `watcher.py` (`refresh_watcher`), `board_build.py` (`rebuild_if_stale`, and the
  catch-up of a command an old build hands over), `board.py` (the board's paths, for
  board_build) and `jobs.py`; and `start.py` (`launch` and `hand_back` for accept,
  prs and tours; `waits_phrase` for task_line),
  `rules.py` (`task_rules`, for review), `hooks.py` (the teardown, for close), `tours.py` (for the stepper),
  `review.py` (what a review covers, `start_review`,
  `keep_approval`, `close_reviewer`, for accept, prs, tours and close)
  and `close.py` (`close`, `close_task`, for accept and auto-accept). There is one bridge, the run queue: a Python save that frees a
  slot runs Rust's `bullpen _queue` (`saves.py`'s `move_queue`, which reads the tasks
  fresh) once the command is done.
- `cli/check/compare.sh` runs each case of `cli/check/cases-*.txt` with the Python and
  the Rust build in a fresh fixture, and compares stdout, stderr, exit code and the
  state folder (and the global config.json), byte for byte, and what the fake Rex was asked and its windows after. Each port adds cases for what it moves. Its Python is
  by default the one of the commit the branch started from (`git merge-base HEAD
  main`, a copy in the work folder), never the live main checkout; `BULLPEN_PY` picks
  another.

## The Rust

`core/src/`:

- `fail.rs`: `Fail`, a command's error ("bullpen: ...", exit 1), and `Res`
- `paths.rs`: bullpen's checkout (from the program's own resolved path), the state
  folder, Rex, `bin/bullpen` and `bin/bullpen-py`
- `settings.rs`: the settings table, in `config`'s order (watch.lua's and the board's
  copies of some defaults are held to it by tests/test_config.py)
- `config.rs`: the defaults, the global config, a repo's, and the migrations
- `repo.rs`: `Repo`: its config and tasks, `save_task` (which returns the task as it
  was: the cli runs what a save sets off), the repo a command runs in
- `task.rs`: a task's status (`set_status`), stops, and the board's words for it
- `json.rs`: JSON as Python's `json` writes it (indent 2, ASCII only, keys in order)
- `text.rs`, `widths.rs`, `printable.rs`: widths and cuts as Python's, textwrap,
  `isprintable` and `shlex.join`
- `words.rs`: what the board says about a task, for `ls`, `show` and the board: its
  mark, the board's order, its age, and the board's sentence (`board_detail`)
- `timeutil.rs`: ISO stamps as Python's `isoformat`, ages, the clock
- `proc.rs`: running a program, `git`, `file_hash` (`sha256.rs`)
- `sha1.rs`: SHA-1, for a review's own lines (the same hashes as `review.py`'s)
- `lock.rs`: flock, the same lock as Python's `fcntl.flock`

`cli/src/`:

- `main.rs`: the entry: look the command up, run it or hand it to Python
- `commands.rs`: the table, in `--help`'s order, one line per command
- `args.rs`: a command's arguments by argparse's rules, and its usage and help
- `help.rs`: the top-level help and the unknown-command line, as the Python wrote them
- `show.rs`: `ls` and `show`
- `task_line.rs`: one task line, as the board says it (or its rows, on a narrow
  terminal), cut to the terminal's width and coloured there
- `saves.rs`: saving a task and what it sets off (`save`): the run queue (at
  later-time), the person's notification, the PR to follow; `later` and `run_later`
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
  (`on_board`), the session to work in (`session_for`), adding and pruning them, and a
  gone coordinator's pane looked at later, as a job (`coordinator_gone`, `_coordinator_gone`)
- `news/`: `mod.rs`: a news line (`news`, `tell_coordinator`, `ready_news`), the outboxes and typing
  them into coordinators' panes (`deliver`, `has_draft`); `read.rs`: `news` and
  `news --follow`
- `notifications.rs`: the person's notifications (the save hook) and `_flush`
- `stops.rs`: a worker or reviewer stopping, and a triage passed on to the person
- `hooks.rs`: a worktree's setup and teardown hooks (`set_up`), and removing a
  worktree (`remove_worktree`)
- `rules.rs`: the rules every worker and reviewer gets (`task_rules`), and `rules`
- `limit.rs`: the usage limit: in force for `start` and the queue (`limit_active`,
  `limit_phrase`), and read off an agent's turn or screen for the watcher (`limit_hit`,
  `limit_over`, `limit_reset`, with Python's patterns matched by hand)
- `modes.rs`: the task's and the repo's base ref, the task's mode (`is_local`), `ready`'s
  `publish`, `withdraw` (close and drop), and the one way to push (`quiet_git`,
  `push_base`); `merge`, `after_merge` and `refresh` move with accept (T9)
- `jobs.rs`: starting a background job so it outlives the command and its pane (a
  double fork), with its output in `jobs.log` and its failure in the news
- `up.rs`: `up`
- `guide.rs`: `guide`, and telling running coordinators their guide changed (with
  difflib's diff, ported)
- `watcher.rs`: starting the watcher, and replacing it when the code changes (the same
  stamp as the Python's)
- `build.rs`: `build` and `_build_board` (the build lock passed by fd), and `up`'s
  `rebuild_if_stale`
- `push_remote.rs`: the push remote's background push (`_push`, one at a time, under
  its lock), `up`'s catch-up push, and doctor's check of the push remote
- `doctor/`: `mod.rs`: `doctor` and `up`'s hint; `checks.rs`: the checks
- `start/mod.rs`: `add`, `after`, `model`, `review-mode`, what a task waits on, and
  which saves move the run queue (`slot_freed`); `launch.rs`: `start`, `resume`, `tell`,
  the worker's brief and pane (`start_task`, `launch`) and `hand_back`; `queue.rs`: the
  run queue (`start_queued`) and `_queue`; `peek.rs`: `peek`
- `triage.rs`: `ask`, `ok` and `note`
- `config.rs`: `config`, its flags made from the settings table
- `pick.rs`: the jump picker (`pick`, `--filter`, `--pane`)
- `line_edit.rs`: typing on a terminal: raw mode, bracketed paste, the keys a read
  gives, and the line editor the picker (and the stepper, from T11b) uses
- `review.rs`: what a review covers (`patch_id`, `own_lines`, `rebase_check`,
  `keep_approval`), the kind of a first review, starting a reviewer (`start_review`),
  `close_reviewer`, what `ready` does with review on (`review_at_ready`), and the
  coordinator's `review`
- `ready.rs`: `ready`, `finish`, and the summary a worker or reviewer gives (`summary_text`)
- `close.rs`: the one close step (`close`) that close, drop and finish's auto-accept
  share (and accept from T9), `close_task`, and the `close` and `drop` commands
- `auto_accept.rs`: a reviewer's `verdict`; an approval with auto-accept runs Python's
  `_auto_accept` as a job and waits for it, before the verdict closes the reviewer's pane
- `answer.rs`: answering a prompt from the board (`prompt`, `answer`), and `reply`
- `screen/mod.rs`: what Claude Code's screen says, shared by events, check and answer
  (a background shell still running, compacting, `screen_state`), with Python's
  regexes matched by hand; `screen/prompt.rs`: the prompt a pane's screen ends with
  (`read_prompt`): its question, its options and its ID
- `events/`: what the watcher (lua/watch.lua) reports: `mod.rs`: an agent's events
  (`_event`) and a pane that closed (`_gone`); `stuck.rs`: an agent that shows no
  progress (`_stuck`) and the usage limit's reset (`_limit`)
- `check.rs`: `check`, and `--fix`, which applies the events the watcher missed
  through `events::event` and `events::gone`
- `tours/`: guided review tours. `mod.rs`: a tour's files, the diff it is written on
  (`diff_files`, `covers`), its state (`tour_state`, `tour_stale`) and a step's patch
  (`step_diff`); `write.rs`: the tour agent, `_tour` (run it, check its answer against
  the diff, write the tour, and open the stepper where the board asked); `comments.rs`:
  `tour-comment`. Python's stepper starts `_tour`, for `tour --wait` too

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
  finding it again, and keeping it there (a twin, for launch and review)
- `news.py`: news lines, and the outboxes typed into coordinators' panes
- `notifications.py`: the person's notifications
- `guide.py`: telling running coordinators their guide changed (a twin, for accept)
- `stops.py`: a worker or reviewer stopping: triage, or the person
- `hooks.py`: a worktree's teardown hook, and removing a worktree (a twin, for close)
- `rules.py`: the rules every worker and reviewer gets (a twin, for review)
- `saves.py`: the run queue's save hook (`move_queue`), which runs Rust's `_queue`
- `start.py`: what a task waits on, an agent's pane, handing a worker a message (twins)
- `review.py`: what a review covers, starting a reviewer (a twin)
- `watcher.py`: starting the watcher, and replacing it when the code changes (a twin,
  for accept and board_build)
- `close.py`: the one close step (`close`, `close_task`) for accept and auto-accept (a
  twin)
- `line_edit.py`: typing on a terminal, for the stepper (a twin of `line_edit.rs`)
- `tours.py`: a tour's state and diff, and sending the person's comments, for the
  stepper (a twin of `cli/src/tours/`; the tour agent and `tour-comment` are Rust's)
- `stepper.py`: reading a tour on the board, and the `tour` command that opens it
- `board.py`: where the board's program is built (for board_build)
- `board_build.py`: rebuilding bullpen's command line and the board when their source
  changes (a twin, for accept), the catch-up of a command an old build hands over, and
  the build entries an old build hands over (`build_entry`)
- `keys.py`: bullpen's key bindings in the Rex config
- `accept.py`: accepting a task, and rebasing the others after it
- `auto_accept.py`: auto-accept (`_auto_accept`, which `verdict` and `ready` start)
- `prs.py`: following a task's pull request after ready
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
no other code calls it. Auto-accept calls `accept.accept`.

## Closing a task

Accept, close, drop, finish's auto-accept and auto-accept end a task through one close
step, `close` in `cli/src/close.rs` (and its twin `close.close(repo, t, status, then)`
for the Python's accept), in one order: the reviewer's pane, the PR
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
remote), with the same verbs: `publish` (at ready) and `withdraw` (close and drop) are
Rust's (`cli/src/modes.rs`); `merge`, `after_merge`, `withdraw`, `refresh` (push a branch bullpen rebased) and `base_ref`.
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
  for the watcher to follow, and `saves.py` moves the run queue when a task leaves
  `working` or closes (`move_queue`, through Rust's `bullpen _queue`). A command imports only its own module, so
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
- A tour is written by Rust's `_tour` (`cli/src/tours/write.rs`), which opens it in
  the stepper once written when the board asked for that (the task's `tour.open`).

A file stays under 1,000 lines unless there's a good reason; a test
(`tests/test_cli.py`) holds every Python module and every `.rs` file in `core/` and
`cli/` to it.
