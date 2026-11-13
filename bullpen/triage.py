"""The coordinator's commands for a task that stopped: ask, ok and review; and note."""

from .core import (
    CLOSED, Fail, Repo, ask, command, is_status, ok, repo_root,
    task_base_ref, write_json,
)
from .agents import running, waits_on, whose_turn
from .news import tell_coordinator
from .review import start_review


def cmd_ask(args):
    """(coordinator) the task needs the person: the question shows on the board."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] in CLOSED:
        raise Fail(is_status(t))
    question = " ".join(" ".join(args.question).split())
    if not question:
        raise Fail("give the question to ask")
    ask(t, question)
    repo.save_task(t)
    tell_coordinator(repo, t, "ask", f"{t['id']} needs the person: {question}")
    print(f"{t['id']} needs the person now: {question}")


def cmd_ok(args):
    """(coordinator) a task's stop is seen to: back to what it was, typing nothing."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] != "triage":
        raise Fail(f"{is_status(t)}, not waiting on its coordinator")
    if waits_on(t) == "reviewer":
        if not running(t):
            raise Fail(f"{t['id']}'s reviewer isn't running: `bullpen review {t['id']}` starts a "
                       f"new one, or `bullpen ask` the person")
    elif not running(t):
        raise Fail(f"{t['id']}'s worker isn't running: `bullpen resume {t['id']}` restarts it")
    ok(t)  # `check` leaves its agent idle; its next turn clears that
    repo.save_task(t)
    status = t["status"]
    print(f"{t['id']} is {'in review' if status == 'review' else status} again (nothing typed to its agent)")


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


def cmd_note(args):
    """An informational line on the board about a task (cleared when its status changes).
    A question for the person is `ask`."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    t["note"] = " ".join(" ".join(args.text).split())
    write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
    print(f"noted on {t['id']}")


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "ask", cmd_ask, "Coordinator",
                help="(coordinator) the task needs the person: the question shows on the board")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("question", nargs="+", help="what the person should decide")

    p = command(sub, "ok", cmd_ok, "Coordinator",
                help="(coordinator) a task waiting on you is seen to: back to work, nothing typed")
    p.add_argument("id", help="the task, like BP-3")

    p = command(sub, "review", cmd_review, "Coordinator",
                help="(coordinator) a new reviewer in place of one that stopped without a verdict")
    p.add_argument("id", help="the task, like BP-3")

    p = command(sub, "note", cmd_note, "Coordinator", help="an informational line on the board about a task")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("text", nargs="+", help="the line")
