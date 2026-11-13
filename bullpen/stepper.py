"""The stepper: reading a tour on the board, step by step, in a floating pane."""

import codecs
import os
import re
import select
import shutil
import subprocess
import sys
import termios
import tty
from pathlib import Path

from .core import (
    Fail, SELF, TERMINAL, find_pane, find_panes, home_of, now, read_json, rex_alive, rex_api,
    rex_close, rex_focus, rex_view, set_layer, with_state, write_json,
)
from .tours import (
    add_comment, comments_path, read_comments, send_comments, start_tour, step_diff, step_files,
    tour_diff, tour_log, tour_path, tour_stale,
)
from .line_edit import LineEdit, PASTE_OFF, PASTE_ON, read_keys


# The board's g runs `tour <ID> --pane`: with a current tour (or a stale one, once the
# person chose it), it opens the stepper in a large floating pane in the middle of the
# window, the way the jump picker opens; with none, it starts the agent and the stepper
# opens once the tour is written. The stepper (`tour <ID> --view`) shows one step at a
# time: its title and note, the person's comments on it, then its hunks from git (in
# delta when installed). After the last step comes a summary of the comments, from
# which they're sent back or the task accepted. Where the person was is kept with the
# drafts, so opening the tour again resumes there.

TOUR_BOUNDS = [0.05, 0.04, 0.9, 0.9]  # most of the window: x, y, w, h


def tour_label(t):
    return f"bullpen: {t['id']} tour"


def tour_pane(repo, t, state, args):
    """The board's g: open the stepper on a current tour (or a stale one: the board asked
    first), or start writing a tour that opens when it's written."""
    session = args.session or os.environ.get("REX_SESSION") or t.get("session")
    opener = {"session": session, "from": args.from_block}
    if state == "writing":
        t["tour"]["open"] = opener
        write_json(repo.task_path(t["id"]), t)
        print(f"{t['id']}'s tour is being written: it opens when it's done")
        return
    if state in ("none", "failed") or args.new:
        start_tour(repo, t, False, opener)
        print(f"writing a tour of {t['id']}: it opens when it's done")
        return
    open_stepper(repo, t, session, args.from_block)
    print(f"opened {t['id']}'s tour" + (" (stale: the code changed since it was written)" if state == "stale" else ""))


def open_stepper(repo, t, session, from_block=None):
    """The stepper in a floating pane over the window of from_block (else of the task's
    board or coordinator), focused. One per task: an old one closes."""
    view = rex_view(session)
    for _, _, b in find_panes(view, tour_label(t)):
        rex_close(session, b)
    argv = [SELF, "tour", t["id"], "--view", "--back"]
    spec = {"bounds": TOUR_BOUNDS, "layout": {"block": {
        "flavor": TERMINAL, "label": tour_label(t),
        "options": {"command": with_state(argv), "cwd": str(repo.root)}}}}
    home = home_of(repo, t)
    anchor = from_block or (home.get("board") or {}).get("block") or home.get("block")
    where = find_pane(view, block=anchor) if anchor else None
    if where:
        spec["window_id"] = where[0]
    r = rex_api(session, "session.new_layer", spec)
    set_layer(session, r["layer_id"], "normal", focus=True)


def open_when_written(repo, t, opener):
    """The tour the person asked for on the board is written: open it where they asked."""
    try:
        open_stepper(repo, t, opener.get("session"), opener.get("from"))
    except Exception as e:  # the tour is written all the same: g opens it
        with tour_log(repo, t).open("a") as f:
            f.write(f"{now()} couldn't open the tour: {e}\n")


