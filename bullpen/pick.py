"""pick: jump to any open task by typing part of it."""

import codecs
import os
import shutil
import subprocess
import sys
import termios
import tty
from pathlib import Path

from .core import (
    CLOSED, Fail, MARK_SET, SELF, TERMINAL, board_order, command, find_pane, find_panes,
    find_repo, fit, known_repos, mark_color, rex_api, rex_close, rex_view, set_layer, task_line,
    with_state,
)
from .line_edit import LineEdit, PASTE_OFF, PASTE_ON, read_keys


PICK_LABEL = "bullpen: jump"
PICK_BOUNDS = [0.15, 0.12, 0.7, 0.6]  # the middle of the window: x, y, w, h


# what the picker lists first: what needs you, then what's ready, then the board's order
PICK_FIRST = {"needs": 0, "stopped": 0, "ready": 1, "finished": 1}


def pick_items(home=None):
    """(repo, task) of every open task bullpen knows: home's first, then the other
    repositories', each with what needs you first, then what's ready, then the rest in
    the board's order."""
    repos = known_repos()
    if home:
        repos = [home] + [r for r in repos if r.dir != home.dir]
    first = lambda t: PICK_FIRST.get(t["status"], 2)
    return [(r, t) for r in repos for t in sorted(board_order(r.tasks()), key=first)
            if t["status"] not in CLOSED]


def pick_lines(items):
    """One line per task: its mark, ID and title (task_line without age or sentence, so
    a search for "12" finds BP-12, not a task 12m old), after its repository's name when
    the tasks come from more than one."""
    many = len({r.dir for r, _ in items}) > 1
    rw = max((len(r.root.name) for r, _ in items), default=0)
    iw = max((len(t["id"]) for _, t in items), default=0)
    return [(f"{r.root.name:<{rw}}  " if many else "") + task_line(r, t, idw=iw, age=False, detail=False)
            for r, t in items]


def fuzzy_score(query, text):
    """How well text matches query, or None when it doesn't. Each word of the query must
    be in one word of text, its letters in order but not necessarily together (case
    ignored): "lgn" finds "login", but "digest" doesn't find "Migrate sessions table".
    Letters together, and at the start of a word, score higher."""
    words, total = text.lower().split(), 0
    for word in query.lower().split():
        scores = [s for low in words for start, c in enumerate(low) if c == word[0]
                  for s in [word_score(word, low, start)] if s is not None]
        if not scores:
            return None
        total += max(scores)
    return total


def word_score(word, low, start):
    """The score of word's letters found in low from start on, each as early as it can be."""
    score, prev, i = 0, None, start
    for c in word:
        i = low.find(c, i)
        if i < 0:
            return None
        if prev is not None:
            score += 3 if i == prev + 1 else -min(i - prev - 1, 5)
        if i == 0 or not low[i - 1].isalnum():
            score += 2
        score += 1
        prev, i = i, i + 1
    return score


def pick_filter(query, lines):
    """The indexes of the lines matching query, best first; ties keep their order."""
    scored = [(s, i) for i, line in enumerate(lines) for s in [fuzzy_score(query, line)] if s is not None]
    return [i for _, i in sorted(scored, key=lambda si: (-si[0], si[1]))]


PICK_UP = ("\x1b[A", "\x1bOA", "\x10", "\x0b")  # ↑, ctrl+p, ctrl+k
PICK_DOWN = ("\x1b[B", "\x1bOB", "\x0e")  # ↓, ctrl+n
PICK_PROMPT = "jump to> "
PICK_HINT = "⏎ go · esc close · ↑↓ move"


