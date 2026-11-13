"""Local mode and remote mode: the steps that differ between them, as two adapters.

A repository with an `origin` remote is in remote mode: `ready` pushes the task's
branch and opens a pull request, and `accept` merges it on GitHub. Without one it is
in local mode: nothing is pushed, and `accept` merges the branch into the local base
branch. A task's mode is fixed when it's published (its first `ready`) and recorded
on it (`local`: true or false); every later step goes by that record, so a task
finishes the way it started even if an origin is added or removed meanwhile. Before
that, it goes by the repo as it is now.

Each adapter has the same six verbs: publish, merge, after_merge, withdraw, refresh
(after a rebase) and base_ref. Every push goes through quiet_git, which never lets
git wait on a prompt.
"""

import os
import subprocess
from pathlib import Path

from .core import Fail, SELF, commits, git, run
from .jobs import background


def mode(repo, t):
    """The adapter for a task: the one recorded when it was published, else the repo's."""
    if t.get("local") is not None:
        return LOCAL if t["local"] else REMOTE
    if t.get("pr"):
        return REMOTE  # published before the record was kept
    return REMOTE if repo.has_origin() else LOCAL


def task_base_ref(repo, t):
    """What the task's branch is compared with: origin's copy of its base branch in
    remote mode (when origin has it), else the local base branch."""
    return mode(repo, t).base_ref(repo, t)


def repo_base_ref(repo):
    """The base branch new work starts from: origin's copy when there is one."""
    base = repo.base()
    if repo.has_origin() and git(repo.root, "rev-parse", "-q", "--verify", f"origin/{base}",
                                check=False).returncode == 0:
        return f"origin/{base}"
    return base


# ---------- one way to push ----------


def push_env(repo):
    """git that never waits for a password or a key's passphrase: it fails instead."""
    env = {**os.environ, "GIT_TERMINAL_PROMPT": "0", "GCM_INTERACTIVE": "never"}
    if not env.get("GIT_SSH_COMMAND") and git(repo.root, "config", "core.sshCommand", check=False).returncode != 0:
        env["GIT_SSH_COMMAND"] = "ssh -o BatchMode=yes"
    return env


def quiet_git(repo, *args, timeout=60, cwd=None):
    """git on the network, away from any terminal (no prompt can reach the person)."""
    try:
        return subprocess.run(["git", "-C", str(cwd or repo.root), *args], env=push_env(repo), text=True,
                              stdin=subprocess.DEVNULL, capture_output=True, timeout=timeout,
                              start_new_session=True)
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(args, 1, "", f"no answer from the remote in {timeout}s")


def failed(p):
    """What a failed git said, on one line."""
    return " ".join((p.stderr or p.stdout).split())


def push_remote(repo):
    """The remote accept pushes the base branch to in local mode, or None (unset, or
    the repo has an origin, where the setting does nothing)."""
    name = repo.cfg.get("push_remote")
    return name if name and not repo.has_origin() else None


def push_base(repo, base, t=None):
    """After a local merge: push base to the push remote in the background. Says what it
    started, or None when there's no push remote."""
    remote = push_remote(repo)
    if not remote:
        return None
    argv = [SELF, "_push", base] + (["--task", t["id"]] if t else [])
    background(argv, repo.root, repo.dir / "jobs.log", f"Pushing {base} to {remote}", t and t["id"],
               wait=bool(os.environ.get("BULLPEN_PUSH_WAIT")))  # the tests: wait, to look at what it did
    return f"pushing {base} to {remote} in the background"


# ---------- remote mode: origin and GitHub ----------


class Remote:
    local = False

    def publish(self, repo, t, ahead, base_ref, summary):
        """Push the branch and open its PR (once). Says what it did."""
        p = quiet_git(repo, "push", "-q", "--force-with-lease", "-u", "origin",
                      f"HEAD:refs/heads/{t['branch']}", cwd=t["worktree"], timeout=600)
        if p.returncode != 0:
            raise Fail(f"pushing {t['branch']} to origin failed: {failed(p)}")
        if t.get("gh"):
            t["gh"].update(new=0, checks=None)  # what it says about the new push comes later
        if not t.get("pr"):
            body = (summary.strip() or t["title"]) + f"\n\nBullpen task {t['id']}.\n"
            p = run([repo.gh(), "pr", "create", "--base", t["base"], "--head", t["branch"],
                     "--title", f"{t['title']} ({t['id']})", "--body-file", "-"], cwd=t["worktree"], input=body)
            t["pr"] = p.stdout.strip().splitlines()[-1]
        return f"pushed {t['branch']}; pull request: {t['pr']}"

    def merge(self, repo, t):
        """Merge the PR on GitHub, unless it was merged there already. Raises Fail,
        leaving the task as it was, when GitHub refuses."""
        if not t.get("pr") or (t.get("gh") or {}).get("state") == "merged":
            return
        p = run([repo.gh(), "pr", "merge", t["pr"], f"--{repo.cfg['merge']}"], cwd=repo.root, check=False)
        if p.returncode != 0:
            raise Fail(f"merge refused: {(p.stderr or p.stdout).strip()}")

    def after_merge(self, repo, t):
        """Delete the merged branch on origin, and fetch the base it moved."""
        if t.get("pr") and t.get("branch"):
            quiet_git(repo, "push", "-q", "origin", "--delete", t["branch"])
        quiet_git(repo, "fetch", "-q", "origin", t["base"])
        return None

    def withdraw(self, repo, t, delete_branch=True):
        """Close the task's PR without merging it (and delete its branch on origin)."""
        if not t.get("pr"):
            return
        run([repo.gh(), "pr", "close", t["pr"]], cwd=repo.root, check=False)
        if delete_branch and t.get("branch"):
            quiet_git(repo, "push", "-q", "origin", "--delete", t["branch"])

    def refresh(self, repo, t):
        """After bullpen rebased the task's branch: push it to its PR. Says why it
        failed, or None."""
        if not t.get("pr"):
            return None
        p = quiet_git(repo, "push", "-q", "--force-with-lease", "origin", f"HEAD:refs/heads/{t['branch']}",
                      cwd=t["worktree"], timeout=600)
        return failed(p) if p.returncode != 0 else None

    def base_ref(self, repo, t):
        if t.get("worktree") and git(t["worktree"], "rev-parse", "-q", "--verify", f"origin/{t['base']}",
                                     check=False).returncode == 0:
            return f"origin/{t['base']}"
        return t["base"]


