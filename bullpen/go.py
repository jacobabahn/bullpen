"""Getting around: `go`, `hide`, and putting a repo's panes where they belong."""

import json
import os
import sys
from pathlib import Path

from .core import (
    CLOSED, FLOAT_BOUNDS, Fail, Repo, STATE, WAITING, command, find_repo, index_block,
    is_status, known_repos, lookup_block, read_json, repo_root, safe, set_status, unindex_block,
)
from .rex import (
    board_label, coordinator_label, find_pane, find_panes, review_label, rex_alive, rex_api,
    rex_close, rex_focus, rex_view, set_layer,
)
from .coordinators import (
    alive, calling_coordinator, coordinator_at, home_of, on_board, owner, primary, session_for,
)
from .limit import is_limited
from .board import ensure_board


def go_target(repo, args):
    """(repo, session, block, name) of the pane `go` should focus."""
    if args.next:
        # only over the tasks the board of the pane's coordinator shows
        c = pane_coordinator(repo, args.from_block)
        needs = [(repo, t) for t in repo.tasks() if t["status"] == "needs" and t.get("block")
                 and on_board(repo, c, t)]
        if args.all_repos:  # and every other repository's, whoever's board they're on
            needs += [(r, t) for r in known_repos() if r.dir != repo.dir
                      for t in r.tasks() if t["status"] == "needs" and t.get("block")]
        needs.sort(key=lambda rt: rt[1].get("since") or rt[1].get("updated") or "")
        if not needs:
            raise Fail("nothing needs you" + (" in any repository" if args.all_repos else ""))
        blocks = [t["block"] for _, t in needs]
        here = args.from_block
        r, t = needs[(blocks.index(here) + 1) % len(needs) if here in blocks else 0]
        return r, t["session"], t["block"], t["id"] if r.dir == repo.dir else f"{r.root.name} {t['id']}"
    return (repo, *pane_target(repo, args))


def next_to_accept(repo, args):
    """`go --next` once nothing needs you: (repo, task) of the task ready to accept, else
    finished, that has waited longest, on a board (the pane's coordinator's, in this
    repository), or None."""
    c = pane_coordinator(repo, args.from_block)
    found = [(repo, t) for t in repo.tasks() if t["status"] in ("ready", "finished") and on_board(repo, c, t)]
    if args.all_repos:
        found += [(r, t) for r in known_repos() if r.dir != repo.dir
                  for t in r.tasks() if t["status"] in ("ready", "finished")]

    def boarded(r, t):
        try:
            return bool(task_board(r, t))
        except Fail:
            return False
    found = [(r, t) for r, t in found if boarded(r, t)]
    found.sort(key=lambda rt: (rt[1]["status"] != "ready", rt[1].get("since") or rt[1].get("updated") or ""))
    return found[0] if found else None


def pane_target(repo, args):
    """(session, block, name) of a task's pane, its reviewer's, the coordinator's or the board's."""
    if args.id and args.board:
        _, b = task_board(repo, repo.task(args.id))
        return b.get("session"), b["block"], "board"
    if args.id and args.review:
        t = repo.task(args.id)
        r = t.get("review") or {}
        if not r.get("block"):
            raise Fail(f"{t['id']} has no reviewer running")
        return r.get("session") or t["session"], r["block"], f"{t['id']}'s reviewer"
    if args.id:
        t = repo.task(args.id)
        if not t.get("block"):
            hint = f"`bullpen resume {t['id']}` starts it again" if t["status"] == "stopped" else \
                is_status(t).replace(f"{t['id']} is ", "it's ", 1).replace(f"{t['id']} ", "it ", 1)
            raise Fail(f"{t['id']} has no pane: {hint}")
        return t["session"], t["block"], t["id"]
    c = coordinator_for(repo, args.from_block)
    if not c:
        raise Fail("no coordinator: run `bullpen up`")
    if args.board:
        b = c.get("board") or next((o["board"] for o in repo.cfg["coordinators"] if o.get("board")), None)
        if not b:
            raise Fail("no board: run `bullpen up`")
        return b.get("session"), b["block"], "board"
    return c.get("session"), c["block"], "coordinator"


def task_board(repo, t):
    """(coordinator, board) of a board that shows the task: its owner's, else any
    other that shows it (one showing other coordinators' tasks too)."""
    own = home_of(repo, t)
    boards = [c for c in [own] + repo.cfg["coordinators"] if c.get("board")]
    if not boards:
        raise Fail("no board: run `bullpen up`")
    shows = [c for c in boards if on_board(repo, c, t)]
    # a board that's running first: one that was quit would only fail
    c = next((c for c in shows if rex_alive(c["board"].get("session"), c["board"].get("block"))),
             shows[0] if shows else None)
    if not c:
        raise Fail(f"no board shows {t['id']}: it is {own['id']}'s, which has no board (o on a board shows it)")
    return c, c["board"]