STEP_NEXT = ("n", "l", "\x1b[C", "\x1bOC")  # n, l, →
STEP_BACK = ("p", "h", "\x1b[D", "\x1bOD")  # p, h, ←
LINE_DOWN = ("j", "\x1b[B", "\x1bOB", "\x0e")  # j, ↓, ctrl+n
LINE_UP = ("k", "\x1b[A", "\x1bOA", "\x10")  # k, ↑, ctrl+p
PAGE_DOWN = (" ", "f", "\x1b[6~", "\x04", "\x06")  # space, f, page down, ctrl+d, ctrl+f
PAGE_UP = ("b", "\x1b[5~", "\x15", "\x02")  # b, page up, ctrl+u, ctrl+b
# the keys at the foot, longest first: the first that fits is shown
STEP_KEYS = ("n next · p back · j k scroll · c comment · t tell · v diff · ⏎ open · a accept · esc close",
             "n next · p back · c comment · t tell · v diff · ⏎ open · a accept · esc close",
             "n next · p back · c comment · t tell · esc close",
             "n p · c comment · t tell · esc close")
COMMENT_KEYS = ("⏎ saves · ctrl+j new line · esc cancels · mention a line to point at it",
                "⏎ saves · ctrl+j new line · esc cancels",
                "⏎ save · ctrl+j line · esc cancel")
END_KEYS = ("s send them back · a accept · p back · t tell · v diff · ⏎ open · esc close",
            "s send · a accept · p back · t tell · esc close",
            "s send · a accept · p back · esc close")
BOLD, DIM, OFF = "\x1b[1m", "\x1b[2m", "\x1b[0m"


def plural(n, word):
    return f"{n} {word}{'s' if n != 1 else ''}"


def wrap_text(text, width, indent=""):
    """Text wrapped to width, its own lines and blank lines kept."""
    import textwrap
    out = []
    for line in (text or "").split("\n"):
        lead = re.match(r"\s*([-*] |\d+\. )?", line).group(0)
        out += textwrap.wrap(line, max(width, 10), initial_indent=indent, subsequent_indent=indent + " " * len(lead),
                             drop_whitespace=True) or [""]
    return out


ANSI_RE = re.compile(r"\x1b(\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(\x07|\x1b\\)|.)")


def clip_line(line, width):
    """A line cut to width columns on screen, its colour codes kept (they take none)."""
    import unicodedata
    out, used, i = [], 0, 0
    while i < len(line):
        m = ANSI_RE.match(line, i)
        if m:
            out.append(m.group(0))
            i = m.end()
            continue
        c = line[i]
        w = 0 if unicodedata.combining(c) else 2 if unicodedata.east_asian_width(c) in "WF" else 1
        if used + w > width:
            break
        out.append(c)
        used += w
        i += 1
    return "".join(out)


def colour_diff(diff):
    """A patch's lines coloured as git would, for a terminal without delta."""
    out = []
    for line in diff.rstrip("\n").split("\n"):
        line = line.expandtabs(4)
        if line.startswith(("diff --git", "index ", "--- ", "+++ ", "new file", "deleted file", "rename ", "similarity")):
            out.append(BOLD + line + OFF)
        elif line.startswith("@@"):
            out.append("\x1b[36m" + line + OFF)
        elif line.startswith("+"):
            out.append("\x1b[32m" + line + OFF)
        elif line.startswith("-"):
            out.append("\x1b[31m" + line + OFF)
        else:
            out.append(line)
    return out


def diff_lines(diff, width, delta):
    """A step's patch as the lines shown: through delta when installed, else coloured."""
    if not diff.strip():
        return [DIM + "(no code in this step)" + OFF]
    if delta:
        p = subprocess.run([delta, "--paging=never", f"--width={width}"], input=diff, text=True,
                           capture_output=True)
        if p.returncode == 0 and p.stdout.strip():
            return p.stdout.rstrip("\n").split("\n")
    return colour_diff(diff)


