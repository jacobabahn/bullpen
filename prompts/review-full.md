Start with `git -C {worktree} log --oneline {base_ref}..HEAD` and
`git -C {worktree} diff {base_ref}...HEAD`.

## What to check

1. Does it do what the brief asked, all of it, and nothing it didn't ask for?
2. Is it correct? Look for bugs, missed cases and broken behaviour. Read the code,
   and try the change by hand where you can.
3. Does it follow the repository's conventions (the rules above if any, AGENTS.md,
   CLAUDE.md, the code around it), with tests where the repo has them?

A finding is anything this change gets wrong: a case you can name where it
does the wrong thing (however rare), something the brief asked for that is
missing, a claim in the summary you couldn't confirm, a rule above not
followed. Say how you know (the command you ran, or the lines you read).
Notes are only for what isn't wrong in this change: a defect that was already
there, a stale comment elsewhere, an idea for later. Before you decide, read
your notes again: one that describes this change doing the wrong thing is a
finding. If there are no findings, approve.
