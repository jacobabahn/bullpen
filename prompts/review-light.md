This is a light review: the change is small ({why}), so one careful read of it is
enough. Read it all: `git -C {worktree} diff {base_ref}...HEAD`.

## What to check

1. Is it correct? Look for mistakes and missed cases in what changed.
2. Does it match the brief: what it asked, and nothing it didn't?

A finding is anything this change gets wrong: a case you can name where it
does the wrong thing (however rare), something the brief asked for that is
missing, a claim in the summary you couldn't confirm, a rule above not
followed. Say how you know (the command you ran, or the lines you read).
Notes are only for what isn't wrong in this change: a defect that was already
there, a stale comment elsewhere, an idea for later. Before you decide, read
your notes again: one that describes this change doing the wrong thing is a
finding. If there are no findings, approve.
