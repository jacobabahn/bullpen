"""The coordinator's `review`, for a task whose reviewer stopped without a verdict. (ask, ok
and note are Rust's: cli/src/triage.rs.)"""

from .core import CLOSED, Fail, Repo, command, is_status, repo_root
from .modes import task_base_ref
from .agents import whose_turn
from .review import start_review


def cmd_review(args):
    """(coordinator) a new reviewer on the task's current commit, in place of one that
    stopped or exited without a verdict."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    r = t.get("review") or {}
    if t["status"] in CLOSED or not t.get("worktree"):
        raise Fail(is_status(t) + ("" if t.get("worktree") else ", with no worktree"))
    if not r:
        raise Fail(f"{t['id']} has had no review: its worker starts one with `ready`")
    if r.get("verdict") or whose_turn(t) != "reviewer":
        raise Fail(f"{is_status(t)}, not waiting on a reviewer")
    rounds = repo.cfg["review_rounds"]
    if r.get("round", 0) >= rounds and r.get("kind") != "tour":
        raise Fail(f"{t['id']} has used its {rounds} review rounds: `bullpen ask {t['id']}` the person")
    start_review(repo, t, task_base_ref(repo, t))
    print(f"{t['id']}: a new reviewer started (round {t['review']['round']} of {rounds})")


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "review", cmd_review, "Coordinator",
                help="(coordinator) a new reviewer in place of one that stopped without a verdict")
    p.add_argument("id", help="the task, like BP-3")
