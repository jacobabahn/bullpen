# bullpen

Run a coordinator agent and a pool of worker agents in [Rex](https://www.superlogical.com/rex) panes — each task in its own git worktree, branch, and pane, tracked on a live task board beside your coordinator.

You talk to the coordinator. It turns your request into tasks, starts one worker agent per task, triages hiccups, and hands work back to you once it is ready. By default, a separate reviewer agent checks each completed task before it reaches you, labelling its risk and opening a pull request (or keeping everything local when there is no `origin` remote). You review, take a guided tour, or accept and merge right from the board.

## Quick start

```sh
./install.sh            # build bullpen + bullpen-board and link bin/bullpen into ~/.local/bin
bullpen keys --install  # add cmd+; keybindings to your Rex config (init.lua) and reload it
bullpen doctor          # verify Rex, git, gh, agents, cargo, and hooks (with --fix where supported)
bullpen up              # in a Rex pane inside your repo: start coordinator, board, and watcher
```

Tell the coordinator what you want (for example, *"fix the login redirect and cover it with a test"*). It creates tasks and launches workers. From the board beside it:

| Key | Action |
| --- | --- |
| `n` | Jump to the next task that needs you (or the next ready/finished one) |
| `i` | Answer a worker prompt or coordinator question without leaving the board |
| `p` / `⏎` | Peek live at a worker's or reviewer's screen, or open its pane |
| `v` / `g` | View the diff, or walk through a [guided review tour](docs/tours.md) |
| `a` / `d` | Accept (merge) a ready task or close a finished one, or drop it |
| `I` / `l` | Toggle the kitty-graphics [image look](docs/board.md#the-image-look) or compact layout |
| `?` | Show every key and status mark |

See [GLOSSARY.md](GLOSSARY.md) for the terms bullpen uses.

## How it works

### One tab per repository

Inside your repository in Rex:

```sh
bullpen up            # start the coordinator, task board, and watcher
bullpen up --here     # make the agent already running in this pane a coordinator
bullpen up --verbose  # also print each pane's block ID and news mode
```

Everything stays organized in one Rex tab per repository:

- **Coordinator** on the left, **task board** on the right.
- **Workers** start as minimized floating capsules at the bottom right (`bullpen: BP-3`), each in its own git worktree and branch. `bullpen go BP-3` (the board's `⏎`, or `cmd+;` [keys](docs/keys.md)) brings one up and minimizes any other; returning to the coordinator (`c`) puts them all away. Use `bullpen config --workers tab|tabs|background` (or `bullpen start BP-3 --pane tab` per task) to place worker panes elsewhere — see [Where panes live](docs/settings.md#where-panes-live).
- **Reviewers** run in the background (no tab clutter) until you open or peek at one.
- **The watcher** (`lua/watch.lua`, via `rex do --all`) runs detached across all repositories.

### Status and coordinator news

Bullpen learns what agents are doing directly from Rex's program status protocol (`working`, `blocked`, `done`) plus worker `Stop` hooks — no polling hacks. The watcher turns agent events into `bullpen · ` news lines for the coordinator whenever a worker needs input, finishes a turn without `ready`, fails, stops, hits a usage limit, or shows no progress.

News reaches the coordinator in one of three modes (`bullpen config --news`, see [News](docs/news.md)):

- **`type`** (default): typed into the coordinator's pane once its input box has no draft.
- **`wake`**: a Claude Code `Stop`/`StopFailure` hook in `.claude/settings.local.json` waits in the background and wakes the idle session when news arrives, without touching the input box.
- **`monitor`**: Claude Code's Monitor tool streams `bullpen news --follow`.

### Review, accept, and everything in between

- **Automated review & risk labels:** When a worker runs `bullpen ready`, bullpen pushes the branch, opens a PR (when `origin` exists), and runs a reviewer agent (light review for small changes, full review otherwise, up to `review_rounds`). Approved tasks show a `low`/`medium`/`high` risk label on the board. See [Review](docs/review.md).
- **Guided tours:** Press `g` on any ready task (or run `bullpen tour`) for a step-by-step walkthrough of the diff, rendered as text or crisp kitty-graphics images. See [Guided review tours](docs/tours.md).
- **Clean merges & queues:** Accepting a task merges its PR (or local branch), removes its worktree, rebases remaining open tasks onto the new base, and starts any queued tasks that depended on it. Tasks with nothing to merge use `bullpen finish` (or `--keep-branch`) and close cleanly. See [Accept](docs/accept.md), [Dependencies](docs/dependencies.md), and [Finish with nothing to merge](docs/finish.md).
- **Backlog & GitHub issues:** Queue future ideas with `bullpen later` (and optionally let `pickup` start them automatically when slots open), or pull open GitHub issues into the backlog with `bullpen issues`. See [The backlog](docs/backlog.md) and [GitHub issues](docs/issues.md).
- **Multi-repo & multi-coordinator:** Run multiple coordinators per repo or work across repositories with `bullpen mission` (`cmd+;` `m`). See [Mission control](docs/mission.md), [Several repositories](docs/several-repositories.md), and [Several coordinators](docs/several-coordinators.md).

### Restarts and automatic rebuilds

- **After a Rex restart**, run `bullpen up`: it reconnects panes by label, marks workers whose panes are gone as `stopped`, and restarts the watcher.
- **After source changes**, `up`, `accept`, and the watcher's periodic check rebuild `target/bullpen` and `target/bullpen-board` in the background, replace an outdated watcher, and reload running boards in place without losing your selection (see [Rebuilds](docs/rebuilds.md)). Run `bullpen build` to rebuild immediately.

## Install and requirements

```sh
./install.sh                  # builds bullpen + bullpen-board and links into ~/.local/bin
BIN_DIR=/usr/local/bin ./install.sh  # override link destination
```

Requirements:

- **[Rex](https://www.superlogical.com/rex)** (`preview.13` or later, for minimized floating panes)
- **git**
- **Rust / `cargo`** (both `bullpen` and `bullpen-board` are Rust binaries; `install.sh` requires `cargo`)
- **Claude Code** (default agent; customizable via `config.json`)
- **`gh`** (signed in) — required for GitHub pull requests and issues; optional in [local mode](docs/local-mode.md)

## Docs

### Using bullpen

- [The board](docs/board.md): groups, status marks, keys, the kitty-graphics image look, and answering from the board
- [Keys](docs/keys.md): `cmd+;` keybindings in Rex and the cross-repo jump picker
- [Commands](docs/commands.md): driving bullpen directly from a shell
- [Doctor](docs/doctor.md): what `bullpen doctor` checks and fixes
- [Several coordinators](docs/several-coordinators.md): running more than one coordinator in a repo
- [Several repositories](docs/several-repositories.md): working across multiple repos
- [Mission control](docs/mission.md): every repository on one screen (`cmd+;` `m`)
- [bullpen web](docs/web.md): every coordinator and task as a page on your tailnet, through `tailscale serve`

### Workflows and automation

- [The backlog](docs/backlog.md): `bullpen later` and autonomous `pickup`
- [GitHub issues](docs/issues.md): `bullpen issues`, scoring, trust rules, and write-back
- [Dependencies](docs/dependencies.md): task ordering (`bullpen after`) and the queue
- [Accept](docs/accept.md): post-accept rebasing, `auto_accept`, and `bullpen revert`
- [Review](docs/review.md): full vs. light review, rounds, risk labels, and PR checks/comments
- [Guided review tours](docs/tours.md): interactive diff walkthroughs and comments
- [Finish with nothing to merge](docs/finish.md): research/ops tasks and `--keep-branch`
- [Triage](docs/triage.md): how the coordinator handles quiet or stuck workers before escalating
- [No progress and status drift](docs/check.md): `bullpen check`
- [Stats](docs/stats.md): task timelines and totals (`bullpen stats`)
- [Usage limit](docs/usage-limit.md): pausing and auto-continuing across rate limits
- [News](docs/news.md): `type`, `wake`, `monitor`, the backstop, and guide updates
- [Notifications](docs/notifications.md): native desktop notifications
- [Rules](docs/rules.md): standing instructions shared with every worker and reviewer
- [Worktree setup](docs/worktree-setup.md): `.bullpen/setup.sh`, `teardown.sh`, and `--setup`
- [Local mode](docs/local-mode.md): working without an `origin` remote and `push_remote`
- [Rebuilds](docs/rebuilds.md): background rebuilds, live board reloads, and multiple checkouts

### Reference

- [Settings](docs/settings.md): `bullpen config` and `config.json`
- [Where things live](docs/where-things-live.md): state, worktrees, and logs on disk
- [The code](docs/code.md): crates, entry points, and `bin/code-map`
- [Tests](docs/tests.md): running the Rust and Python test suites
- [GLOSSARY.md](GLOSSARY.md): definitions of bullpen's terms
- [docs/prd/](docs/prd/): design notes
- [The Mojo experiment](docs/experiments/mojo.md): partial port to Mojo and benchmark (`mojo` branch)
- [The Rust experiment](docs/experiments/rust.md): partial port to Rust and benchmark (`rust` branch)
- [Where the test suite's time goes](docs/experiments/suite-time.md): suite profiling and Rust test fakes
