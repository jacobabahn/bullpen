# Commands

`bullpen --help` lists the commands in four groups: yours, the coordinator's,
the worker's and reviewer's, and setup. `bullpen COMMAND --help` says more about
one. A command bullpen doesn't have gets one line, with the closest one if there
is one: ``bullpen: no command `acept`. Did you mean `accept`? `bullpen --help`
lists them.``

To drive it by hand, in place of telling the coordinator:

```sh
bullpen add "Fix login redirect" --brief-file brief.md   # prints BP-1
bullpen add "Cover logout" --brief-file b2.md --after BP-1 # BP-2 starts once BP-1 is accepted
bullpen after BP-2 --none   # change (or clear) what a task waits on
bullpen model BP-2 sonnet   # the worker model it starts and resumes with (--clear: the config's)
bullpen review-mode BP-2 full  # its first review: full, light or auto (by the size of the change)
bullpen start BP-1    # worktree + branch + a Rex pane running the worker
bullpen ls            # open tasks as the board says them, in its order (--json: the raw status)
bullpen ls --all-repos  # the same for every repository bullpen knows, by repository
bullpen repos         # every repository, a line each: its coordinators (alive or gone, working or idle),
                      #   what needs you, what's ready, what's running (--json: the whole snapshot)
bullpen peek BP-1     # the end of the worker's screen (`--review`: its reviewer's)
bullpen prompt BP-1   # the prompt its worker (or reviewer) is at, and its screen, as JSON
bullpen answer BP-1 2 # press option 2 of that prompt (or y, n), as you would in its pane
bullpen reply BP-1 "Postgres"   # answer the coordinator's `ask`: it goes to the coordinator
bullpen say "Start BP-4 next"   # type a message to the coordinator (--repo PATH, --coordinator c2)
bullpen diff BP-1     # its changes against its base, in a pager (--stat: just the files)
bullpen check         # tasks whose status is out of step with their agent; --fix catches up
bullpen go BP-1       # focus its pane (`go` alone: the coordinator; --next: next needing you;
                      #   --board: the board, started first if none runs)
bullpen go --next --all-repos   # the next task needing you in any repository
bullpen mission       # mission control: every repository on one screen, in its own Rex session (cmd+; m)
bullpen go BP-1 --repo ~/shop   # a task of another repository
bullpen go BP-1 --board         # its board, with BP-1 selected
bullpen news --recent --all-repos   # the last 20 news lines of every repository (N: the last N); moves no cursor
bullpen pick          # type part of any open task, in any repository, and go to it (the cmd+; j picker)
bullpen pick --filter "lgn"   # just print the open tasks matching, best first
bullpen tell BP-1 "Also cover the logout path"   # typed bare; from a coordinator's pane, after "Coordinator: "
bullpen brief BP-1   # print its brief; --add "..." adds a dated section, --file FILE replaces it (see below)
bullpen resume BP-1   # restart a stopped worker, continuing its conversation
bullpen ask BP-1 "..."   # (coordinator) BP-1 needs you: the question shows on the board
bullpen ok BP-1       # (coordinator) a task in triage is seen to: back to work, nothing typed
bullpen review BP-1   # (coordinator) a new reviewer for one that stopped without a verdict
bullpen note BP-1 "..."   # an informational line on the board
bullpen accept BP-1   # merge its PR (or local branch; rebase), close its pane, remove its worktree;
                      # a finished task (nothing to merge): remove its worktree, branch and pane
bullpen close BP-1    # the same as accept, for a finished task only
bullpen close BP-1 --keep-branch --as port  # end it unmerged, its branch kept as port (see finish.md)
bullpen drop BP-1     # close its PR, pane and worktree without merging
bullpen build         # build bullpen's command line and the board now (see Rebuilds)
bullpen later "Tidy the errors"  # an idea for later, on the backlog (see backlog.md)
bullpen stats BP-1    # where its time went: its timeline and totals (no ID: across closed tasks; see stats.md)
```

A command that can't act on a task says its status in the board's words, with the
status itself after them when they differ: "BP-4 has no pane: it's queued (todo)",
"BP-2 needs you (stopped), not waiting on its coordinator".

## Changing a task's brief

`bullpen brief BP-1` prints a task's brief. `bullpen brief BP-1 --add "Also cover
logout."` adds that to its end as a dated section (`## Added 2026-11-16 09:30`), and
`bullpen brief BP-1 --file new.md` replaces it with the file's text. Use it in place of
editing the file in the state folder, or of a `tell` that changes what's wanted: a
`tell` reaches the worker, but the reviewer reads the brief.

- A task that hasn't started just gets the new brief.
- A started task's worker is told, as `tell` would: the added section, or the diff, typed
  into its pane, so it needn't read the whole brief again. A change of more than 40 lines
  is named by its file. The brief the worker reads (`briefs/<ID>.full.md`) is written
  again. A task waiting on its coordinator goes back to work, as with `tell`.
- A worker that isn't running, or is at a prompt, isn't typed to: the command prints the
  `resume` or `tell` to run for it.
- A task with a review under way (in review, or stopped on its reviewer, or asked about
  during its review): that review is of work on the brief as it was, so its reviewer is
  closed and the task goes back to its worker (resumed, if its pane is gone), which runs
  `ready` again. The stopped round isn't counted. If the message doesn't reach the
  worker, the task is its coordinator's (triage) and the command prints the `tell` to run.
- A ready, finished or closed task is refused: send it back first (`bullpen tell`), or
  add a follow-up task.

Each change is kept as `briefs/<ID>.brief-<n>.md` (the added section, or the diff) and
listed on the task (`brief_changes`); `bullpen show` says how often the brief changed. A
review always reads the brief as it is now, and a round that follows another is told
when the brief changed since that one, with the files that say what changed
(docs/review.md).

## More on some commands

`bullpen diff BP-1` shows `git diff <base>...HEAD` from the task's worktree, then
any changes not committed yet, in `delta` if it's installed, else `less -R`. The
base is `origin/<base>` when there's a remote, the local base branch when there
isn't. On the board, `v` opens the same view in a floating pane; `q` closes it and
takes you back to the board.

A worker finishes with `bullpen ready <ID> --summary-file <file>`, which refuses
uncommitted changes, pushes the branch and opens (or updates) the pull request, or,
when the task leaves nothing to merge, with `bullpen finish <ID> --summary-file <file>`
(`--keep-branch` when its branch is to be kept unmerged: see finish.md).

`bullpen tell` says who is typing. Run from a coordinator's own pane it types
`Coordinator: ` before the words; from anywhere else (your shell, the board, the
stepper) it types them bare, as yours. The worker's instructions say what the prefix
means, and that a line cut off or meant for another pane isn't an instruction.
