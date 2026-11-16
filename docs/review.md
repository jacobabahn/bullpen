# Review

When a worker runs `ready`, bullpen pushes its branch, opens the PR and starts a
reviewer agent on it, in the background (`bullpen: BP-3 review`, in no tab; `go BP-3
--review` or the board shows it, and `bullpen config --reviewers` puts reviewers in
capsules or tabs instead: see [Where panes live](settings.md#where-panes-live)). The reviewer
reads the brief and the diff, runs the tests that check a claim (never a whole
suite: the worker's summary should say it passed), changes nothing, and ends with
`bullpen verdict BP-3 approve` or `changes`, run from the worktree. Its own files go
in a scratch folder bullpen makes for it (`scratch/BP-3-review` in the repo's state
folder) and removes at the verdict or when the reviewer closes. Changes go back to the worker,
which fixes them and runs `ready` again, for up to 2 rounds. The task is ready
to accept once review approves, or after the last round with the open findings
listed. `bullpen config --review off` skips review; `bullpen config --review-rounds 3`
sets the rounds, and `reviewer` in the repo's `config.json` the reviewer's command.

The reviewer's verdict file has two parts: `## Findings`, what must change (or, for
an approval, what it checked, with a `## Risk` line: see [The risk
label](#the-risk-label)), and an optional `## Notes`, things it saw that aren't
worth another round (a small defect outside the brief, a stale rule, a follow-up
worth a task). Notes go to the coordinator, not the worker: the ready and
auto-accepted news say "approved by review with 2 notes", and `bullpen show BP-3`
prints them. When a round asks for changes, the worker is told its notes aren't for
it, and the next round's reviewer gets the findings without them.

Review costs only what it has to:

- **The first round** is a full review on the reviewer model (`--reviewer-model`):
  the brief, the whole diff, the tests. A small change gets a **light review**
  instead: one round on the follow-up model, checking that it's correct and matches
  the brief. Small means docs only (every changed file under `docs/` or ending in
  `.md`), or at most `light_review_lines` changed lines (40 by default,
  `config --light-review-lines`) in at most 2 files. A change that touches
  `prompts/` or `.bullpen/rules.md` is never small: those are what agents are told
  to do, a wrong one changes behaviour, so it gets a full review however few lines
  it is ("full review (changes a prompt)", "full review (changes the rules)").
  `bullpen add --review full|light|auto` or `bullpen review-mode BP-3
  full|light|auto` overrides that for one task, a light one for a prompt too; auto,
  the default, goes by what the change touches and its size.
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
  worker, and the next `ready` is a follow-up round.
- **A review after a rebase.** When the last round approved and the branch has
  been rebased since (its base moved), but the worker changed more than a rebase
  check covers (own lines changed outside the conflicting files, or a clean rebase
  the worker then fixed up), the round is a **review after a rebase**: a
  follow-up round with no findings to check, so it asks only whether what changed
  since the approval just fits the change onto the new base, and whether that is
  correct. It counts as a round. Without a rebase, a change after an approval
  gets a follow-up review.

A reviewer is pointed at the task's brief as it is now. When the brief changed
(`bullpen brief`, docs/commands.md) since the round this one follows, its prompt says so
under "What was asked", with the files that say what changed. A change during a review
stops that review, and the round isn't counted.

A reviewer's brief is headed with its round and kind, `# BP-3 review, round 2 of 2
(follow-up review)`; a rebase check or a tour's review, which don't count as rounds,
and a round past the limit leave out the "of 2".

The reviewer's brief, `show` and the ready news say which kind of review it was and
why ("approved by light review: 12 lines in 1 file", "full review (changes a
prompt)"); the board says the kind.

## The risk label

Every task that is ready with a change has a risk label: low, medium or high, and
one line of why (`risk: high · changes how accept merges`), so you can see at a
glance which ones need a careful look. It only labels: it changes nothing about what
auto-accepts. It has two parts, and the higher wins (on a tie, the reviewer's why):

- **bullpen's rules**, from the diff, which the reviewer can't lower. A path in the
  repository's `risk_paths` is high, and so is a new dependency in a `Cargo.toml` (a
  name added under a dependencies table; a changed version isn't one). Docs alone
  (under `docs/`, or a `.md` outside `prompts/`), tests alone (under `tests/`, or a
  `test_` file), or both, are low. A prompt (under `prompts/`) is at least medium.
  Other code goes by its size, docs and tests left out: at most
  `light_review_lines` lines in at most 2 files is low ("a small change (12 lines
  in 1 file)"), over 400 lines is high ("a large change"), and anything between is
  medium. The rules read the repository's settings, never the branch's files.
- **the reviewer's judgement**, given with its verdict as a `## Risk` part of its
  findings (`medium: changes when the queue starts a task`), for what rules can't
  see: a subtle change in behaviour, something it couldn't run or check, a claim in
  the summary it couldn't confirm. It comes from the round that approved; a round
  that gives none, and a rebase check, keep the last round's. A light review that was
  set for the task (`review-mode light`) on a change that calls for a full one
  makes the label at least medium ("a light review where its size calls for a full
  one", "a light review, though it changes a prompt"): it looked at less than the
  change needs.

It follows the code. Each step that makes a task ready works the rules out again:
an approval kept after a rebase that only moved the base says the same (the same
lines), and keeps the reviewer's part; a change after the approval gets a new review
round, whose approval labels it afresh. With review off, or after the last round
with findings open, the rules alone label it.

`risk_paths` is a repository's list of sensitive paths, set in its `config.json` by
hand, as `{"pattern": "why"}` (or a list of patterns, each "changes <pattern>"). A
pattern ending in `/` is a folder, `*` is any run of characters, and anything else
is a file (or a folder of that name). For bullpen itself:

```json
"risk_paths": {
  "cli/src/accept/": "changes how accept merges",
  "cli/src/auto_accept.rs": "changes what auto-accepts",
  "cli/src/modes.rs": "changes how bullpen merges and pushes",
  "cli/src/push_remote.rs": "changes how bullpen pushes",
  "cli/src/close.rs": "changes how a task's worktree and branch are removed",
  "cli/src/keep.rs": "changes how a kept branch is pushed",
  "cli/src/hooks.rs": "changes how worktrees are removed",
  "cli/src/build.rs": "changes the build and self-update",
  "cli/src/main.rs": "changes the build's catch-up",
  "install.sh": "changes the install",
  "bin/": "changes what everything runs",
  "Cargo.lock": "changes the locked dependencies"
}
```

Where it shows: the end of the ready news (`BP-3 is ready: ... (1 commit, PR #12,
approved by full review); risk: high · changes how accept merges`), `show`'s `risk:`
line (with both parts), the end of a ready task's sentence on the board, in `ls` and
in mission control ("High risk: changes how accept merges."), a coloured word before
its age on the same rows (`low` green, `medium` amber, `high` red), the `ready`
entry of `stats`' timeline, and `risk` in the task's record (`ls --json`, `show
--json`, and the timeline's entry in `stats --json`).

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
