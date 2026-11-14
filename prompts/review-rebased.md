Round {prev_round} approved this change (its notes: `{findings_file}`). Since
then the branch was rebased onto `{base_ref}` and the worker changed it to fit.
This round checks only that.

What changed since round {prev_round} looked (`{since}`):

`````diff
{diff}
`````

## What to check

1. Does what changed only fit the approved change onto the new base (conflicts
   resolved, both sides kept, a test or doc moved to where the base now has it)?
   Anything more is a finding.
2. Is what changed correct? Run only a test that checks it.

The rest of the change is as round {prev_round} approved it: don't read it again.
If the rebase is right, approve.
