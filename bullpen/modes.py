"""Local mode and remote mode: what a task's branch is compared with (task_base_ref).

A repository with an `origin` remote is in remote mode: `ready` pushes the task's
branch and opens a pull request, and `accept` merges it on GitHub. Without one it is
in local mode: nothing is pushed, and `accept` merges the branch into the local base
branch. A task's mode is fixed when it's published (its first `ready`) and recorded
on it (`local`: true or false); every later step goes by that record, so a task
finishes the way it started even if an origin is added or removed meanwhile. Before
that, it goes by the repo as it is now.

The modes' steps are Rust's (cli/src/modes.rs); this is task_base_ref's twin, for the
Python that's left.
"""

from .core import git


def remote(repo, t):
    """Is the task in remote mode? The mode recorded when it was published, else the repo's."""
    if t.get("local") is not None:
        return not t["local"]
    if t.get("pr"):
        return True  # published before the record was kept
    return repo.has_origin()


def task_base_ref(repo, t):
    """What the task's branch is compared with: origin's copy of its base branch in
    remote mode (when origin has it), else the local base branch."""
    if remote(repo, t) and t.get("worktree") and git(t["worktree"], "rev-parse", "-q", "--verify",
                                                     f"origin/{t['base']}", check=False).returncode == 0:
        return f"origin/{t['base']}"
    return t["base"]
