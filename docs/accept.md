# Accept

## After accept

Accepting a task moves the base branch, so bullpen brings the other open tasks
on that base up to date, and they don't drift behind and conflict later:

- Working: told only when the accepted change meets its work. bullpen compares
  the files the accepted change touched with the files the task changed
  (committed on its branch, uncommitted, or untracked). No overlap: left alone.
  Overlap: a trial merge (`git merge-tree`, which writes nothing) tells whether its
  committed work conflicts. A conflict, or uncommitted changes in a file the
  accepted change touched, and its worker is told the base moved, which task landed
  (its ID and title), which files, and to rebase onto it at a good point (after
  `git fetch origin`, with a remote), keeping both where both sides changed the
  same place. With uncommitted changes it's told to commit them first, and nothing
  about conflicts: there are none yet. Otherwise it's left alone: nothing needs a working task to be
  on top of its base (`accept` rebases when it merges). Its worktree isn't touched
  under it.
- In review, or with its reviewer waiting on you: left alone (`BP-5 left in review`),
  and noted on the task. When that review approves, bullpen checks the branch
  against its base first (after `git fetch origin`, with a remote), with the same
  trial merge as `show`'s `rebase:` line. If it conflicts, it gets what a ready task
  that conflicts gets here: bullpen keeps both sides of a trivial conflict, or sends
  it back to its worker to rebase, keep both sides, run the full suite again and run
  `ready` again. The news is the same words, with no ready news: `After BP-4: BP-5
  conflicts in bin/bullpen, sent back.` (`After BP-4 and BP-6: ...` when two accepts
  went by during one review). Its next `ready` treats the approval as after any rebase
  ([Review](review.md)): kept, a rebase check of the resolution, or a review after a
  rebase. If it doesn't conflict, nothing is done to it: it's ready on the old base,
  not rebased (bullpen rebases only for a conflict). Findings leave it to its worker,
  as always. With auto-accept on, a task sent back here isn't tried, so it isn't one
  of the auto-accept's trips back (below).
- Waiting on you or the coordinator mid-turn (a permission prompt, a failure): not
  rebased, and the news says so.
- Between turns (needs you, triage, stopped, ready): bullpen rebases its branch itself,
  if its worktree is clean, and pushes it to the PR (`--force-with-lease`). The
  status stays; a ready task says "Rebased onto main." on the board until its
  next `ready`. An approved task keeps its approval when the rebase left the task's
  own lines as they were (only the base moved, see [Review](review.md)); if they
  changed, a reviewer checks it again (a follow-up round). If the rebase conflicts
  only where both sides added lines at the same place, in a doc (under `docs/` or
  `prompts/`, or a `.md` file) or a test file (under `tests/`, or `test_*`),
  bullpen keeps both sides, the base's lines first, and goes on. Any other conflict
  undoes the rebase, the branch stays as it was, and the task goes back to its
  worker with the conflicting files (resumed if its pane is gone). Uncommitted
  changes: skipped.

One news line sums it up: `After BP-4: BP-5 told to rebase (conflicts in
bin/bullpen); BP-7 left alone (no overlap); BP-10 left alone (no conflict); BP-8 rebased; BP-6
rebased, keeping both sides of a conflict in README.md; its approval carried over
(only the base moved); BP-9 conflicts in bin/bullpen, sent back.` `bullpen config
--rebase-others off` turns this off.

## Overlap warning

Before anything is accepted, bullpen says when two open tasks (working, ready, in
review, waiting on you) change the same file, so you know early that the second to
be accepted will need a rebase. It only says so: nothing waits on it, no worker is
told, and no order is forced.

