"""A worker's `ready` and `finish`."""

import sys
from pathlib import Path

from .core import (
    CLOSED, Fail, Repo, auto_accepts, command, commits, git, is_status, read_json, repo_root,
    set_status, where,
)
from .modes import mode, task_base_ref
from .news import news, ready_news
from .review import close_reviewer, review_at_ready, shortstat
from .close import close_task
from .tours import tour_path, tour_stale


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


def cmd_ready(args):
    repo = Repo(repo_root())
    ready(repo, repo.task(args.id), args)


def ready(repo, t, args):
    wt = t["worktree"]
    if not wt:
        raise Fail(f"{t['id']} has no worktree")
    if git(wt, "status", "--porcelain").stdout.strip():
        raise Fail("uncommitted changes: commit (or discard) them, then run ready again")
    summary = summary_text(args, "summary", required=False)
    if t.get("gh"):
        t["gh"].pop("sent", None)  # whatever GitHub sent it back for is answered now
    t.pop("rebased", None)  # what the board says of a rebase after an accept
    if (t.get("tour") or {}).get("state") == "written":
        tour = read_json(tour_path(repo, t))
        if tour and tour_stale(repo, t, tour):  # new code: not toured yet, as the board says
            t["tour"].pop("opened", None)
            t["tour"]["stale"] = True
    t.pop("from_ready", None)
    m = mode(repo, t)
    t["local"] = m.local  # its mode, fixed now: every later step goes by it
    if not repo.cfg["review"]:
        t.pop("tour_sent", None)  # no reviewer checks the person's comments with review off
    base_ref = task_base_ref(repo, t)
    ahead = int(git(wt, "rev-list", "--count", f"{base_ref}..HEAD").stdout.strip())
    if ahead == 0:
        set_status(t, "ready", "Nothing to merge (no commits)")
        t.update(summary=summary.strip() or None, diffstat=None)
        repo.save_task(t)
        ready_news(repo, t, ["nothing to merge"])
        print("no commits: marked ready with nothing to merge")
        return
    t["diffstat"] = shortstat(wt, base_ref)  # for the board, worked out once per ready
    published = m.publish(repo, t, ahead, base_ref, summary)  # pushed and a PR, or nothing
    t.update(why=None, summary=summary.strip() or t.get("summary"), commits=ahead)
    if repo.cfg["review"]:
        said = review_at_ready(repo, t, base_ref)
        print(f"{published}. {said}")
        return
    set_status(t, "ready")
    repo.save_task(t)
    ready_news(repo, t, [commits(ahead), where(t)])
    print(published)


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
    p = command(sub, "ready", cmd_ready, "Worker and reviewer",
                help="(worker) push the branch, open the PR (none without a remote), tell the coordinator")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--summary", help="what you did, as text")
    p.add_argument("--summary-file", metavar="FILE", help="the same, read from a file")

    p = command(sub, "finish", cmd_finish, "Worker and reviewer",
                help="(worker) the task is done with nothing to merge: no PR, no review")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--summary", help="what you did, as text")
    p.add_argument("--summary-file", metavar="FILE", help="the same, read from a file")