def select_on_board(repo, tid):
    """Ask the board the task is on to select it. The board reads select/<its
    coordinator> when the state directory changes, selects the task and removes the
    file; it ignores one older than 30 seconds (no board read it then)."""
    c, _ = task_board(repo, repo.task(tid))
    path = repo.dir / "select" / safe(c.get("id") or "board")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(repo.task(tid)["id"] + "\n")


def coordinator_for(repo, block):
    """The coordinator a pane goes with (pane_coordinator), else the primary one."""
    return pane_coordinator(repo, block) or primary(repo)


def pane_coordinator(repo, block):
    """The coordinator a pane goes with: itself, the one beside its board, or the one
    owning its task; None for any other pane."""
    c = coordinator_at(repo, block)
    if not c and block:
        entry = read_json(STATE / "blocks" / f"{safe(block)}.json") or {}
        role = entry.get("role") or ""
        if entry.get("repo") == str(repo.root) and role not in ("coordinator", "board"):
            try:
                c = owner(repo, repo.task(role.split()[0]))
            except Fail:
                pass
    return c


def cmd_go(args):
    if args.all_repos and not args.next:
        raise Fail("--all-repos goes with --next")
    home = go_home(args)
    if args.board and not args.id:
        home = board_for_go(home, args)
    try:
        repo, session, block, name = go_target(home, args)
    except Fail as e:
        if not args.next or not str(e).startswith("nothing needs you"):
            raise
        boards = any(c.get("board") for c in home.cfg["coordinators"])
        if boards:  # a board that was quit starts again, as `go --board` does
            home = board_for_go(home, args)
        up = next_to_accept(home, args)
        if up:
            # nothing needs you, but something is yours to accept or close: the board, on it
            r, t = up
            name = t["id"] if r.dir == home.dir else f"{r.root.name} {t['id']}"
            print(f"bullpen: {e}; {name} is {t['status']}, on the board", file=sys.stderr)
            args.next, args.board, args.id, home = False, True, t["id"], r
        elif boards:
            # A key that does nothing looks broken: show the board, where that's plain.
            print(f"bullpen: {e}; showing the board", file=sys.stderr)
            args.next, args.board = False, True
        else:
            raise
        repo, session, block, name = go_target(home, args)
    if not rex_alive(session, block):
        # after a Rex restart its ID changed (and maybe its session's)
        relink(repo, args.session or os.environ.get("REX_SESSION") or session)
        home = Repo(home.root)
        repo, session, block, name = go_target(home, args)
        if not rex_alive(session, block):
            raise Fail(f"{name}'s pane is gone")
    if args.json:
        print(json.dumps({"session": session, "block": block, "name": name}))
    else:
        if args.board and args.id:
            select_on_board(repo, args.id)
        show_pane(repo, session, block)


def board_for_go(repo, args):
    """`go --board`: when the board it would show isn't running, start the pane's
    coordinator's (as `up` would), rather than refuse. The repo, read again if a Rex
    restart had to be caught up with."""
    c = coordinator_for(repo, args.from_block)
    if c and not alive(c):  # after a Rex restart its ID changed (and maybe its session's)
        relink(repo, args.session or os.environ.get("REX_SESSION") or c.get("session"))
        repo = Repo(repo.root)
        c = coordinator_for(repo, args.from_block)
    if not alive(c):
        return repo  # go_target says why
    b = c.get("board") or next((o["board"] for o in repo.cfg["coordinators"] if o.get("board")), None)
    if not (b and rex_alive(b.get("session"), b.get("block"))):
        ensure_board(repo, c["session"], c, out=sys.stderr)
    return repo


def go_home(args):
    """The repository `go` is about: --repo, else the one the pane or folder belongs to."""
    if not args.repo:
        return find_repo(args.from_block)
    repo = Repo(Path(args.repo).resolve())
    if not (repo.dir / "config.json").exists():
        raise Fail(f"bullpen has no tasks for {args.repo}")
    return repo


def cmd_hide(args):
    """Put the worker or reviewer pane you're in back into its capsule and go back
    to the board. Exits 3 if that pane isn't one of bullpen's floating panes, so a
    key binding can let the key through."""
    repo, role = lookup_block(args.from_block)
    if not repo or role in ("coordinator", "board"):
        sys.exit(3)
    t = repo.task(role.split()[0])
    session = args.session or (t.get("review") or {}).get("session") or t.get("session")
    where = find_pane(rex_view(session), block=args.from_block)
    if not where or where[1].get("kind") != "floating":
        sys.exit(3)
    set_layer(session, where[1]["layer_id"], "minimized")
    c = home_of(repo, t)
    for p in (c.get("board") or {}, c):
        if rex_alive(p.get("session"), p.get("block")):
            rex_focus(p.get("session"), p["block"])
            break


