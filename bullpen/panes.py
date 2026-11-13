"""Where a worker's, reviewer's or diff's pane lives: one placement each, with the same
steps (place, show, hide, find), so the code that makes, shows, hides or finds a pane
again never asks which one it is."""

from .core import CLOSED, FLOAT_BOUNDS, TERMINAL
from .rex import HIDE_HINT, find_pane, is_label, rex_api, rex_focus, rex_run, rex_view, set_layer, with_state
from .agents import placed
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

    def show(self, repo, session, where, view):
        """Bring up the pane at where (find's). False if the caller should focus it."""
        rex_focus(session, where[2])
        return True

    def hide(self, session, where):
        """Put the pane at where away. False if this placement has no away to put it."""
        return False

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
        args = {"bounds": FLOAT_BOUNDS, "layout": {"block": spec(t, label, argv, hint)}}
        where = find_pane(rex_view(session), block=home_of(repo, t).get("block"))
        if where:
            args["window_id"] = where[0]
        r = rex_api(session, "session.new_layer", args)
        set_layer(session, r["layer_id"], "minimized")
        return r["block_ids"][0]

    def show(self, repo, session, where, view):
        return set_layer(session, where[1]["layer_id"], "normal", focus=True)

    def hide(self, session, where):
        set_layer(session, where[1]["layer_id"], "minimized")
        return True

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


# Where a detached block is, as find gives it: in no window, on no layer.
DETACHED = {"kind": "detached"}


class Background(Placement):
    """A detached block: running, but in no tab, so nothing shows it. `go` floats it over
    its coordinator's tab, with focus; the hide key detaches it again. Rex's view lists
    only placed blocks, so a detached one is found through session.list_blocks. Rex keeps
    detached blocks only while its server runs (rexurrect doesn't save them either)."""
    name = "background"

    def place(self, repo, t, session, label, argv, hint=True, focus=False):
        block = rex_api(session, "session.new_block", spec(t, label, argv, hint))["block_id"]
        if focus:
            view = rex_view(session)
            if view is not None:
                self.show(repo, session, (None, DETACHED, block), view)
        return block

    def show(self, repo, session, where, view):
        if where[1].get("kind") == "floating":
            return set_layer(session, where[1]["layer_id"], "normal", focus=True)
        if where[1].get("kind") != "detached":
            return False  # moved into a tab by hand: focus it there
        anchor = anchor_for(repo, view, where[2])
        if not anchor or rex_api(session, "session.move_block",
                                 {"block_id": where[2], "anchor_block_id": anchor}, check=False) is None:
            return False
        r = rex_api(session, "session.float_block", {"block_id": where[2], "bounds": FLOAT_BOUNDS}, check=False)
        return bool(r) and set_layer(session, r["layer_id"], "normal", focus=True)

    def hide(self, session, where):
        return rex_api(session, "session.detach_block", {"block_id": where[2]}, check=False) is not None

    def find(self, session, view, block=None, label=None):
        if view is None:
            return None
        if "detached" not in view:  # one call to Rex per view, however many panes are looked for
            listed = rex_api(session, "session.list_blocks", check=False) or {}
            view["detached"] = [b for b in listed.get("blocks", []) if b.get("placement") == "detached"]
        for b in view["detached"]:
            if (block and b.get("block_id") == block) or (not block and is_label(b, label)):
                return None, DETACHED, b["block_id"]
        return None


FLOAT, TABS, BACKGROUND = Float(), Tabs(), Background()
PLACEMENTS = {p.name: p for p in (FLOAT, TABS, BACKGROUND)}


def spec(t, label, argv, hint):
    """The terminal block a placement makes for task t (the hint: the hide key's, in its label)."""
    return {"flavor": TERMINAL, "label": label + (HIDE_HINT if hint else ""),
            "options": {"command": with_state(argv), "cwd": str(t["worktree"])}}


def anchor_for(repo, view, block):
    """A block in the tab a background pane comes up over: its task's coordinator's, else
    the primary coordinator's, else whatever has focus in the active window."""
    t = owner_task(repo, block)
    for c in ([home_of(repo, t)] if t else []) + [primary(repo) or {}]:
        if find_pane(view, block=c.get("block")):
            return c["block"]
    w = next((w for w in view.get("windows", []) if w["window_id"] == view.get("active_window_id")), {})
    return w.get("focused_block_id")


def owner_task(repo, block):
    """The task whose worker or reviewer pane block is, or None."""
    return next((t for t in repo.tasks() if block in (t.get("block"), (t.get("review") or {}).get("block"))), None)


