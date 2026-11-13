"""A repo's coordinators, and who sees to a task (bullpen/coordinators.py).

A coordinator is an object in repo.cfg's "coordinators" list: these return its node
index in repo.cfg, or -1 for none. Read its fields with repo.cfg.str(c, "block")."""

from std.os import getenv

from bullpen.core import Repo, Task
from bullpen.rex import rex_alive


def coordinators(repo: Repo) -> List[Int]:
    var out = List[Int]()
    var cs = repo.cfg.get(0, "coordinators")
    for i in range(repo.cfg.len(cs)):
        out.append(repo.cfg.item(cs, i))
    return out^


def coordinator(repo: Repo, cid: String) -> Int:
    if not cid:
        return -1
    for c in coordinators(repo):
        if repo.cfg.str(c, "id") == cid:
            return c
    return -1


def coordinator_at(repo: Repo, block: String, boards: Bool = True) -> Int:
    """The coordinator whose pane is block (or whose board is, with boards)."""
    if not block:
        return -1
    for c in coordinators(repo):
        if repo.cfg.str(c, "block") == block or (boards and repo.cfg.str(repo.cfg.get(c, "board"), "block") == block):
            return c
    return -1


def alive(repo: Repo, c: Int) raises -> Bool:
    return c >= 0 and rex_alive(repo.cfg.str(c, "session"), repo.cfg.str(c, "block"))


def owner(repo: Repo, t: Task) -> Int:
    """The task's coordinator, if it is still one."""
    return coordinator(repo, t.s("coordinator"))


def primary(repo: Repo) -> Int:
    var cs = coordinators(repo)
    return cs[0] if len(cs) else -1


def sees_to(repo: Repo, t: Task) raises -> List[Int]:
    """Its owner while the owner's pane is running, else every running coordinator."""
    var own = owner(repo, t)
    if alive(repo, own):
        return [own]
    var out = List[Int]()
    for c in coordinators(repo):
        if alive(repo, c):
            out.append(c)
    return out^


def calling_coordinator(repo: Repo) -> Int:
    """The coordinator whose own pane ran this command (REX_BLOCK), or -1."""
    return coordinator_at(repo, getenv("REX_BLOCK"), False)