class Picker:
    """What the picker shows and does with each key, apart from the terminal."""

    def __init__(self, lines, colors=None):
        """colors: each line's mark colour (an SGR code), as on the board."""
        self.lines, self.colors, self.edit, self.sel = lines, colors, LineEdit(), 0
        self.shown = pick_filter("", lines)
        self.msg = "" if lines else "no open tasks in any repository"

    def key(self, k):
        """Apply one key: "choose" or "cancel" when it ends the picker, else None."""
        if k in ("\r", "\n"):
            return "choose" if self.shown else None
        if k in ("\x1b", "\x03", "\x07") or (k == "\x04" and not self.query):
            return "cancel"
        if k in PICK_UP:
            self.sel = max(self.sel - 1, 0)
        elif k in PICK_DOWN:
            self.sel = min(self.sel + 1, max(len(self.shown) - 1, 0))
        else:
            query = self.query
            if self.edit.key(k) and self.query != query:
                self.search()
        return None

    @property
    def query(self):
        return self.edit.text

    def search(self):
        self.sel, self.msg = 0, ""
        self.shown = pick_filter(self.query, self.lines)

    def chosen(self):
        return self.shown[self.sel] if self.shown else None

    def typed(self, width):
        """The query as shown after the prompt, and the cursor's column on screen, from 1."""
        shown, col = self.edit.view(width - len(PICK_PROMPT) - 1)
        return shown, len(PICK_PROMPT) + col + 1

    def screen(self, width, height):
        """The picker's lines: the query, a hint, then the matching tasks around the selected one."""
        rows = max(height - 2, 1)
        first = max(0, self.sel - rows + 1)
        count = f"{len(self.shown)}/{len(self.lines)}"
        # a failure in the normal weight, after the count; else the keys, dim
        hint = (f"\x1b[2m{count}\x1b[0m  {fit(self.msg, max(width - len(count) - 2, 0))}" if self.msg
                else f"\x1b[2m{fit(f'{count}  {PICK_HINT}', width)}\x1b[0m")
        out = [f"{PICK_PROMPT}{self.typed(width)[0]}", hint]
        for n, i in enumerate(self.shown[first:first + rows], first):
            line = self.paint(fit(self.lines[i], max(width - 2, 0)), i)
            out.append(f"\x1b[7m> {line}\x1b[0m" if n == self.sel else f"  {line}")
        return out

    def paint(self, line, i):
        """The line with its mark in colour; the reset keeps the selected row reversed."""
        at = next((j for j, c in enumerate(line) if c in MARK_SET), None)
        if not self.colors or at is None:
            return line
        return f"{line[:at]}\x1b[{self.colors[i]}m{line[at]}\x1b[22;39m{line[at + 1:]}"


def pick_tui(p, choose):
    """Run the picker on the terminal until a task is chosen (True) or it's closed (False).
    choose(index) goes to that task and returns None, or why it couldn't."""
    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    decoder = codecs.getincrementaldecoder("utf-8")("replace")
    tty.setraw(fd)
    sys.stdout.write(PASTE_ON)
    try:
        while True:
            size = shutil.get_terminal_size()
            lines = p.screen(size.columns, size.lines)
            sys.stdout.write("\x1b[H\x1b[2J" + "\r\n".join(lines) + f"\x1b[1;{p.typed(size.columns)[1]}H")
            sys.stdout.flush()
            for k in read_keys(fd, decoder):
                act = p.key(k)
                if act == "cancel":
                    return False
                if act == "choose":
                    p.msg = choose(p.chosen()) or ""
                    if not p.msg:
                        return True
                    break
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
        sys.stdout.write(PASTE_OFF + "\x1b[H\x1b[2J")
        sys.stdout.flush()


def cmd_pick(args):
    if args.pane:
        return pick_pane(args)
    try:
        home = find_repo(args.from_block)
    except Fail:
        home = None
    items = pick_items(home)
    lines = pick_lines(items)
    if args.filter is not None:
        for i in pick_filter(args.filter, lines):
            print(lines[i])
        return
    if not sys.stdin.isatty():
        raise Fail("pick needs a terminal (--filter prints the matching tasks instead)")

    def choose(i):
        """Go to the task's pane; with none (queued, stopped, a pane that's gone), to
        its board with the task selected; failing both, why it couldn't go there."""
        r, t = items[i]
        go = lambda *more: subprocess.run(
            [SELF, "go", t["id"], "--repo", str(r.root), *more,
             *(["--session", args.session] if args.session else [])],
            stdin=subprocess.DEVNULL, text=True, capture_output=True)
        p = go()
        if p.returncode and go("--board").returncode == 0:
            return None
        return None if p.returncode == 0 else (p.stderr.strip().removeprefix("bullpen: ") or "couldn't go there")

    pick_tui(Picker(lines, [mark_color(t) for _, t in items]), choose)


def pick_pane(args):
    """Open the picker in a floating pane in the middle of the window of the pane you're
    in; it closes itself when you choose or press esc. One picker at a time."""
    session = args.session or os.environ.get("REX_SESSION")
    view = rex_view(session)
    for _, _, b in find_panes(view, PICK_LABEL):
        rex_close(session, b)
    argv = [SELF, "pick", *(["--from", args.from_block] if args.from_block else []),
            *(["--session", session] if session else [])]
    spec = {"bounds": PICK_BOUNDS, "layout": {"block": {
        "flavor": TERMINAL, "label": PICK_LABEL,
        "options": {"command": with_state(argv), "cwd": str(Path.home())}}}}
    where = find_pane(view, block=args.from_block) if args.from_block else None
    if where:
        spec["window_id"] = where[0]
    r = rex_api(session, "session.new_layer", spec)
    set_layer(session, r["layer_id"], "normal", focus=True)


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "pick", cmd_pick, "You",
                help="type part of any open task, in any repository, and go to it")
    p.add_argument("--pane", action="store_true", help="open the picker in a floating pane (the jump key)")
    p.add_argument("--filter", metavar="QUERY", help="print the open tasks matching QUERY, best first")
    p.add_argument("--from", dest="from_block", default=os.environ.get("REX_BLOCK"),
                   help="the pane you're in (default: this one)")
    p.add_argument("--session", help="the session you're in (key bindings pass it)")
