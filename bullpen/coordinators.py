"""A repo's coordinators, and who sees to a task."""

import os

from .rex import rex_alive, rex_view


# A repo can have several coordinators, each an agent in its own pane with its own
# board beside it. cfg["coordinators"] lists them, oldest (the primary) first:
# {"id": "c1", "block", "session", "board": {"block", "session"}, "board_others"}.
# Its board shows only its own tasks (and those with no owner, or a gone one) unless
# board_others (the board's `o`) shows the others' too. The ID stays when
# a Rex restart gives the pane a new block. A task belongs to the coordinator that
# added it (t["coordinator"], its ID). Who sees to it (sees_to) is that one alone while
# its pane is running, else every running coordinator: they get its news, its stops
# and its panes, and their boards show it. A coordinator whose pane closes is dropped
# (`_coordinator_gone`, in Rust). `bullpen adopt` moves a task to another.


def coordinator(repo, cid):
    return next((c for c in repo.cfg["coordinators"] if cid and c["id"] == cid), None)


def coordinator_at(repo, block, boards=True):
    """The coordinator whose pane is block (or whose board is, with boards)."""
    if not block:
        return None
    for c in repo.cfg["coordinators"]:
        if c.get("block") == block or (boards and (c.get("board") or {}).get("block") == block):
            return c
    return None


def on_board(repo, c, t):
    """Does coordinator c's board show the task? Only one c sees to, or one nobody sees
    to, while c's board_others is off. With one coordinator (or c None), every task."""
    if not c or c.get("board_others") or len(repo.cfg["coordinators"]) < 2:
        return True
    sees = sees_to(repo, t)
    return not sees or any(o["id"] == c["id"] for o in sees)


def alive(c):
    return bool(c) and rex_alive(c.get("session"), c.get("block"))


def owner(repo, t):
    """The task's coordinator, if it is still one (an older task may have none)."""
    return coordinator(repo, t.get("coordinator"))


def primary(repo):
    cs = repo.cfg["coordinators"]
    return cs[0] if cs else None


def sees_to(repo, t):
    """The coordinators who see to a task: its owner while the owner's pane is running,
    else every running coordinator, else none (the task needs the person)."""
    own = owner(repo, t)
    if alive(own):
        return [own]
    return [c for c in repo.cfg["coordinators"] if alive(c)]


def home_of(repo, t):
    """Where a task's panes live: with the coordinator who sees to it, else (none
    running) with its owner, else the primary coordinator."""
    sees = sees_to(repo, t)
    return sees[0] if sees else owner(repo, t) or primary(repo) or {}


def calling_coordinator(repo):
    """The coordinator whose own pane ran this command, or None. Rex gives each pane
    its block ID as REX_BLOCK. Unsure (no REX_BLOCK, or a stored block that is out of
    date) counts as none."""
    return coordinator_at(repo, os.environ.get("REX_BLOCK"), boards=False)


def session_for(repo, args=None):
    """The Rex session to work in: --session, else the primary coordinator's while it
    exists (a Rex restart gives sessions new IDs), else the one we're running in."""
    s = getattr(args, "session", None) if args else None
    c = (primary(repo) or {}).get("session")
    if s:
        return s
    if c and (c == os.environ.get("REX_SESSION") or rex_view(c) is not None):
        return c
    return os.environ.get("REX_SESSION") or c

