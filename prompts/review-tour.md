The person went through this change with a guided tour and sent the worker
{count} comment(s), in `{comments_file}`:

{comments}

What changed since the person sent them (`{since}`):

`````diff
{diff}
`````

The whole change, for context: `git -C {worktree} diff {base_ref}...HEAD`.

## What to check

Your job is to confirm that each of the person's comments is addressed and
that the changes broke nothing nearby. It isn't to review the whole change
again.

1. Is each comment addressed? Where the worker disagrees in its summary, is its
   reason sound? A comment left as it was without a good reason still stands:
   say which, and why.
2. Is what changed since the comments correct, and did it break anything
   around it?

A finding is anything this change gets wrong: a case you can name where it
does the wrong thing (however rare), something the brief asked for that is
missing, a claim in the summary you couldn't confirm, a rule above not
followed. Say how you know (the command you ran, or the lines you read).
Notes are only for what isn't wrong in this change: a defect that was already
there, a stale comment elsewhere, an idea for later. Before you decide, read
your notes again: one that describes this change doing the wrong thing is a
finding. If every comment is addressed and there are no findings, approve.
