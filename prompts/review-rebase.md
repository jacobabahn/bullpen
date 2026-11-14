This is a rebase check, not a full review. Round {prev_round} approved the change
(its notes: `{findings_file}`). Since then `{base_ref}` moved, the branch was
rebased onto it, and the rebase conflicted. The change's own lines (what it adds
and removes) are as round {prev_round} approved them everywhere except in
{conflict_files}, where the conflicts were resolved.

The conflicts as the rebase met them:

`````diff
{conflicts}
`````

The change in those files now:

`````diff
{now}
`````

How the commits changed in the rebase (`{since}`):

`````diff
{diff}
`````

## What to check

1. Is each resolution right? It should keep both sides: what `{base_ref}` changed
   and what the task changed, with nothing lost, nothing doubled, and the result
   making sense where they meet (a list still in order, a table still whole, code
   that still runs).
2. Did the resolution change anything beyond fitting the task's change onto the
   new base? It shouldn't.

Don't review the rest of the change: round {prev_round} approved it. If the
resolution is right, approve.
