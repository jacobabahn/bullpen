# Worktree setup

A new worktree has no dependencies installed, no `.env` and no build cache. A repo
can say what to run in it: `.bullpen/setup.sh`, committed (read as committed on the
task's base, like the rules), or a command of your own, `bullpen config --setup
"npm ci"` (`--setup ""` clears it). With both, yours runs, and `start` says so.
`start` runs it in the new worktree, after making it and before starting the worker,
with these in its environment:

- `BULLPEN_TASK`: the task ID
- `BULLPEN_WORKTREE`: the worktree (also the current folder)
- `BULLPEN_MAIN`: your main checkout, to copy `.env` from, say
- `BULLPEN_BRANCH`: the task's branch

Its output goes to `<ID>.setup.log` beside the task's brief; `bullpen show` names
it. If it fails, the worker doesn't start: the task goes to triage (to you if no
coordinator is running), its reason and the news line naming the log. Fix what it
needs and `bullpen start` (or `resume`) the task: that sets it up again and starts
the worker. A worktree that was set up is never set up again, not by `resume` either.

`.bullpen/teardown.sh` (or `bullpen config --teardown "..."`) runs the same way just
before `accept`, `close` or `drop` removes a worktree, its output in
`<ID>.teardown.log`. A failure is said and logged, but the worktree goes anyway.
