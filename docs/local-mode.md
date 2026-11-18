# Local mode

In a repository with no `origin` remote, bullpen works locally. `ready` pushes
nothing and opens no pull request: it checks the branch against the local base
branch and starts the reviewer as usual, and news names no PR (nor the local
branch: in this repo they all are). `accept` merges the task's branch into the local base branch
itself (rebase, squash or merge, as `config --merge` says). Where the base branch
is checked out, it is fast-forwarded with `git merge --ff-only`, so uncommitted
changes to other files survive. If anything stands in the way (a conflict, or an
uncommitted change to a file the task touches), `accept` says why and leaves the
task ready, with its worktree and branch as they were.

A task's mode is fixed at its first `ready`, and recorded on it: if you add an
`origin` while a task is open, a task that was ready before still finishes locally
(another `ready` pushes nothing; `accept` merges locally), and one that wasn't
ready yet goes through a pull request. The same holds the other way: a task with a
pull request is merged on GitHub even if `origin` is removed meanwhile.

To keep a copy elsewhere without PRs, add a remote under another name than `origin`
(so bullpen stays in local mode) and name it: `bullpen config --push-remote github`.
After each `accept` (and auto-accept) that merges something, bullpen pushes the base
branch, and only it, to that remote: `git push github main`, in the background, so
the accept doesn't wait on the network. Pushes take turns and push the branch as it
is then, so two accepts in a row both land. It never force-pushes, and git is never
let to ask for a password. A push that fails (offline, rejected because the remote
has commits the base doesn't, no access) doesn't undo the accept: one news line gives
the reason, and `bullpen doctor` says how far behind the remote is. `close` pushes
nothing (there's no merge), nor does a merge you do by hand. But `bullpen up` pushes
the base branch when it has commits the remote didn't get (as far as this checkout
last saw the remote), so things catch up after an offline spell. `bullpen config
--push-remote ""` or `--unset push_remote` turns it off. With an `origin` remote
(remote mode), the setting does nothing: merges happen on GitHub.

With [GitHub issues](issues.md) on, an accepted issue's task closes its issue once that
push is over, with a comment naming the commit (or "not pushed yet" without a push
remote, or when the push failed): see [What's written back](issues.md#whats-written-back).
