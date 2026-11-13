"""`ls`, `show` and `diff`."""

import argparse
import json
import subprocess
import sys
import tty
from pathlib import Path

from .core import (
    Fail, Repo, SELF, command, git, is_status, known_repos, listed, own_model, repo_root,
)
from .modes import task_base_ref
from .rex import find_pane, rex_alive, rex_close, rex_focus, rex_view
from .agents import is_limited
from .coordinators import home_of, owner, session_for
from .hooks import hook_log
from .panes import place, show
from .start import waits_phrase
from .review import review_kind
from .task_line import (
    ago, board_detail, board_order, colored, is_stuck, pr_phrase, stuck_phrase, task_line,
    task_rows, term_width, text_width, wrap,
)


def cmd_ls(args):
    if args.all_repos:
        return ls_repos(args)
    repo = Repo(repo_root())
    every = repo.tasks()  # read once: reading them is most of ls's own time
    tasks = listed(every, args)
    if args.json:
        print(json.dumps(tasks, indent=2))
        return
    if not tasks:
        print("no open tasks" + ("" if args.all else " (--all shows finished ones)"))
    for line in ls_lines(repo, tasks, term_width(), colored(), every):
        print(line)


SENTENCE_ROOM = 24  # columns a cut line must leave for the board's sentence, or ls takes two rows


def ls_lines(repo, tasks, width=None, color=False, every=None):
    """`ls`'s lines, in the board's order: one task_line each, whole when piped (width
    None), cut to the width on a terminal. When a line would leave under SENTENCE_ROOM
    columns for some task's sentence (a narrow terminal, a long title), the board's
    two rows each instead. every: all the repo's tasks, when the caller has read them."""
    every = {t["id"]: t for t in (repo.tasks() if every is None else every)}
    idw = max((len(t["id"]) for t in tasks), default=0)
    tasks = board_order(tasks)
    if width is None:
        return [task_line(repo, t, every, idw, color=color) for t in tasks]
    room = lambda t: width - 1 - (2 + idw + 2) - text_width(t["title"]) - (len(ago(t)) + 2 if ago(t) else 0) - 2
    if all(room(t) >= SENTENCE_ROOM or text_width(task_line(repo, t, every, idw)) < width for t in tasks):
        return [task_line(repo, t, every, idw, width - 1, color) for t in tasks]
    return [row for t in tasks for row in task_rows(repo, t, every, idw, width, color)]


def ls_repos(args):
    """`ls --all-repos`: the open tasks of every repository bullpen knows, by repository.
    One with nothing open, or whose checkout is gone, is left out."""
    groups = [(r, ts, every) for r in known_repos() for every in [r.tasks()]
              for ts in [listed(every, args)] if ts]
    if args.json:
        print(json.dumps([{"repo": str(r.root), "tasks": ts} for r, ts, _ in groups], indent=2))
        return
    if not groups:
        print("no open tasks in any repository" + ("" if args.all else " (--all shows finished ones)"))
    width = term_width()
    for i, (r, ts, every) in enumerate(groups):
        if i:
            print()
        print(f"{r.root.name}  ({r.root})")
        for line in ls_lines(r, ts, width and width - 2, colored(), every):
            print("  " + line)


def model_line(repo, t):
    """ "opus (its own, over the config's sonnet)", and what runs now if that's another."""
    cfg = repo.cfg.get("worker_model")
    line = f"{own_model(repo, t)} (its own, over the config's{' ' + cfg if cfg else ''})"
    now_on = t.get("model") if rex_alive(t.get("session"), t.get("block")) else None
    if now_on and now_on != own_model(repo, t):
        line += f"; running {now_on} until its next start or resume"
    return line


