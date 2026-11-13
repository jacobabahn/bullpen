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
bullpen peek BP-1     # the end of the worker's screen
bullpen prompt BP-1   # the prompt its worker (or reviewer) is at, and its screen, as JSON
bullpen answer BP-1 2 # press option 2 of that prompt (or y, n), as you would in its pane
bullpen reply BP-1 "Postgres"   # answer the coordinator's `ask`: it goes to the coordinator
bullpen diff BP-1     # its changes against its base, in a pager (--stat: just the files)
bullpen check         # tasks whose status is out of step with their agent; --fix catches up
bullpen go BP-1       # focus its pane (`go` alone: the coordinator; --next: next needing you;
                      #   --board: the board, started first if none runs)
bullpen go --next --all-repos   # the next task needing you in any repository
bullpen go BP-1 --repo ~/shop   # a task of another repository
bullpen go BP-1 --board         # its board, with BP-1 selected
bullpen pick          # type part of any open task, in any repository, and go to it (the cmd+; j picker)
bullpen pick --filter "lgn"   # just print the open tasks matching, best first
bullpen tell BP-1 "Also cover the logout path"
bullpen resume BP-1   # restart a stopped worker, continuing its conversation
bullpen ask BP-1 "..."   # (coordinator) BP-1 needs you: the question shows on the board
bullpen ok BP-1       # (coordinator) a task in triage is seen to: back to work, nothing typed
bullpen review BP-1   # (coordinator) a new reviewer for one that stopped without a verdict
bullpen note BP-1 "..."   # an informational line on the board
bullpen accept BP-1   # merge its PR (or local branch; rebase), close its pane, remove its worktree;
                      # a finished task (nothing to merge): remove its worktree, branch and pane
bullpen close BP-1    # the same as accept, for a finished task only
bullpen drop BP-1     # close its PR, pane and worktree without merging
bullpen build         # build bullpen's command line and the board now (see Rebuilds)
```

A command that can't act on a task says its status in the board's words, with the
status itself after them when they differ: "BP-4 has no pane: it's queued (todo)",
"BP-2 needs you (stopped), not waiting on its coordinator".

`bullpen diff BP-1` shows `git diff <base>...HEAD` from the task's worktree, then
any changes not committed yet, in `delta` if it's installed, else `less -R`. The
base is `origin/<base>` when there's a remote, the local base branch when there
isn't. On the board, `v` opens the same view in a floating pane; `q` closes it and
takes you back to the board.

A worker finishes with `bullpen ready <ID> --summary-file <file>`, which refuses
uncommitted changes, pushes the branch and opens (or updates) the pull request, or,
when the task leaves nothing to merge, with `bullpen finish <ID> --summary-file <file>`.