class Stepper:
    """What the stepper shows and does with each key, apart from the terminal and the
    task. at is the step shown (0-based); at == len(steps) is the summary at the end.
    diff(i, width) gives step i's hunks as lines."""

    def __init__(self, tid, title, tour, comments, at, diff):
        self.tid, self.title, self.steps, self.commit = tid, title, tour["steps"], tour["commit"]
        self.comments, self.diff = comments, diff
        self.at = min(max(at, 0), len(self.steps))
        self.top, self.mode, self.edit, self.msg, self.sure = 0, "read", LineEdit(), "", False
        self.cache = {}

    def at_end(self):
        return self.at == len(self.steps)

    def go(self, i):
        i = min(max(i, 0), len(self.steps))
        if i != self.at:
            self.at, self.top = i, 0

    def key(self, k, rows=20):
        """Apply one key. Returns what the stepper can't do itself: "close", "send",
        "accept", "open" or "diff" (close, then open the task's pane or diff, as the
        board's ⏎ and v do), ("comment", text) to save or ("tell", text) to send; else None."""
        if self.mode in ("comment", "tell"):
            return self.comment_key(k)
        sure, self.sure, self.msg = self.sure, False, ""
        if k in ("\x1b", "q", "\x03", "\x07"):
            return "close"
        if k in ("\r", "\n"):
            return "open"
        if k == "v":
            return "diff"
        if k == "t":
            self.mode, self.edit = "tell", LineEdit()
            return None
        if k == "a":
            return self.accept_key(sure)
        if self.at_end():
            if k == "s":
                if self.comments:
                    return "send"
                self.msg = "No comments to send: p goes back to the steps."
                return None
        elif k == "c":
            self.mode, self.edit = "comment", LineEdit(lines=True)
            return None
        if k in STEP_NEXT:
            self.go(self.at + 1)
        elif k in STEP_BACK:
            self.go(self.at - 1)
        elif k in LINE_DOWN:
            self.top += 1
        elif k in LINE_UP:
            self.top = max(self.top - 1, 0)
        elif k in PAGE_DOWN:
            self.top += max(rows - 2, 1)
        elif k in PAGE_UP:
            self.top = max(self.top - max(rows - 2, 1), 0)
        elif k == "g":
            self.top = 0
        elif k == "G":
            self.top = 10 ** 9  # clamped when drawn
        return None

    def accept_key(self, sure):
        """a: at the end, accept (a second a when comments aren't sent); before the
        end, a second a accepts all the same."""
        unsent = f"{plural(len(self.comments), 'comment')} not sent: " if self.comments else ""
        if sure or (self.at_end() and not unsent):
            return "accept"
        self.sure = True
        if self.at_end():
            self.msg = f"{unsent}a again accepts anyway, s sends them back instead."
        else:
            self.msg = f"{unsent}a again accepts {self.tid} before the end."
        return None

    def comment_key(self, k):
        """A key while a comment (lines), or a line to tell the worker, is typed."""
        if self.edit.key(k):
            return None
        if k in ("\r", "\n"):
            mode, text, self.mode = self.mode, self.typed_text(), "read"
            return (mode, text) if text else None
        if k in ("\x1b", "\x03", "\x07"):
            self.mode, self.msg = "read", "Comment not saved." if self.mode == "comment" else "Not sent."
        return None

    def typed_text(self):
        """What's typed, to save or send: a tell on one line, a comment's lines without
        the spaces at their ends or blank lines around them."""
        if self.mode == "tell":
            return " ".join(self.edit.text.split())
        return "\n".join(line.rstrip() for line in self.edit.text.split("\n")).strip("\n")

    def on_step(self, i):
        """The drafts on step i of this tour."""
        return [c for c in self.comments if c["step"] == i + 1 and c.get("commit") == self.commit]

    def body(self, width):
        """The lines under the header: the step's title, files, note, comments and
        hunks; or, at the end, the comments to send."""
        w = max(width - 2, 10)
        if self.at_end():
            out = [BOLD + "End of the tour" + OFF, ""]
            if not self.comments:
                return out + ["No comments. a accepts the task, esc closes, p goes back."]
            out.append(f"Your {plural(len(self.comments), 'comment')}, not sent yet:")
            for n, c in enumerate(self.comments, 1):
                out += ["", f"{n:>2}. {BOLD}step {c['step']}, {c['title']}{OFF}"]
                out += wrap_text(c["text"], w - 4, "    ")
            return out + ["", "s sends them back to the worker, who fixes them and runs ready again."]
        s = self.steps[self.at]
        out = [BOLD + s["title"] + OFF, DIM + step_files(s) + OFF, ""]
        if s.get("note"):
            out += wrap_text(s["note"], w) + [""]
        for c in self.on_step(self.at):
            out += ["\x1b[33m" + line + OFF for line in wrap_text(c["text"], w - 2, "▍ ")] + [""]
        if (self.at, width) not in self.cache:
            self.cache[(self.at, width)] = self.diff(self.at, width)
        return out + self.cache[(self.at, width)]

    def prompt(self):
        return f"Comment on step {self.at + 1}: " if self.mode == "comment" else f"Tell {self.tid}: "

    def box(self, width, height):
        """The rows of the box something is typed in, and the cursor's (row, column) in
        them from 0. A tell is one line after its prompt; a comment is its prompt, then its
        lines, up to half the body's rows."""
        if self.mode == "tell":
            shown, col = self.edit.view(width - len(self.prompt()) - 3)
            return [f" {BOLD}{self.prompt()}{OFF}{shown}"], (0, 1 + len(self.prompt()) + col)
        lines, (row, col) = self.edit.rows(width - 2, max((height - 5) // 2, 1))
        return [f" {BOLD}{self.prompt().rstrip()}{OFF}"] + [" " + line for line in lines], (row + 1, 1 + col)

    def cursor(self, width, height):
        """(row, column) of the cursor, from 1, while a comment is typed."""
        rows, (row, col) = self.box(width, height)
        return height - len(rows) + row, col + 1

    def screen(self, width, height):
        """The stepper's lines: a header, the body from self.top, a message line and the keys."""
        n = len(self.steps)
        where = "end" if self.at_end() else f"step {self.at + 1} of {n}"
        if self.comments:
            where += f" · {plural(len(self.comments), 'comment')}"
        head = f"{self.tid}  {self.title}"
        room = max(width - len(where) - 4, 0)
        head = (head[:room - 1] + "…") if len(head) > room else head
        out = [BOLD + " " + head + " " * max(width - 2 - len(head) - len(where), 1) + where + OFF, ""]
        typing = self.box(width, height)[0] if self.mode in ("comment", "tell") else [""]
        rows = max(height - 3 - len(typing), 1)
        body = self.body(width)
        self.top = min(self.top, max(len(body) - rows, 0))
        shown = body[self.top:self.top + rows]
        out += [" " + line + OFF for line in shown] + [""] * (rows - len(shown))
        out = [clip_line(line, width - 1) for line in out]
        more = len(body) - self.top - rows
        if self.mode in ("comment", "tell"):
            out += typing
            keys = COMMENT_KEYS if self.mode == "comment" else ("⏎ sends it now · esc cancels",)
            out.append(f" {DIM}{next((k for k in keys if len(k) <= width - 2), keys[-1])}{OFF}")
        else:
            down = f"  ↓ {more} more" if more > 0 else ""
            out.append(" " + (BOLD + self.msg + OFF if self.msg else DIM + down.strip() + OFF))
            keys = END_KEYS if self.at_end() else STEP_KEYS
            out.append(f" {DIM}{next((k for k in keys if len(k) <= width - 2), keys[-1])}{OFF}")
        return [clip_line(line, width - 1) + OFF for line in out]


def save_place(repo, t, commit, at):
    """Where the person is in the tour, kept with the drafts: opening it again resumes there."""
    c = read_comments(repo, t)
    c["at"] = {"commit": commit, "step": at + 1}
    write_json(comments_path(repo, t), c)


def stepper_tui(st, act, moved):
    """Run the stepper on the terminal until act() says it's done (True). act(action)
    does what the stepper can't: save a comment, send, accept, close. moved(at) is
    told of each step the person goes to."""
    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    decoder = codecs.getincrementaldecoder("utf-8")("replace")
    tty.setraw(fd)
    sys.stdout.write("\x1b[?1049h\x1b[?7l" + PASTE_ON)  # its own screen; long lines are cut, not wrapped
    try:
        while True:
            size = shutil.get_terminal_size()
            lines = st.screen(size.columns, size.lines)
            cursor = ("\x1b[?25h\x1b[{};{}H".format(*st.cursor(size.columns, len(lines)))
                      if st.mode in ("comment", "tell") else "\x1b[?25l")
            sys.stdout.write("\x1b[H\x1b[2J" + "\r\n".join(lines) + cursor)
            sys.stdout.flush()
            if not select.select([fd], [], [], 0.5)[0]:
                continue  # redraw: the pane may have changed size
            for k in read_keys(fd, decoder):
                at = st.at
                a = st.key(k, size.lines - 4)
                if a and act(a):
                    return
                if st.at != at:
                    moved(st.at)
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
        sys.stdout.write(PASTE_OFF + "\x1b[?7h\x1b[?25h\x1b[?1049l")
        sys.stdout.flush()


def view_tour(repo, t, back=False):
    """Step through the task's tour on this terminal: the board's g opens it in a pane."""
    tour = read_json(tour_path(repo, t))
    if not tour:
        raise Fail(f"{t['id']} has no tour: `bullpen tour {t['id']}` writes one")
    if not sys.stdin.isatty():
        raise Fail("the tour needs a terminal (the board's g opens it in a pane)")
    run_ = t.get("tour") or {}
    if run_.get("state") == "written" and not run_.get("stale") and not tour_stale(repo, t, tour):
        run_["opened"] = tour["commit"]  # the board marks it toured (not for the old code's tour)
        write_json(repo.task_path(t["id"]), t)
    place = read_comments(repo, t).get("at") or {}
    at = place.get("step", 1) - 1 if place.get("commit") == tour["commit"] else 0
    wt = t.get("worktree")
    files = tour_diff(repo, t, tour) if wt and Path(wt).is_dir() else []
    delta = shutil.which("delta")
    st = Stepper(t["id"], t["title"], tour, read_comments(repo, t)["comments"], at,
                 lambda i, w: diff_lines(step_diff(files, tour["steps"][i]), w, delta))

    def bp(*args):
        """Run bullpen: whether it worked, and its last line."""
        p = subprocess.run([SELF, *args], cwd=repo.root, stdin=subprocess.DEVNULL, text=True, capture_output=True)
        last = ((p.stderr or p.stdout) if p.returncode else p.stdout).strip().splitlines() or [""]
        return p.returncode == 0, last[-1].removeprefix("bullpen: ")

    left = {"for": None}  # the pane or diff the stepper closed for: the board isn't focused after

    def act(a):
        if a == "close":
            return True
        if a in ("open", "diff"):
            # as the board's ⏎ and v: the worker's pane (the reviewer's while in review), or the diff
            args = (["diff", t["id"], "--pane"] if a == "diff"
                    else ["go", t["id"], "--review"] if repo.task(t["id"])["status"] == "review" else ["go", t["id"]])
            ok, said = bp(*args)
            if ok:
                left["for"] = a
                return True
            st.msg = said or f"couldn't open {t['id']}"
            return False
        if isinstance(a, tuple) and a[0] == "tell":
            ok, said = bp("tell", t["id"], a[1])
            st.msg = f"Sent to {t['id']}." if ok else said
            return False
        if isinstance(a, tuple):
            n = add_comment(repo, repo.task(t["id"]), st.at + 1, a[1])
            st.comments = read_comments(repo, t)["comments"]
            st.msg = f"Saved comment {n} on step {st.at + 1}."
            return False
        if a == "send":
            import contextlib
            import io
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    send_comments(repo, repo.task(t["id"]))
            except Fail as e:
                st.msg = str(e)
                return False
            return True
        if a == "accept":
            st.msg = f"Accepting {t['id']}…"
            size = shutil.get_terminal_size()
            sys.stdout.write("\x1b[H\x1b[2J" + "\r\n".join(st.screen(size.columns, size.lines)))
            sys.stdout.flush()
            ok, said = bp("accept", t["id"])
            if ok:
                return True
            st.msg = said or "accept failed"
        return False

    stepper_tui(st, act, lambda at: save_place(repo, t, tour["commit"], at))
    if back and not left["for"]:
        b = home_of(repo, t).get("board") or {}
        if rex_alive(b.get("session"), b.get("block")):
            rex_focus(b.get("session"), b["block"])
