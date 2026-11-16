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

## The Rust

`core/src/`:

- `fail.rs`: `Fail`, a command's error ("bullpen: ...", exit 1), and `Res`
- `paths.rs`: bullpen's checkout (from the program's own resolved path), the state
  folder, Rex, `bin/bullpen`
- `settings.rs`: the settings table, in `config`'s order (watch.lua's copies of some
  defaults are held to it by tests/test_config.py)
- `config.rs`: the defaults, the global config, a repo's, and the migrations
- `repo.rs`: `Repo`: its config and tasks, `save_task` (which returns the task as it
  was: the cli runs what a save sets off), the repo a command runs in
- `task.rs`: a task's status (`set_status`), stops, and the board's words for it
- `json.rs`: JSON as Python's `json` writes it (indent 2, ASCII only, keys in order)
- `text.rs`, `printable.rs`: the width of text (`unicode-width`'s, one function for
  `ls` and the board), cuts as Python's, textwrap, `isprintable` and `shlex.join`
- `words.rs`: what the board says about a task, for `ls`, `show` and the board: its
  mark (and a backlog task's `↻`, `from_mark`, `from_detail`), the board's order, its age, and the board's sentence (`board_detail`, and its
  parts: `status_detail`, `with_gh`, `auto_detail`, `model_detail`, `doing_said`)
- `fleet.rs`: every known repo in one snapshot (`Fleet`): its root and settings, its
  coordinators (with the watcher's note on each, and Rex's alive, asked apart:
  `ask_alive`), its open tasks with the board's words and counts. `repos`,
  `ls --all-repos`, the board's top line and mission control read it; a `Reader` kept
  between snapshots (the board's) reads a file again only when its time or size changed.
  `fleet::repos` is its list of repos alone (`news --recent --all-repos`), and
  `fleet::unread` the ones whose settings it can't read (`doctor --all-repos`)
- `timeutil.rs`: ISO stamps as Python's `isoformat`, ages, the clock
- `timeline.rs`: a task's timeline (`timeline/<ID>.jsonl`): what a save changed
  (`changes`, from `save_task`), the trip-back mark a step that sends a task back sets
  (`send_back`; `hand_back` takes its reason), and `record` for an event no save shows
- `proc.rs`: running a program, `git`, `file_hash` (`sha256.rs`)
- `risk.rs`: a task's risk label as it's said (`shown`, `stored`, `label`): the
  command line works it out (cli's `risk.rs`), this only reads it
- `sha1.rs`: SHA-1, for a review's own lines (hashlib's)
- `lock.rs`: flock
- `backlog.rs`: the backlog (`backlog.json`): its items, loading and saving under its
  lock (`change`), the order and moves

`cli/src/`:

- `main.rs`: the entry: look the command up and run it, or catch up (an internal one)
  or say it has none
- `commands.rs`: the table, in `--help`'s order, one line per command
- `args.rs`: a command's arguments by argparse's rules, and its usage and help
- `help.rs`: the top-level help and the unknown-command line (as the Python wrote them)
- `show.rs`: `ls` and `show`
- `repos.rs`: `repos`, a line per repository from core's `fleet.rs`, or its JSON
- `mission.rs`: `mission`: make or focus mission control's session and pane
  (`mission.json`), and `--here`, which runs the board's program as mission control
- `task_line.rs`: one task line, as the board says it (or its rows, on a narrow
  terminal), cut to the terminal's width and coloured there
- `saves.rs`: saving a task and what it sets off (`save`): the run queue and then
  pickup (at later-time), the person's notification, the PR to follow; `later` and `run_later`
- `rex.rs`: Rex: running its commands, finding (`find_pane`, by ID or label), typing
  into, focusing and closing panes, a pane's terminal and program, the end of its screen
  (`screen_end`)
- `panes/`: where a worker's, reviewer's or diff's pane lives (see "Where a pane lives"):
  `mod.rs` (`place`, `show`, `hide`, `find`, `gather`, `placement_at`, `for_new`), and one
  file per placement that does more than focus: `float.rs`, `tab.rs`, `background.rs`
- `go/`: `mod.rs`: `go`, `hide` and `adopt` (and `repo_or_here`, the repo a `--repo` names, `say`'s too); `relink.rs`: finding a repo's panes again
  after a Rex restart
- `board.rs`: `board`, and a coordinator's board pane beside it (`ensure_board`)
- `diff.rs`: `diff` (the pager, `--stat`, `--pane`)
- `agents.rs`: a task's worker and reviewer, and the one it waits on
- `coordinators.rs`: a repo's coordinators, who sees to a task, whose board shows it
  (`on_board`), the session to work in (`session_for`), adding and pruning them, and a
  gone coordinator's pane looked at later, as a job (`coordinator_gone`, `_coordinator_gone`)
- `news/`: `mod.rs`: a news line (`news`, `tell_coordinator`, `ready_news`), the outboxes and typing
  them into coordinators' panes (`deliver`, `has_draft`); `read.rs`: `news` and
  `news --follow`; `recent.rs`: `news --recent`, which reads the log's end and moves no cursor;
  `wake.rs`: the backstop for news no coordinator reads (`_wake_check`, which news is worth a
  wake: `wakes`) and `news --pause` / `--resume`
- `notifications.rs`: the person's notifications (the save hook) and `_flush`
- `overlap.rs`: two open tasks changing the same file: checked at a worker's end of turn
  and on `ready` (`check`), one news line per pair, and `show`'s list (`overlaps_of`)
- `stops.rs`: a worker or reviewer stopping, and a triage passed on to the person
- `hooks.rs`: a worktree's setup and teardown hooks (`set_up`), and removing a
  worktree (`remove_worktree`)
- `rules.rs`: the rules every worker and reviewer gets (`task_rules`), and `rules`
- `limit.rs`: the usage limit: in force for `start` and the queue (`limit_active`,
  `limit_phrase`), and read off an agent's turn or screen for the watcher (`limit_hit`,
  `limit_over`, `limit_reset`, with Python's patterns matched by hand)
- `modes.rs`: the task's and the repo's base ref, the task's mode (`is_local`), `ready`'s
  `publish`, accept's `merge` (the PR on GitHub, or the branch into the local base
  branch) and `after_merge`, `refresh` (a rebased branch pushed to its PR), `withdraw`
  (close and drop), and the one way to push (`quiet_git`, `push_base`)
- `jobs.rs`: starting a background job so it outlives the command and its pane (a
  double fork), with its output in `jobs.log` and its failure in the news
- `up.rs`: `up`
- `guide.rs`: `guide`, and telling running coordinators their guide changed (with
  difflib's diff, ported)
- `watcher.rs`: starting the watcher from the copy of bullpen the repo's coordinator
  uses, and replacing it when that copy's `watch.lua` or build changes (its stamp:
  their hashes); after a merge (`refresh_watcher`) or a build
- `copies.rs`: the copy of bullpen a coordinator was started from (`up` records it), a
  command another copy ran noted for doctor, and doctor's `copy` check
- `build.rs`: `build` and `_build_board` (the build lock passed by fd), the
  `rebuild_if_stale` of `up`, accept's `rebuild_merged_into` (the checkout merged into,
  when it runs from its own build), and the catch-up (`catch_up`)
