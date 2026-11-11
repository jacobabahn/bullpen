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

## Install

```sh
./install.sh          # links bin/bullpen into ~/.local/bin and builds the board
```

Needs Rex (a server from preview.13 on, for minimized floating panes), git, `gh`
(signed in), Claude Code and, to build the task board, Rust's `cargo`. Run
`./install.sh` again after pulling changes to the board.

## Use

In a Rex pane, inside the repository:

```sh
bullpen up            # opens the coordinator's tab, the task board and the watcher
bullpen up --here     # or: make the agent already in this pane the coordinator
```

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
It groups tasks by what you do next: needs you, ready to accept, in review,
working, queued. Each task shows how long it has been there and, on a second
line, what it needs or where it stands ("Asks: python3 or python?", "PR #4,
approved by review."). A task in review or ready also shows the size of its
change ("3 files, +120 −14"), worked out once when its worker runs `ready`. A
task that needs you gets an amber bar; its age turns red after 5 minutes. A
working task's mark spins, and one in review turns; one whose agent has shown no
progress for a while (below) stops turning and gets the amber bar too. The top
line counts the tasks in each state. `l` switches to a compact layout, one line
per task with the selected task's detail at the bottom.

The keys at the bottom follow the selected task: `⏎` opens its worker (or its
reviewer), `a` accepts and `d` drops (each asks y/n first), `t` tells, `p`
peeks, `v` shows its diff, `s` starts, `r` resumes, `c` goes back to the
coordinator, `n` selects the next task that needs you. `?` lists them all.
`bullpen up --no-board` leaves the board out. Accept, drop, start and resume
run in the background, with a spinner on the task, so the board stays usable
meanwhile; a peek keeps the worker's colours and updates until you press a key.

Accepting or dropping from the board (or a shell) is news to the coordinator:
`BP-3 accepted (merged PR #12, rebase)`, `BP-3 dropped`. Run from the
coordinator's own pane, `accept` and `drop` post nothing: it already knows.
Tasks that waited on the accepted one start by themselves (see
[Dependencies](#dependencies)).

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
working or in review (3 by default; set `"max_running"` in the repo's
`config.json`). When an accept or a drop frees a slot, the next queued task starts
too. Only tasks with dependencies are queued like this: a plain todo task waits
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
- Waiting on you mid-turn (a permission prompt, a failure): not rebased, and the
  news says so.
- Between turns (needs you, stopped, ready): bullpen rebases its branch itself,
  if its worktree is clean, and pushes it to the PR (`--force-with-lease`). The
  status stays; a ready task says "Rebased onto main." on the board until its
  next `ready`. If the rebase conflicts, it is undone, the branch stays as it was,
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
listed. `bullpen config --review off` skips review; `review_rounds` and
`reviewer` in the repo's `config.json` set the rounds and the reviewer's command.

A task goes back to `working` only once its worker has the message: review
findings, a rebase conflict or PR feedback. If typing it into the worker's pane
fails, it needs you instead (as "Review findings didn't reach its worker", or
the PR's reason), or for a rebase conflict stays where it was, and news says
so, with where the findings are, so you can `bullpen tell` it. `verdict` and
`ready` refuse an empty `--summary-file`: an empty file is a write that failed
(a full disk, `/tmp` out of inodes), not "no findings".

### Auto-accept

Off by default: you accept each task yourself. Turn it on with `bullpen config
--auto-accept on` when review is the check you trust, so approved work lands
without waiting for you, and the next queued tasks start sooner. Once the reviewer
approves, bullpen runs the normal accept itself (same merge, cleanup, rebase of the
other tasks and start of queued ones), and the news says so: `BP-3 approved by
review and auto-accepted (merged into main, rebase)`.

It never accepts a task review didn't approve: one ready after the last round with
findings open, or any task while review is off. With a PR, it waits for the PR's
checks: still running, the task stays ready and accepts once they pass; failing, or
new comments from people, it isn't accepted and the news says why. A merge that is
refused (a conflict, an uncommitted change in the way) leaves the task ready, with
the reason as news. `bullpen add` or `start` with `--no-auto-accept` keeps one task
for you while it's on (and `--auto-accept` auto-accepts one task while it's off).
The board shows `· auto-accept` beside the repo's name while it's on, and a task
held back says "Kept for you: no auto-accept."

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
needs you instead; if the worker is waiting on you (a permission prompt), the news
says so and nothing is typed into it. The board and `show` say what GitHub last said ("checks
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
sets how it reaches the coordinator, and `bullpen up` says which is in use:

- `type` (the default): bullpen types each line into the coordinator's pane. It
  works with any coordinator agent, and goes on working when the coordinator's
  session is restarted, since nothing has to be running inside it. But news that
  arrives while you're typing there waits (in `outbox.txt`) until the input is
  empty, so it never gets sent along with half a message: it comes late, after
  you send your own.
- `monitor`: nothing is typed. The coordinator runs Claude Code's Monitor tool on
  `bullpen news --follow`, and each line shows up in its chat as a notification
  without touching the input, so news never waits on your typing. It needs a
  coordinator with that tool (Claude Code), and the coordinator has to start the
  monitor and start it again every 30 minutes, when it expires; its guide tells
  it to. News logged while no monitor runs is not lost: the next one prints it.

`bullpen news` prints the lines logged since the last time and exits;
`bullpen news --follow` keeps running and prints each new line as it is logged.
Both remember how far they got (a byte offset in `news.cursor`), so each line is
printed once, across restarts too. Only one reads at a time: a second
`--follow` waits for the first to end and then takes over. The first run starts
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

### Keys

`bullpen keys` prints a line to add to your Rex config (`init.lua`). It defines
three actions, also in the command palette under Bullpen, and binds:

- `cmd+;` then `n`: the next task that needs you. Again: the one after it.
- `cmd+;` then `b`: the board.
- `cmd+;` then `c`: the coordinator.

And in a worker's or reviewer's floating pane, `ctrl+escape` puts it back into its
capsule and returns you to the board (`bullpen hide`); elsewhere the key goes
through.

`setup{ prefix = "ctrl+b" }` moves them behind another key, and
`keys = { next = "cmd+shift+j" }` sets single ones. Run `rex config reload` after.

Then tell the coordinator what you want. To drive it by hand:

```sh
bullpen add "Fix login redirect" --brief-file brief.md   # prints BP-1
bullpen add "Cover logout" --brief-file b2.md --after BP-1 # BP-2 starts once BP-1 is accepted
bullpen after BP-2 --none   # change (or clear) what a task waits on
bullpen start BP-1    # worktree + branch + a Rex window running the worker
bullpen ls            # todo, working, needs, stopped, ready
bullpen peek BP-1     # the end of the worker's screen
bullpen diff BP-1     # its changes against its base, in a pager (--stat: just the files)
bullpen go BP-1       # focus its pane (`go` alone: the coordinator; --next: next needing you)
bullpen tell BP-1 "Also cover the logout path"
bullpen resume BP-1   # restart a stopped worker, continuing its conversation
bullpen note BP-1 "Asks: ..."   # say on the board what it needs from you
bullpen accept BP-1   # merge its PR (or local branch; rebase), close its pane, remove its worktree
bullpen drop BP-1     # close its PR, window and worktree without merging
```

`bullpen diff BP-1` shows `git diff <base>...HEAD` from the task's worktree, then
any changes not committed yet, in `delta` if it's installed, else `less -R`. The
base is `origin/<base>` when there's a remote, the local base branch when there
isn't. On the board, `v` opens the same view in a floating pane; `q` closes it and
takes you back to the board.

A worker finishes with `bullpen ready <ID> --summary-file <file>`, which refuses
uncommitted changes, pushes the branch and opens (or updates) the pull request.

### Local mode

In a repository with no `origin` remote, bullpen works locally. `ready` pushes
nothing and opens no pull request: it checks the branch against the local base
branch and starts the reviewer as usual, and news says "local branch" where it
would name a PR. `accept` merges the task's branch into the local base branch
itself (rebase, squash or merge, as `config --merge` says). Where the base branch
is checked out, it is fast-forwarded with `git merge --ff-only`, so uncommitted
changes to other files survive. If anything stands in the way (a conflict, or an
uncommitted change to a file the task touches), `accept` says why and leaves the
task ready, with its worktree and branch as they were.

## Where things live

- Tasks, briefs, settings and your own rules: `~/.local/state/bullpen/repos/<repo>/` (`BULLPEN_STATE` moves it).
- Worktrees: `<repo>-bullpen/<ID>` next to the repository.
- News: `news.log` beside the tasks, with `news.cursor` (how far `bullpen news` has
  read) and `outbox.txt` (typed news waiting for an empty input). See [News](#news).

`bullpen config --auto-accept on` accepts approved tasks by themselves (off by
default; see [Auto-accept](#auto-accept)). `bullpen config --news monitor` changes how news reaches the coordinator (`type`
by default). `bullpen config --merge squash` changes how `accept` merges (`rebase` by default;
`squash` or `merge`). `bullpen config --stuck-after 45` sets how many minutes a
working agent can show no progress before you hear of it (20 by default, `0` for
never). `bullpen config --prefix XY --base develop` changes the task prefix (default `BP`)
and the base branch (default: origin's default branch). The worker and coordinator
commands are in that folder's `config.json` (`worker`, `coordinator_cmd`). `"rebase_others": false`
there stops `accept` rebasing the other open tasks (see [After accept](#after-accept)).

`bullpen config --model opus` sets the model for workers and reviewers;
`--worker-model` and `--reviewer-model` set one each. It takes anything `claude --model`
does (an alias like `opus`, or a full model ID), and `default` lets Claude Code pick
again. Bullpen adds `--model` when it starts an agent, in place of any `--model` in its
command; it leaves other agents' commands alone. (To use a `--model` written in the
command instead, set `worker_model` or `reviewer_model` back to `null` in `config.json`.) `bullpen config` shows the models in
use, `bullpen show BP-3` the one a worker was started with, and `bullpen start BP-3
--model sonnet` picks one for a single task.

## Tests

```sh
python3 -m unittest discover -s tests
```

The tests use fake `rex` and `gh` programs and a local bare repository as origin.
Each test removes its temp folder when it ends. The suite runs inside a temp folder
of its own and fails if anything is left in it afterwards.
