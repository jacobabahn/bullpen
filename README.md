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
pane, or the coordinator follows it with Claude Code's Monitor tool (see [News](docs/news.md)).

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
./install.sh          # builds bullpen and its board, and links bin/bullpen into ~/.local/bin
```

Needs Rex (a server from preview.13 on, for minimized floating panes), git, `gh`
(signed in), Claude Code, Python 3, and Rust's `cargo`: bullpen's command line and its
task board are Rust programs, and `install.sh` stops without cargo. After a change to
their source, bullpen rebuilds them by itself (see [Rebuilds](docs/rebuilds.md));
`bullpen build` builds them now.

## How it works

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
the [keys](docs/keys.md)) brings one up and puts any other worker back into its capsule;
going to the coordinator puts them all back. Reviewers run in the background, in
no tab, until `go` (or the board) brings one up. The watcher runs detached, with no
tab. `bullpen config --workers tab` puts new workers' capsules in one workers tab
instead, `--workers tabs` gives them a tab each, and `--workers background` keeps
them out of sight too; `bullpen start BP-3 --pane tab` chooses for one task (see
[Where panes live](docs/settings.md#where-panes-live)).

After a Rex server restart, run `bullpen up` again: it finds its panes by their
labels, marks workers whose panes are gone as stopped, and restarts the watcher.

The watcher is also replaced when it is out of date. `up` and `accept` compare
`lua/watch.lua` and bullpen's code (`bin/`, `bullpen/` and the build, `target/bullpen`) with what the running watcher was started from
(hashes kept in `watcher.json`); if either changed, say because a merge or a
pull brought a new version, they close the old watcher and start a new one. A
current watcher is left alone.

## Docs

Using it:

- [The board](docs/board.md): its groups, marks and keys, answering from the board
- [Keys](docs/keys.md): the `cmd+;` keys in Rex, and the jump picker
- [Commands](docs/commands.md): driving bullpen by hand from a shell
- [Doctor](docs/doctor.md): what `bullpen doctor` checks
- [Several coordinators](docs/several-coordinators.md)
- [Several repositories](docs/several-repositories.md)

How it behaves: what bullpen does by itself as tasks move along, and the settings for it.

- [Dependencies](docs/dependencies.md): tasks that wait on others, and the queue
- [Accept](docs/accept.md): after accept, and auto-accept
- [Review](docs/review.md): the reviewer, its rounds, and after ready (the PR's checks and comments)
- [Guided review tours](docs/tours.md)
- [Finish with nothing to merge](docs/finish.md)
- [Triage](docs/triage.md)
- [No progress and status drift](docs/check.md): `bullpen check`
- [Usage limit](docs/usage-limit.md)
- [News](docs/news.md): how news reaches the coordinator, and guide changes
- [Notifications](docs/notifications.md)
- [Rules](docs/rules.md)
- [Worktree setup](docs/worktree-setup.md)
- [Local mode](docs/local-mode.md): no `origin` remote, and the push remote
- [Rebuilds](docs/rebuilds.md): bullpen's command line and the board

Reference:

- [Settings](docs/settings.md): `bullpen config`
- [Where things live](docs/where-things-live.md)
- [The code](docs/code.md): the entry, the Rust (`core/`, `cli/`) and the `bullpen/` package
- [Tests](docs/tests.md): running them
- [GLOSSARY.md](GLOSSARY.md): the words bullpen uses
- [docs/prd/](docs/prd/): design notes
- [The Mojo experiment](docs/experiments/mojo.md): a partial port to Mojo and its benchmark (code on the `mojo` branch)
