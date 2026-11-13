# Accept

## After accept

Accepting a task moves the base branch, so bullpen brings the other open tasks
on that base up to date, and they don't drift behind and conflict later:

- Working: told only when the accepted change meets its work. bullpen compares
  the files the accepted change touched with the files the task changed
  (committed on its branch, uncommitted, or untracked). No overlap: left alone.
  Overlap: a trial merge (`git merge-tree`, which writes nothing) tells whether its
  committed work conflicts. A conflict, or uncommitted changes in a file the
  accepted change touched, and its worker is told the base moved, which files, and
  to rebase onto it at a good point (after `git fetch origin`, with a remote) and
  fix the conflicts. Otherwise it's left alone: nothing needs a working task to be
  on top of its base (`accept` rebases when it merges). Its worktree isn't touched
  under it.
- In review, or with its reviewer waiting on you: left alone. Its next `ready`
  brings it up to date.
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

## The steps after the merge

Once the task is merged and saved as done, `accept` (and auto-accept) goes on in this
order: the news line, the merge's follow-up (the branch deleted on origin, or the push
to the push remote in local mode), bullpen and the board rebuilt (in bullpen's own
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
... Log: .../jobs.log`.

A verdict's auto-accept is a job too (`_auto_accept`, its output in `jobs.log`), but the
verdict waits for it: the accept closes the worker's pane, not the reviewer's, and the
verdict closes the reviewer's only once the accept is done. An approval carried over at
`ready` starts it in the background, since that accept closes the pane `ready` runs in.

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
the reason as news, except a conflict with its base (below). `bullpen add` or `start` with `--no-auto-accept` keeps one task
for you while it's on (and `--auto-accept` auto-accepts one task while it's off).
The board shows `· auto-accept` beside the repo's name while it's on, and a task
held back says "Kept for you: no auto-accept."

### Refused for a conflict

Another task may land first, while this one is in review, and then its branch no
longer rebases onto its base. When an auto-accept is refused, bullpen tries a
rebase in the task's worktree (after `git fetch origin`, with a remote) and undoes
it. If it conflicts, the task goes back to its worker, as `rebase-others` sends
one back: the files it conflicts in, and to rebase onto its base, fix the
conflicts, rerun the tests, commit and run `ready` again (resumed in a new pane if
its pane is gone). The conflicts are noted on its review, so that `ready` gets the
usual follow-up: its approval carried over, a rebase check of the resolution, or a
follow-up review ([Review](review.md)), then the auto-accept again. The board says
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
