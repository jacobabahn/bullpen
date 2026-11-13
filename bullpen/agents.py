"""A task's agents: its worker and its reviewer, where each one's pane is, and the one
the task waits on."""

from .core import WAITING, reviewers_stop
from .rex import rex_alive


def pane(t, who):
    """(session, block) of the task's worker or reviewer pane; block None when it has
    none. A reviewer runs in its worker's session unless its own is recorded."""
    if who == "reviewer":
        r = t.get("review") or {}
        return r.get("session") or t.get("session"), r.get("block")
    return t.get("session"), t.get("block")


def placed(t, who):
    """The placement the task's worker or reviewer pane was made with ("float", "tabs",
    "background"), or None for one made before bullpen recorded it."""
    return ((t.get("review") or {}) if who == "reviewer" else t).get("pane")


def is_limited(t):
    """Is the agent the task is waiting on stopped at the usage limit?"""
    lim = t.get("limit")
    return bool(lim) and t["status"] == ("review" if lim["who"] == "reviewer" else "working")


def on_reviewer(t):
    """Is the task stopped on its reviewer (a prompt, or no verdict)?"""
    return t["status"] in WAITING and reviewers_stop(t)


def whose_turn(t):
    """"reviewer" while the task is in review or its stop is the reviewer's (but not a
    usage-limit stop, seen to as its worker's), "worker" while it's working or
    stopped on anything else, else None. The usage limit makes no difference here."""
    if t["status"] == "review" or on_reviewer(t):
        return "reviewer"
    if t["status"] in ("working", *WAITING):
        return "worker"
    return None


def waits_on(t):
    """"worker" or "reviewer": the agent the task waits on now (whose_turn), or None
    when it waits on the usage limit, or on no agent."""
    return None if is_limited(t) else whose_turn(t)


def waited_on(t):
    """(who, session, block) of the agent the task waits on now, or None: it waits on
    none, or that agent has no pane."""
    who = waits_on(t)
    session, block = pane(t, who) if who else (None, None)
    return (who, session, block) if block else None


def running(t, who=None):
    """Is the pane of the task's worker or reviewer (by default the one it waits on) alive?"""
    who = who or waits_on(t)
    return bool(who) and rex_alive(*pane(t, who))
