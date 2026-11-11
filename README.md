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
stops or shows no progress for a long time. News is typed into the coordinator's
pane, or the coordinator follows it with Claude Code's Monitor tool (see [News](#news)).

## Install

```sh
./install.sh          # links bin/bullpen into ~/.local/bin
```

Needs Rex (a server from preview.13 on, for minimized floating panes), git, `gh`
(signed in) and Claude Code.

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

The task board (`bullpen board`) sits in a narrow pane beside the coordinator.
It groups tasks by what you do next: needs you, ready to accept, in review,
working, queued. Each task shows how long it has been there and, on a second
line, what it needs or where it stands ("Asks: python3 or python?", "PR #4,
approved by review."). A task in review or ready also shows the size of its
change ("3 files, +120 −14"), worked out once when its worker runs `ready`. A
task that needs you gets an amber bar; its age turns red after 5 minutes. So
does a task whose agent has shown no progress for a while (below). `l`
switches to a compact layout, one line per task with the selected task's detail
at the bottom.

The keys at the bottom follow the selected task: `⏎` opens its worker (or its
reviewer), `a` accepts and `d` drops (each asks y/n first), `t` tells, `p`
peeks, `v` shows its diff, `s` starts, `r` resumes, `c` goes back to the
coordinator, `n` selects the next task that needs you. `?` lists them all.
`bullpen up --no-board` leaves the board out.

Accepting or dropping from the board (or a shell) is news to the coordinator:
`BP-3 accepted (merged PR #12, rebase)`, `BP-3 dropped`. That is how it learns
it can start queued tasks that waited for the accepted one. Run from the
coordinator's own pane, `accept` and `drop` post nothing: it already knows.

### Review

When a worker runs `ready`, bullpen pushes its branch, opens the PR and starts a
reviewer agent on it, in its own capsule (`bullpen: BP-3 review`). The reviewer
reads the brief and the diff, runs the tests, changes nothing, and ends with
`bullpen verdict BP-3 approve` or `changes`. Changes go back to the worker,
which fixes them and runs `ready` again, for up to 2 rounds. The task is ready
to accept once review approves, or after the last round with the open findings
listed. `bullpen config --review off` skips review; `review_rounds` and
`reviewer` in the repo's `config.json` set the rounds and the reviewer's command.

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
board the task gets the amber bar and "No progress for 20 min." It stays in its
group and nothing is typed into its pane: a long build looks the same, so you
(or the coordinator) decide. The pane's next status event clears it, and a new
quiet spell is reported again. The same goes for a reviewer
("BP-3's reviewer has shown no progress ...").

`bullpen config --stuck-after 45` changes the minutes for this repository; `0`
turns it off. The watcher only knows of events since it started, so a pane that
was already silent when the watcher (re)started isn't noticed until its next
event.

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

- Tasks, briefs and settings: `~/.local/state/bullpen/repos/<repo>/` (`BULLPEN_STATE` moves it).
- Worktrees: `<repo>-bullpen/<ID>` next to the repository.
- News: `news.log` beside the tasks, with `news.cursor` (how far `bullpen news` has
  read) and `outbox.txt` (typed news waiting for an empty input). See [News](#news).

`bullpen config --news monitor` changes how news reaches the coordinator (`type`
by default). `bullpen config --merge squash` changes how `accept` merges (`rebase` by default;
`squash` or `merge`). `bullpen config --stuck-after 45` sets how many minutes a
working agent can show no progress before you hear of it (20 by default, `0` for
never). `bullpen config --prefix XY --base develop` changes the task prefix (default `BP`)
and the base branch (default: origin's default branch). The worker and coordinator
commands are in that folder's `config.json` (`worker`, `coordinator_cmd`).

`bullpen config --model opus` sets the model for workers and reviewers;
`--worker-model` and `--reviewer-model` set one each. It takes anything `claude --model`
does (an alias like `opus`, or a full model ID), and `default` lets Claude Code pick
again. Bullpen adds `--model` when it starts an agent, in place of any `--model` in its
command; it leaves other agents' commands alone. `bullpen config` shows the models in
use, `bullpen show BP-3` the one a worker was started with, and `bullpen start BP-3
--model sonnet` picks one for a single task.

## Tests

```sh
python3 -m unittest discover -s tests
```

The tests use fake `rex` and `gh` programs and a local bare repository as origin.

## Not yet

- Dependencies between tasks and a run queue: the coordinator holds tasks back itself.
