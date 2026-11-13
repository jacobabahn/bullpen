"""Closing a task: the one close step that accept, close, drop and auto-accept share,
and the `close` and `drop` commands."""

import os
from pathlib import Path

from .core import Fail, Repo, command, git, is_status, repo_root, set_status, unindex_block
from .modes import mode
from .rex import close_last
from .news import news, tell_coordinator
from .hooks import remove_worktree
from .start import dropped_dep
from .review import close_reviewer


def close(repo, t, status, then=None, withdraw=True, force=True, **fields):
    """The one close step, in one order: the reviewer, the PR (withdrawn, unless it was
    merged), the worktree and branch, the status and the save (which moves the run
    queue), then what the caller says and does (then), and the worker's pane last,
    since this may run in it. fields go on the task with the status."""
    os.chdir(repo.root)  # we may be running in the worktree this removes
    worker = (t.get("session"), t.get("block"))
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


def cmd_close(args):
    """(the person's call) close a finished task: its worktree, branch and pane go.
    `accept` does the same on a finished task; this stays as another name for it."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] != "finished":
        hint = f": `accept {t['id']}` merges it" if t["status"] == "ready" else ""
        raise Fail(f"{is_status(t)}, not finished{hint}")
    close_task(repo, t)


def close_task(repo, t, auto=False, first=None):
    """A finished task (nothing to merge): close its PR if it has one, and mark it done."""
    def said():
        if auto:
            news(repo, t, "closed", f"{t['id']} finished (nothing to merge) and was closed by auto-accept: {first}")
        else:
            print(f"{t['id']} closed")
            tell_coordinator(repo, t, "closed", f"{t['id']} closed (finished, nothing to merge)")
    close(repo, t, "done", said, closed=True, auto_accepted=auto)


# ---------- drop ----------


def cmd_drop(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t.get("worktree") and Path(t["worktree"]).exists() and not args.force:
        if git(t["worktree"], "status", "--porcelain").stdout.strip():
            raise Fail(f"{t['id']} has uncommitted changes in {t['worktree']}; pass --force to discard them")

    def said():
        print(f"{t['id']} dropped")
        tell_coordinator(repo, t, "dropped", f"{t['id']} dropped")
        dropped_dep(repo, t)
    close(repo, t, "dropped", said, force=args.force)


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "close", cmd_close, "You",
                help="(the person's call) close a finished task: remove its worktree, branch and pane (the same as accept on it)")
    p.add_argument("id", help="the task, like BP-3")

    p = command(sub, "drop", cmd_drop, "You", help="stop a task, close its PR, remove its worktree")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--force", action="store_true", help="drop it even with uncommitted changes in its worktree")
