"""`relink`: a repo's panes found again after a Rex restart, for `up` (Python's). `go`,
`hide` and `adopt` are Rust's (cli/src/go/), with its own relink; this copy goes with up.py
in T5."""

from .core import CLOSED, WAITING, index_block, set_status, unindex_block
from .rex import board_label, coordinator_label, find_pane, find_panes, review_label, rex_close, rex_view
from .agents import is_limited, whose_turn
from .coordinators import alive
from .panes import find
from .stops import stop


def relink(repo, session):
    """A Rex restart brings panes back with new block IDs: find bullpen's own again
    by their labels. Panes that are gone are forgotten (a working task is stopped)."""
    view = rex_view(session)
    if view is None:
        return
    known = {}

    def ours(stored):
        """Is a pane recorded in session `stored` one to look for here? Yes if it's
        this session, or if that session no longer exists (a restart renamed it)."""
        if stored in (None, session, view.get("session_id"), view.get("label")):
            return True
        if stored not in known:
            known[stored] = rex_view(stored) is None
        return known[stored]

    cs = repo.cfg["coordinators"]
    # A pane bullpen started is found again by its label; one made a coordinator with
    # `up --here` isn't (its label is its agent's): it's forgotten.
    taken = {c.get("block") for c in cs} | {(c.get("board") or {}).get("block") for c in cs}
    for c in list(cs):
        if find_pane(view, block=c.get("block")):
            c["session"] = session
            continue
        if not ours(c.get("session")) or alive(c):
            continue
        unindex_block(c.get("block"))
        hit = next((h for h in find_panes(view, coordinator_label(repo)) if h[2] not in taken), None)
        if hit:
            c.update(block=hit[2], session=session)
            taken.add(hit[2])
            index_block(hit[2], repo, "coordinator")
            print(f"coordinator {c['id']}: found again ({hit[2]})")
        else:
            b = c.get("board") or {}
            if find_pane(view, block=b.get("block")):
                rex_close(session, b["block"])  # its coordinator is gone
            unindex_block(b.get("block"))
            cs.remove(c)
            print(f"coordinator {c['id']}: its pane is gone, so it is dropped")
    for c in cs:
        b = c.get("board")
        if not b:
            continue
        if find_pane(view, block=b.get("block")):
            b["session"] = session
            continue
        if not ours(b.get("session")):
            continue
        unindex_block(b.get("block"))
        # the board beside its coordinator, else any board nobody has
        home = find_pane(view, block=c.get("block"))
        hits = [h for h in find_panes(view, board_label(repo)) if h[2] not in taken]
        hit = next((h for h in hits if home and h[0] == home[0]), hits[0] if hits else None)
        c["board"] = {"block": hit[2], "session": session} if hit else None
        if hit:
            taken.add(hit[2])
            index_block(hit[2], repo, "board")
            print(f"board of {c['id']}: found again ({hit[2]})")
    repo.save()
    for t in repo.tasks():
        if t["status"] in CLOSED or not t.get("block") or find(session, view, block=t["block"]):
            continue
        if not ours(t.get("session")):
            continue
        unindex_block(t["block"])
        hit = find(session, view, label=f"bullpen: {t['id']}")
        if hit:
            t.update(block=hit[2], session=session)
            index_block(hit[2], repo, t["id"])
            print(f"{t['id']}: found again ({hit[2]})")
        else:
            t["block"] = None
            if t["status"] in ("working", *WAITING) and not is_limited(t):  # a limited one is resumed later
                set_status(t, "stopped", "Its pane was gone", {"by": "worker", "kind": "pane_gone"})
        repo.save_task(t)
    relink_reviewers(repo, session, view)


def relink_reviewers(repo, session, view):
    for t in repo.tasks():
        r = t.get("review") or {}
        if t["status"] in CLOSED or not r.get("block") or find(session, view, block=r["block"]):
            continue
        unindex_block(r["block"])
        hit = find(session, view, label=review_label(t))
        r.update(block=hit[2] if hit else None, session=session)
        if hit:
            index_block(hit[2], repo, f"{t['id']} review")
        elif whose_turn(t) == "reviewer" and not r.get("verdict") and not is_limited(t):
            # gone after a Rex restart (a background reviewer always is): its coordinator
            # hears of it, as of one that exited (a limited one starts again at the reset)
            stop(repo, t, "reviewer", "pane_gone", "Reviewer's pane was gone",
                 f"{t['id']}'s reviewer's pane was gone after a Rex restart: "
                 f"`bullpen review {t['id']}` starts a new one")
        repo.save_task(t)