def show_pane(repo, session, block):
    """Focus a pane. A worker's floating pane comes up from its capsule; any other
    worker pane that's up goes back to being a capsule."""
    gather(repo, session)
    view = rex_view(session)
    if view is None:
        return rex_focus(session, block)
    mine = {b for t in repo.tasks() for b in (t.get("block"), (t.get("review") or {}).get("block")) if b}
    target = find_pane(view, block=block)
    for w in view["windows"]:
        for layer in w.get("layers", []):
            if layer.get("kind") != "floating" or layer.get("state") == "minimized":
                continue
            if target and layer["layer_id"] == target[1]["layer_id"]:
                continue
            if any(b.get("block_id") in mine for b in layer.get("blocks", [])):
                set_layer(session, layer["layer_id"], "minimized")
    if target and target[1].get("kind") == "floating":
        if set_layer(session, target[1]["layer_id"], "normal", focus=True):
            return
    rex_focus(session, block)


# ---------- a repo's panes, back where they belong: `up`, `go`, `adopt` ----------


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
        if t["status"] in CLOSED or not t.get("block") or find_pane(view, block=t["block"]):
            continue
        if not ours(t.get("session")):
            continue
        unindex_block(t["block"])
        hit = find_pane(view, label=f"bullpen: {t['id']}")
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


def gather(repo, session):
    """Keep every worker and reviewer pane in its coordinator's tab, as capsules (the
    primary coordinator's, when its own is gone). A coordinator that was restarted
    opens in a new tab: move the panes there and close any tab that moving them left
    empty."""
    if repo.cfg.get("workers") == "tabs":
        return
    view = rex_view(session)
    fallback = primary(repo) or {}
    emptied = set()
    for t in repo.tasks():
        if t["status"] in CLOSED:
            continue
        c = home_of(repo, t)
        home = find_pane(view, block=c.get("block"))
        if not home:
            c, home = fallback, find_pane(view, block=fallback.get("block"))
        if not home:
            continue
        for b in (t.get("block"), (t.get("review") or {}).get("block")):
            where = find_pane(view, block=b) if b else None
            if not where or where[0] == home[0]:
                continue
            if rex_api(session, "session.move_block", {"block_id": b, "anchor_block_id": c["block"]},
                       check=False) is None:
                continue
            r = rex_api(session, "session.float_block", {"block_id": b, "bounds": FLOAT_BOUNDS}, check=False)
            if r:
                set_layer(session, r["layer_id"], "minimized")
            emptied.add(where[0])
    if emptied:
        for w in (rex_view(session) or {}).get("windows", []):
            if w["window_id"] in emptied and not any(l.get("blocks") for l in w.get("layers", [])):
                rex_api(session, "session.close_window", {"window_id": w["window_id"]}, check=False)


def relink_reviewers(repo, session, view):
    for t in repo.tasks():
        r = t.get("review") or {}
        if t["status"] in CLOSED or not r.get("block") or find_pane(view, block=r["block"]):
            continue
        unindex_block(r["block"])
        hit = find_pane(view, label=review_label(t))
        r.update(block=hit[2] if hit else None, session=session)
        if hit:
            index_block(hit[2], repo, f"{t['id']} review")
        repo.save_task(t)


def cmd_adopt(args):
    """Make the coordinator in this pane the task's own: its news comes here."""
    repo = Repo(repo_root())
    c = calling_coordinator(repo)
    if not c:
        raise Fail("run `adopt` from a coordinator's own pane (`bullpen up --here` makes one)")
    for tid in args.ids:
        t = repo.task(tid)
        was = t.get("coordinator")
        if was == c["id"]:
            print(f"{t['id']} is yours already")
            continue
        t["coordinator"] = c["id"]
        repo.save_task(t)
        print(f"{t['id']} is yours now (was {was or 'nobody'}'s): its news comes here")
    session = session_for(repo)
    if session:
        gather(repo, session)  # its panes move to this coordinator's tab


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "go", cmd_go, "You", help="focus a task's pane (or the coordinator's, or the board's)")
    p.add_argument("id", nargs="?", help="the task (default: the coordinator)")
    p.add_argument("--next", action="store_true",
                   help="the next task that needs you; once none does, the board on one ready to accept")
    p.add_argument("--all-repos", action="store_true",
                   help="with --next: in any repository bullpen knows, not just this one")
    p.add_argument("--board", action="store_true",
                   help="the task board (with an ID: the task's board, with that task selected)")
    p.add_argument("--review", action="store_true", help="the task's reviewer, not its worker")
    p.add_argument("--from", dest="from_block", default=os.environ.get("REX_BLOCK"),
                   help="the pane you're in (default: this one)")
    p.add_argument("--session", help="the session you're in (key bindings pass it)")
    p.add_argument("--repo", help="the repository the task is in (default: the pane's, else this folder's)")
    p.add_argument("--json", action="store_true", help="print the target instead of focusing it")

    p = command(sub, "hide", cmd_hide, "You", help="put the worker pane you're in back into its capsule")
    p.add_argument("--from", dest="from_block", default=os.environ.get("REX_BLOCK"),
                   help="the pane to put away (default: this one)")
    p.add_argument("--session", help="the session you're in (key bindings pass it)")

    p = command(sub, "adopt", cmd_adopt, "Coordinator",
                help="(coordinator) make tasks yours: their news comes to this pane")
    p.add_argument("ids", nargs="+", metavar="ID", help="the tasks")
