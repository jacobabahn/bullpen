"""Accepting an approved task by itself (config --auto-accept). (The reviewer's `verdict`
is Rust's: cli/src/auto_accept.rs, which runs `_auto_accept` for an approval.)"""

import json
import os

from .core import Fail, Repo, command, pr_short, repo_root, run, where, write_json
from .news import ready_news
from .accept import accept


def cmd_auto_accept(args):
    """(bullpen) the auto-accept of an approval carried over at ready, away from the worker's pane."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] == "ready" and (t.get("review") or {}).get("verdict") == "approved":
        auto_accept(repo, t)


# ---------- auto-accept ----------
#
# With `config --auto-accept on`, an approve verdict runs the normal accept itself:
# the person said in advance that an approval is enough. A task's own "auto_accept"
# (`add`/`start --no-auto-accept`, or `--auto-accept`) wins over the config's. In remote
# mode it waits for the PR's checks ("auto_wait"), which the PR follow-up looks at.


def auto_accept(repo, t):
    """An approved task: accept it now, or, with a PR whose checks are still running,
    once they pass. Anything that stops it leaves the task ready, with news."""
    if t.get("pr") and not t.get("local"):
        state = pr_checks_now(repo, t)
        if state == "failing":
            ready_news(repo, t, [pr_short(t["pr"]), "approved by review"],
                       ", but not auto-accepted: its PR's checks are failing")
            return
        if state == "running":
            t["auto_wait"] = True
            write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
            ready_news(repo, t, [pr_short(t["pr"]), "approved by review"],
                       "; it auto-accepts once its PR's checks pass")
            return
    run_auto_accept(repo, t)


def pr_checks_now(repo, t):
    """"failing", "running", or "passing" (no checks counts as passing). Unsure (gh
    failed) counts as running: the PR follow-up looks again."""
    p = run([repo.gh(), "pr", "view", t["pr"], "--json", PR_FIELDS], cwd=repo.root, check=False)
    try:
        summary, _ = checks(json.loads(p.stdout)) if p.returncode == 0 else ("running", [])
    except ValueError:
        return "running"
    return summary or "passing"


def run_auto_accept(repo, t):
    """The normal accept, as if the person ran it. A refused merge leaves the task ready."""
    os.chdir(repo.root)  # we may be running in the worktree accept removes
    try:
        accept(repo, t, auto=True)
    except Fail as e:
        t = repo.task(t["id"])
        t.pop("auto_wait", None)
        write_json(repo.task_path(t["id"]), t)
        ready_news(repo, t, [where(t), "approved by review"],
                   f", but the auto-accept was refused: {e}. Send it back to rebase")


FAILED_CHECK = ("FAILURE", "ERROR", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE")
PR_FIELDS = "state,headRefOid,statusCheckRollup,reviews,comments"


def checks(pr):
    """(summary, names of failed checks) from a PR's statusCheckRollup."""
    failed, running, total = [], False, 0
    for c in pr.get("statusCheckRollup") or []:
        total += 1
        name = c.get("name") or c.get("context") or "check"
        result = c.get("conclusion") or c.get("state") or ""
        if result in FAILED_CHECK:
            failed.append(name)
        elif (c.get("status") or "COMPLETED") != "COMPLETED" or result in ("PENDING", "EXPECTED"):
            running = True
    summary = "failing" if failed else "running" if running else "passing" if total else None
    return summary, failed


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "_auto_accept", cmd_auto_accept)
    p.add_argument("id", help="the task, like BP-3")
