"""A triage passed on to the person once it is overdue (bullpen/stops.py), as far as
`check` reads it. Stopping and escalating (which save and send news) stay in Python."""

from bullpen.core import Repo, Task, stamp
from bullpen.coordinators import sees_to
from bullpen.timeutil import seconds_since


struct Late(Copyable, Movable):
    """overdue's (what the reason gets, why); why "" for not overdue."""

    var gets: String
    var why: String

    def __init__(out self, gets: String, why: String):
        self.gets = gets
        self.why = why


def triage_minutes(repo: Repo) -> Float64:
    return repo.cfg.num(repo.cfg.get(0, "triage_minutes"))


def overdue(repo: Repo, t: Task) raises -> Late:
    """Late("", "") unless the task waits on its coordinator and goes to the person now."""
    if t.status() != "triage":
        return Late("", "")
    if len(sees_to(repo, t)) == 0:
        return Late(" (no coordinator is running)", "no coordinator is running")
    var m = triage_minutes(repo)
    var since = stamp(t)
    if (seconds_since(since) if since else 0.0) >= m * 60:
        return Late(" (coordinator didn't resolve it)", "its coordinator didn't resolve it in " + minutes_text(repo) + " min")
    return Late("", "")


def minutes_text(repo: Repo) -> String:
    """triage_minutes as written in the config (an int, or a float's repr)."""
    var n = repo.cfg.get(0, "triage_minutes")
    return repo.cfg.nodes[n].text if n >= 0 else ""
