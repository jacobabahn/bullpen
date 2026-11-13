# Review

When a worker runs `ready`, bullpen pushes its branch, opens the PR and starts a
reviewer agent on it, in the background (`bullpen: BP-3 review`, in no tab; `go BP-3
--review` or the board shows it, and `bullpen config --reviewers` puts reviewers in
capsules or tabs instead: see [Where panes live](settings.md#where-panes-live)). The reviewer
reads the brief and the diff, runs the tests, changes nothing, and ends with
`bullpen verdict BP-3 approve` or `changes`. Changes go back to the worker,
which fixes them and runs `ready` again, for up to 2 rounds. The task is ready
to accept once review approves, or after the last round with the open findings
listed. `bullpen config --review off` skips review; `bullpen config --review-rounds 3`
sets the rounds, and `reviewer` in the repo's `config.json` the reviewer's command.

Review costs only what it has to:

- **The first round** is a full review on the reviewer model (`--reviewer-model`):
  the brief, the whole diff, the tests. A small change gets a **light review**
  instead: one round on the follow-up model, checking that it's correct and matches
  the brief. Small means docs only (every changed file under `docs/` or `prompts/`,
  or ending in `.md`), or at most `light_review_lines` changed lines (40 by default,
  `config --light-review-lines`) in at most 2 files. `bullpen add --review
  full|light|auto` or `bullpen review-mode BP-3 full|light|auto` overrides that
  for one task; auto, the default, goes by the size.
- **Later rounds follow up**: the reviewer gets the last round's findings and the
  diff since the commit it reviewed (a range-diff after a rebase), with the whole
  diff for context, and checks that each finding is fixed and that the fixes broke
  nothing nearby. They run on `reviewer_followup_model` (`config
  --reviewer-followup-model`, sonnet by default; `null` in `config.json` uses the
  reviewer model).
- **An approval is kept while the code doesn't change.** Each review records the
  commit it looked at, a patch-id of the task's diff against its base (`git diff
  base...HEAD | git patch-id --stable`), and the task's own lines: what it adds
  and removes in each file, without context lines or line numbers (`git diff -U0`).
  When the worker runs `ready` again on the same commit, on the same patch, or
  after a rebase with the same own lines (only the base moved: even if the base
  changed lines next to the task's, or a conflict was resolved by keeping both
  sides), the task is ready at once with its approval, and news says why: `BP-3 is
  ready again after a rebase: Fix login redirect (2 commits, PR #12, its approval
  carried over: only the base moved)`. `ready` run again while a reviewer is
  already on that very commit leaves it be.
- **A rebase check, after a conflict.** When a rebase conflicted (bullpen sent it
  back, or kept both sides itself) and the task's own lines changed only in the
  files that conflicted, a **rebase check** looks at the resolution: a short review
  on the follow-up model, given the conflicts as the rebase met them, those files'
  change now and the range-diff, asking whether the resolution is right and keeps
  both sides. It doesn't count as a round. Its approval says `a rebase check
  approved the conflict resolution`; if it asks for changes, they go back to the
  worker, and the next `ready` is a follow-up round. Own lines that changed
  anywhere else (the worker changed more than the resolution) get a follow-up
  review, as any change does.

The board, `show` and the ready news say which kind of review it was ("approved by
light review (12 lines in 1 file)").

A reviewer whose turn ends while a background shell of its own still runs (the tests,
say) is busy, not stopped, like a worker: it stays in review. Past `stuck_after`
minutes with the shell still running, `check` counts it as stopped without a verdict.

## After ready

While a task waits in review or for you, the watcher looks at its pull request
every 2 minutes (one `gh pr view` per PR; none when no task has one open):

- A check that fails: news (`BP-3's PR checks failed: test`), and the worker is
  told which checks failed, to fix them and run `ready` again. The task goes
  back to working, and a review still running is stopped (its round doesn't count).
- New comments, or changes requested, by people on GitHub (not bots, not
  approvals): the same, with the comment text.
- A PR merged or closed on GitHub: one news line saying so. Nothing else changes:
  `bullpen accept` (merged) or `drop` (closed) cleans up.

Each failure or comment is reported once. If the worker's pane is gone, the task
goes to the coordinator (triage) instead; if the worker is stopped mid-turn (a
permission prompt, a failure), the news says so and nothing is typed into it. The board and `show` say what GitHub last said ("checks
failing", "1 new comment"); while a task is working they say it only if that is what
sent it back ("Fixing what GitHub said."), since it's about the old push. In local mode there's no PR, so nothing is looked at.
