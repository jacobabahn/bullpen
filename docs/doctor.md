# Doctor

`bullpen doctor` checks the setup and prints one line per check: `ok`, `FAIL` with
the exact fix, `WARN` for something to see to that stops nothing, or `--` when a
check doesn't apply or can't be told. It exits non-zero when anything fails (a warning
isn't a failure). It checks:

- `rex` is on PATH and its server is preview.13 or later (`rex server status`)
- `gh` is installed and signed in (`gh auth status`), unless the repo is in
  [local mode](local-mode.md). With [`issues`](issues.md) on it is checked in local
  mode too, and for the `repo` scope, which reading a private repository's issues
  needs: `gh auth login` when it isn't signed in, `gh auth refresh -s repo` when the
  scope is missing. A token that names no scopes (a fine-grained one) passes: doctor
  can't tell what it may read
- the programs in the coordinator, worker and reviewer commands (Claude Code's
  `claude`, unless you changed them) are on PATH
- bullpen's command line (`bullpen`) and the board (`board`) are each built from
  their source as it is now (by hash: see [Rebuilds](rebuilds.md)), or being rebuilt.
  Else it says why: the last build failed (with the log), the source has uncommitted
  changes, or it just hasn't been rebuilt yet; `bullpen build` fixes each. For the
  copy the watcher runs, a build older than its committed source is caught up by the
  watcher's next check (every 3 minutes: see [Rebuilds](rebuilds.md#the-watchers-check))
- the keys are in your Rex config: Rex lists the `bullpen_next`, `bullpen_board`,
  `bullpen_coordinator`, `bullpen_jump` and `bullpen_mission` actions (`rex actions`). With some of them
  listed and one missing, the line is there but Rex loaded an older `keys.lua`: the
  fix is `rex config reload`. With none listed, it's `bullpen keys --install`
- the watcher is running, from the current `watch.lua` and bullpen build (the build's
  stamp, `target/bullpen.json`; the hashes in `watcher.json`) of the copy of bullpen it
  runs: this one, or the one `watcher.json` names, which the line then says
- the state folder is writable, and this repo's `config.json` parses
- the temp folder (`temp`): how full its filesystem is, and the Claude Code scratch
  folders ([Worktree setup](worktree-setup.md#claude-codes-scratch-folder)) closed
  tasks left in it, of any repository bullpen knows: "/tmp is 71% full (4.1 GB free);
  132 Claude scratch folders of closed tasks hold 9.8 GB: `bullpen doctor --fix`
  removes them". A folder counts only when it's named for a closed task's worktree,
  that worktree isn't there, and no open task's worktree has its name. 90% full or
  more is a warning (`WARN`), never a failure: a full `/tmp` stops workers with a quota
  error, but nothing about bullpen is broken. The temp folder is each of
  `$CLAUDE_CODE_TMPDIR`, `$TMPDIR` and `/tmp` that has a `claude-<uid>` folder in it
  (`/tmp` when none has)
- the global `config.json` (see [Settings](settings.md)), if there is one, parses
  and sets nothing only a repository can have
- the setup and teardown hooks, if there are any: `.bullpen/setup.sh` as committed
  on the base (its `#!` interpreter exists, and an `sh` or `bash` script parses), or
  the `config --setup` command (it parses, and its program is found). A
  `.bullpen/setup.sh` that is there but not committed on the base is a failure: it
  wouldn't run.
- the push remote, if one is set (see [Local mode](local-mode.md)): the remote is
  there, answers without a prompt, and has the base branch as it is here. Else, for
  one behind: "github is 2 commits behind main: `git push github main`"
- the repository's sensitive paths (`risk`; see [Review](review.md#the-risk-label)):
  how many it has, high and medium. With none it says so, once, and what a ready
  task's risk label goes by then (its size), with the `config --risk-path` command
  that names one. It is never a failure
- the coordinators in `config.json` still have their panes and boards, and each
  running board runs the program `bullpen board` runs now (Rex says which program a
  pane runs). One from an older install's path never reloads by itself: "c1's board
  is from an older build: `bullpen up` restarts it". `up` opens a new board in its
  place and closes the old one

- the copy of bullpen this repo's coordinator uses (`copy`; see
  [Rebuilds](rebuilds.md#one-watcher-from-the-coordinators-copy)): the one `up`
  recorded when it started the coordinator. With several coordinators it is the first
  one's, which the watcher follows; a later coordinator recorded with another copy is
  named ("this repo's first coordinator (c1) uses this copy, ~/dev/bullpen; c2 uses
  ~/wt/bullpen"), and that copy counts as one of the repo's own below. It fails when
  - that copy is gone: "this repo's coordinator (c1) was started from ~/old/bullpen,
    which is gone ... Run `~/dev/bullpen/bin/bullpen up --here` in its pane to have it
    use this copy"
  - doctor is run by a copy no coordinator here was started from, whose source
    differs: "this is ~/wt/bullpen's
    bullpen, but this repo's coordinator (c1) uses ~/dev/bullpen's, and this one's
    source is older than that one's (10 commits behind): run ~/dev/bullpen/bin/bullpen
    here"
  - the watcher runs from another copy than the coordinator's: `bullpen up` restarts it
  - a command that saved a task here was run by such a copy, whose source differs:
    "`accept` was run here by another copy of bullpen (~/wt/bullpen,
    2026-11-16T10:02:11-08:00), whose source is older than this one's (it lacks 10 of
    the other's commits, and has 1 of its own), the one this repo's coordinator (c1)
    uses. Run this repo's commands with ~/dev/bullpen/bin/bullpen ...". Only the last
    such command is kept; `bullpen up` from the coordinator's copy clears it. A copy
    from before this note leaves none itself: when `check --fix` labels a task that
    such a copy made ready without a risk label, it notes the copy the task's brief
    names (as `ready`). When the other copy's source has no risk label
    (`cli/src/risk.rs`) and this one's has, the line ends "That copy has no risk
    label: a task it makes ready gets its label late, from this one's `check --fix`
    (the watcher runs it)".

  Source is the command line's (`core/`, `cli/`, the workspace's files) and
  `lua/watch.lua`; two copies with the same source are fine. Which is older is by
  commits: behind or ahead when one has all of the other's, and for branches that
  parted, the one whose last commit is earlier (by date alone when neither checkout
  has the other's commits; not told without git). A coordinator started before copies
  were recorded is `--` until `bullpen up` records it.

- the news hook (`news`; see [News](news.md#the-waiter-config---news-wake)). With
  `config --news wake`:
  - the repo's `.claude/settings.local.json` parses, and has bullpen's Stop and
    StopFailure entries, once each, running the copy of bullpen the repo's
    coordinator uses: `bullpen up` writes them
  - git ignores that file (a committed one isn't ignored)
  - `coordinator_cmd`'s program is `claude`. Else it says `wake` needs Claude Code,
    and to use `type`
  - Claude Code (`claude --version`) is 2.1.325 or later, the version the hook was
    checked on. A program that doesn't say its version isn't a failure
  - every running coordinator that has been idle for 10 seconds (by Rex's status of
    its pane's program), and
    not paused, has a waiter: "c1 is idle with no news waiter: it hears news by
    typing until its next turn". Saying anything to it starts one; if its turns
    keep ending without one, the hook doesn't run in that session (`/hooks` there
    lists the hooks it has)

  With any other news it is `--`, unless the file still has bullpen's entries:
  `bullpen doctor --fix` removes them.

- the workers' Stop hook (`workers`; see [Triage](triage.md#a-worker-that-ended-its-turn)):
  each running Claude Code worker has bullpen's Stop hook in its worktree's
  `.claude/settings.local.json`. A worker started before workers had one isn't a
  problem: the line counts it, and it gets one when it next starts. `--` with no
  worker running.

`bullpen doctor --fix` first fixes what bullpen can by itself, and then runs the
checks. That is the Claude scratch folders the `temp` line counts, which it removes
(it works outside a repository too); and in this repository, the news hook (written
when news is `wake`, removed when it isn't) and a running worker's missing Stop hook
(written again). It says what it did ("fixed  temp: removed
132 Claude scratch folders of closed tasks (9.8 GB)", "fixed  news: removed bullpen's
news hook from .claude/settings.local.json"), on stderr with `--json`, and it
doesn't go with `--all-repos`.

`bullpen up` runs the quick checks (not `gh` or the push remote, which need the
network, nor `temp`, which sizes folders, nor the watcher, the coordinators and the news hook, which `up` sees to itself) and, if any fail, prints one
line: "2 problems: run `bullpen doctor`". Doctor itself ends with the failed checks'
names ("2 problems: keys, watcher."), and writes paths in your home folder with `~`.

## Every repo at once

`bullpen doctor --all-repos` checks every repository bullpen has state for (as
`ls --all-repos` does, it works from anywhere, and leaves out a repo whose checkout is
gone). The checks about the setup, which are the same for every repo (`rex`,
`bullpen`, `board`, `cargo`, `keys`, `watcher`, `state`, `temp`, `global`), run once, under
"Setup, for every repo". Then each repo gets its own (`gh`, `agents`, `config`,
`setup`, `teardown`, `coordinators`, `copy`, `risk`, `news`, `workers`, `push`) under its name and folder. A repo whose
board runs an older build shows up on its `coordinators` line, and one whose
`config.json` doesn't parse is named by its state folder. The last line names each
problem, a repo's as `repo/check`: "2 problems: keys, shop/coordinators." Two repos
of the same name are told apart by the folders above them: `a/shop/push`, `b/shop/push`.

Here the push remote is checked from local refs only, with no network: the remote's
base branch as this checkout last saw it (its last fetch, or bullpen's last push to
it) against the base. "backup is 1 commit behind main, as this checkout last saw it".
Plain `doctor` in a repo asks the remote itself.

## As JSON

`--json`, alone or with `--all-repos`, prints the same checks for a script or the
mission screen's health line. Each check is `{name, state, text}`, with `state` one of
`ok`, `warn`, `fail` or `skip` and the text as doctor prints it. Alone it is
`{repo, checks, problems}` (`repo` is null outside a repository); with `--all-repos`,
`{shared, repos: [{repo, name, checks}], problems}` (`repo` is null for a repo whose
`config.json` doesn't parse). `problems` lists the failed checks' names, as the last
line does; with `--all-repos` each is `{name, check, repo, text}`, `repo` being the
folder to run the fix in (null for a check about the setup, or a repo whose
`config.json` doesn't parse) and `text` what the check says. Either way doctor exits non-zero when anything fails.
