# bullpen

A coordinator agent and its workers, each in its own [Rex](https://www.superlogical.com/rex) pane.

You talk to the coordinator. It splits your ask into tasks and starts one worker
agent per task: each gets its own git worktree, its own branch and its own Rex
pane labelled `bullpen: <ID>`. When a worker finishes, it pushes its branch and
opens a pull request. You accept (merge) or drop it.

The coordinator learns what its workers are doing from Rex itself. Rex records the
status coding agents report (working, blocked, done) and a Lua watcher
(`lua/watch.lua`, run with `rex do --all`) turns that into news for the coordinator,
lines starting `bullpen · `, when a worker needs you, ends its turn, fails,
stops, runs into the usage limit or shows no progress for a long time. News is typed into the coordinator's
pane, or the coordinator follows it with Claude Code's Monitor tool (see [News](#news)).

## Quick start

```sh
./install.sh            # link bullpen into ~/.local/bin and build the board
bullpen keys --install  # the cmd+; keys: adds a line to your Rex config (init.lua), reloads it
bullpen doctor          # checks all of that, and says how to fix what's missing
bullpen up              # in a Rex pane in your repository: the coordinator and its board
```

Then tell the coordinator what you want ("fix the login redirect and cover it
with a test"). It adds tasks and starts a worker for each. Watch the board: `n`
selects the next task that needs you, `i` answers what it asks (or `⏎` opens its
pane), and `a` accepts (merges) a ready one, or closes a finished one. The words used here are in [GLOSSARY.md](GLOSSARY.md).

## Install

```sh
./install.sh          # links bin/bullpen into ~/.local/bin and builds the board
```

Needs Rex (a server from preview.13 on, for minimized floating panes), git, `gh`
(signed in), Claude Code and, to build the task board, Rust's `cargo`. After a
change to the board's source, bullpen rebuilds it by itself (see
[Board rebuilds](#board-rebuilds)); `bullpen build-board` builds it now.

### Doctor

`bullpen doctor` checks the setup and prints one line per check: `ok`, `FAIL` with
the exact fix, or `--` when a check doesn't apply or can't be told. It exits
non-zero when anything fails. It checks:

- `rex` is on PATH and its server is preview.13 or later (`rex server status`)
- `gh` is installed and signed in (`gh auth status`), unless the repo is in
  [local mode](#local-mode)
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
- the watcher is running, from the current `watch.lua` and `bin/bullpen` (the hashes
  in `watcher.json`)
- the state folder is writable, and this repo's `config.json` parses
- the global `config.json` (see [Settings](#settings)), if there is one, parses
  and sets nothing only a repository can have
- the setup and teardown hooks, if there are any: `.bullpen/setup.sh` as committed
  on the base (its `#!` interpreter exists, and an `sh` or `bash` script parses), or
  the `config --setup` command (it parses, and its program is found). A
  `.bullpen/setup.sh` that is there but not committed on the base is a failure: it
  wouldn't run.
- the push remote, if one is set (see [Local mode](#local-mode)): the remote is
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

## Use

In a Rex pane, inside the repository:

```sh
bullpen up            # opens the coordinator's tab, the task board and the watcher
bullpen up --here     # or: make the agent already in this pane a coordinator
```

`up` says what it did in one line: "Coordinator, board and watcher started. Tell
the coordinator what you want." the first time, "Coordinator already running."
after. `bullpen up --verbose` names each pane's block ID and how news reaches the
coordinator.

Everything stays in one tab per repository. The coordinator is on the left and
the board on the right. Each worker is a minimized floating pane, a capsule at
the bottom right labelled `bullpen: BP-3`. `bullpen go BP-3` (or the board, or
the keys below) brings one up and puts any other worker back into its capsule;
going to the coordinator puts them all back. The watcher runs detached, with no
tab. `bullpen config --workers tabs` gives new workers their own tabs instead.

After a Rex server restart, run `bullpen up` again: it finds its panes by their
labels, marks workers whose panes are gone as stopped, and restarts the watcher.

The watcher is also replaced when it is out of date. `up` and `accept` compare
`lua/watch.lua` and `bin/bullpen` with what the running watcher was started from
(hashes kept in `watcher.json`); if either changed, say because a merge or a
pull brought a new version, they close the old watcher and start a new one. A
current watcher is left alone.

The task board (`bullpen board`, a Rust program in `board/`) sits in a narrow
pane beside the coordinator and updates as soon as a task changes.
It groups tasks by what you do next: needs you, coordinator on it (dimmed: see
[Triage](#triage)), ready to accept, finished (nothing to merge, see
[Finish](#finish-with-nothing-to-merge)), in review, working, queued; `f` adds
the closed ones (accepted, closed, dropped). `l` switches to a compact layout,
one line per task under its group's name (dim), with the selected task's detail
at the bottom (what its agent is doing first). A board with tasks but none to
show says why: "Nothing open. f shows 2 closed.", or "Nothing of yours open. o
shows 4 of c3's." Under 12 rows the list takes the blank rows, the scroll mark
sits at the end of its last row, and a message takes the key row; under 36
columns an ID is followed by one space, and (grouped) a task's age starts its
detail line.

Each task starts with its mark, the same one `?` shows under "Marks":

| Mark | The task |
| --- | --- |
| `●` amber | needs you |
| `■` red | stopped: its agent exited |
| `◎` dim | the coordinator is on it |
| `✔` green | ready to accept |
| `◆` green | finished, nothing to merge |
| `◜` blue, turning slowly (`◐` still) | in review |
| `⠋` turning slowly (`○` still) | working |
| `◌` dim | queued, not started |
| `·` dim | accepted or closed |
| `–` dim | dropped |
| `⣾` turning, in place of the mark | a command (accept, drop, start, resume) or its tour being written |
| `▌` at the left edge | needs you (amber), stopped (red), or no progress (amber) |

A turning mark stands still, in amber, when its agent has shown no progress for
a while (see [No progress](#no-progress)), and in its own colour at the usage
limit. The working and review marks turn twice a second, so they stay quiet in
the corner of your eye; a command's spinner turns fast. Each task shows how long
it has been there; a task that needs you or stopped turns its age red after 5
minutes. On the lines under it (from under its ID) it shows
what it needs or where it stands ("Asks: python3 or python?", "PR #4, approved
by light review.", "Follow-up review on round 2 of 2."): one or two lines, cut
to fit, and for the selected task all of it, up to eight lines. The lines of a task
that needs you, stopped, is ready or finished are at normal weight; the rest
(working, queued, in review, the coordinator's, closed, and a change's size) are
dimmed. The selected
task has its ID in reverse video, which shows in any theme, and a tint under its
whole block, title and lines. A working task shows there, dimmed, what
its worker is doing now, and a task in review what its reviewer is doing: the
text of the agent's `working: ...` status, cut to fit and updated as it changes.
With no such text, or text from before the task's current status or over 30
minutes old, it shows the usual line. A task in review or ready also shows the
size of its change ("3 files, +120 −14"), worked out once when its worker runs
`ready`.

The top line counts only what you act on, at its right: `2 need you · 3 ready`
(need you: needs or stopped; ready: ready to accept, or finished), in amber and
green, shortened to `●2 ✔3` when the words don't fit. Before it come the other
repositories with tasks that need you or are ready (see [Several
repositories](#several-repositories)), which get the room first. On a crowded
line the hidden tasks' note drops its "o shows" first, then this repository's
counts shrink to marks, then to what needs you alone; the other repositories go
only when even their marks don't fit.

`bullpen ls`, `bullpen show` and the picker (`cmd+;` `j`) say a task the way the
board does: its mark (from the table above), its ID, title and age, then the
board's line for it, needs-you first as on the board.
Piped, `ls` prints each task whole on one line; on a terminal it is in colour and
each line is cut to the width. When that would leave too little room for the line
(a narrow terminal like the board's), it takes the board's two rows: title and
age, then the line under it. `--json` keeps the raw status.
`show` starts with that line and ends with `next:`, what you can do now
(`next: bullpen reply BP-3 <answer>, or i on the board`).

`bullpen --help` lists the commands in four groups: yours, the coordinator's,
the worker's and reviewer's, and setup. `bullpen COMMAND --help` says more about
one. A command bullpen doesn't have gets one line, with the closest one if there
is one: ``bullpen: no command `acept`. Did you mean `accept`? `bullpen --help`
lists them.``

The board leaves its bottom row empty: Rex draws minimized panes (the workers'
and reviewers' capsules) over the bottom right of the window, which would cover
the end of the key line. `bullpen config --board-bottom-gap 0` takes the row
back (or give more rows); a board too short to spare it drops the gap first.

The key line at the bottom shows the selected task's keys. The board's keys,
in the order `?` lists them:

| Key | Does |
| --- | --- |
| `j` `k` (`↑` `↓`) | select a task |
| `n` | select the next task that needs you; once nothing does, the next one ready to accept, then finished ("Nothing needs you: BP-3 is ready.") |
| `⏎` | open its worker's pane, or its reviewer's |
| `c` | back to the coordinator |
| `a` | accept a ready task (merge it), or close a finished one; asks y/n first |
| `d` | drop it; asks y/n first |
| `i` | answer its question, on a task that needs you (see [Answering from the board](#answering-from-the-board)) |
| `t` | tell its worker something |
| `p` | peek at its worker's screen, in its colours, updating until `esc` or `q`; `t` `⏎` `i` `a` `d` `v` in a peek go back to the board and act on that task |
| `v` | view its diff |
| `g` | its guided tour (see [Guided review tours](#guided-review-tours), with the keys inside a tour) |
| `s` | start it |
| `r` | resume a stopped worker |
| `l` | switch layout: compact, one line a task, or back |
| `f` | show the closed tasks too |
| `o` | show other coordinators' tasks too (see [Several coordinators](#several-coordinators)) |
| `esc` | put a red message away (see below) |
| `?` | every key, one short line each, and the marks |
| `q` | quit the board, once you say `y` (`ctrl+c` quits at once) |

The `y`/`n` question for `a` and `d` goes above the key line, and the task keeps
its title with `y / n` in place of its age. It says which of the two it is, and in red,
first, what could go wrong: checks failing or still running, changes asked for,
not reviewed, a worker or reviewer still running; a drop is always red. Once
an accept or close goes through, the next task that needs you is selected, else
the next one ready or finished, so you can run down a queue of ready tasks with
`a`, `y`. On a narrow or short
board, `j` `k` `space` `b` scroll the `?` list and any other key closes it. The
key line always ends in `?`: when the task's keys fill it, its last keys give
way, not `?`. `bullpen up --no-board` leaves the board out. Accept, drop, start
and resume run in the background, with a spinner on the task, so the board stays
usable meanwhile.

What a key did shows above the key line, wrapped over up to three rows: green
when it worked ("Accepted BP-7."), red when a command failed, with its reason,
and plain otherwise (`l` says "Compact layout.", `o` "Showing c2's tasks too.",
`f` "Showing closed tasks too."). A peek that can't read the screen says why
(its agent isn't running), and its foot drops "live".
A green or plain message goes with the next key; a red one stays while you move
around, until the next thing you do to a task, or `esc`.

Accepting or dropping from the board (or a shell) is news to the coordinator:
`BP-3 accepted (merged PR #12, rebase)`, `BP-3 dropped`. Run from the
coordinator's own pane, `accept` and `drop` post nothing: it already knows.
Tasks that waited on the accepted one start by themselves (see
[Dependencies](#dependencies)).

### Keys

`bullpen keys --install` adds a line to the end of your Rex config (`init.lua`,
or `REX_CONFIG`) and runs `rex config reload`. It only appends, so what's above
the line stays first, and it adds nothing when the line is there (it still
reloads). If the config already loads another bullpen's `keys.lua`, it says so
and changes nothing: edit that line. `bullpen keys` only prints the line. It defines
four actions, also in the command palette under Bullpen, and binds:

- `cmd+;` then `n`: the next task that needs you. Again: the one after it. Once
  nothing needs you, the board, with the task ready to accept (else finished) that
  has waited longest selected.
- `cmd+;` then `b`: the board.
- `cmd+;` then `c`: the coordinator.
- `cmd+;` then `j`: jump to any open task. A picker opens in the middle of the
  window, listing every open task (the repository's name when there is more
  than one, then its mark in the board's colour, ID and title) of every
  repository bullpen knows, this one's first, each with what needs you first,
  then what's ready, then the rest in the board's order. Type part of it: each
  word you type must be in one word of the line, its letters in order but not
  necessarily together (`lgn` finds "login", `rl` finds "Rate-limit", but
  "digest" doesn't find "Migrate sessions table"), and the closest matches come
  first. `↑` `↓` (or `ctrl+p` `ctrl+n`) move, `⏎` goes to that task as
  `bullpen go` does, and `esc` closes the picker. A task without a pane (queued
  or stopped, say) opens its board with that task selected, where its keys
  (`s`, `r`, `i`) are: its coordinator's board, else one that shows other
  coordinators' tasks (`o`). If even that fails, the picker says why after the count
  and stays open. It works from any pane
  (`bullpen pick --pane`; `bullpen pick` runs it in the terminal you're in).

And in a worker's or reviewer's floating pane, `ctrl+escape` puts it back into its
capsule and returns you to the board (`bullpen hide`); elsewhere the key goes
through. Those panes say so in their label ("bullpen: BP-3 · ctrl+esc hides").

`setup{ prefix = "ctrl+b" }` moves them behind another key, and
`keys = { next = "cmd+shift+j", jump = "cmd+k" }` sets single ones. Run `rex config reload` after.

Rex reads `keys.lua` only when its config loads. An accept that changes it says
"Keys changed: run `rex config reload`" once, on the board's message line (in place
of "Accepted") and in the news. `cmd+;` then `b` with no board running (it was
quit, or its coordinator was started with `--no-board`) starts one beside the
coordinator, as `bullpen up` would, and shows it.

Then tell the coordinator what you want. To drive it by hand:

```sh
bullpen add "Fix login redirect" --brief-file brief.md   # prints BP-1
bullpen add "Cover logout" --brief-file b2.md --after BP-1 # BP-2 starts once BP-1 is accepted
bullpen after BP-2 --none   # change (or clear) what a task waits on
bullpen model BP-2 sonnet   # the worker model it starts and resumes with (--clear: the config's)
bullpen review-mode BP-2 full  # its first review: full, light or auto (by the size of the change)
bullpen start BP-1    # worktree + branch + a Rex pane running the worker
bullpen ls            # open tasks as the board says them, in its order (--json: the raw status)
bullpen ls --all-repos  # the same for every repository bullpen knows, by repository
bullpen peek BP-1     # the end of the worker's screen
bullpen prompt BP-1   # the prompt its worker (or reviewer) is at, and its screen, as JSON
bullpen answer BP-1 2 # press option 2 of that prompt (or y, n), as you would in its pane
bullpen reply BP-1 "Postgres"   # answer the coordinator's `ask`: it goes to the coordinator
bullpen diff BP-1     # its changes against its base, in a pager (--stat: just the files)
bullpen check         # tasks whose status is out of step with their agent; --fix catches up
bullpen go BP-1       # focus its pane (`go` alone: the coordinator; --next: next needing you;
                      #   --board: the board, started first if none runs)
bullpen go --next --all-repos   # the next task needing you in any repository
bullpen go BP-1 --repo ~/shop   # a task of another repository
bullpen go BP-1 --board         # its board, with BP-1 selected
bullpen pick          # type part of any open task, in any repository, and go to it (the cmd+; j picker)
bullpen pick --filter "lgn"   # just print the open tasks matching, best first
bullpen tell BP-1 "Also cover the logout path"
bullpen resume BP-1   # restart a stopped worker, continuing its conversation
bullpen ask BP-1 "..."   # (coordinator) BP-1 needs you: the question shows on the board
bullpen ok BP-1       # (coordinator) a task in triage is seen to: back to work, nothing typed
bullpen review BP-1   # (coordinator) a new reviewer for one that stopped without a verdict
bullpen note BP-1 "..."   # an informational line on the board
bullpen accept BP-1   # merge its PR (or local branch; rebase), close its pane, remove its worktree;
                      # a finished task (nothing to merge): remove its worktree, branch and pane
bullpen close BP-1    # the same as accept, for a finished task only
bullpen drop BP-1     # close its PR, pane and worktree without merging
bullpen build-board   # build the board now (see Board rebuilds)
```

A command that can't act on a task says its status in the board's words, with the
status itself after them when they differ: "BP-4 has no pane: it's queued (todo)",
"BP-2 needs you (stopped), not waiting on its coordinator".

### Answering from the board

A task that needs you for a question gets `i answer` on its key line:

- **At a prompt** (its worker or reviewer asks permission, or to choose): `i` shows
  the end of its screen, as `p` does, with the prompt's options under it, each with
  its key. Press one (`1`, `2`, ..., or `y`, `n` for a "(y/n)" question), and bullpen
  presses it in the agent's pane. It reads the screen again first, and sends nothing
  if the prompt there isn't the one you saw (answered in the pane meanwhile, or
  another command asking). A menu's "Type something." isn't offered: `⏎` opens the
  pane for that. `esc` goes back. The task works again once its agent does, as when
  you answer in the pane.
- **The coordinator asked** (`bullpen ask`): `i` shows the task and the whole
  question, wrapped to the pane, with a line under it, "Answer:" (`⏎` sends, `esc`
  goes back). What you type goes to the task's coordinator, not its worker: typed into its pane as
  `BP-3: <your answer>`, or, with `config --news monitor`, as a news line it reads.
  A task asked about while it was working, in review or ready goes back to that.
  One that was stopped leaves needs as `bullpen ok` would (working, or in review),
  or, with its agent's pane gone, goes to its coordinator ("coordinator on it"). The
  coordinator acts on the answer. With no coordinator running, it's
  refused: `t` tells the worker yourself.

`bullpen prompt`, `answer` and `reply` do the same from a shell.

`bullpen diff BP-1` shows `git diff <base>...HEAD` from the task's worktree, then
any changes not committed yet, in `delta` if it's installed, else `less -R`. The
base is `origin/<base>` when there's a remote, the local base branch when there
isn't. On the board, `v` opens the same view in a floating pane; `q` closes it and
takes you back to the board.

A worker finishes with `bullpen ready <ID> --summary-file <file>`, which refuses
uncommitted changes, pushes the branch and opens (or updates) the pull request, or,
when the task leaves nothing to merge, with `bullpen finish <ID> --summary-file <file>`.

### Several coordinators

A repository can have more than one coordinator at a time, each an agent in its
own pane. `bullpen up --here` in another agent's pane adds it as a coordinator and
leaves the others as they are; `bullpen up` from elsewhere starts one only when
none is running. Each coordinator has an ID (`c1`, `c2`, ...) and its own board
beside it. With several coordinators, each board shows only its own
coordinator's tasks, plus those with no coordinator or whose coordinator is gone
(those are everyone's). Its top line names its coordinator and says what it
hides ("+4 in c2 · o shows"). `o` shows the other coordinators' tasks too, in
their usual groups, with dimmed titles that start with their coordinator's ID
("c2 · Fix the login"); their closed ones follow `f`. Each board remembers its
choice (its entry's `board_others` in config.json; `bullpen config
--board-others on --coordinator c1` sets it from a shell). `go --next` from a
coordinator's pane, its board or one of its workers skips the tasks its board
hides, as do the board's `n` and selection. With one coordinator, every board
shows every task.

A task belongs to the coordinator that added it: the one whose pane ran `bullpen
add`, or the first coordinator when `add` ran anywhere else (`bullpen show`
says which, with several). News about a task goes to its own coordinator only:
typed into its pane, or printed by its own `bullpen news`. News that isn't about
one task (several tasks to start after an accept, a dropped dependency, the
usage limit resetting, rebases after an accept) goes to every coordinator. So
does news for a task whose coordinator's pane is gone, and news that was waiting
to be typed into it. `bullpen adopt BP-3`, run in a coordinator's pane, makes the
task that coordinator's own (its panes move to that coordinator's tab too).
Accepting or dropping a task from a coordinator's pane tells its own
coordinator if that is another one.

`bullpen up` forgets coordinators whose panes are gone and closes their boards.
After a Rex restart it finds the coordinators it started by their labels; one
made with `--here` isn't found again: run `bullpen up --here` there again, then
`bullpen adopt` its tasks. `max_running` and the queue are shared by all of
them. Worker panes go in their own coordinator's tab. `bullpen go` from a
board goes to its coordinator, from a worker to the task's coordinator, and
from elsewhere to the first one.

A config from before this, with one coordinator, loads as coordinator `c1`, and
its tasks (which name no coordinator) are news for every coordinator until one
adopts them.

### Several repositories

Each repository has its own coordinator, board and tasks, but bullpen knows them
all: their state is under `~/.local/state/bullpen/repos/`. A repository whose
checkout is gone is left out of what follows.

- `bullpen ls --all-repos` lists the open tasks of every repository, under each
  one's name and path, leaving out those with nothing open. It works from any
  folder. `--all` adds the closed tasks; `--json` prints a list of
  `{"repo": ..., "tasks": [...]}`.
- `bullpen go --next --all-repos` goes to the next task that needs you in any
  repository, the one waiting longest first, as `go --next` does in one. In the
  repository it runs for, it skips what that board hides, as `go --next` does.
- `cmd+;` then `j` (`bullpen pick`) picks any open task of any repository by
  typing part of it (see [Keys](#keys)); `bullpen go BP-1 --repo PATH` goes to a
  task of another repository.
- The board's top line, after the repository's name, names the other
  repositories with something for you: `shop: 1 needs you, 2 ready` (need you:
  needs or stopped; ready: ready to accept, or finished), shortened to
  `shop ●1 ✔2` when the words don't fit, cut (to no less than 8 columns) beside
  this repository's counts, and left out when even that has no room.
  It is amber while one of them needs you. The board looks at the other
  repositories every 10 seconds, and reads a repository's tasks again only when
  its tasks folder changed. `bullpen config --board-repos off` turns it off
  (`--global` for every board).

## How it behaves

What bullpen does by itself as tasks move along, and the settings for it.

### Dependencies

A task can wait on others: `bullpen add "..." --brief-file f --after BP-8`
(repeat `--after`, or give several as `BP-8,BP-9`). `bullpen after BP-9 BP-8`
sets or changes what an existing task waits on, and `bullpen after BP-9 --none`
clears it. Both refuse an unknown task, a dropped one, and a cycle (BP-8 waiting
on BP-9 while BP-9 waits on BP-8).

`start` refuses a task whose dependencies aren't all accepted, naming them;
`--force` starts it anyway. The board, `ls` and `show` say "Waits on BP-8" for it.

When a task is accepted, bullpen starts each queued task whose dependencies are
now all accepted, in ID order, as long as fewer than `max_running` tasks are
working (3 by default; `bullpen config --max-running 5` changes it). A task in
review doesn't take a slot: reviewers are short and cheap. So when a worker
hands its task off (to review, ready, or finished), or an accept or a drop frees
a slot, the next queued task starts too. A task sent back from review works
again and takes a slot again, even if that puts the count over `max_running`:
it is finishing work already started, and the queue starts nothing more until
the count is under the limit. Only tasks with dependencies are queued like this: a plain todo task waits
for the coordinator (or you) to start it. Each start is one news line, `BP-9
started (BP-8 was accepted)`. While the usage limit is reached nothing starts;
the news says which tasks could.

If a dependency is dropped, the tasks waiting on it stay todo and one news line
asks what to do: `bullpen after BP-9 --none` (or other tasks) and `start` it, or
drop it.

A task with dependencies gets one more section in its brief when it starts, "What
this builds on": each dependency's title, its summary (the PR description its
worker wrote) and where its review findings are, so it knows what just landed.

### After accept

Accepting a task moves the base branch, so bullpen brings the other open tasks
on that base up to date, and they don't drift behind and conflict later:

- Working: its worker is told the base moved and to rebase onto it at a good
  point (after `git fetch origin`, with a remote), fix conflicts and rerun the
  tests. Its worktree isn't touched under it.
- In review, or with its reviewer waiting on you: left alone. Its next `ready`
  brings it up to date.
- Waiting on you or the coordinator mid-turn (a permission prompt, a failure): not
  rebased, and the news says so.
- Between turns (needs you, triage, stopped, ready): bullpen rebases its branch itself,
  if its worktree is clean, and pushes it to the PR (`--force-with-lease`). The
  status stays; a ready task says "Rebased onto main." on the board until its
  next `ready`. An approved task keeps its approval when the rebase left its patch
  as it was (the same patch-id); if the patch changed, a reviewer checks it again
  (a follow-up round). If the rebase conflicts, it is undone, the branch stays as it was,
  and the task goes back to its worker with the conflicting files (resumed if its
  pane is gone). Uncommitted changes: skipped.

One news line sums it up: `After BP-4: BP-5 told to rebase; BP-8 rebased; BP-9
conflicts in bin/bullpen, sent back.` Set `"rebase_others": false` in the repo's
`config.json` to turn this off.

### Review

When a worker runs `ready`, bullpen pushes its branch, opens the PR and starts a
reviewer agent on it, in its own capsule (`bullpen: BP-3 review`). The reviewer
reads the brief and the diff, runs the tests, changes nothing, and ends with
`bullpen verdict BP-3 approve` or `changes`. Changes go back to the worker,
which fixes them and runs `ready` again, for up to 2 rounds. The task is ready
to accept once review approves, or after the last round with the open findings
listed. `bullpen config --review off` skips review; `bullpen config --review-rounds 3`
sets the rounds, and `reviewer` in the repo's `config.json` the reviewer's command.

Review costs only what it has to:

- **The first round** is a full review on the reviewer model (`--reviewer-model`):
  the brief, the whole diff, the tests. A small change gets a **light review**
  instead: one round on the follow-up model, checking that it's correct and matches
  the brief. Small means docs only (every changed file under `docs/` or `prompts/`,
  or ending in `.md`), or at most `light_review_lines` changed lines (40 by default,
  `config --light-review-lines`) in at most 2 files. `bullpen add --review
  full|light|auto` or `bullpen review-mode BP-3 full|light|auto` overrides that
  for one task; auto, the default, goes by the size.
- **Later rounds follow up**: the reviewer gets the last round's findings and the
  diff since the commit it reviewed (a range-diff after a rebase), with the whole
  diff for context, and checks that each finding is fixed and that the fixes broke
  nothing nearby. They run on `reviewer_followup_model` (`config
  --reviewer-followup-model`, sonnet by default; `null` in `config.json` uses the
  reviewer model).
- **An approval is kept while the code doesn't change.** Each review records the
  commit it looked at and a patch-id of the task's diff against its base (`git diff
  base...HEAD | git patch-id --stable`). When the worker runs `ready` again on the
  same commit, or on the same patch after a clean rebase, the task is ready at once
  with its approval, and news says why: `BP-3 is ready again after a rebase: Fix
  login redirect (2 commits, PR #12, its approval carried over: the same patch
  after a clean rebase onto origin/main)`.
  A changed patch, or a rebase conflict bullpen sent back (resolved by hand), is
  reviewed again. `ready` run again while a reviewer is already on that very commit
  leaves it be.

The board, `show` and the ready news say which kind of review it was ("approved by
light review (12 lines in 1 file)").

A reviewer whose turn ends while a background shell of its own still runs (the tests,
say) is busy, not stopped, like a worker: it stays in review. Past `stuck_after`
minutes with the shell still running, `check` counts it as stopped without a verdict.

### Guided review tours

A larger change reads better in order. `bullpen tour BP-3` asks a cheap agent for a
**tour** of a ready task (or one in review): the change in reading order, one
**step** per logical change, each with a title, a note on why it changed and what
to look at, and the hunks it covers. You step through it on the board (`g`),
comment on steps, then accept or send the comments back. The design is in
[docs/prd/guided-review.md](docs/prd/guided-review.md).

**On the board**, `g` on a ready task (or one in review) opens its tour in a large
floating pane in the middle of the window, the way the jump picker opens. With no
tour yet (or a failed one), it starts the agent: the task gets a spinner, and the
tour opens by itself once it's written. With a stale one, the board asks: `n`
writes a new one, `o` opens the old one anyway. On any other task it says why not.
In the tour:

- Each step shows its title, the files and lines it covers, its note and your
  comments on it, then its hunks from git (through delta when it's installed).
  `n` and `p` (or `→` `←`) go to the next and previous step, `j` `k` `space` `b`
  `g` `G` scroll, `esc` (or `q`) closes it back to the board.
- `c` comments on the step: type it and press `⏎` (`esc` drops it). To point at
  a line, mention it in the text. Comments are drafts, kept with the task, and
  opening the tour again resumes at the step you were on.
- After the last step, the end lists your comments: `s` sends them back to the
  worker (`tour --send`), `a` accepts the task (warning first, if comments are
  unsent: `a` again accepts anyway), `esc` closes.
- As on the board, `t` tells the worker something now (type it, `⏎` sends), `a`
  accepts from any step (a second `a`, since you haven't reached the end), and `⏎`
  and `v` close the tour and open the task's pane or its diff.

The line with the size of a ready task's change says "Toured" once you've opened
its tour (until a `ready` changes the code), else "Not toured yet" or "Tour written,
not opened yet": a nudge only, accepting never needs a tour. A failed tour shows as
"tour failed: <why>" on the task.

```sh
bullpen tour BP-3             # with a tour, step through it (off a terminal: its path); else start writing one
bullpen tour BP-3 --wait      # write it now, then step through it (or print its path)
bullpen tour BP-3 --path      # its path, even on a terminal
bullpen tour BP-3 --new       # a new tour, even though there is one
bullpen tour BP-3 --show      # its state (writing, failed, current, stale or none) and steps; --json
bullpen tour BP-3 --step 2    # step 2's hunks, from git, as a patch (--json: with its title and note)
bullpen tour-comment BP-3 2 "Line 40 should log the error."   # a draft comment on step 2
bullpen tour-comment BP-3     # the drafts (--json); --remove N removes one
bullpen tour BP-3 --send      # send the drafts back to the worker
bullpen tour BP-3 --view      # step through it on this terminal (the board's g: --pane)
```

- **Only on request**, and only for a task that is ready or in review: a working
  task's code is still changing. `tour` starts the agent in the background (with a
  note on the task that it's being written, for the board) and returns.
- **The agent** runs headless (`claude -p`) in the task's worktree, on `tour_model`
  (`config --tour-model`, sonnet by default; `--global` for every repository). Its
  command is `tour` in `config.json` (`claude`). It gets the diff against the base,
  the brief, the worker's summary and the review findings, may only read files and
  run `git diff`, `log` and `show`, and answers with the tour as JSON: bullpen writes
  the file. It doesn't count toward `max_running`.
- **Its answer.** With `claude`, the answer's shape is given as a JSON schema
  (`--json-schema`), and bullpen takes Claude Code's structured output. From another
  agent (or text), it reads the text as JSON, else the last JSON object in it that is
  a tour (an answer in a code fence, or one corrected after a first try), and notes
  the extra text in the log. An answer that still isn't a tour is asked for once
  more, both attempts in the log, before the tour fails.
- **The tour file** is `briefs/<ID>.tour.json`: the commit and patch-id it was
  written on, and its steps, each `{"title", "note", "hunks": [{"file", "lines":
  [start, end]}]}` (lines in the new file; a binary file has none). Hunks that
  match nothing in the diff are left out, and any hunk no step covers goes into a
  last step, "Other changes". Code is always shown from git (`--step`), never from
  the agent's text.
- **Current or stale.** A tour stays current while the patch is the same: the same
  commit, or the same patch-id after a clean rebase, as review approvals carry over.
  Otherwise it is stale: `tour` says so and shows the old one, and `--new`
  writes another.
- **A failed tour** (the agent failed, or its answer isn't a tour twice) is a line on the
  task on the board, "tour failed: <why>", with the agent's output in
  `briefs/<ID>.tour.log`. No news, no triage: `tour` again retries.
- **Comments** are drafts per task and step (`briefs/<ID>.tour-comments.json`), each
  keeping its step's title, files and lines. `tour --send` writes them to
  `briefs/<ID>.tour-<n>.comments.md`, one section per comment with its step, files
  and lines, tells the worker to read it and address each one, and puts the task
  back to working (closing a reviewer that was on it). The coordinator gets a news
  line for information: `BP-3 sent back by the person after the tour: 2 comments`.
- **Their review.** The worker's next `ready` gets a review of the person's comments
  (with review on): the reviewer gets the comments and the diff since they were sent,
  and checks that each is addressed. It keeps the round it follows, so it doesn't count
  toward `review_rounds`, and its findings always go back to the worker; a later round
  follows up on them as usual. An approval doesn't carry it over: the comments are
  reviewed even if the code didn't change.

### Finish with nothing to merge

Some tasks leave nothing in the repository: a video, research notes, an answer.
Their worker ends with `bullpen finish BP-3 --summary-file <file>` instead of
`ready`. It refuses while the branch changes anything against its base (commits
that cancel each other out count as no change), and says to run `ready` instead.
Otherwise the task is `finished`, with the summary kept as `ready` keeps it: no PR,
no review. The news says `BP-3 finished (nothing to merge): <first line of the
summary>`, and the board lists it under "Finished" with that line.

You close it with `a` on the board or `bullpen accept BP-3` (`bullpen close BP-3`
does the same): its worktree, branch
and worker pane go, with no warning about losing work (there is nothing to merge),
and it counts as done, so tasks added `--after` it start. With auto-accept on (or
the task's `--auto-accept`), a finished task closes by itself, with a news line.
A finished task whose worker starts working again goes back to working and has to
run `finish` (or `ready`) again. Every worker's instructions say when to use which.

A task goes back to `working` only once its worker has the message: review
findings, a rebase conflict or PR feedback. If typing it into the worker's pane
fails, it goes to the coordinator instead (triage, as "Review findings didn't
reach its worker", or the PR's reason), or for a rebase conflict stays where it
was, and news says so, with where the findings are, so it can `bullpen tell` it. `verdict` and
`ready` refuse an empty `--summary-file`: an empty file is a write that failed
(a full disk, `/tmp` out of inodes), not "no findings".

### Auto-accept

Off by default: you accept each task yourself. Turn it on with `bullpen config
--auto-accept on` when review is the check you trust, so approved work lands
without waiting for you, and the next queued tasks start sooner. Once the reviewer
approves, bullpen runs the normal accept itself (same merge, cleanup, rebase of the
other tasks and start of queued ones), and the news says so: `BP-3 approved by
review and auto-accepted (merged into main, rebase)`. A finished task, with nothing
to merge, closes as soon as its worker runs `finish`.

It never accepts a task review didn't approve: one ready after the last round with
findings open, or any task while review is off. With a PR, it waits for the PR's
checks: still running, the task stays ready and accepts once they pass; failing, or
new comments from people, it isn't accepted and the news says why. A merge that is
refused (a conflict, an uncommitted change in the way) leaves the task ready, with
the reason as news. `bullpen add` or `start` with `--no-auto-accept` keeps one task
for you while it's on (and `--auto-accept` auto-accepts one task while it's off).
The board shows `· auto-accept` beside the repo's name while it's on, and a task
held back says "Kept for you: no auto-accept."

### Triage

A task that stops is either the coordinator's to see to or yours:

- **triage**, "needs the coordinator": the worker's turn failed; it ended its
  turn without `ready`; review findings or PR feedback didn't reach it; it
  couldn't be continued after the usage limit; its reviewer stopped or exited
  without a verdict; its worktree's setup failed (see [Worktree
  setup](#worktree-setup)). The news starts `For the coordinator:`. The board shows the
  task dimmed under "Coordinator on it", with no amber bar, and when it comes to you
  if the coordinator doesn't settle it first ("Ended its turn without ready. Yours
  in 7 min."); `go --next` (and `n`) skip it.
- **needs**, "needs you": the worker or the reviewer is at a permission or input
  prompt, or the coordinator ran `bullpen ask BP-3 "question"`, which shows the
  question on the board. The news says it needs the person.

The coordinator settles triage with `tell` or `resume` (which put the task back to
work), `bullpen review BP-3` (a new reviewer on the task's current commit in place of
one that stopped without a verdict; it counts as a review round and is refused once
they're used up), `bullpen ok BP-3` (back to working, or in review, typing nothing to
the agent), or `ask`. A task also leaves triage when its agent starts working again.
One still in triage after `triage_minutes` (10 by default; `bullpen config
--triage-minutes 15`) goes to you, its reason ending "(coordinator didn't resolve
it)", with one news line. The watcher's `check` run, every 3 minutes, sees to that.
With the task's coordinator (or, for a task without one, every coordinator) not
running, it goes to you at once.

A ready task whose worker starts working again (you typed to it, say) goes back to
working, with a news line, and has to run `ready` again. Its approval is dropped
only if the code changes: if that turn ends with the code as it was approved
(committed, the same patch), the task is ready again with its approval, and news
says so; `ready` keeps it too.

Tasks that needed you before triage existed are sorted out once, the first time
bullpen runs after the update: those stopped for one of the triage reasons above
become triage; prompts stay needs.

### After ready

While a task waits in review or for you, the watcher looks at its pull request
every 2 minutes (one `gh pr view` per PR; none when no task has one open):

- A check that fails: news (`BP-3's PR checks failed: test`), and the worker is
  told which checks failed, to fix them and run `ready` again. The task goes
  back to working, and a review still running is stopped (its round doesn't count).
- New comments, or changes requested, by people on GitHub (not bots, not
  approvals): the same, with the comment text.
- A PR merged or closed on GitHub: one news line saying so. Nothing else changes:
  `bullpen accept` (merged) or `drop` (closed) cleans up.

Each failure or comment is reported once. If the worker's pane is gone, the task
goes to the coordinator (triage) instead; if the worker is stopped mid-turn (a
permission prompt, a failure), the news says so and nothing is typed into it. The board and `show` say what GitHub last said ("checks
failing", "1 new comment"); while a task is working they say it only if that is what
sent it back ("Fixing what GitHub said."), since it's about the old push. In local mode there's no PR, so nothing is looked at.

### No progress

A worker can say it's working and be stuck: a command that hangs, a loop, a wait
on something Rex doesn't report as blocked. The watcher notes the time of every
status event from each pane. Once a minute it looks for panes that say they're
working and have sent none for 20 minutes, and reports each once: news
(``BP-3 has shown no progress for 20 min: `bullpen peek BP-3` ``), and on the
board the task gets the amber bar, a still amber mark and "No progress for 20
min." It stays in its group and nothing is typed into its pane: a long build
looks the same, so you (or the coordinator) decide. The pane's next status event clears it, and a new
quiet spell is reported again. The same goes for a reviewer
("BP-3's reviewer has shown no progress ...").

`bullpen config --stuck-after 45` changes the minutes for this repository; `0`
turns it off. The watcher only knows of events since it started, so a pane that
was already silent when the watcher (re)started isn't noticed until its next
event.

### Status drift

A task's status follows its agent's status events. One that goes missing (a
watcher or Rex restart, a bug) leaves the task out of step: marked working while
its agent sits at its prompt, say. `bullpen check` compares each open task that
has an agent in a pane (its worker, or its reviewer while in review) with what
that agent is doing now, and prints one line per mismatch:

```
BP-3 is marked working but its agent is idle at its prompt: `bullpen peek` it
BP-4 is marked needs (permission) but its agent is working again
BP-7 waits on its coordinator, but its coordinator didn't resolve it in 10 min: it needs the person
BP-5 is in review but its reviewer is idle without a verdict
BP-6 is marked working but its pane is gone
```

It prints nothing, and exits 0, when all agree (1 when something doesn't). It
reads the status record Rex keeps for each pane (the one the watcher's events come
from), and goes by the screen where Rex has none: Claude Code's spinner, its
prompt. An idle, blocked or error state that began in the last minute is left alone, since its event may
still be on the way, and so is a task bullpen itself moved in the last minute
(`start`, `tell`, `ready`): its agent has yet to report. `bullpen check BP-3` checks one task. An agent
idle after the coordinator's `bullpen ok` isn't drift, nor is a task waiting on an
`ask` whatever its agent does. `check` also passes a task left in triage too long to
the person (see [Triage](#triage)).

`bullpen check --fix` applies what the missed event would have done, news
included, as if it had come; it never types into an agent or approves anything.
The watcher runs it every 3 minutes, quietly, so drift fixes itself and you hear
of it once, in the usual words.

An agent that ended its turn while a background shell of its own still runs
(Claude Code's footer says `1 shell`) isn't finished: it takes another turn when
the shell ends. Bullpen counts it as busy, and holds back the "ended its turn
without `ready`" news for that turn. A shell it left running for good (a server, a
watch) would hide it, so once it has been idle that way for `--stuck-after` minutes
(20 by default), `check` reports it, and the news says a shell still runs. `check` also hints at an idle worker with
commits and a clean worktree that never ran `ready`; that one it leaves to you.

An agent that compacts its conversation by itself (Claude Code's auto-compaction:
`✽ Compacting conversation…` on its screen) says it's working, but that's no new
turn. Bullpen goes by the screen and leaves the task as it was: a ready or finished
task stays so, with its approval and summary, and a task in triage or waiting on you
stays there. When the compaction ends, its end of turn gives no news either, and
`check` sees no drift while it lasts. A real turn after it (typed input, `tell`, or
the agent going on by itself) counts as usual.

### Usage limit

All of bullpen's agents share one Claude subscription. When it runs out, a
worker or reviewer stops with a line like `You've hit your 5-hour limit ·
resets 3pm`. Bullpen reads that line from the status the agent reports (or,
for a turn that failed or went idle, from the end of its screen) and:

- keeps the task where it was, working or in review, marked "Usage limit, resets
  3:00 PM" on the board and in `ls`. It doesn't need you;
- posts one news line (`BP-3 stopped at the usage limit, resets 3:00 PM: nothing
  to do, it is continued then`);
- makes `bullpen start` refuse, saying why, until the reset time or until a
  limited agent is working again. `bullpen start BP-4 --force` starts anyway.

Two minutes after the reset time the watcher continues each limited task: it
tells the agent to carry on, or resumes the worker (starts the reviewer again)
if its pane is gone, and posts one news line (`the usage limit has reset:
continued BP-3, BP-5's reviewer`). The two minutes leave room for Claude Code's
own "Continue automatically at usage limit" setting; an agent that is working
again by then is left alone.

If the reset time can't be read from the message, the news says so and the task
isn't continued by itself: `bullpen tell BP-3 continue` once the limit has
reset. `start` then refuses for 5 hours, or until that agent works again. The
limit is kept in `limit.json` in the state folder, for every repository.

### News

Every news line is logged in `news.log` beside the tasks. `bullpen config --news`
sets how it reaches the coordinator, and `bullpen up --verbose` says which is in use
(`up` says it by itself for `monitor`):

- `type` (the default): bullpen types each line into the coordinator's pane. It
  works with any coordinator agent, and goes on working when the coordinator's
  session is restarted, since nothing has to be running inside it. But news that
  arrives while you're typing there waits (in `outbox.txt`) until the input is
  empty, so it never gets sent along with half a message: it comes late, after
  you send your own. Claude Code's dimmed prompt suggestion doesn't count as
  typing (bullpen reads the input's styling with `rex capture --format=vt`), so
  news doesn't wait behind one.
- `monitor`: nothing is typed. The coordinator runs Claude Code's Monitor tool on
  `bullpen news --follow`, and each line shows up in its chat as a notification
  without touching the input, so news never waits on your typing. It needs a
  coordinator with that tool (Claude Code), and the coordinator has to start the
  monitor and start it again every 30 minutes, when it expires; its guide tells
  it to. News logged while no monitor runs is not lost: the next one prints it.

News is for the coordinator. You hear of a task that needs you by a notification
(see [Notifications](#notifications)).

Each line starts with the task's ID and what happened, so the start is what
counts when a narrow pane cuts it: `BP-3 is ready: Fix login redirect (2 commits,
PR #12, approved by full review)`, `BP-4 is at a permission prompt: Allow npm
install?`, ``BP-5 is working again (was ready): it must run `ready` again``. A task
ready a second time is "ready again" ("ready again after a rebase" when its base
moved), and a line word for word the last one about its task isn't sent.

`bullpen news` prints the lines logged since the last time and exits;
`bullpen news --follow` keeps running and prints each new line as it is logged.
Both remember how far they got (a byte offset in `news-c1.cursor`, one per
coordinator), so each line is printed once, across restarts too. They print the
news of the coordinator whose pane runs them (`REX_BLOCK`), or of the first one
outside Rex. From any other Rex pane they refuse, and a `--follow` running in a pane
that stops being a coordinator stops: each line is printed once, so a reader in an
agent that used to be the coordinator would take lines meant for the real one. Only
one reads per coordinator at a time: a second `--follow` waits
for the first to end and then takes over. The first run starts
from the end of the log, and switching to `monitor` starts from there too, so
old news is never printed again.

You can switch while a coordinator runs. Switching to `monitor` types one last
line into its pane telling it to start the monitor (not when you run `config`
from the coordinator's own pane: it sees the answer). Switching to `type` makes
a running `news --follow` print `bullpen · news is typed into your pane now` and
exit, so the coordinator stops restarting it and no line arrives twice; in `type`
mode `--follow` always does that, and plain `bullpen news` still works. News
logged in `monitor` mode but not yet read when you switch back is not typed:
`bullpen news` prints it.

When the coordinator's guide changes. A coordinator gets its guide
(`prompts/coordinator.md`) once: in its system prompt, or by reading `bullpen guide`
after `up --here`, and a running agent can't load it again. So each coordinator
records the hash of the guide it was given, and the text is kept in
`~/.local/state/bullpen/guides/<hash>.md`. When `bullpen up` or `bullpen accept` (a
pull or a merge may have changed it) finds that a running coordinator's guide is
out of date, in any repository, it sends that coordinator alone one news line, and
records the new hash, so each change is told once. When the diff is short (40
lines or less) it comes after the line, as a unified diff (typed with it as one
message, or printed after it by `bullpen news`, logged as lines starting with a
tab):

```
bullpen · Your coordinator guide changed (section: News). Read the change below and follow it from now on:
--- guide (yours)
+++ guide (now)
@@ -314,4 +314,4 @@
...
```

A longer change names the sections it touches instead:

```
bullpen · Your coordinator guide changed (sections: Review, News), 63 lines of diff: run `bullpen guide` to read it all, and follow it from now on.
```

A coordinator from before this (no hash),
or one whose old text is gone, is told once that its guide may have changed, to run
`bullpen guide`, with no diff.

### Notifications

When a task comes to need you (its worker or reviewer is at a prompt, the
coordinator ran `bullpen ask`, or triage went to you), you get a native
notification, titled `bullpen · <repo>`: "BP-3 needs you: <the question>".
`bullpen config --notify all` also sends one when a task is ready ("BP-3 is ready:
<title>"), finished, or auto-accepted; `--notify off` sends none; `needs` is the
default. Each goes out once, when the task's status changes, and not while the
task's own pane or its coordinator's board has focus in Rex. When several tasks
come to need you at once (within 10 seconds of a notification for another), the
first is sent as usual and the rest wait for a 3-second lull, then go out as one:
"3 tasks need you: BP-3, BP-4, BP-5" (a single one goes out as it was). The
watcher sends it, on the round that types waiting news.

Bullpen writes the notification (OSC 777) to the terminal of the task's pane, or of
its board, or its coordinator, when the task's has none, and Rex for Mac shows it:
clicking it opens that pane's tab. Turn on Settings > Notifications > System
Notifications > Terminal notifications in Rex for Mac. Each one, and why one wasn't
sent, is logged in `notify.log` beside the tasks.

### Rules

Rules every worker and reviewer should follow go in `.bullpen/rules.md`, committed
in the repository, so everyone using bullpen on it gets them. Rules of your own go
in `rules.md` in bullpen's folder for the repository (`bullpen rules --edit` opens
it in `$EDITOR`); they aren't committed. `start` puts both, the repository's first,
in the worker's brief under "Rules for this repository", and the reviewer gets the
same rules and checks the work against them. No rules, no heading.

The repository's file is read as committed on the task's base branch (`origin/<base>`,
or the local base branch without a remote): an uncommitted edit in your checkout, or
a task's own edit when it is started again, isn't used. `bullpen rules`
prints the rules in effect and where each came from.

### Worktree setup

A new worktree has no dependencies installed, no `.env` and no build cache. A repo
can say what to run in it: `.bullpen/setup.sh`, committed (read as committed on the
task's base, like the rules), or a command of your own, `bullpen config --setup
"npm ci"` (`--setup ""` clears it). With both, yours runs, and `start` says so.
`start` runs it in the new worktree, after making it and before starting the worker,
with these in its environment:

- `BULLPEN_TASK`: the task ID
- `BULLPEN_WORKTREE`: the worktree (also the current folder)
- `BULLPEN_MAIN`: your main checkout, to copy `.env` from, say
- `BULLPEN_BRANCH`: the task's branch

Its output goes to `<ID>.setup.log` beside the task's brief; `bullpen show` names
it. If it fails, the worker doesn't start: the task goes to triage (to you if no
coordinator is running), its reason and the news line naming the log. Fix what it
needs and `bullpen start` (or `resume`) the task: that sets it up again and starts
the worker. A worktree that was set up is never set up again, not by `resume` either.

`.bullpen/teardown.sh` (or `bullpen config --teardown "..."`) runs the same way just
before `accept`, `close` or `drop` removes a worktree, its output in
`<ID>.teardown.log`. A failure is said and logged, but the worktree goes anyway.

### Local mode

In a repository with no `origin` remote, bullpen works locally. `ready` pushes
nothing and opens no pull request: it checks the branch against the local base
branch and starts the reviewer as usual, and news names no PR (nor the local
branch: in this repo they all are). `accept` merges the task's branch into the local base branch
itself (rebase, squash or merge, as `config --merge` says). Where the base branch
is checked out, it is fast-forwarded with `git merge --ff-only`, so uncommitted
changes to other files survive. If anything stands in the way (a conflict, or an
uncommitted change to a file the task touches), `accept` says why and leaves the
task ready, with its worktree and branch as they were.

To keep a copy elsewhere without PRs, add a remote under another name than `origin`
(so bullpen stays in local mode) and name it: `bullpen config --push-remote github`.
After each `accept` (and auto-accept) that merges something, bullpen pushes the base
branch, and only it, to that remote: `git push github main`, in the background, so
the accept doesn't wait on the network. Pushes take turns and push the branch as it
is then, so two accepts in a row both land. It never force-pushes, and git is never
let to ask for a password. A push that fails (offline, rejected because the remote
has commits the base doesn't, no access) doesn't undo the accept: one news line gives
the reason, and `bullpen doctor` says how far behind the remote is. `close` pushes
nothing (there's no merge), nor does a merge you do by hand. But `bullpen up` pushes
the base branch when it has commits the remote didn't get (as far as this checkout
last saw the remote), so things catch up after an offline spell. `bullpen config
--push-remote ""` or `--unset push_remote` turns it off. With an `origin` remote
(PR mode), the setting does nothing: merges happen on GitHub.

### Board rebuilds

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

## Where things live

- Tasks, briefs, settings and your own rules: `~/.local/state/bullpen/repos/<repo>/` (`BULLPEN_STATE` moves it).
- Settings for every repository: `~/.config/bullpen/config.json` (`XDG_CONFIG_HOME` moves it). See [Settings](#settings).
  Setup and teardown logs are in `briefs/` there, as `<ID>.setup.log` and `<ID>.teardown.log`,
  and tours with their drafts and logs as `<ID>.tour.json`, `<ID>.tour-comments.json` and
  `<ID>.tour.log` (see [Guided review tours](#guided-review-tours)).
- Worktrees: `<repo>-bullpen/<ID>` next to the repository.
- News: `news.log` beside the tasks (each line names the coordinators it is for,
  as `[c1]`), with `news-<id>.cursor` (how far each coordinator's `bullpen news`
  has read) and `outbox-<id>.txt` (typed news waiting for an empty input). See
  [News](#news).
- The coordinator guides running coordinators were given, for the diff when it changes:
  `~/.local/state/bullpen/guides/<hash>.md`. See [News](#news).
- Notifications: `notify.log` beside the tasks. See [Notifications](#notifications).
- What each worker and reviewer is doing, for the board: `~/.local/state/bullpen/doing/<pane>.json`,
  written by the watcher (at most every 3 seconds a pane, no news) and removed with the pane.

## Settings

`bullpen config` lists the settings in groups (tasks, agents, review, board, news),
each with its value and where that comes from:
`repo` (this repository's `config.json`), `global` (the one for every repository,
`~/.config/bullpen/config.json`, or `$XDG_CONFIG_HOME/bullpen/config.json`) or
`default` (bullpen's own). A repository's own value wins over the global one, and
the global one over the default. `bullpen config --json` prints the same as JSON,
with each setting's source under `"sources"`.

Every flag below changes this repository's value. With `--global` it changes the
global one instead, so every repository that doesn't set its own gets it:

```sh
bullpen config --global --model opus --merge squash --max-running 4
bullpen config --unset merge             # this repo goes back to the global merge
bullpen config --global --unset merge    # and the global one back to the default
```

`--unset` takes the setting's name as `config.json` has it (`worker_model`,
`review_rounds`). `--global` works outside a repository too. The prefix, the base
branch, the news mode, the push remote and the coordinators belong to one repository,
so `--global` refuses `--prefix`, `--base`, `--news`, `--push-remote` and `--board-others`. The commands
(`worker`, `reviewer`, `coordinator_cmd`, `tour`) and `rebase_others` have no flags: set them
in either `config.json` by hand.

A repository's `config.json` holds only what it sets itself. One from before the
global file held every setting: the first time bullpen reads it, it drops the
values that are just the default, so the global ones apply there.

`bullpen config --auto-accept on` accepts approved tasks by themselves (off by
default; see [Auto-accept](#auto-accept)). `bullpen config --news monitor` changes how news reaches the coordinator (`type`
by default). `bullpen config --merge squash` changes how `accept` merges (`rebase` by default;
`squash` or `merge`). `bullpen config --stuck-after 45` sets how many minutes a
working agent can show no progress before you hear of it (20 by default, `0` for
never). `bullpen config --triage-minutes 15` sets how long a task can wait on its
coordinator before it goes to you (10 by default; see [Triage](#triage)). `bullpen config
--notify all` sends you a notification when a task is ready, finished or auto-accepted
too, and `off` none (`needs`, when a task needs you, by default; see
[Notifications](#notifications)). `bullpen config
--board-bottom-gap 0` lets the board use its bottom row (1 empty row by default, for
Rex's capsules). `bullpen config --board-others on --coordinator c1` makes c1's
board show the other coordinators' tasks too (see [Several
coordinators](#several-coordinators); the board's `o`). `bullpen config --board-repos off`
takes the other repositories off the board's top line (on by default; see [Several
repositories](#several-repositories)). `bullpen config --prefix XY --base develop` changes the task prefix (default `BP`)
and the base branch (default: origin's default branch). `bullpen config --push-remote
github` pushes the base branch to the `github` remote after each accept, in local mode
only (see [Local mode](#local-mode)). The worker and coordinator
commands are in that folder's `config.json` (`worker`, `coordinator_cmd`). `"rebase_others": false`
there stops `accept` rebasing the other open tasks (see [After accept](#after-accept)).

`bullpen config --model opus` sets the model for workers and reviewers (follow-up
and light reviews too); `--worker-model`, `--reviewer-model` and
`--reviewer-followup-model` set one each (`--tour-model` the tour agent's, see
[Guided review tours](#guided-review-tours)). It takes anything `claude --model`
does (an alias like `opus`, or a full model ID), and `default` lets Claude Code pick
again. Bullpen adds `--model` when it starts an agent, in place of any `--model` in its
command; it leaves other agents' commands alone. (To use a `--model` written in the
command instead, set `worker_model` or `reviewer_model` back to `null` in `config.json`,
or `bullpen config --unset worker_model` when it was only set for this repository.) `bullpen config` shows the models in
use, and `bullpen show BP-3` the one a worker was started with.

A task can have a worker model of its own, over the config's: `bullpen add "..."
--model sonnet` gives it one from the start, so a task that starts by itself (after
its dependencies are accepted, or when a slot frees up) uses it. `bullpen model BP-3
sonnet` sets or changes it later, and `bullpen model BP-3 --clear` goes back to the
config's. A running worker isn't restarted: the change takes effect at its next start
or resume. `bullpen start BP-3 --model sonnet` picks one as it starts, and keeps it
for later resumes. `ls`, `show` and the board name a task's model when it isn't the
config's.

## Tests

```sh
python3 tests/run.py                  # the whole suite, across CPUs: about 75 s on 16
python3 tests/run.py tour local_mode  # only tests whose module, class or name has one of these
python3 tests/run.py --slowest 20     # and list the 20 slowest
python3 -m unittest discover -s tests # the same tests one at a time: about 9 minutes
cd board && cargo test                # the board's: well under a second once built
```

There's a module per area (`tests/test_tour.py`, `tests/test_news.py`, ...); what they
share is in `tests/support.py`. Run the tests for what you're changing while you work,
and the whole suite once before `ready`. Reviewers run only the tests that check a
claim, never the whole suite.

The tests use fake `rex` and `gh` programs and a local bare repository as origin, and
run `bin/bullpen` from bytecode compiled once: Python would otherwise compile it on
every call, most of a call's time. Each test removes its temp folder when it ends.
Each module runs inside a temp folder of its own and fails if anything is left in it
afterwards.
