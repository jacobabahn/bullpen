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

The coordinator settles triage with `tell` or `resume` (which put the task back to
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
says so; `ready` keeps it too.

Tasks that needed you before triage existed are sorted out once, the first time
bullpen runs after the update: those stopped for one of the triage reasons above
become triage; prompts stay needs.
