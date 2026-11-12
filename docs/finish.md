# Finish with nothing to merge

Some tasks leave nothing in the repository: a video, research notes, an answer.
Their worker ends with `bullpen finish BP-3 --summary-file <file>` instead of
`ready`. It refuses while the branch changes anything against its base (commits
that cancel each other out count as no change), and says to run `ready` instead.
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

A task goes back to `working` only once its worker has the message: review
findings, a rebase conflict or PR feedback. If typing it into the worker's pane
fails, it goes to the coordinator instead (triage, as "Review findings didn't
reach its worker", or the PR's reason), or for a rebase conflict stays where it
was, and news says so, with where the findings are, so it can `bullpen tell` it. `verdict` and
`ready` refuse an empty `--summary-file`: an empty file is a write that failed
(a full disk, `/tmp` out of inodes), not "no findings".
