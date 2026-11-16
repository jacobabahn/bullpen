# Finish with nothing to merge

Some tasks leave nothing in the repository: a video, research notes, an answer.
Their worker ends with `bullpen finish BP-3 --summary-file <file>` instead of
`ready`. It refuses while the branch changes anything against its base (commits
that cancel each other out count as no change), and names both ways out: `ready`,
or `finish --keep-branch` when the brief asked for the branch to be kept.
Otherwise the task is `finished`, with the summary kept as `ready` keeps it: no PR,
no review. The news says `BP-3 finished (nothing to merge): <first line of the
summary>`, and the board lists it under "Finished" with that line.

You close it with `a` on the board or `bullpen accept BP-3` (`bullpen close BP-3`
does the same): its worktree, branch
and worker pane go, with no warning about losing work (there is nothing to merge),
and it counts as done, so tasks added `--after` it start. With auto-accept on (or
the task's `--auto-accept`), a finished task closes by itself, with a news line.
A finished task whose worker starts working again goes back to working and has to
run `finish` (or `ready`) again. Every worker's instructions say when to use which.

## End a task and keep its branch

Some work stays on a branch of its own and never merges: an experiment, a port tried
out. `bullpen close BP-3 --keep-branch` (the person's call, like accept, close and
drop; workers and coordinators don't run it unasked) ends any open task that way:

- It refuses uncommitted changes in the worktree (commit them, and they're kept), a
  task with no branch, or a branch with no commits on its base (`drop` it).
- It pushes the branch first: to `origin` in remote mode, else to the push remote
  (`config --push-remote`), as `--as <name>` if given (BP-95 went up as `rust`).
  Never forced: if the push fails, or the remote has a branch by that name with
  other commits, it refuses and nothing changes, so the work can't be lost. With
  no remote it keeps the local branch and says so.
- Then the close step, keeping the local branch (renamed to `--as`'s name): the
  reviewer, the PR (closed; its branch on origin stays when it's the one kept), the
  worktree and the pane go. The task is `done`, with `kept: {"branch", "remote"}`.
- `ls --all`, `show` and the board's closed list say "Closed, branch kept: rust on
  github." (or "branch kept here: ..., not pushed."), and the news "BP-3 closed
  without merging. ...". It doesn't read as dropped.
- Tasks added `--after` it stay queued, as after a drop (its work isn't on the base
  branch), with the same news line.

`close` on an unfinished task points to it.

### The worker's side: `finish --keep-branch`

A worker whose brief asks for its branch to be kept and not merged ends with
`bullpen finish BP-3 --keep-branch --summary-file <file>` (its instructions say so).
It refuses uncommitted changes (commit them and run it again) and a branch with no
commits on its base (plain `finish` is for that). Otherwise the task is `finished` with `keep_branch: true`: no
PR, no review, nothing pushed yet, and the worktree and pane stay.

- The news says `BP-3 finished (branch to be kept, not merged): <first line of the
  summary>`; `ls`, `show` and the board say "Finished, branch to be kept: ...", and
  `show` names `bullpen close BP-3 --keep-branch` as the next step.
- The close is yours. `bullpen close BP-3 --keep-branch [--as NAME]` ends it as
  above; so do plain `bullpen close BP-3`, `bullpen accept BP-3` and `a` on the board
  (which asks "Close it and keep its branch?"): on a task finished this way they
  keep the branch too, never remove it. `drop` still discards it.
- Auto-accept never closes it: the close pushes the branch, which is your call.
- If its worker runs `ready` afterwards, the task is to merge again.

## Sent back

A task goes back to `working` only once its worker has the message: review
findings, a rebase conflict or PR feedback. If typing it into the worker's pane
fails, it goes to the coordinator instead (triage, as "Review findings didn't
reach its worker", or the PR's reason), or for a rebase conflict stays where it
was, and news says so, with where the findings are, so it can `bullpen tell` it. `verdict` and
`ready` refuse an empty `--summary-file`: an empty file is a write that failed
(a full disk, `/tmp` out of inodes), not "no findings".