It checks when a worker ends a turn and on `ready`, for that task against the other
open tasks on the same base: the files each branch changed since it left its base
(its commits; uncommitted changes don't count). A new overlap is one news line, once
per pair of tasks: `BP-122 and BP-123 both changed cli/src/accept/mod.rs (and 2
more)`. When more files join later, one more line says those (`... both changed
README.md too`); a check that finds nothing new says nothing. `bullpen show BP-122`
lists the tasks it overlaps with and the files (`overlap: BP-123 changed
cli/src/accept/mod.rs, ... too`). A task accepted, closed, dropped or finished drops
out of its pairs.

Files every task may change harmlessly never count: lock files (`Cargo.lock`,
`package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, `poetry.lock`, `uv.lock`) and
`CHANGELOG.md`, by name in any folder (the list is `IGNORED` in `cli/src/overlap.rs`).
The pairs said so far are kept in `overlaps.json` beside the tasks.

Overlap isn't conflict: two tasks can change different parts of one file and merge
cleanly. When the first is accepted, [After accept](#after-accept) works out whether
the other really conflicts.

## The steps after the merge

Once the task is merged and saved as done, `accept` (and auto-accept) goes on in this
order: the news line, the merge's follow-up (the branch deleted on origin, or the push
to the push remote in local mode, after which a task made from a GitHub issue closes
the issue: [What's written back](issues.md#whats-written-back)), bullpen and the board rebuilt (in bullpen's own
repository: docs/rebuilds.md), the other tasks rebased, the run queue, telling the
coordinators their guide changed, the watcher refreshed, and the news that the keys
changed. Each step runs whatever
another did: one that fails is said on stderr and in a news line, `BP-4 was accepted,
but the run queue failed: ...`, and the rest still run.

In local mode, accepting a task in bullpen's own repository merges into the checkout
bullpen runs from, so the merge replaces the source under the accept that's running,
and its rebuild may put a new `target/bullpen` in place before the accept is done.
The accept is one compiled program, and the new build is renamed into place (the
running one keeps its old file), so it runs its own code to the end and never mixes old
and new. What it starts after the merge runs the new code: the background push, the
watcher's calls, a rebuilt board, the next call.

What an accept starts in the background (the push, the rebuild, an auto-accept, a
tour, ...) leaves the command's process tree at once (a double fork), so it outlives
the command and its pane. An auto-accept runs in the reviewer's pane and closes it
last, and Claude Code kills the whole tree of the command it ran as it ends, new
sessions too: a job still the command's child died with it (BP-118). A job's output
goes to `jobs.log` in the repo's state folder, and a job that fails without saying so
itself (killed, or a traceback) is one news line: `Pushing main to backup failed:
... Log: .../jobs.log`. Jobs often run at once (an accept's push and its rebuild), all
appending to that log, so bullpen writes each of its lines in one go (`cli/src/lines.rs`):
one job's line never lands in the middle of another's.

A verdict's auto-accept is a job too (`_auto_accept`, its output in `jobs.log`), but the
verdict waits for it: the accept closes the worker's pane, not the reviewer's, and the
verdict closes the reviewer's only once the accept is done. An approval carried over at
`ready` starts it in the background, since that accept closes the pane `ready` runs in,
and so does a worker's turn that leaves a ready task's code as approved.

## Auto-accept

Off by default: you accept each task yourself. Turn it on with `bullpen config
--auto-accept on` when review is the check you trust, so approved work lands
without waiting for you, and the next queued tasks start sooner. Once the reviewer
approves, bullpen runs the normal accept itself (same merge, cleanup, rebase of the
other tasks and start of queued ones), and the news says so: `BP-3 approved by
review and auto-accepted (merged into main, rebase)`. A finished task, with nothing
to merge, closes as soon as its worker runs `finish`.

It never accepts a task review didn't approve: one ready after the last round with
findings open, or any task while review is off. With a PR, it waits for the PR's
checks: still running, the task stays ready and accepts once they pass; failing, or
new comments from people, it isn't accepted and the news says why. A merge that is
refused (a conflict, an uncommitted change in the way) leaves the task ready, with
the reason as news, except a conflict with its base (below). An approval kept without
a new review auto-accepts too: at `ready` (the same commit, or only the base moved),
and when a ready task's worker takes a turn that leaves the code as approved (you
typed to it, say). `bullpen add` or `start` with `--no-auto-accept` keeps one task
for you while it's on (and `--auto-accept` auto-accepts one task while it's off). A
task made from a GitHub issue whose author isn't trusted never auto-accepts, whatever
these say (see [GitHub issues](issues.md#a-task-from-an-issue)).
The board shows `· auto-accept` beside the repo's name while it's on, and a task
held back says "Kept for you: no auto-accept."

### Refused for a conflict

Another task may land first, while this one is in review, and then its branch no
longer rebases onto its base. When an auto-accept is refused, bullpen tries a
rebase in the task's worktree (after `git fetch origin`, with a remote) and undoes
it. If it conflicts, the task goes back to its worker, as `rebase-others` sends
one back: the files it conflicts in, and to rebase onto its base (keeping both
where both sides changed the same place), run the full suite again, commit and
run `ready` again (resumed in a new pane if
its pane is gone). The conflicts are noted on its review, so that `ready` gets the
usual follow-up: its approval carried over, a rebase check of the resolution, or a
review after a rebase ([Review](review.md)), then the auto-accept again. The board says
"Approved, rebasing onto main: its branch conflicted.", and the news:
`BP-3 approved by review, but the auto-accept was refused: it conflicts with main
in README.md. Sent back to its worker to rebase (trip 1 of 3)`.

After 3 trips back for a conflict, bullpen stops sending it and leaves the task
ready for you: `... it still conflicts with main in README.md after 3 trips back
to rebase. Left for you`. If the message doesn't reach its worker, the task goes
to its coordinator with news to `resume` or `tell` it. A refusal that isn't a
conflict (uncommitted changes, a check GitHub requires, ...) leaves the task ready
with the reason, as before; so does a refused `bullpen accept` of your own: it says
why, and sends nothing back (`bullpen tell` the worker to rebase if you want it to).

## Taking an accept back

`bullpen revert BP-3 [--why "..."]` takes an accepted task back. It reverts nothing
itself: it adds a task, `Revert BP-3: <its title>`, and starts it (if as many tasks
work as `max_running` allows, it stays todo and the output says so: `bullpen start`
it later). So the revert gets a branch, a review and an accept like any other change,
and with an `origin` a pull request. It prints the new task's ID. It is your call:
the coordinator runs it only when you ask.

bullpen writes the new task's brief (from `prompts/revert.md`). It names the commits
to revert, newest first, and tells the worker to revert them as one commit (`git
revert --no-commit <commits>`, then one `git commit`), to keep the later work where
the revert conflicts with it, to run the full suite, and to say in its summary
anything that couldn't be cleanly taken back: a later task that built on it, or a
change to settings or a state file that the older code can't read. `--why` goes into
the brief. One commit for the lot, not one per commit, because the revert is read,
accepted and (if it comes to that) taken back as one change, and a conflict with
later work is settled once.

The commits come from the task's record, never from commit messages. An accept
records `merged` (the base's head after the merge; with a PR, the commit gh says the
merge left), `merged_how` (`config --merge` at the time), and `merged_from` (local
mode: the base's head before it) or `merged_commits` (a PR: how many commits gh says
it had when it was merged, which counts a commit pushed after the last `ready`);
`ready` records `commits` (how many the branch had then):

| Merged by | The commits |
| --- | --- |
| `rebase` | local mode: those from `merged_from` to `merged`. A PR: the last `merged_commits` ones up to `merged` (refused without that count). |
| `squash` | `merged`, the one commit. |
| `merge` | `merged`, the merge commit, reverted with `-m 1` (against the base as it was). |
| not recorded | A task accepted before `merged_how` was kept, or a PR merged on GitHub outside bullpen. A `merged` commit with two parents is a merge commit; a branch of one commit (`merged_commits`, else `commits`) left one commit. Anything else is refused: a rebase and a squash of several commits leave different commits, and the record can't tell them apart. |

A task with a PR and no `merged` on record (accepted before it was kept) asks gh for
the merge commit, and how many commits the PR had, when you revert it.

`revert` refuses, saying why: a task that wasn't accepted (open, dropped, closed
with its branch kept, finished with nothing to merge); one whose commits are no
longer in its base's history (the base is fetched first, with an `origin`); one whose
record can't give the commits; one that has an open revert task; and one whose revert
was accepted (`bullpen revert` that revert task brings the change back, and after
that the first task can be reverted again).

The new task has `reverts: BP-3` on its record. `bullpen show` says it on both:
`reverts: BP-3` on the revert task, and on BP-3 `a revert is open: BP-9` until the
revert is accepted, then `reverted by BP-9`. A revert task that is dropped counts
for nothing.