def cmd_show(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if args.json:
        print(json.dumps(t, indent=2))
        return
    detail, width = board_detail(repo, t), term_width()
    if width is None or text_width(task_line(repo, t)) < width:
        print(task_line(repo, t, color=colored()))
    else:
        print("\n".join(task_rows(repo, t, width=width, color=colored(), lines=99)))
    for k in ("why", "base", "branch", "worktree", "block", "model", "pr", "diffstat", "summary"):
        if k == "model" and own_model(repo, t):
            print(f"{k:>9}: {model_line(repo, t)}")
        elif k == "why" and t.get(k) and t[k] in detail:
            continue  # the first line says it
        elif t.get(k):
            print(f"{k:>9}: " + "\n".join((" " * 11 if l else "") + l for l in str(t[k]).strip().splitlines()).lstrip())
        elif k == "pr" and t.get("local"):
            print(f"{k:>9}: none; local branch, merged into {t['base']} by accept")
    if t.get("after"):
        print(f"    after: {', '.join(t['after'])}")
    if len(repo.cfg["coordinators"]) > 1 or (t.get("coordinator") and not owner(repo, t)):
        c = owner(repo, t)
        print(f"    coord: " + (f"{c['id']} ({c['block']})" if c else
                                 f"{t.get('coordinator') or 'none'}, gone: every running coordinator sees to it"))
    if t.get("auto_accept") is not None:
        print(f"     auto: {'accepts by itself once approved' if t['auto_accept'] else 'kept for you (--no-auto-accept)'}")
    if waits_phrase(repo, t):
        print(f"    waits: {waits_phrase(repo, t)}")
    if pr_phrase(t) and pr_phrase(t).lower() not in detail.lower():
        print(f"   github: {pr_phrase(t)}")
    if is_stuck(t):
        print(f"    quiet: its {t['stuck']['who']} has shown {stuck_phrase(t)}")
    r = t.get("review") or {}
    if t.get("placement"):
        print(f"     pane: {t['placement']} (its own, over config --workers)")
    if t.get("review_mode"):
        print(f"     mode: {t['review_mode']} first review (its own)")
    if r:
        state = r.get("verdict") or "in progress"
        if r.get("verdict") == "approved" and r.get("kept"):
            state += f", kept {r['kept']} time{'s' if r['kept'] != 1 else ''} as the code didn't change"
        kind = review_kind(r) + (f" on {r['model']}" if r.get("model") else "")
        print(f"   review: round {r.get('round')} ({kind}), {state}"
              + (f"; findings: {r['findings']}" if r.get("findings") else ""))
    for name in ("setup", "teardown"):
        log = hook_log(repo, t, name)
        if log.exists():
            failed = " (failed: the worker didn't start)" if name == "setup" and t.get("needs_setup") else ""
            print(f"{name:>9}: {log}{failed}")
    print(f"    brief: {repo.dir / 'briefs' / (t['id'] + '.md')}")
    nxt = next_steps(repo, t)
    print("     next: " + ("\n" + " " * 11).join(wrap(nxt, width - 12, 99) if width else [nxt]))


def next_steps(repo, t):
    """What you can do with the task now: the commands, with the board's keys."""
    tid, st = t["id"], t["status"]
    if st == "needs" and t.get("ask"):
        return f"`bullpen reply {tid} <answer>`, or i on the board"
    if st == "needs" and (t.get("stop") or {}).get("kind") == "prompt":
        return f"`bullpen answer {tid} <option>` (`bullpen peek {tid}` shows the prompt), or i on the board"
    if st == "needs":
        return f"`bullpen go {tid}` opens it (⏎ on the board), or `bullpen tell {tid} <message>` (t)"
    if st == "stopped":
        return f"`bullpen resume {tid}` (r on the board), or `bullpen drop {tid}` (d)"
    if st == "triage":
        return f"nothing: its coordinator is on it. `bullpen go {tid}` opens it (⏎ on the board)"
    if st == "ready":
        return f"`bullpen accept {tid}` (a on the board); `bullpen diff {tid}` shows the change (v)"
    if st == "finished":
        return f"`bullpen close {tid}` (a on the board), or `bullpen drop {tid}` (d)"
    if st == "review":
        return f"nothing until its review is done; `bullpen go {tid} --review` opens the reviewer (⏎ on the board)"
    if st == "working" and (is_stuck(t) or (is_limited(t) and not t["limit"].get("resets"))):
        return f"`bullpen peek {tid}` shows its screen (p on the board), `bullpen tell {tid} <message>` (t)"
    if st == "working":
        return f"nothing until it's ready; `bullpen tell {tid} <message>` steers it (t on the board)"
    if st == "todo" and waits_phrase(repo, t):
        return f"nothing: it starts by itself once what it waits on is accepted; `bullpen start {tid} --force` starts it now"
    if st == "todo":
        return f"`bullpen start {tid}` (s on the board), or `bullpen drop {tid}` (d)"
    return "nothing: it's closed"


def diff_label(t):
    return f"bullpen: {t['id']} diff"


def cmd_diff(args):
    """A task's changes against its base, through a pager; --stat just the files."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    wt = t.get("worktree")
    if not wt or not Path(wt).is_dir():
        raise Fail(f"{is_status(t)}, with no worktree, so there's nothing to diff")
    if args.pane:
        return diff_pane(repo, t, args)
    base_ref = task_base_ref(repo, t)
    if args.stat:
        print(diff_stat(wt, base_ref, t["id"]))
        return
    import shutil  # here, not at the top: ls and show don't need it
    delta = shutil.which("delta")
    tty = sys.stdout.isatty()
    color = ["--color=always"] if tty and not delta else ["--no-color"]
    text = git(wt, "diff", *color, f"{base_ref}...HEAD").stdout
    loose = git(wt, "diff", *color, "HEAD").stdout
    untracked = git(wt, "ls-files", "--others", "--exclude-standard").stdout.splitlines()
    if loose or untracked:
        text += ("\n" if text else "") + f"{UNCOMMITTED}\n\n" + loose + "".join(f"untracked: {f}\n" for f in untracked)
    if not text:
        text = f"{t['id']} has no changes against {base_ref}.\n"
    page(text, delta if tty else None, tty)
    if args.back:
        b = home_of(repo, t).get("board") or {}
        if rex_alive(b.get("session"), b.get("block")):
            rex_focus(b.get("session"), b["block"])


UNCOMMITTED = "Uncommitted (not in any commit yet):"


def diff_stat(wt, base_ref, tid):
    """The files the branch changed since its base, then any not committed yet."""
    parts = []
    committed = git(wt, "diff", "--stat", f"{base_ref}...HEAD").stdout.rstrip()
    parts.append(committed or f"{tid} has no commits against {base_ref}.")
    loose = git(wt, "diff", "--stat", "HEAD").stdout.rstrip()
    untracked = git(wt, "ls-files", "--others", "--exclude-standard").stdout.splitlines()
    if loose or untracked:
        parts.append("\n".join([UNCOMMITTED] + ([loose] if loose else []) +
                               [f" untracked: {f}" for f in untracked]))
    return "\n\n".join(parts)


def page(text, delta, tty):
    """Show text in delta, else `less -R`; straight to stdout when that's no terminal.
    The pager never quits by itself on a short diff: in a pane, that would close it at once."""
    if not tty:
        sys.stdout.write(text)
        return
    pager = [delta, "--paging=always"] if delta else ["less", "-R", "-+F"]
    try:
        subprocess.run(pager, input=text, text=True)
    except FileNotFoundError:
        sys.stdout.write(text)


def diff_pane(repo, t, args):
    """Show the diff in a floating pane over the board (a tab with --workers tabs);
    quitting the pager closes it and focuses the board again."""
    session = session_for(repo, args) or t.get("session")
    old = find_pane(rex_view(session), label=diff_label(t))
    if old:
        rex_close(session, old[2])  # one diff pane per task, showing the latest
    argv = [SELF, "diff", t["id"], "--back"]
    block, _ = place(repo, t, session, diff_label(t), argv, role="diff", hint=False, focus=True)
    show(repo, session, block)


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "ls", cmd_ls, "You", help="list tasks")
    p.add_argument("--all", action="store_true", help="closed tasks too (done and dropped)")
    p.add_argument("--all-repos", action="store_true",
                   help="every repository bullpen knows, by repository (those with nothing open left out)")
    p.add_argument("--json", action="store_true", help="the task records as JSON, with their raw status "
                   "(with --all-repos: a list of {repo, tasks})")

    p = command(sub, "show", cmd_show, "You", help="one task")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--json", action="store_true", help="its task record as JSON")

    p = command(sub, "diff", cmd_diff, "You",
                help="a task's changes against its base, in a pager (delta if installed)")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--stat", action="store_true", help="just the files changed, no pager")
    p.add_argument("--pane", action="store_true", help="show it in a floating pane (the board's v)")
    p.add_argument("--back", action="store_true", help=argparse.SUPPRESS)  # focus the board after
    p.add_argument("--session", help="the Rex session for its pane (default: the coordinator's)")
