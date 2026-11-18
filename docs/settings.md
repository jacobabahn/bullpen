# Settings

`bullpen config` lists the settings in groups (tasks, agents, review, board, news, pickup, issues),
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
branch, the news mode, the push remote, the issues' repository and the coordinators belong to one repository,
so `--global` refuses `--prefix`, `--base`, `--news`, `--push-remote`, `--issues-repo`, `--risk-path` and `--board-others`. The commands
(`worker`, `reviewer`, `coordinator_cmd`, `tour`) have no flags: set them in either
`config.json` by hand. `risk_paths`, the paths whose change makes a ready task's
risk high or medium, has its own flags, for one repository only: `bullpen config
--risk-path PATTERN --level high|medium --why "..."` adds or changes a path and
`--remove-risk-path PATTERN` drops one. `--risk-large-lines` (400) is how many changed
lines in a medium path make it high, and `--risk-huge-lines` (1,200; `0` for never)
how many lines of code make a change high whatever it touches
([Review](review.md#the-risk-label)). `bullpen config --review off` skips review: a
task is ready once its worker runs `ready` (on by default). `--light-review-lines` (40) is the most lines a
change in at most 2 files can have for a light review ([Review](review.md)). `bullpen config --help` gives each flag's default; the defaults
are in one table, `SETTINGS` in `core/src/settings.rs`, with each setting's kind and
whether only a repository can hold it.

A repository's `config.json` holds only what it sets itself. One from before the
global file held every setting: the first time bullpen reads it, it drops the
values that are just the default, so the global ones apply there.

`bullpen config --auto-accept on` accepts approved tasks by themselves (off by
default; see [Auto-accept](accept.md#auto-accept)). `bullpen config --news wake` changes how news reaches the coordinator (`type`
by default; `wake` writes a hook into the repo's `.claude/settings.local.json` and needs Claude Code
as the coordinator, `monitor` has the coordinator follow it with its Monitor tool; see [News](news.md)). `bullpen config --merge squash` changes how `accept` merges (`rebase` by default;
`squash` or `merge`). `bullpen config --stuck-after 45` sets how many minutes a
working agent can show no progress before you hear of it (20 by default, `0` for
never). `bullpen config --triage-minutes 15` sets how long a task can wait on its
coordinator before it goes to you (10 by default; see [Triage](triage.md)). `bullpen config
--notify all` sends you a notification when a task is ready, finished or auto-accepted
too, and `off` none (`needs`, when a task needs you, by default; see
[Notifications](notifications.md)). `bullpen config
--board-bottom-gap 0` lets the board use its bottom row, and a number leaves that many
rows empty. Unset, it's 1 row while workers or reviewers float (Rex draws their
capsules there), else none. `bullpen config --board-layout compact` starts the board in
its compact layout, one line a task (`grouped` by default; its `l` switches, see
[The board](board.md)). `bullpen config --board-others on --coordinator c1` makes c1's
board show the other coordinators' tasks too (see [Several
coordinators](several-coordinators.md); the board's `o`). `bullpen config --board-repos off`
takes the other repositories off the board's top line (on by default; see [Several
repositories](several-repositories.md)). `bullpen config --prefix XY --base develop` changes the task prefix (default `BP`)
and the base branch (default: origin's default branch). `bullpen config --push-remote
github` pushes the base branch to the `github` remote after each accept, in local mode
only (see [Local mode](local-mode.md)). `bullpen config --setup "npm ci"` runs that
command in each new worktree before its worker starts, and `--teardown CMD` one in a
worktree before it's removed, over `.bullpen/setup.sh` and `.bullpen/teardown.sh` (see
[Worktree setup](worktree-setup.md)). The worker and coordinator
commands are in that folder's `config.json` (`worker`, `coordinator_cmd`). `bullpen config
--rebase-others off` stops `accept` rebasing the other open tasks (see [After
accept](accept.md#after-accept)), and `--gh PATH` runs that gh instead of the one on PATH
(`BULLPEN_GH` wins over both). `bullpen config --pickup on` starts backlog items by
themselves when there's room, under caps (`--pickup-running`, `--pickup-waiting`,
`--pickup-per-day`, `--pickup-model`, `--pickup-auto-accept`; see
[Pickup](backlog.md#pickup)). `bullpen config --issues on` lets `bullpen issues --now` read
the repository's open GitHub issues into the backlog, for the coordinator to judge (off by
default), and `--issues-repo OWNER/REPO` names the repository when it isn't origin's, or
in local mode the push remote's. While it is on the watcher reads them too, every
`--issues-every` minutes (30), and tells the coordinator of new ones. `--issues-start
off|small|medium` (`small`) says which judged issues go on the list by themselves, and
so start through pickup: only with `--pickup on`, whatever `issues_start` says. Only a
trusted issue does: its author is the gh account's login or in `--issues-trusted
"ann,bob"` (nobody else by default), or that login added the label `--issues-label`
(`bullpen`) (see [GitHub issues](issues.md)).

## Where panes live

`bullpen config --workers` says where new workers' panes go:

- `float` (the default): a capsule, a minimized floating pane at the bottom right of
  the coordinator's tab. `go` brings it up, and `ctrl+esc` minimizes it again.
- `tab`: a capsule in one workers tab, `bullpen: <repo> workers`, out of the
  coordinator's tab (a second coordinator's is `bullpen: <repo> workers c2`). bullpen
  makes the tab, without switching to it, when a pane first goes there, and Rex
  closes it when the last one there closes. `go` switches to that tab and brings the
  pane up, and `ctrl+esc` minimizes it and takes you back to the board's tab. Rex
  keeps one pane up in a tab that holds only floating ones, so the last one up there
  stays up. A worker that needs you puts Rex's dot on that tab. `bullpen up` (and
  `go`) moves a pane dragged out of it back in.
- `tabs`: a tab each. `go` focuses it, and `ctrl+esc` does nothing there.
- `background`: in no tab at all, out of sight. `go` (and `go --next`, the jump key,
  the board's Enter) floats it over the coordinator's tab, with focus, and `ctrl+esc`
  puts it back in the background.

`bullpen config --reviewers` does the same for reviewers (full, light, follow-up and
rebase-check reviews): `background` by default, or `float`, `tab`, `tabs`, or `same`
(where workers go, the task's own choice included). A reviewer is read through its findings rather than its screen; when one
stops on a prompt or needs you, `go --next` and the board bring it up like a worker.
The board's diff (`v`) floats, or gets a tab with `--workers tabs`, whatever these say.

A task can have its own: `bullpen start BP-3 --pane background` (also on `add` and
`resume`) puts its worker there, whatever `--workers` says, from then on: it's kept in
the task, and `bullpen show BP-3` says it. Its reviewer goes there too only with
`--reviewers same`.

A change applies to panes made after it: a pane that's running stays where it is and
shows and hides as before.

A pane in the background still runs as any other: `peek`, `tell`, `answer`, the
watcher and the board work the same. A notification about it comes from the board
instead (a pane in no tab has nothing to open), and Rex's tab dots don't show it.

Background panes don't survive a Rex server restart: Rex keeps them only while its
server runs, and rexurrect doesn't save them. After one, `bullpen up` (or `go`) finds
the pane gone and marks its task stopped, as for any pane that's gone; `bullpen resume
BP-3` starts the worker again and continues its conversation. A reviewer lost that
way costs only a new review: when `up` (or `go`) finds its pane gone, the task stops
on its reviewer and goes to its coordinator (or to you, with none running), whose
`bullpen review BP-3` starts a new one.

`bullpen config --model opus` sets the model for workers and reviewers (follow-up
and light reviews too); `--worker-model`, `--reviewer-model` and
`--reviewer-followup-model` set one each (`--tour-model` the tour agent's, see
[Guided review tours](tours.md)). It takes anything `claude --model`
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
