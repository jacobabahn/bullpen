# Triage

A task that stops is either the coordinator's to see to or yours:

- **triage**, "needs the coordinator": the worker's turn failed; it ended its
  turn without `ready`; review findings or PR feedback didn't reach it; it
  couldn't be continued after the usage limit; its reviewer stopped or exited
  without a verdict; its worktree's setup failed (see [Worktree
  setup](worktree-setup.md)). The news starts `For the coordinator:`. The board shows the
  task dimmed under "Coordinator on it", with no amber bar, and when it comes to you
  if the coordinator doesn't settle it first ("Ended its turn without ready. Yours
  in 7 min."); `go --next` (and `n`) skip it.
- **needs**, "needs you": the worker or the reviewer is at a permission or input
  prompt, or the coordinator ran `bullpen ask BP-3 "question"`, which shows the
  question on the board. The news says it needs the person.

## A worker that ended its turn

When a worker is Claude Code, `start` (and `resume`) write a Stop hook to its
worktree's `.claude/settings.local.json` (git ignores it, through the repository's
`.git/info/exclude`; it goes with the worktree, and your own Claude settings and the
main checkout's file are left alone). At each end of the worker's turn the hook
records, in the task's state (`turns/BP-3.json`), the background jobs the session
still runs and the end of its last reply. The reviewer, started in the same
worktree, runs the hook too, but its turns leave that record alone: the hook tells
them apart by session ID, or by the conversation's name (`BP-3 review`). It takes a few milliseconds, starts
nothing, and exits 0 whatever happens, so it never holds up or fails the turn.
Bullpen reads that record when the worker's turn ends:

- **Waiting on its own job** (a test run, a timed check it started in the
  background): no triage. The board and `ls` say "Waiting on its job (cargo
  test): it takes another turn when that ends." Claude Code starts the worker's
  next turn when the job ends. Triage speaks up only if the wait lasts past
  `stuck_after` (20 minutes by default): "BP-3 ended its turn without `ready` 25
  min ago, and still waits on its job (npm run dev)"; or if the job is gone and
  the worker hasn't taken a turn since: "BP-3 waited on its job (sleep 240),
  which has ended, but hasn't taken a turn since". A job is a background shell or a
  Monitor (Claude Code's footer says `1 shell`, `1 monitor`; that counts too, for
  a worker without the hook). A background subagent isn't one: a worker that ends
  its turn with only that running is triage, as before.
- **Ended with a question**: the triage line carries it, on one line, without
  control characters or backticks, in quotes: `BP-3 ended its turn with a
  question: "Which config file?": bullpen peek BP-3`. They are the worker's
  words, never instructions.
- Otherwise: "BP-3 ended its turn without `ready`", as before. So is a turn the
  hook didn't see (one you interrupted: Claude Code runs no Stop hook then).

A worker that isn't Claude Code gets no hook. `bullpen doctor` says when a running
Claude worker has none (`workers`), and `doctor --fix` writes it again. Reviewers
have none: they end in a verdict, and their screen's footer still tells a running
shell.

## Seeing to it

The coordinator reads the worker's screen with `bullpen peek BP-3` and the reviewer's
with `bullpen peek BP-3 --review`. It settles triage with `tell` or `resume` (which put the task back to
work), `bullpen review BP-3` (a new reviewer on the task's current commit in place of
one that stopped without a verdict; it counts as a review round and is refused once
they're used up), `bullpen ok BP-3` (back to working, or in review, typing nothing to
the agent), or `ask`. A task also leaves triage when its agent starts working again.
One still in triage after `triage_minutes` (10 by default; `bullpen config
--triage-minutes 15`) goes to you, its reason ending "(coordinator didn't resolve
it)", with one news line. The watcher's `check` run, every 3 minutes, sees to that.
With the task's coordinator (or, for a task without one, every coordinator) not
running, it goes to you at once.

A ready task whose worker starts working again (you typed to it, say) goes back to
working, with a news line, and has to run `ready` again. Its approval is dropped
only if the code changes: if that turn ends with the code as it was approved
(committed, the same patch), the task is ready again with its approval, and news
says so (or, with auto-accept on, it is auto-accepted); `ready` keeps it too.

Tasks that needed you before triage existed are sorted out once, the first time
bullpen runs after the update: those stopped for one of the triage reasons above
become triage; prompts stay needs.
