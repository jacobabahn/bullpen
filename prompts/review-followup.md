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
nothing nearby. It isn't to review the whole change again. If fixing a finding
shows a problem in code round {prev_round} already read, say so: that is a finding
too.

1. Is each finding fixed? Where the worker disagrees in its summary, is its
   reason sound? A finding left as it was without a good reason still stands.
2. Is what changed since round {prev_round} correct, and did it break anything
   around it?

A finding is anything this change gets wrong: a case you can name where it
does the wrong thing (however rare), something the brief asked for that is
missing, a claim in the summary you couldn't confirm, a rule above not
followed. Say how you know (the command you ran, or the lines you read).
Notes are only for what isn't wrong in this change: a defect that was already
there, a stale comment elsewhere, an idea for later. Before you decide, read
your notes again: one that describes this change doing the wrong thing is a
finding. If there are no findings, approve.
