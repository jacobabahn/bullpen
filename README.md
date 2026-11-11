# bullpen

A coordinator agent and its workers, each in its own [Rex](https://www.superlogical.com/rex) pane.

You talk to the coordinator. It splits your ask into tasks and starts one worker
agent per task: each gets its own git worktree, its own branch and its own Rex
pane labelled `bullpen: <ID>`. When a worker finishes, it pushes its branch and
opens a pull request. You accept (merge) or drop it.

The coordinator learns what its workers are doing from Rex itself. Rex records the
status coding agents report (working, blocked, done) and a Lua watcher
(`lua/watch.lua`, run with `rex do --all`) types news into the coordinator's pane,
as messages starting `bullpen · `, when a worker needs you, goes quiet, fails or
stops.

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

The task board (`bullpen board`) sits in a narrow pane beside the coordinator.
It groups tasks by what you do next: needs you, ready to accept, in review,
working, queued. Each task shows how long it has been there and, on a second
line, what it needs or where it stands ("Asks: python3 or python?", "PR #4,
approved by review."). A task in review or ready also shows the size of its
change ("3 files, +120 −14"), worked out once when its worker runs `ready`. A
task that needs you gets an amber bar; its age turns red after 5 minutes. `l`
switches to a compact layout, one line per task with the selected task's detail
at the bottom.

The keys at the bottom follow the selected task: `⏎` opens its worker (or its
reviewer), `a` accepts and `d` drops (each asks y/n first), `t` tells, `p`
peeks, `v` shows its diff, `s` starts, `r` resumes, `c` goes back to the
coordinator, `n` selects the next task that needs you. `?` lists them all.
`bullpen up --no-board` leaves the board out.

### Review

When a worker runs `ready`, bullpen pushes its branch, opens the PR and starts a
reviewer agent on it, in its own capsule (`bullpen: BP-3 review`). The reviewer
reads the brief and the diff, runs the tests, changes nothing, and ends with
`bullpen verdict BP-3 approve` or `changes`. Changes go back to the worker,
which fixes them and runs `ready` again, for up to 2 rounds. The task is ready
to accept once review approves, or after the last round with the open findings
listed. `bullpen config --review off` skips review; `review_rounds` and
`reviewer` in the repo's `config.json` set the rounds and the reviewer's command.

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
- News: every line is logged in `news.log` beside the tasks. News that arrives while
  you're typing in the coordinator waits (in `outbox.txt`) until its input is empty,
  so it never gets sent along with half a message.

`bullpen config --merge squash` changes how `accept` merges (`rebase` by default;
`squash` or `merge`). `bullpen config --prefix XY --base develop` changes the task prefix (default `BP`)
and the base branch (default: origin's default branch). The worker and coordinator
commands are in that folder's `config.json` (`worker`, `coordinator_cmd`).

## Tests

```sh
python3 -m unittest discover -s tests
```

The tests use fake `rex` and `gh` programs and a local bare repository as origin.

## Not yet

- Dependencies between tasks and a run queue: the coordinator holds tasks back itself.
- Hearing about pull request checks or reviews after `ready`.
- Noticing a worker that's working but stuck for a long time.
