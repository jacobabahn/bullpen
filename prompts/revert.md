Take {id} ("{title}") back: revert the commits its accept left on `{base}`. The revert is a task like any other, so it gets this branch, a review and an accept.
{why}
## The commits

{id} was accepted ({where}). It left {count} on `{base}`, listed newest first, which is the order to revert them in:

{commits}

These come from {id}'s record, not from commit messages: revert exactly these, and no other task's commits.

## What to do

1. Revert them as one commit:

       {revert}
       {commit}

   {one}
2. If the revert conflicts with later work, keep the later work: take out only what {id} put in, and leave the lines a later task changed or added as they are now.
3. Run the full suite.
4. In your summary, say anything that couldn't be cleanly taken back: a later task that built on {id} (name it, and what of {id} it still needs), lines you kept because later work changed them, and a change to settings or a state file that the code before {id} can't read (a key it added, a file it moved, a migration it ran), with what the person should do about it. If everything came back cleanly, say that.
