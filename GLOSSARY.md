# Glossary

Words bullpen's code and docs use. The code for a task's lifecycle is in
`core/src/task.rs` (`set_status`), and in `cli/src/stops.rs` for its stops.

## Where things run

**Coordinator**: the agent you talk to. It turns what you ask into tasks, starts their workers and sees to their stops. A repository can have several, `c1`, `c2`, ...

**Sees to**: the coordinators who see to a task: its own (the one that added it, or adopted it) while that one's pane is running, else every running coordinator, else nobody, and the task needs the person. They get its news and its triage stops, its panes go in their tab, and their boards show it. A coordinator whose pane closes is dropped, so a listed coordinator is a running one. `sees_to` in `cli/src/coordinators.rs` is the rule.

**Worker**: the agent doing one task, in its own pane, worktree and branch. A **reviewer** is the agent that checks it after `ready`.

**Pane**: where an agent or the board runs: a Rex pane, labelled `bullpen: <ID>` for a worker and `bullpen: <ID> review` for its reviewer.
_Avoid_: window, block (Rex's API word for it).

**Waits on**: the agent a task waits on: its reviewer while the task is in review or stopped on its reviewer (a usage-limit stop is seen to as its worker's), else its worker while it's working or waiting on its coordinator or the person, and no agent while it waits on the usage limit or isn't running (queued, stopped, ready, finished, closed). Its prompt is the one `answer` presses, its pane comes first in a notification, and `ok` and `reply` check it's running. `waits_on` in `cli/src/agents.rs` is the rule, and `pane` there says where a worker's or reviewer's pane is.

**Capsule**: a worker's or reviewer's pane minimized, shown at the bottom right of the coordinator's tab (or of its workers tab, with `--workers tab`). `bullpen go` brings it up; `bullpen hide` (ctrl+escape) puts it back.

**Board**: the live task list beside the coordinator (`bullpen board`). It groups tasks by what you do next; `ls` uses its words for each status.

**News**: lines starting `bullpen · ` that tell a coordinator what its tasks did: typed into its pane, or followed with `bullpen news --follow`. A **note** (`bullpen note`) is a line on the board, not news.

## A task's lifecycle

**Task status**: where a task is: `todo`, `working`, `review`, `ready`, `finished`, `triage`, `needs`, `stopped`, `done`, `dropped`. It changes only through `set_status`. The board and `ls` show it in words: queued (`todo`), working, in review, ready to accept, finished, coordinator on it (`triage`), needs you (`needs`, `stopped`), closed (`done`, `dropped`).

**Stop**: why a task waits on someone, recorded as `{"by": "worker" | "reviewer", "kind": ...}`: at a prompt, failed, ended its turn without `ready`, exited, its pane gone after a Rex restart, a message that didn't reach the worker, not continued after the usage limit, or its worktree's setup failed (so no worker started). Only a task in `triage`, `needs` or `stopped` has one. The task's `why` says the same thing in words, for people and the board; code never goes by its wording.
_Avoid_: reason (for the recorded stop), needs (for any stop).

**Triage**: a stop the coordinators who **see to** the task can settle; "coordinator on it" on the board. It becomes **needs** if none is running, or once `triage_minutes` pass (escalation).

**Needs**: a task that needs the person ("needs you" on the board, as is a stopped one): an agent at a prompt, an `ask`, or an escalated triage.

**Ask**: the coordinator puts a question to the person about a task (`bullpen ask`). The question becomes the task's `why`; its stop stays as it was. The person's **reply** (`bullpen reply`, the board's `i`) goes to the coordinator, and the task goes back to the status it had when asked, or, if it was stopped, leaves needs as with ok.

**Ok**: the coordinator has seen to a triage stop (`bullpen ok`). The task goes back to working, or to review for a reviewer's stop, and its idle agent isn't a stop again until its next turn.

**Hand back**: give the worker a message to act on (review findings, a rebase conflict, PR feedback, carrying on after the usage limit), typing it into its pane or resuming it in a new one. The task is working again once the message reaches the worker.

**Timeline**: what happened to a task, one entry per event (added, a change of status, a turn ended, a review started or finished, an auto-accept refused), written by its saves. `bullpen stats` adds it up into the time in each **phase** (working, rebasing, in review, waiting for the person, ...). `core/src/timeline.rs`; see `docs/stats.md`.

**Trip back**: the task sent back to work after `ready`: by a review, a rebase conflict, the person, or its PR's comments. Its reason is on the timeline.

**Run queue**: the `todo` tasks that wait on others (`--after`): bullpen starts each by itself once those are all accepted, while fewer than `max_running` tasks are working. A plain `todo` task waits for the coordinator to start it. A task leaving `working` (to review, ready or finished), or closing (accepted, closed or dropped), frees a slot, and the queue starts the next; a stop doesn't. It hears of this from the status change itself (`move_queue` in `cli/src/saves.rs`), so no caller has to remember it.

**Close step**: the one way a task ends, for accept, close, drop and auto-accept: its reviewer, its PR, its worktree and branch, its status, the news, and its worker's pane last. `close` in `cli/src/close.rs`.

**Kept branch**: a task ended with `close --keep-branch`: unmerged, its branch pushed (to origin, or the push remote) and kept. It is `done` with a `kept` record, and says "branch kept", never dropped; tasks after it stay queued. `cli/src/keep.rs`.

## Where work goes

**Remote mode**: a repository with an `origin` remote. `ready` pushes the task's branch and opens a pull request; `accept` merges it on GitHub.

**Local mode**: a repository with no `origin` remote. Nothing is pushed and there is no pull request; `accept` merges the branch into the local base branch (and backs that up to the push remote, if one is set). See `docs/local-mode.md`.

A task's mode is fixed when it is published (its first `ready`) and recorded on the task; every later step goes by that, so a task finishes the way it started even if an `origin` is added or removed meanwhile. The two adapters are in `cli/src/modes.rs`.
_Avoid_: PR mode (say remote mode).

## Reviewing a change

**Tour**: a guided walk through a ready (or in-review) task's change, written on request by a cheap headless agent (`bullpen tour`): its steps in reading order, tied to the commit it was written on. It is **current** while the patch is the same, else **stale**. The person's comments on it go back to the worker with `tour --send`.
_Avoid_: walkthrough, guide (the coordinator's guide is `bullpen guide`).

**Step**: one logical change in a tour: a title, a note on why it changed and what to look at, and the hunks it covers (file and line ranges), shown from git. A last step, "Other changes", holds any hunk no other step covers. A comment is on one step.

## Settings

**Setting**: one of bullpen's options, like `merge` or `max_running`, changed with `bullpen config`. Each has a row in `SETTINGS` in `core/src/settings.rs`: its default, its kind and its help. A repository's own value (its `config.json`) wins over the **global** one (`~/.config/bullpen/config.json`), which wins over the default. A **repo-only** setting (the prefix, the base branch, the news mode, the push remote, the coordinators) belongs to one repository: the global file can't hold it.
_Avoid_: option, preference.
