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
   around it? Run only the tests you need to check that, never the whole suite:
   the worker ran it before `ready`.

Only report findings that matter. If every comment is addressed and nothing
broke, approve.
