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
the reason as news. `bullpen add` or `start` with `--no-auto-accept` keeps one task
for you while it's on (and `--auto-accept` auto-accepts one task while it's off).
The board shows `· auto-accept` beside the repo's name while it's on, and a task
held back says "Kept for you: no auto-accept."
