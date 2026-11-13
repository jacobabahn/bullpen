"""The push remote: a backup of the base branch, in local mode."""

import argparse
import fcntl
import os
import subprocess

from .core import Fail, Repo, SELF, clip, command, commits, git, repo_root
from .news import news


# config --push-remote names a remote (not origin: with origin, bullpen is in PR mode
# and the setting does nothing). After accept merges into the local base branch, the
# base branch alone is pushed there, in the background, never forced. Pushes take a
# lock and push the branch as it is then, so two accepts in a row can't race: the
# later push carries both. A failed push leaves the accept standing, with one news line.


PUSH_NEWS = {"id": "push"}  # the news log's task column when no task is behind a push


def push_remote(repo):
    """The remote accept pushes the base branch to, or None (unset, or PR mode)."""
    name = repo.cfg.get("push_remote")
    return name if name and not repo.has_origin() else None


def push_env(repo):
    """git that never waits for a password or a key's passphrase: it fails instead."""
    env = {**os.environ, "GIT_TERMINAL_PROMPT": "0", "GCM_INTERACTIVE": "never"}
    if not env.get("GIT_SSH_COMMAND") and git(repo.root, "config", "core.sshCommand", check=False).returncode != 0:
        env["GIT_SSH_COMMAND"] = "ssh -o BatchMode=yes"
    return env


def quiet_git(repo, *args, timeout=60):
    """git on the network, away from any terminal (no prompt can reach the person)."""
    try:
        return subprocess.run(["git", "-C", str(repo.root), *args], env=push_env(repo), text=True,
                              stdin=subprocess.DEVNULL, capture_output=True, timeout=timeout,
                              start_new_session=True)
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(args, 1, "", f"no answer from the remote in {timeout}s")


def push_base(repo, base, t=None):
    """After a local merge: push base to the push remote in the background. Says what it
    started, or None when there's no push remote."""
    remote = push_remote(repo)
    if not remote:
        return None
    argv = [SELF, "_push", base] + (["--task", t["id"]] if t else [])
    p = subprocess.Popen(argv, cwd=repo.root, start_new_session=True,
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if os.environ.get("BULLPEN_PUSH_WAIT"):  # the tests: wait for it, to look at what it did
        p.wait()
    return f"pushing {base} to {remote} in the background"


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


def catch_up_push(repo):
    """`up`: push the base branch when it has commits the push remote didn't get (after
    an offline spell, say), as far as this checkout last saw it. Says what it started."""
    remote = push_remote(repo)
    if not remote or git(repo.root, "remote", "get-url", remote, check=False).returncode != 0:
        return None
    base = repo.base()
    if git(repo.root, "rev-parse", "-q", "--verify", f"refs/heads/{base}", check=False).returncode != 0:
        return None
    seen = f"refs/remotes/{remote}/{base}"
    if git(repo.root, "rev-parse", "-q", "--verify", seen, check=False).returncode == 0:
        if int(git(repo.root, "rev-list", "--count", f"{seen}..refs/heads/{base}").stdout.strip()) == 0:
            return None
    return push_base(repo, base)


def check_push(repo, why_not=None):
    """The push remote: there, reachable without a prompt, and how far behind the base."""
    if not repo:
        return "skip", f"can't tell: {why_not or 'not in a git repository'}"
    name = repo.cfg.get("push_remote")
    if not name:
        return "skip", "none (`bullpen config --push-remote <remote>` backs up the base branch after each accept)"
    if repo.has_origin():
        return "skip", f"{name} is set, but this repo has an origin remote (PR mode), so nothing is pushed to it"
    if git(repo.root, "remote", "get-url", name, check=False).returncode != 0:
        return "fail", (f"there's no remote {name}: `git remote add {name} <url>`, "
                        f"or `bullpen config --push-remote \"\"`")
    base = repo.base()
    p = quiet_git(repo, "ls-remote", "--heads", name, f"refs/heads/{base}", timeout=20)
    if p.returncode != 0:
        return "fail", f"can't reach {name} without a prompt: {push_reason(p.stderr or p.stdout, name, base)}"
    theirs = (p.stdout.split() or [None])[0]
    if not theirs:
        return "fail", f"{name} has no {base} yet: `git push {name} {base}`"
    if git(repo.root, "cat-file", "-e", f"{theirs}^{{commit}}", check=False).returncode != 0 or \
            git(repo.root, "merge-base", "--is-ancestor", theirs, f"refs/heads/{base}", check=False).returncode != 0:
        return "fail", (f"{name}'s {base} has commits this one doesn't, so pushes are rejected: "
                        f"`git pull {name} {base}`, then `git push {name} {base}`")
    n = int(git(repo.root, "rev-list", "--count", f"{theirs}..refs/heads/{base}").stdout.strip())
    if n:
        return "fail", f"{name} is {commits(n)} behind {base}: `git push {name} {base}`"
    return "ok", f"{name} has {base} as it is here"


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "_push", cmd_push_bg)
    p.add_argument("base")
    p.add_argument("--task", help=argparse.SUPPRESS)