- `push_remote.rs`: the push remote's background push (`_push`, one at a time, under
  its lock), `up`'s catch-up push, and doctor's check of the push remote
- `doctor/`: `mod.rs`: `doctor` (and `--json`) and `up`'s hint; `all_repos.rs`: `doctor --all-repos`; `checks.rs`: the checks
- `start/mod.rs`: `add`, `after`, `model`, `review-mode`, what a task waits on, and
  which saves move the run queue (`slot_freed`); `launch.rs`: `start`, `resume`, `tell`,
  the worker's brief and pane (`start_task`, `write_full`, `launch`) and `hand_back`; `queue.rs`: the
  run queue (`start_queued`); `peek.rs`: `peek` (and `peek --review`)
- `triage.rs`: `ask`, `ok` and `note`
- `brief.rs`: `brief`: a task's brief printed, replaced or added to; the change kept
  (`briefs/<ID>.brief-<n>.md`, the task's `brief_changes`), its worker told what changed
  (`message`, `tell`), and the line a reviewer's follow-up round gets (`changed_since`)
- `backlog/`: `later` (`mod.rs`: adding, who's calling, keep, drop, skip, moves, and
  `show`'s from line; `list.rs`: the list's lines; `start.rs`: an item made a task;
  `pickup.rs`: idle pickup, its caps, and the watcher's `_pickup`)
- `config.rs`: `config`, its flags made from the settings table
- `pick.rs`: the jump picker (`pick`, `--filter`, `--pane`)
- `line_edit.rs`: typing on a terminal: raw mode, bracketed paste, the keys a read
  gives, and the line editor the picker and the stepper's comment and tell boxes use
- `review.rs`: what a review covers (`patch_id`, `own_lines`, `rebase_check`,
  `keep_approval`), the kind of a first review, starting a reviewer (`start_review`),
  `close_reviewer`, what `ready` does with review on (`review_at_ready`), and the
  coordinator's `review`
- `ready.rs`: `ready`, `finish` (and `finish --keep-branch`), and the summary a worker or reviewer gives (`summary_text`)
- `risk.rs`: a ready task's risk label (docs/review.md): bullpen's rules on the diff
  (`rules_of`, `risk_paths`, `adds_dependency`), the reviewer's `## Risk`
  (`reviewer_of`), and `assess`, which each step that makes a task ready calls
- `close.rs`: the one close step (`close`) that accept, close, drop and finish's
  auto-accept share, `close_task`, and the `close` and `drop` commands
- `keep.rs`: `close --keep-branch`: push the branch, then the close step keeping it
- `auto_accept.rs`: a reviewer's `verdict`; an approval with auto-accept runs
  `_auto_accept` as a job and waits for it, before the verdict closes the reviewer's
  pane; `_auto_accept` accepts, or waits for the PR's checks (`checks`, `pr_view`);
  a merge refused for a conflict sends the task back to rebase (`base_conflict`,
  `send_back_conflict` in `accept/rebase.rs`), at most `CONFLICT_SENDS` times
- `accept/`: `mod.rs`: `accept`, and the steps after the merge, each on its own
  (`run_steps`: a failed one is said, the rest still run); `rebase.rs`: rebasing the
  other tasks onto the moved base (`rebase_others`, keeping both sides of a trivial
  conflict in a doc or test file), what a worker is told (`told`), and `show`'s line
  for a ready task that no longer rebases (`stale_conflicts`)
- `prs.rs`: `_prs`, following a task's pull request after ready: its checks, people's
  feedback, a merge or close outside bullpen, a waiting auto-accept
- `keys.rs`: `keys` (and `--install`), and the hash accept checks to say keys.lua changed
- `answer.rs`: answering a prompt from the board (`prompt`, `answer`), and `reply`
- `say.rs`: `say`, a message typed into a repo's coordinator from any pane
- `screen/mod.rs`: what Claude Code's screen says, shared by events, check and answer
  (a background shell still running, compacting, `screen_state`), with Python's
  regexes matched by hand; `screen/prompt.rs`: the prompt a pane's screen ends with
  (`read_prompt`): its question, its options and its ID
- `events/`: what the watcher (lua/watch.lua) reports: `mod.rs`: an agent's events
  (`_event`) and a pane that closed (`_gone`); `stuck.rs`: an agent that shows no
  progress (`_stuck`) and the usage limit's reset (`_limit`)
- `stats/`: `stats` (docs/stats.md): `mod.rs`: the command, one task's stats and the
  totals across tasks, in words or JSON; `totals.rs`: the time in each phase and the
  counts, from a timeline; `old.rs`: a task's news, for one from before timelines
- `check.rs`: `check`, and `--fix`, which applies the events the watcher missed
  through `events::event` and `events::gone`
- `tours/`: guided review tours. `mod.rs`: a tour's files, the diff it is written on
  (`diff_files`, `covers`), its state (`tour_state`, `tour_stale`) and a step's patch
  (`step_diff`); `write.rs`: the tour agent, `_tour` (run it, check its answer against
  the diff, write the tour, and open the stepper where the board asked); `comments.rs`:
  `tour-comment`
- `stepper/`: the `tour` command. `mod.rs`: starting `_tour` (in the background, or
  waited for with `--wait`), the board's g (`--pane`, `open_stepper`), `--show` and
  `--step` for the board, and sending the person's draft comments back (`--send`,
  `send_comments`); `screen.rs`: the stepper's keys and screen (`Stepper`), apart from
  the terminal; `view.rs`: the stepper on a terminal (`--view`), and what its keys ask
  for (a comment saved, a line told, send, accept, the task's pane or diff)

`board/src/` (`bullpen-board`) reads tasks and settings through `core`:

- `model/task.rs`: a `Task` is the object core read (`obj()`, which core's sentences
  read) and the board's typed view of the fields it reads itself; `load_tasks` (core's
  `task_files`), what a queued task waits on and what its agent is doing
- `model/config.rs`: the repo's settings as core layers them (`load_config`), with the
  board's typed view; a setting of the wrong shape counts as its default
- `model/detail.rs`: the sentences under a task, core's `words.rs` but for the board's
  own: a finished task's (no "a closes it."; the key row says it), when a task its
  coordinator is on comes to you, and where a ready task's tour stands
- `model/groups.rs`, `top.rs`, `confirm.rs`, `text.rs`: the groups and what a board
  hides, the top line's counts (other repos from core's `fleet.rs` snapshot), the confirm
  questions, and fitting text (core's width)
- `model/backlog.rs`: the backlog's open items as the board shows them (`load_later`,
  core's `backlog::read`), the Later line, and each item's mark, line and keys
- `app/` (keys, background jobs, reloading; `later.rs`: the Later group's keys, `L` and
  an item's `y` `n` `s` `J` `K` `⏎`), `ui/` (drawing; `later.rs`: the Later group's
  rows), `edit.rs` (a line editor)
- `mission/` (`bullpen-board --mission`): mission control. `items.rs` is what it lists,
  from core's `fleet.rs` snapshot (the inbox, each repo's line and tasks, the totals);
  `mod.rs` its state and keys: each repo gets a board's `App` with its tasks from the
  snapshot, and a key on a task goes to that App, so the board's keys, questions and
  background commands serve it unchanged; `draw.rs` its layouts (140 and 80 columns),
  with the board's drawing for the questions, answers and typed lines; `run.rs` the
  loop: a watch on the state folder, `bullpen repos --json` for Rex's alive every 5 s,
  and reloading when rebuilt

## Closing a task

Accept, close, drop, finish's auto-accept and auto-accept end a task through one close
step, `close` in `cli/src/close.rs`, in one order: the reviewer's pane, the PR
(withdrawn, unless it was merged), the worktree and branch, the status and the save,
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
