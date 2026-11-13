"""A task's agents (bullpen/agents.py): where its worker's and reviewer's panes are, and
the one the task waits on."""

from bullpen.core import Task, waiting, reviewers_stop
from bullpen.rex import rex_alive


struct Agent(Copyable, Movable):
    """(who, session, block): an agent's pane. who is "" for none."""

    var who: String
    var session: String
    var block: String

    def __init__(out self, who: String, session: String, block: String):
        self.who = who
        self.session = session
        self.block = block


def pane(t: Task, who: String) -> Agent:
    """The task's worker or reviewer pane; block "" when it has none. A reviewer runs in
    its worker's session unless its own is recorded."""
    if who == "reviewer":
        var r = t.get("review")
        var session = t.sub(r, "session")
        return Agent(who, session if session else t.s("session"), t.sub(r, "block"))
    return Agent(who, t.s("session"), t.s("block"))


def is_limited(t: Task) -> Bool:
    """Is the agent the task is waiting on stopped at the usage limit?"""
    var lim = t.get("limit")
    if not t.doc.is_true(lim):
        return False
    return t.status() == ("review" if t.sub(lim, "who") == "reviewer" else "working")


def on_reviewer(t: Task) -> Bool:
    return waiting(t.status()) and reviewers_stop(t)


def whose_turn(t: Task) -> String:
    if t.status() == "review" or on_reviewer(t):
        return "reviewer"
    if t.status() == "working" or waiting(t.status()):
        return "worker"
    return ""


def waits_on(t: Task) -> String:
    return "" if is_limited(t) else whose_turn(t)


def waited_on(t: Task) -> Agent:
    """The agent the task waits on now; who "" when it waits on none, or that agent has no pane."""
    var who = waits_on(t)
    if not who:
        return Agent("", "", "")
    var p = pane(t, who)
    return p^ if p.block else Agent("", "", "")


def running(t: Task, who: String = "") raises -> Bool:
    """Is the pane of the task's worker or reviewer (by default the one it waits on) alive?"""
    var w = who if who else waits_on(t)
    if not w:
        return False
    var p = pane(t, w)
    return rex_alive(p.session, p.block)
