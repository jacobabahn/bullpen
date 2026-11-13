"""Closing a finished task (nothing to merge), and dropping a task."""

import os
import sys
from pathlib import Path

from .core import Fail, Repo, command, git, is_status, repo_root, run, set_status, unindex_block
from .rex import rex_close
from .news import news, tell_coordinator
from .hooks import remove_worktree
from .start import dropped_dep, start_queued
from .review import close_reviewer


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
    """Remove a finished task's worktree and branch (no warning: it has nothing to
    merge), close its panes and any PR, and mark it done."""
    os.chdir(repo.root)  # we may be running in the worktree this removes
    worker = (t.get("session"), t.get("block"))
    unindex_block(t.get("block"))
    close_reviewer(t)
    if t.get("pr"):
        run([repo.gh(), "pr", "close", t["pr"]], cwd=repo.root, check=False)
        git(repo.root, "push", "-q", "origin", "--delete", t["branch"], check=False)
    remove_worktree(repo, t, force=True)
    set_status(t, "done", t.get("why"))
    t.update(closed=True, block=None, worktree=None, auto_accepted=auto)
    repo.save_task(t)
    if auto:
        news(repo, t, "closed", f"{t['id']} finished (nothing to merge) and was closed by auto-accept: {first}")
    else:
        print(f"{t['id']} closed")
        tell_coordinator(repo, t, "closed", f"{t['id']} closed (finished, nothing to merge)")
    start_queued(repo, t, "closed")
    sys.stdout.flush()
    rex_close(*worker)  # last: this may close the pane we run in


# ---------- drop ----------


def cmd_drop(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t.get("worktree") and Path(t["worktree"]).exists() and not args.force:
        if git(t["worktree"], "status", "--porcelain").stdout.strip():
            raise Fail(f"{t['id']} has uncommitted changes in {t['worktree']}; pass --force to discard them")
    rex_close(t["session"], t["block"])
    unindex_block(t["block"])
    close_reviewer(t)
    if t.get("pr"):
        run([repo.gh(), "pr", "close", t["pr"]], cwd=repo.root, check=False)
    remove_worktree(repo, t, force=args.force)
    set_status(t, "dropped", t.get("why"))
    t.update(block=None, worktree=None)
    repo.save_task(t)
    print(f"{t['id']} dropped")
    tell_coordinator(repo, t, "dropped", f"{t['id']} dropped")
    dropped_dep(repo, t)
    start_queued(repo, t, "dropped")


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "close", cmd_close, "You",
                help="(the person's call) close a finished task: remove its worktree, branch and pane (the same as accept on it)")
    p.add_argument("id", help="the task, like BP-3")

    p = command(sub, "drop", cmd_drop, "You", help="stop a task, close its PR, remove its worktree")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--force", action="store_true", help="drop it even with uncommitted changes in its worktree")
