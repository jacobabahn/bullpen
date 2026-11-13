"""A reviewer's verdict, and accepting an approved task by itself (config --auto-accept)."""

import json
import os
import sys

from .core import (
    Fail, Repo, WAITING, auto_accepts, clip, command, commits, pr_short, repo_root, run,
    set_status, unindex_block, where, write_json,
)
from .rex import rex_close
from .news import ready_news
from .stops import stop
from .start import hand_back, start_queued
from .review import review_kind
from .ready import summary_text
from .accept import accept


def cmd_auto_accept(args):
    """(bullpen) the auto-accept of an approval carried over at ready, away from the worker's pane."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] == "ready" and (t.get("review") or {}).get("verdict") == "approved":
        auto_accept(repo, t)


def cmd_verdict(args):
    """(reviewer) approve the task, or send findings back to its worker."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    r = t.get("review") or {}
    if t["status"] not in ("review", *WAITING) or not r.get("block"):
        raise Fail(f"{t['id']} isn't in review")
    text = summary_text(args, "findings")
    tour, rebase = r.get("kind") == "tour", r.get("kind") == "rebase"
    findings = repo.dir / "briefs" / (f"{t['id']}.tour-{r['tour']}.review.findings.md" if tour else
                                      f"{t['id']}.review-{r['round']}.rebase.findings.md" if rebase
                                      else f"{t['id']}.review-{r['round']}.findings.md")
    findings.write_text(text + "\n")
    rounds = repo.cfg["review_rounds"]
    r.update(verdict="approved" if args.verdict == "approve" else "changes", findings=str(findings))
    reviewer = (r.get("session"), r.get("block"))
    unindex_block(r["block"])
    r["block"] = None
    pr = where(t)
    t.pop("auto_wait", None)  # a new verdict decides again
    if tour:
        t.pop("tour_sent", None)  # reviewed: the next review follows up on this one
    if args.verdict == "approve":
        set_status(t, "ready")
        repo.save_task(t)
        if auto_accepts(repo, t):
            auto_accept(repo, t)
        else:
            why = f": {r['why']}" if r.get("why") else ""  # last, so not in brackets in brackets
            ready_news(repo, t, [commits(t.get("commits") or 0), pr,
                                 "a rebase check approved the conflict resolution" if rebase else
                                 f"approved by {review_kind({**r, 'why': None})}{why}"])
    elif tour or rebase or r["round"] < rounds:
        said = ("The reviewer says the person's comments aren't all addressed yet" if tour else
                "A rebase check says the conflict resolution isn't right" if rebase else
                f"The reviewer asked for changes (review round {r['round']} of {rounds})")
        msg = (f"{said}. Read {findings}, fix what you agree with, commit, and run your ready command "
               f"again. If you disagree with a finding, say why in your ready summary.")
        if t.get("gh"):
            t["gh"].pop("sent", None)  # it's the review it is fixing now, not GitHub
        try:
            failed = hand_back(repo, t, msg)
        except Fail as e:
            failed = clip(str(e), 160)
        if failed:
            # nobody is on it: the coordinator passes the findings on
            stop(repo, t, "worker", "not_sent", "Review findings didn't reach its worker",
                 f"{t['id']}'s reviewer asked for changes, but the message didn't reach its worker "
                 f"({failed}). The findings are in {findings}: `bullpen tell {t['id']}` it to read them",
                 "not-sent")
    else:
        set_status(t, "ready", f"Review still asks for changes after {rounds} rounds")
        repo.save_task(t)
        ready_news(repo, t, [pr], f", but review still asks for changes after {rounds} rounds: "
                   f"findings in {findings}")
    print(f"verdict recorded: {args.verdict}")
    sys.stdout.flush()
    rex_close(*reviewer)  # last: this closes the pane we may be running in


# ---------- auto-accept ----------
#
# With `config --auto-accept on`, an approve verdict runs the normal accept itself:
# the person said in advance that an approval is enough. A task's own "auto_accept"
# (`add`/`start --no-auto-accept`, or `--auto-accept`) wins over the config's. In PR
# mode it waits for the PR's checks ("auto_wait"), which the PR follow-up looks at.


def auto_accept(repo, t):
    """An approved task: accept it now, or, with a PR whose checks are still running,
    once they pass. Anything that stops it leaves the task ready, with news."""
    if t.get("pr") and not t.get("local"):
        state = pr_checks_now(repo, t)
        if state == "failing":
            ready_news(repo, t, [pr_short(t["pr"]), "approved by review"],
                       ", but not auto-accepted: its PR's checks are failing")
            start_queued(repo, t, "ready")  # its worker is done: its slot is free
            return
        if state == "running":
            t["auto_wait"] = True
            write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
            ready_news(repo, t, [pr_short(t["pr"]), "approved by review"],
                       "; it auto-accepts once its PR's checks pass")
            start_queued(repo, t, "ready")
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
        start_queued(repo, t, "ready")


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
    p = command(sub, "verdict", cmd_verdict, "Worker and reviewer",
                help="(reviewer) approve a task or send it back with findings")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("verdict", choices=["approve", "changes"], help="approve it, or send it back with changes to make")
    p.add_argument("--summary", help="what you did, as text")
    p.add_argument("--summary-file", metavar="FILE", help="the same, read from a file")

    p = command(sub, "_auto_accept", cmd_auto_accept)
    p.add_argument("id", help="the task, like BP-3")
