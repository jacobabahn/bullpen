"""A task's agents (bullpen/agents.py): where its worker's and reviewer's panes are,
and the one the task waits on. "" stands for Python's None."""

from bullpen.core import Task, waiting, reviewers_stop
from bullpen.rex import rex_alive


def pane_session(t: Task, who: String) -> String:
    """The session of the task's worker or reviewer pane: a reviewer runs in its
    worker's session unless its own is recorded."""
    if who == "reviewer":
        var s = t.doc.str(t.get("review"), "session")
        return s if s else t.s("session")
    return t.s("session")


def pane_block(t: Task, who: String) -> String:
    if who == "reviewer":
        return t.doc.str(t.get("review"), "block")
    return t.s("block")


def is_limited(t: Task) -> Bool:
    """Is the agent the task is waiting on stopped at the usage limit?"""
    var lim = t.get("limit")
    if not t.doc.is_true(lim):
        return False
    return t.status() == ("review" if t.doc.str(lim, "who") == "reviewer" else "working")


def on_reviewer(t: Task) -> Bool:
    return waiting(t.status()) and reviewers_stop(t)


def whose_turn(t: Task) -> String:
    if t.status() == "review" or on_reviewer(t):
        return "reviewer"
    if t.status() == "working" or waiting(t.status()):
        return "worker"
    return ""


def waits_on(t: Task) -> String:
    """The agent the task waits on now, or "" (the usage limit, or no agent)."""
    return "" if is_limited(t) else whose_turn(t)


def running(t: Task, who: String = "") raises -> Bool:
    """Is the pane of the task's worker or reviewer (by default the one it waits on) alive?"""
    var w = who if who else waits_on(t)
    return w.byte_length() > 0 and rex_alive(pane_session(t, w), pane_block(t, w))
