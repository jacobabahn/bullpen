"""`diff` (`ls` and `show` are Rust's: cli/src/show.rs)."""

import argparse
import subprocess
import sys
import tty
from pathlib import Path

from .core import Fail, Repo, SELF, command, git, is_status, repo_root
from .modes import task_base_ref
from .rex import find_pane, rex_alive, rex_close, rex_focus, rex_view
from .coordinators import home_of, session_for
from .panes import place, show


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
    p = command(sub, "diff", cmd_diff, "You",
                help="a task's changes against its base, in a pager (delta if installed)")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--stat", action="store_true", help="just the files changed, no pager")
    p.add_argument("--pane", action="store_true", help="show it in a floating pane (the board's v)")
    p.add_argument("--back", action="store_true", help=argparse.SUPPRESS)  # focus the board after
    p.add_argument("--session", help="the Rex session for its pane (default: the coordinator's)")
