"""A task's worker or reviewer stopping: whether it goes to its coordinator (triage)
or to the person, and a triage passed on to the person once it is overdue.
"""

from .core import now, seconds_since, set_status
from .coordinators import sees_to
from .news import news


# A task that stopped on something its coordinator can handle (a failed turn, a turn
# ended without ready, a message that didn't reach its worker, a reviewer without a
# verdict) is "triage": its coordinator hears of it, and settles it with tell, resume,
# `review`, `ok` or `ask`. Still there after triage_minutes, or with no coordinator
# running, it needs the person ("needs"), as prompts and `ask` do at once.


def stop(repo, t, by, kind, why, lines=(), event="stopped"):
    """The task's worker or reviewer (by) stopped (kind; why in words). A prompt needs
    the person. A worker whose pane is gone is stopped: resume restarts it. Anything
    else waits on its coordinator, or on the person when no coordinator is running to
    see to it. Saves the task, then sends each of lines as news, worded for whoever has
    it now. Returns the task's new status."""
    if kind == "prompt":
        t.pop("ask", None)  # the prompt is what it needs the person for now
        status, say = "needs", (lambda line: line)
    elif by == "worker" and kind in ("exited", "pane_gone"):
        status, say = "stopped", (lambda line: line)
    elif sees_to(repo, t):
        m = triage_minutes(repo)
        status, say = "triage", (lambda line: f"For the coordinator: {line} (still here in {m} min, "
                                              f"it goes to the person)")
    else:
        status, say = "needs", (lambda line: f"{line}. No coordinator is running to see to it, "
                                             f"so it needs the person")
    set_status(t, status, why, {"by": by, "kind": kind})
    repo.save_task(t)
    for line in [lines] if isinstance(lines, str) else lines:
        news(repo, t, event, say(line))
    return status


def escalate(repo, t, late):
    """Pass a task its coordinator didn't settle on to the person, with one news line.
    late: (what its why gets, why it goes now), from overdue."""
    why = (t.get("why") or "Stopped") + late[0]
    set_status(t, "needs", why)
    repo.save_task(t)
    news(repo, t, "needs-you", f"{t['id']} needs the person: {why}")


def triage_minutes(repo):
    return repo.cfg["triage_minutes"]


def overdue(repo, t, at=None):
    """(what the reason gets, why) when a task waiting on its coordinator goes to the
    person now, else None."""
    if t["status"] != "triage":
        return None
    if not sees_to(repo, t):
        return " (no coordinator is running)", "no coordinator is running"
    m = triage_minutes(repo)
    if seconds_since(t.get("since") or t.get("updated") or now(), at) >= m * 60:
        return " (coordinator didn't resolve it)", f"its coordinator didn't resolve it in {m} min"
    return None
