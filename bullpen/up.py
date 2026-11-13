"""`up`: a repo's coordinator, its board and the watcher."""

import os

from .core import Fail, Repo, SELF, command, repo_root, resolve
from .rex import coordinator_label, rex_run
from .coordinators import add_coordinator, coordinator_at, prune, session_for
from .panes import gather
from .news import news_mode_line
from .guide import give_guide, guide_file, guide_text, tell_guide_changes
from .watcher import ensure_watcher
from .board import ensure_board
from .board_build import rebuild_board_if_stale
from .push_remote import catch_up_push
from .go import relink
from .doctor import up_hint


def cmd_up(args):
    repo = Repo(repo_root())
    session = session_for(repo, args)
    if not session:
        raise Fail("not inside Rex: pass --session")
    relink(repo, session)
    prune(repo)
    live = list(repo.cfg["coordinators"])  # pruned: only live ones are left
    said = []  # (what happened, to what, block IDs): one line, or a line each with --verbose
    if args.here:
        block = os.environ.get("REX_BLOCK")
        if not block:
            raise Fail("--here needs REX_BLOCK (run it from the agent's own Rex pane)")
        c = coordinator_at(repo, block, boards=False)
        if c:
            c["session"] = session
            print(f"coordinator {c['id']}: this pane ({block}), already.", end=" ")
        else:
            c = add_coordinator(repo, block, session)
            others = ", ".join(f"{o['id']} ({o['block']})" for o in live)
            print(f"coordinator {c['id']}: this pane ({block})."
                  + (f" The other coordinators stay: {others}. Tasks you add are yours." if live else ""),
                  end=" ")
        give_guide(c, guide_text())  # it's told to read it now
        print(f"Run `{SELF} guide` to read the coordinator's guide."
              + (" News comes by monitor: start it first, as the guide says."
                 if repo.cfg.get("news") == "monitor" else ""))
        mine = [c]
    elif live:
        said.append(("already running", "coordinators " + " and ".join(c["id"] for c in live)
                     if len(live) > 1 else "coordinator", ", ".join(f"{c['id']} ({c['block']})" for c in live)))
        mine = live
    else:
        argv = resolve(repo.cfg["coordinator_cmd"]) + [
            "-n", coordinator_label(repo),
            "--append-system-prompt-file", str(guide_file(repo)),
            f"You are the bullpen coordinator for {repo.root}. The bullpen CLI is {SELF}. "
            + ("News comes by monitor: start it first, as your guide says. "
               if repo.cfg.get("news") == "monitor" else "")
            + f"Run `{SELF} ls`, then say in one short line that you are ready.",
        ]
        block = rex_run(session, repo.root, coordinator_label(repo), argv, focus=True)
        c = add_coordinator(repo, block, session)
        give_guide(c, guide_text())
        said.append(("started", "coordinator", block))
        mine = [c]
    repo.save()
    tell_guide_changes(repo)  # the others (all of them, without --here), after a pull
    if not args.no_board:
        for c in mine:
            ensure_board(repo, session, c, said=said)
    gather(repo, session)
    ensure_watcher(session, said=said)
    if args.verbose:
        for what, name, ids in said:
            print(f"{name.split()[0]} {what}: {ids}")
    elif said:
        print(up_line(said, fresh=not args.here and not live))
    pushed = catch_up_push(repo)
    if pushed:
        print(pushed)
    rebuilt = rebuild_board_if_stale(repo)
    if rebuilt:
        print(rebuilt)
    hint = up_hint()
    if hint:
        print(hint)
    if args.verbose or repo.cfg.get("news") == "monitor":
        print(news_mode_line(repo))


def up_line(said, fresh):
    """`up`'s one line for people: "Coordinator, board and watcher started. Tell the
    coordinator what you want." on a first run, else what started or was running
    already. said: (what happened, to what, its block IDs); --verbose prints the IDs."""
    by = {}
    for what, name, _ in said:
        by.setdefault(what, []).append(name)
    parts = [(names[0] if len(names) == 1 else ", ".join(names[:-1]) + " and " + names[-1]) + " " + what
             for what, names in by.items()]
    line = " ".join(p[0].upper() + p[1:] + "." for p in parts)
    if fresh and "coordinator" in by.get("started", []):
        line += " Tell the coordinator what you want."
    return line


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "up", cmd_up, "Setup",
                help="start (or register) this repo's coordinator and the watcher")
    p.add_argument("--here", action="store_true", help="make the agent in this pane the coordinator")
    p.add_argument("--session", help="Rex session (default: this pane's)")
    p.add_argument("--no-board", action="store_true", help="don't open the task board")
    p.add_argument("--verbose", action="store_true",
                   help="name each pane's block ID, and how news reaches the coordinator")
