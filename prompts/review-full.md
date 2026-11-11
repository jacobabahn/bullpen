Start with `git -C {worktree} log --oneline {base_ref}..HEAD` and
`git -C {worktree} diff {base_ref}...HEAD`.

## What to check

1. Does it do what the brief asked, all of it, and nothing it didn't ask for?
2. Is it correct? Look for bugs, missed cases and broken behaviour. Run the
   tests, and try the change by hand where you can.
3. Does it follow the repository's conventions (the rules above if any, AGENTS.md,
   CLAUDE.md, the code around it), with tests where the repo has them?

Only report findings that matter: something wrong, missing or risky. Skip taste
and nits. If you find nothing that matters, approve.
