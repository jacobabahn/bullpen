This is a follow-up round. Round {prev_round} {prev_said}
(`{findings_file}`):

{findings}

What changed since round {prev_round} looked (`{since}`):

`````diff
{diff}
`````

The whole change, for context: `git -C {worktree} diff {base_ref}...HEAD`.

## What to check

Your job is to confirm that each finding is fixed and that the changes broke
nothing nearby. It isn't to review the whole change again: the code round
{prev_round} looked at and that hasn't changed is settled.

1. Is each finding fixed? Where the worker disagrees in its summary, is its
   reason sound? A finding left as it was without a good reason still stands.
2. Is what changed since round {prev_round} correct, and did it break anything
   around it?

Only report findings that matter: something wrong, missing or risky. Skip taste
and nits. If you find nothing that matters, approve.
