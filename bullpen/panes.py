"""Where a worker's, reviewer's or diff's pane lives: one placement each, with the same
four steps (place, show, hide, find), so the code that makes, shows, hides or finds a
pane again never asks which one it is."""

from .core import CLOSED, FLOAT_BOUNDS, TERMINAL
from .rex import HIDE_HINT, find_pane, rex_api, rex_focus, rex_run, rex_view, set_layer, with_state
from .coordinators import home_of, primary


class Placement:
    """One way a pane can live. A pane bullpen placed is found again in the session's
    view, by its ID or its label (a Rex restart gives it a new ID)."""
    name = None
    gathers = False  # does `gather` keep its panes in their coordinator's tab?

    def place(self, repo, t, session, label, argv, hint=True, focus=False):
        """Start argv in task t's worktree, in a new pane labelled label, and return its
        block. hint: it's an agent's pane, which the hide key puts away; focus: show it now."""
        raise NotImplementedError

    def show(self, session, where):
        """Bring up the pane at where (find_pane's). False if the caller should focus it."""
        rex_focus(session, where[2])
        return True

    def hide(self, session, where):
        """Put the pane at where away. False if this placement has no away to put it."""
        return False

    def put_away_others(self, session, view, mine, keep):
        """Put away every pane of bullpen's (blocks in mine) that's up, but not keep's."""

    def keep_home(self, repo, session, view, block, c, home):
        """gather: the pane is in another tab than its coordinator's (c, at home): move it
        there. The window it left, else None."""
        return None

    def find(self, session, view, block=None, label=None):
        """(window_id, layer, block_id) of the pane with this ID, else this label, or None."""
        return find_pane(view, block=block, label=label)


class Float(Placement):
    """A minimized floating pane (a capsule) in the window of its task's coordinator:
    `go` brings it up, the hide key minimizes it again."""
    name = "float"
    gathers = True

    def place(self, repo, t, session, label, argv, hint=True, focus=False):
        args = {"bounds": FLOAT_BOUNDS, "layout": {"block": {
            "flavor": TERMINAL, "label": label + (HIDE_HINT if hint else ""),
            "options": {"command": with_state(argv), "cwd": str(t["worktree"])}}}}
        where = find_pane(rex_view(session), block=home_of(repo, t).get("block"))
        if where:
            args["window_id"] = where[0]
        r = rex_api(session, "session.new_layer", args)
        set_layer(session, r["layer_id"], "minimized")
        return r["block_ids"][0]

    def show(self, session, where):
        return set_layer(session, where[1]["layer_id"], "normal", focus=True)

    def hide(self, session, where):
        set_layer(session, where[1]["layer_id"], "minimized")
        return True

    def put_away_others(self, session, view, mine, keep):
        for w in view["windows"]:
            for layer in w.get("layers", []):
                if layer.get("kind") != "floating" or layer.get("state") == "minimized":
                    continue
                if keep and layer["layer_id"] == keep[1]["layer_id"]:
                    continue
                if any(b.get("block_id") in mine for b in layer.get("blocks", [])):
                    set_layer(session, layer["layer_id"], "minimized")

    def keep_home(self, repo, session, view, block, c, home):
        where = find_pane(view, block=block)
        if not where or where[0] == home[0]:
            return None
        if rex_api(session, "session.move_block", {"block_id": block, "anchor_block_id": c["block"]},
                   check=False) is None:
            return None
        r = rex_api(session, "session.float_block", {"block_id": block, "bounds": FLOAT_BOUNDS}, check=False)
        if r:
            set_layer(session, r["layer_id"], "minimized")
        return where[0]


class Tabs(Placement):
    """A tab of its own. Shown by focusing it; the hide key has nothing to do there."""
    name = "tabs"

    def place(self, repo, t, session, label, argv, hint=True, focus=False):
        return rex_run(session, t["worktree"], label, argv, focus=focus)


FLOAT, TABS = Float(), Tabs()
PLACEMENTS = {p.name: p for p in (FLOAT, TABS)}


def for_new(repo, t, role="worker"):
    """The placement for a new pane of task t's: role is "worker", "reviewer" or "diff".
    Today all three follow the `workers` setting."""
    return PLACEMENTS.get(repo.cfg.get("workers")) or FLOAT


def placement_at(where):
    """The placement of a pane, by where it is now (find_pane's), not by the setting: a
    pane started before the setting changed keeps the way it was placed."""
    if not where:
        return None
    return FLOAT if where[1].get("kind") == "floating" else TABS


def place(repo, t, session, label, argv, role="worker", hint=True, focus=False):
    """Start a pane for task t where its role's placement puts it; its block."""
    return for_new(repo, t, role).place(repo, t, session, label, argv, hint=hint, focus=focus)


def find(session, view, block=None, label=None):
    """(window_id, layer, block_id) of a worker's or reviewer's pane, by ID, else by label,
    wherever it lives, or None."""
    for p in PLACEMENTS.values():
        hit = p.find(session, view, block=block, label=label)
        if hit:
            return hit
    return None


def mine(repo):
    """The blocks of every worker and reviewer pane the repo knows."""
    return {b for t in repo.tasks() for b in (t.get("block"), (t.get("review") or {}).get("block")) if b}


def show(repo, session, block):
    """Focus a pane. A worker's or reviewer's comes up from where it's put away, and any
    other of theirs that's up is put away."""
    gather(repo, session)
    view = rex_view(session)
    if view is None:
        return rex_focus(session, block)
    target = find(session, view, block=block)
    ours = mine(repo)
    for p in PLACEMENTS.values():
        p.put_away_others(session, view, ours, target)
    p = placement_at(target)
    if not (p and p.show(session, target)):
        rex_focus(session, block)


def hide(session, block):
    """Put a pane away. False if it isn't somewhere bullpen can put it away from."""
    where = find(session, rex_view(session), block=block)
    p = placement_at(where)
    return bool(p) and p.hide(session, where)


def gather(repo, session):
    """Keep every open task's worker and reviewer pane where its placement says, in its
    coordinator's tab (the primary coordinator's, when its own is gone). A coordinator
    that was restarted opens in a new tab: the panes move there, and a tab that moving
    them left empty closes."""
    panes = [(t, b, p) for t in repo.tasks() if t["status"] not in CLOSED
             for b, p in ((t.get("block"), for_new(repo, t, "worker")),
                          ((t.get("review") or {}).get("block"), for_new(repo, t, "reviewer")))
             if b and p.gathers]
    if not panes:
        return
    view = rex_view(session)
    fallback = primary(repo) or {}
    emptied = set()
    for t, b, p in panes:
        c = home_of(repo, t)
        home = find_pane(view, block=c.get("block"))
        if not home:
            c, home = fallback, find_pane(view, block=fallback.get("block"))
        left = p.keep_home(repo, session, view, b, c, home) if home else None
        if left:
            emptied.add(left)
    if emptied:
        for w in (rex_view(session) or {}).get("windows", []):
            if w["window_id"] in emptied and not any(l.get("blocks") for l in w.get("layers", [])):
                rex_api(session, "session.close_window", {"window_id": w["window_id"]}, check=False)
