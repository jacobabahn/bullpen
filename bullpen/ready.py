"""A worker's `finish`. (`ready` is Rust's: cli/src/ready.rs, with a twin of summary_text.)"""

import sys
from pathlib import Path

from .core import CLOSED, Fail, Repo, auto_accepts, command, git, is_status, repo_root, set_status
from .modes import task_base_ref
from .news import news
from .review import close_reviewer, shortstat
from .close import close_task


def summary_text(args, what, required=True):
    """The text of --summary-file (or --summary). An empty file is refused: it is a
    write that failed (a full disk, /tmp out of inodes), not "nothing to say"."""
    if not args.summary_file:
        text = (args.summary or "").strip()
        if required and not text:
            raise Fail(f"give your {what} with --summary-file (or --summary)")
        return text
    try:
        text = Path(args.summary_file).read_text().strip()
    except OSError as e:
        raise Fail(f"can't read {args.summary_file}: {e.strerror or e}")
    if not text:
        raise Fail(f"{args.summary_file} is empty, so writing your {what} to it probably failed "
                   f"(a full disk, or /tmp out of inodes). Write it again, somewhere else if need "
                   f"be, and rerun this command")
    return text


# ---------- finish: a task with nothing to merge ----------
#
# Some tasks leave nothing in the repo (a video, research notes). Their worker runs
# `finish` instead of `ready`: no PR and no review. The person closes the task, or
# auto-accept does it at once.


def cmd_finish(args):
    """(worker) the task is done and leaves nothing to merge."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] in CLOSED:
        raise Fail(is_status(t))
    wt = t["worktree"]
    if not wt:
        raise Fail(f"{t['id']} has no worktree")
    if git(wt, "status", "--porcelain").stdout.strip():
        raise Fail("uncommitted changes: commit them and run `ready`, or discard them and run finish again")
    base_ref = task_base_ref(repo, t)
    if git(wt, "diff", "--quiet", f"{base_ref}...HEAD", check=False).returncode != 0:
        raise Fail(f"your branch changes {shortstat(wt, base_ref) or 'files'} against {base_ref}, so "
                   f"there is something to merge: run `ready` instead")
    summary = summary_text(args, "summary")
    close_reviewer(t)
    if t.get("gh"):
        t["gh"].pop("sent", None)
    t.pop("rebased", None)
    set_status(t, "finished")
    t.update(summary=summary, diffstat=None)
    repo.save_task(t)
    first = summary.splitlines()[0].strip().lstrip("#").strip()  # a Markdown title is fine
    if auto_accepts(repo, t):
        print(f"{t['id']} finished, with nothing to merge; auto-accept closes it now. End your turn.")
        sys.stdout.flush()
        close_task(repo, t, auto=True, first=first)  # last: this closes the pane we run in
        return
    news(repo, t, "finished", f"{t['id']} finished (nothing to merge): {first}")
    print(f"{t['id']} finished, with nothing to merge. The person closes it: end your turn.")


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "finish", cmd_finish, "Worker and reviewer",
                help="(worker) the task is done with nothing to merge: no PR, no review")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--summary", help="what you did, as text")
    p.add_argument("--summary-file", metavar="FILE", help="the same, read from a file")