# ---------- local mode: the local base branch, and the push remote ----------


class Local:
    local = True

    def publish(self, repo, t, ahead, base_ref, summary):
        """Nothing leaves this machine."""
        return f"local branch {t['branch']} ({commits(ahead)} ahead of {base_ref}; no remote, so nothing pushed)"

    def merge(self, repo, t):
        if t.get("branch"):
            merge_locally(repo, t)  # raises, leaving the task as it was, if it can't

    def after_merge(self, repo, t):
        """Back up the base branch to the push remote. Says what it started, or None."""
        return push_base(repo, t["base"], t) if t.get("merged") else None

    def withdraw(self, repo, t, delete_branch=True):
        pass  # nothing was published

    def refresh(self, repo, t):
        return None

    def base_ref(self, repo, t):
        return t["base"]


REMOTE, LOCAL = Remote(), Local()


def checked_out_at(repo, branch):
    """The worktree (the main checkout or another) that has branch checked out, or None."""
    p = git(repo.root, "worktree", "list", "--porcelain")
    path = None
    for line in p.stdout.splitlines():
        if line.startswith("worktree "):
            path = line[len("worktree "):]
        elif line == f"branch refs/heads/{branch}":
            return path
    return None


def merge_locally(repo, t):
    """Local mode: merge the task's branch into its local base branch, as config --merge
    says. On any failure, raise with the reason and leave the task, its worktree and its
    branch as they were (a rebase that went through stays: the branch is still intact)."""
    base, branch, wt = t["base"], t["branch"], t.get("worktree")
    how = repo.cfg["merge"]
    if git(repo.root, "rev-parse", "-q", "--verify", f"refs/heads/{branch}", check=False).returncode != 0:
        raise Fail(f"merge refused: {t['id']}'s branch {branch} is gone")
    old = git(repo.root, "rev-parse", f"refs/heads/{base}").stdout.strip()
    if int(git(repo.root, "rev-list", "--count", f"{old}..refs/heads/{branch}").stdout.strip()) == 0:
        return  # nothing to merge
    if how == "rebase":
        if not wt or not Path(wt).exists():
            raise Fail(f"merge refused: {t['id']} has no worktree to rebase in")
        if git(wt, "status", "--porcelain").stdout.strip():
            raise Fail(f"merge refused: {t['id']} has uncommitted changes in {wt}")
        p = git(wt, "rebase", "-q", old, check=False)
        if p.returncode != 0:
            git(wt, "rebase", "--abort", check=False)
            raise Fail(f"merge refused: {branch} doesn't rebase cleanly onto {base}; "
                       f"rebase it by hand in {wt}, then accept again")
        new = git(repo.root, "rev-parse", f"refs/heads/{branch}").stdout.strip()
    else:
        p = git(repo.root, "merge-tree", "--write-tree", "--no-messages", old, f"refs/heads/{branch}",
                check=False)
        if p.returncode != 0:
            raise Fail(f"merge refused: {branch} conflicts with {base}; "
                       f"rebase it onto {base} in {wt}, then accept again")
        tree = p.stdout.split()[0]
        if how == "squash":
            msg = f"{t['title']} ({t['id']})"
            if t.get("summary"):
                msg += "\n\n" + t["summary"].strip()
            parents = ["-p", old]
        else:
            msg = f"Merge {branch}: {t['title']} ({t['id']})"
            parents = ["-p", old, "-p", f"refs/heads/{branch}"]
        new = run(["git", "-C", str(repo.root), "commit-tree", tree, *parents, "-F", "-"],
                  input=msg + "\n").stdout.strip()
    move_base(repo, base, old, new)
    t["merged"] = new


def move_base(repo, base, old, new):
    """Fast-forward the local base branch from old to new. Where it's checked out, do it
    with `merge --ff-only` there, which keeps uncommitted changes to other files and
    refuses if the incoming commits touch one of them."""
    at = checked_out_at(repo, base)
    if at:
        p = git(at, "merge", "-q", "--ff-only", new, check=False)
        if p.returncode != 0:
            msg = " ".join((p.stderr or p.stdout).split())
            raise Fail(f"merge refused: can't fast-forward {base} in {at}: {msg}")
        return
    if git(repo.root, "merge-base", "--is-ancestor", old, new, check=False).returncode != 0:
        raise Fail(f"merge refused: {base} can't be fast-forwarded")
    git(repo.root, "update-ref", "-m", "bullpen accept", f"refs/heads/{base}", new, old)
