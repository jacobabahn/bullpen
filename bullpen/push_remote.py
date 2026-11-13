"""The push remote: a backup of the base branch, in local mode."""

import argparse
import fcntl

from .core import Fail, Repo, clip, command, repo_root
from .modes import push_remote, quiet_git
from .news import news


# config --push-remote names a remote (not origin: with origin, bullpen is in remote mode
# and the setting does nothing). After accept merges into the local base branch, the
# base branch alone is pushed there, in the background, never forced. Pushes take a
# lock and push the branch as it is then, so two accepts in a row can't race: the
# later push carries both. A failed push leaves the accept standing, with one news line.


PUSH_NEWS = {"id": "push"}  # the news log's task column when no task is behind a push


def push_reason(out, remote, base):
    """Why git push (or ls-remote) failed, in a few words."""
    text = " ".join(out.split())
    if "[rejected]" in out or "non-fast-forward" in out or "fetch first" in out:
        return (f"rejected, {remote} has commits {base} doesn't (bullpen never force-pushes: "
                f"`git pull {remote} {base}` by hand, then `git push {remote} {base}`)")
    lines = [l.strip() for l in out.splitlines() if l.strip() and not l.strip().startswith("hint:")]
    fatal = [l for l in lines if l.startswith(("fatal:", "error:", "remote:", "ERROR"))]
    return clip((fatal or lines or [text or "git push failed"])[0], 200)


def cmd_push_bg(args):
    """(bullpen) push the base branch to the push remote, one push at a time."""
    repo = Repo(repo_root())
    remote = push_remote(repo)
    if not remote:
        return
    repo.dir.mkdir(parents=True, exist_ok=True)
    with open(repo.dir / "push.lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)  # an earlier push finishes first
        # The branch as it is now: a later accept's commits go too, never an older tip.
        p = quiet_git(repo, "push", "-q", remote, f"refs/heads/{args.base}:refs/heads/{args.base}")
    if p.returncode == 0:
        return
    try:
        t = repo.task(args.task) if args.task else PUSH_NEWS
    except Fail:
        t = PUSH_NEWS
    after = "" if t is PUSH_NEWS else f"After {t['id']}, "
    news(repo, t, "push", f"{after}{args.base} wasn't pushed to {remote}: "
                          f"{push_reason(p.stderr or p.stdout, remote, args.base)}. "
                          + ("" if t is PUSH_NEWS else "The accept still counts; ")
                          + f"`bullpen doctor` says how far behind {remote} is.",
         everyone=t is PUSH_NEWS)
    return 1


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "_push", cmd_push_bg)
    p.add_argument("base")
    p.add_argument("--task", help=argparse.SUPPRESS)
