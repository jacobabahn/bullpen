# bullpen

A coordinator agent and its workers, each in its own [Rex](https://www.superlogical.com/rex) pane.

You talk to the coordinator. It splits your ask into tasks and starts one worker
agent per task: each gets its own git worktree, its own branch and its own Rex
window labelled `bullpen: <ID>`. When a worker finishes, it pushes its branch and
opens a pull request. You accept (squash-merge) or drop it.

The coordinator learns what its workers are doing from Rex itself. Rex records the
status coding agents report (working, blocked, done) and a Lua watcher
(`lua/watch.lua`, run with `rex do --all`) types news lines into the coordinator's
pane when a worker needs you, goes quiet, fails or stops.

## Install

```sh
./install.sh          # links bin/bullpen into ~/.local/bin
```

Needs Rex, git, `gh` (signed in) and Claude Code.

## Use

In a Rex pane, inside the repository:

```sh
bullpen up            # opens the coordinator's window, the task board and the watcher
bullpen up --here     # or: make the agent already in this pane the coordinator
```

The task board (`bullpen board`) sits in a narrow pane beside the coordinator:
one line per task, with the ones that need you on top, how long each has been
in its status, and why it needs you or its PR. On the board, `enter` jumps to the
selected worker's pane, `c` to the coordinator, `n` selects the next task that
needs you, `p` peeks, `t` tells, `s` starts, `r` resumes, `a` shows finished
tasks too. `bullpen up --no-board` leaves it out.

### Keys

`bullpen keys` prints a line to add to your Rex config (`init.lua`). It defines
three actions, also in the command palette under Bullpen, and binds:

- `cmd+;` then `n`: the next task that needs you. Again: the one after it.
- `cmd+;` then `b`: the board.
- `cmd+;` then `c`: the coordinator.

`setup{ prefix = "ctrl+b" }` moves them behind another key, and
`keys = { next = "cmd+shift+j" }` sets single ones. Run `rex config reload` after.

Then tell the coordinator what you want. To drive it by hand:

```sh
bullpen add "Fix login redirect" --brief-file brief.md   # prints BP-1
bullpen start BP-1    # worktree + branch + a Rex window running the worker
bullpen ls            # todo, working, needs, stopped, ready
bullpen peek BP-1     # the end of the worker's screen
bullpen go BP-1       # focus its pane (`go` alone: the coordinator; --next: next needing you)
bullpen tell BP-1 "Also cover the logout path"
bullpen resume BP-1   # restart a stopped worker, continuing its conversation
bullpen accept BP-1   # squash-merge its PR, close its window, remove its worktree
bullpen drop BP-1     # close its PR, window and worktree without merging
```

A worker finishes with `bullpen ready <ID> --summary-file <file>`, which refuses
uncommitted changes, pushes the branch and opens (or updates) the pull request.

## Where things live

- Tasks, briefs and settings: `~/.local/state/bullpen/repos/<repo>/` (`BULLPEN_STATE` moves it).
- Worktrees: `<repo>-bullpen/<ID>` next to the repository.
- News that couldn't be delivered: `news.log` beside the tasks.

`bullpen config --prefix XY --base develop` changes the task prefix (default `BP`)
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
