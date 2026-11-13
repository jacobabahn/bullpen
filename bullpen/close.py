"""Closing a task: the one close step, for accept and auto-accept. (close, drop and
finish are Rust's, with its own copy: cli/src/close.rs.)"""

import os

from .core import set_status, unindex_block
from .modes import mode
from .rex import close_last
from .agents import pane
from .news import news, tell_coordinator
from .hooks import remove_worktree
from .review import close_reviewer


def close(repo, t, status, then=None, withdraw=True, force=True, **fields):
    """The one close step, in one order: the reviewer, the PR (withdrawn, unless it was
    merged), the worktree and branch, the status and the save (which moves the run
    queue), then what the caller says and does (then), and the worker's pane last,
    since this may run in it. fields go on the task with the status."""
    os.chdir(repo.root)  # we may be running in the worktree this removes
    worker = pane(t, "worker")
    unindex_block(t.get("block"))
    close_reviewer(t)
    if withdraw:
        mode(repo, t).withdraw(repo, t, delete_branch=status == "done")
    remove_worktree(repo, t, force=force)
    set_status(t, status, t.get("why"))
    t.update(block=None, worktree=None, **fields)
    repo.save_task(t)
    if then:
        then()
    close_last(*worker)


def close_task(repo, t, auto=False, first=None):
    """A finished task (nothing to merge): close its PR if it has one, and mark it done."""
    def said():
        if auto:
            news(repo, t, "closed", f"{t['id']} finished (nothing to merge) and was closed by auto-accept: {first}")
        else:
            print(f"{t['id']} closed")
            tell_coordinator(repo, t, "closed", f"{t['id']} closed (finished, nothing to merge)")
    close(repo, t, "done", said, closed=True, auto_accepted=auto)
