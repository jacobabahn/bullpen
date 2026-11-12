# Glossary

Words bullpen's code and docs use. The code for a task's lifecycle is in the
"task lifecycle" section of `bin/bullpen`.

## Where things run

**Coordinator**: the agent you talk to. It turns what you ask into tasks, starts their workers and sees to their stops. A repository can have several, `c1`, `c2`, ...

**Worker**: the agent doing one task, in its own pane, worktree and branch. A **reviewer** is the agent that checks it after `ready`.

**Pane**: where an agent or the board runs: a Rex pane, labelled `bullpen: <ID>` for a worker and `bullpen: <ID> review` for its reviewer.
_Avoid_: window, block (Rex's API word for it).

**Capsule**: a worker's or reviewer's pane minimized, shown at the bottom right of the coordinator's tab. `bullpen go` brings it up; `bullpen hide` (ctrl+escape) puts it back.

**Board**: the live task list beside the coordinator (`bullpen board`). It groups tasks by what you do next; `ls` uses its words for each status.

**News**: lines starting `bullpen · ` that tell a coordinator what its tasks did: typed into its pane, or followed with `bullpen news --follow`. A **note** (`bullpen note`) is a line on the board, not news.

## A task's lifecycle

**Task status**: where a task is: `todo`, `working`, `review`, `ready`, `finished`, `triage`, `needs`, `stopped`, `done`, `dropped`. It changes only through `set_status`. The board and `ls` show it in words: queued (`todo`), working, in review, ready to accept, finished, coordinator on it (`triage`), needs you (`needs`, `stopped`), closed (`done`, `dropped`).

**Stop**: why a task waits on someone, recorded as `{"by": "worker" | "reviewer", "kind": ...}`: at a prompt, failed, ended its turn without `ready`, exited, its pane gone after a Rex restart, a message that didn't reach the worker, not continued after the usage limit, or its worktree's setup failed (so no worker started). Only a task in `triage`, `needs` or `stopped` has one. The task's `why` says the same thing in words, for people and the board; code never goes by its wording.
_Avoid_: reason (for the recorded stop), needs (for any stop).

**Triage**: a stop the task's coordinator can see to; "coordinator on it" on the board. It becomes **needs** if no coordinator is running, or once `triage_minutes` pass (escalation).

**Needs**: a task that needs the person ("needs you" on the board, as is a stopped one): an agent at a prompt, an `ask`, or an escalated triage.

**Ask**: the coordinator puts a question to the person about a task (`bullpen ask`). The question becomes the task's `why`; its stop stays as it was. The person's **reply** (`bullpen reply`, the board's `i`) goes to the coordinator, and the task goes back to the status it had when asked, or, if it was stopped, leaves needs as with ok.

**Ok**: the coordinator has seen to a triage stop (`bullpen ok`). The task goes back to working, or to review for a reviewer's stop, and its idle agent isn't a stop again until its next turn.

**Hand back**: give the worker a message to act on (review findings, a rebase conflict, PR feedback, carrying on after the usage limit), typing it into its pane or resuming it in a new one. The task is working again once the message reaches the worker.

## Reviewing a change

**Tour**: a guided walk through a ready (or in-review) task's change, written on request by a cheap headless agent (`bullpen tour`): its steps in reading order, tied to the commit it was written on. It is **current** while the patch is the same, else **stale**. The person's comments on it go back to the worker with `tour --send`.
_Avoid_: walkthrough, guide (the coordinator's guide is `bullpen guide`).

**Step**: one logical change in a tour: a title, a note on why it changed and what to look at, and the hunks it covers (file and line ranges), shown from git. A last step, "Other changes", holds any hunk no other step covers. A comment is on one step.