def setting(repo, key):
    return PLACEMENTS.get(repo.cfg.get(key))


def for_new(repo, t, role="worker"):
    """The placement for a new pane of task t's: role is "worker", "reviewer" or "diff".
    Workers follow the `workers` setting; reviewers the `reviewers` one ("same": the
    workers'). A diff floats, or gets a tab when workers do: never in the background,
    since it's made to be looked at."""
    if role == "diff":
        return TABS if repo.cfg.get("workers") == "tabs" else FLOAT
    if role == "reviewer" and repo.cfg.get("reviewers") != "same":
        return setting(repo, "reviewers") or BACKGROUND
    return setting(repo, "workers") or FLOAT


def made_with(repo, t, role):
    """The placement task t's worker or reviewer pane was made with: recorded since
    background panes came; before that, workers and reviewers followed `workers`."""
    return PLACEMENTS.get(placed(t, role)) or (TABS if repo.cfg.get("workers") == "tabs" else FLOAT)


def placement_at(where, made=None):
    """The placement that shows and hides a pane, by where it is now (find's), not by the
    setting: a pane started before the setting changed keeps the way it was placed. A
    background pane that's up floats like a capsule: made says which it is."""
    if not where:
        return None
    if made is BACKGROUND or where[1].get("kind") == "detached":
        return BACKGROUND
    return FLOAT if where[1].get("kind") == "floating" else TABS


def made(repo, block):
    """The placement block (a worker's or reviewer's pane) was made with, or None."""
    t = owner_task(repo, block)
    if not t:
        return None
    return made_with(repo, t, "worker" if t.get("block") == block else "reviewer")


def place(repo, t, session, label, argv, role="worker", hint=True, focus=False):
    """Start a pane for task t where its role's placement puts it: (block, the placement's
    name), which the caller records with the pane."""
    p = for_new(repo, t, role)
    return p.place(repo, t, session, label, argv, hint=hint, focus=focus), p.name


def find(session, view, block=None, label=None):
    """(window_id, layer, block_id) of a worker's or reviewer's pane, by ID, else by label,
    wherever it lives, or None. A detached one has no window, and its layer's kind is
    "detached"."""
    for p in (FLOAT, BACKGROUND):  # FLOAT looks through the view, as TABS would
        hit = p.find(session, view, block=block, label=label)
        if hit:
            return hit
    return None


def mine(repo):
    """The blocks of every worker and reviewer pane the repo knows."""
    return {b for t in repo.tasks() for b in (t.get("block"), (t.get("review") or {}).get("block")) if b}


def put_away_others(repo, session, view, keep):
    """Put away every worker's or reviewer's pane that floats over a tab, but not keep's:
    a capsule is minimized, a background pane detached."""
    ours = mine(repo)
    for w in view["windows"]:
        for layer in w.get("layers", []):
            if layer.get("kind") != "floating" or layer.get("state") == "minimized":
                continue
            if keep and keep[1].get("layer_id") == layer["layer_id"]:
                continue
            for b in layer.get("blocks", []):
                if b.get("block_id") in ours:
                    where = (w["window_id"], layer, b["block_id"])
                    placement_at(where, made(repo, b["block_id"])).hide(session, where)
                    break


def show(repo, session, block):
    """Focus a pane. A worker's or reviewer's comes up from where it's put away, and any
    other of theirs that's up is put away."""
    gather(repo, session)
    view = rex_view(session)
    if view is None:
        return rex_focus(session, block)
    target = find(session, view, block=block)
    put_away_others(repo, session, view, target)
    p = placement_at(target, made(repo, block))
    if not (p and p.show(repo, session, target, view)):
        rex_focus(session, block)


def hide(repo, session, block):
    """Put a pane away. False if it isn't somewhere bullpen can put it away from."""
    where = find(session, rex_view(session), block=block)
    p = placement_at(where, made(repo, block))
    return bool(p) and p.hide(session, where)


def gather(repo, session):
    """Keep every open task's worker and reviewer pane where its placement says, in its
    coordinator's tab (the primary coordinator's, when its own is gone). A coordinator
    that was restarted opens in a new tab: the panes move there, and a tab that moving
    them left empty closes. Tabs and background panes are left where they are."""
    panes = [(t, b, p) for t in repo.tasks() if t["status"] not in CLOSED
             for b, p in ((t.get("block"), made_with(repo, t, "worker")),
                          ((t.get("review") or {}).get("block"), made_with(repo, t, "reviewer")))
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
