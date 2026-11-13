"""One task line, as the board says it, for `ls`, `show` and the picker."""

import datetime as dt
import os
import sys
import textwrap
import unicodedata

from .core import CLOSED, STATE, own_model, pr_short, read_json, safe, seconds_since, stamp
from .agents import is_limited
from .limit import clock
from .start import waits_phrase


# `ls`, the picker and `show` say a task the way the board does (board/src/model.rs):
# its mark, ID, title, age and the board's sentence for it, in the board's order.
# Change one, change the other.

MARKS = {"needs": "●", "stopped": "■", "triage": "◎", "ready": "✔", "finished": "◆", "done": "·",
         "review": "◐", "working": "○", "todo": "◌", "dropped": "–"}
# the board's groups, in order: what you do next first
BOARD_ORDER = {"needs": 0, "stopped": 0, "triage": 1, "ready": 2, "finished": 3, "review": 4,
               "working": 5, "todo": 6, "done": 7, "dropped": 7}
MARK_COLORS = {"needs": "1;33", "stopped": "1;31", "ready": "1;32", "finished": "1;32", "review": "1;34"}
MARK_SET = set(MARKS.values())
STALE = 300  # seconds a task can need you before its age turns red, as on the board


def task_num(t):
    return int(t["id"].rsplit("-", 1)[1])


def board_order(tasks):
    """Tasks as the board lists them: by group (needs you first), then by number."""
    return sorted(tasks, key=lambda t: (BOARD_ORDER.get(t["status"], 8), task_num(t)))


def mark_color(t):
    """The SGR colour of a task's mark: a quiet agent's in the colour of one that needs you."""
    return MARK_COLORS["needs"] if is_stuck(t) else MARK_COLORS.get(t["status"], "2")


def ago(t, at=None):
    """ "3m", "2h", "1d", "now", or "" with no time."""
    if not stamp(t):
        return ""
    s = max(int(seconds_since(stamp(t), at)), 0)
    for unit, n in (("d", 86400), ("h", 3600), ("m", 60)):
        if s >= n:
            return f"{s // n}{unit}"
    return "now"


def stale(t, at=None):
    return t["status"] == "needs" and bool(stamp(t)) and seconds_since(stamp(t), at) >= STALE


DOING_FOR = 30 * 60  # how old a note of what an agent is doing can be and still show


def doing_now(t, at=None):
    """What the agent the task waits on last said it's doing, on one line, as the board
    shows it in place of its sentence: "" when it hasn't said, isn't working, said it
    before the task's status (less 30 s), over DOING_FOR ago, or is quiet or limited."""
    block = (t.get("review") or {}).get("block") if t["status"] == "review" else \
        t.get("block") if t["status"] == "working" else None
    if not block or is_stuck(t) or is_limited(t) or t.get("note"):
        return ""
    d = read_json(STATE / "doing" / f"{safe(block)}.json", {})
    if not isinstance(d, dict) or d.get("state") != "working" or not isinstance(d.get("at"), (int, float)):
        return ""
    at = at or dt.datetime.now().astimezone()
    since = dt.datetime.fromisoformat(stamp(t)).timestamp() - 30 if stamp(t) else float("-inf")
    if d["at"] < since or at.timestamp() - d["at"] > DOING_FOR:
        return ""
    return " ".join(str(d.get("text") or "").split())


BOARD_KINDS = {"full": "full review", "light": "light review", "followup": "follow-up review",
               "tour": "review of your comments", "rebase": "rebase check"}


def first_sentence(summary):
    """ "Recorded the demo." from "# Recorded the demo", or "" when there's none."""
    for line in (summary or "").splitlines():
        line = line.strip().lstrip("#").strip()
        if line:
            return line if line.endswith((".", "!", "?")) else line + "."
    return ""


def limit_detail(lim, at=None):
    reviewer = lim.get("who") == "reviewer"
    who = "Reviewer stopped at the usage limit" if reviewer else "Usage limit"
    if lim.get("resets"):
        return f"{who}, resets {clock(dt.datetime.fromisoformat(lim['resets']), at)}. Continues by itself then."
    return f"{who}, reset time unknown. {'⏎ opens it.' if reviewer else 't tells it to continue.'}"


def status_detail(repo, t, every, at=None):
    """The board's sentence for where the task stands."""
    r = t.get("review") or {}
    rnd, why, st = r.get("round") or 1, t.get("why") or "", t["status"]
    pr = pr_short(t.get("pr")) or (f"Local branch {t.get('branch') or ''}" if t.get("local") else "")
    if t.get("note"):
        return t["note"]
    if is_stuck(t):
        quiet = stuck_phrase(t, at)
        if t["stuck"]["who"] == "reviewer":
            return f"Reviewer has shown {quiet}. ⏎ opens it."
        return f"{quiet[0].upper()}{quiet[1:]}. p shows its screen."
    if is_limited(t):
        return limit_detail(t["limit"], at)
    if st in ("needs", "triage") and why == "Ended its turn without saying ready":
        return "Stopped mid-task, probably with a question. p shows its screen." if st == "needs" \
            else "Ended its turn without ready."
    if st in ("needs", "triage"):
        return why or ("Waiting for you." if st == "needs" else "Waiting for its coordinator.")
    if st == "stopped":
        return "Its agent exited. r resumes it."
    if st == "review":
        kind = BOARD_KINDS.get(r.get("kind"), "review" if r.get("kind") else "reviewer")
        return f"{kind[0].upper()}{kind[1:]} on round {rnd} of {repo.cfg['review_rounds']}."
    if st == "ready" and why.startswith("Nothing to merge"):
        return "Done, with nothing to merge."
    if st == "finished":
        line = first_sentence(t.get("summary"))
        return f"Finished, nothing to merge: {line} a closes it." if line else "Finished, nothing to merge. a closes it."
    if st == "ready":
        text = {"approved": f"{pr}, approved by {BOARD_KINDS.get(r.get('kind'), 'review')}.",
                "changes": f"{pr}. Review still asks for changes."}.get(r.get("verdict")) or (f"{pr}." if pr else "Done.")
        return text + (f" Rebased onto {t['rebased']}." if t.get("rebased") else "")
    if st == "working" and sent_back_by_github(t):
        return "Fixing what GitHub said."
    if st == "working" and r.get("verdict") == "changes":
        return f"Fixing what review round {rnd} found."
    if st == "todo":
        waits = waits_phrase(repo, t, every)
        return f"{waits}." if waits else "Not started. s starts it."
    if st == "done" and t.get("local") and not t.get("pr"):
        return f"Accepted. Merged into {t['base']}." if t.get("merged") and t.get("base") else "Accepted."
    if st == "done" and t.get("closed"):
        return "Closed, nothing to merge."
    if st == "done":
        return f"Accepted. {pr} merged." if pr else "Accepted."
    return "Dropped." if st == "dropped" else ""


def auto_detail(repo, t):
    """What auto-accept does with it, when that isn't what the config says for all."""
    on = bool(repo.cfg.get("auto_accept"))
    if t["status"] in CLOSED:
        return ""
    if t.get("auto_wait") and t["status"] == "ready":
        return "Auto-accepts once its checks pass."
    if on and t.get("auto_accept") is False:
        return "Kept for you: no auto-accept."
    if not on and t.get("auto_accept") is True:
        return "Auto-accepts once approved."
    return ""


def board_detail(repo, t, every=None, at=None):
    """The board's sentence for a task: what its agent is doing now, else where it
    stands, what GitHub said, what auto-accept does with it and its own model."""
    doing = doing_now(t, at)
    if doing:
        return doing
    every = every if every is not None else {o["id"]: o for o in repo.tasks()}
    text = status_detail(repo, t, every, at)
    gh = "" if t["status"] in CLOSED or t.get("note") else pr_phrase(t)
    if gh:
        text = f"{text} {gh[0].upper()}{gh[1:]}.".strip()
    model = own_model(repo, t) if t["status"] not in CLOSED else None
    return " ".join(p for p in (text, auto_detail(repo, t), f"Model: {model}." if model else "") if p)


def text_width(text):
    return sum(2 if unicodedata.east_asian_width(c) in "WF" else 0 if unicodedata.combining(c) else 1
               for c in text)


def fit(text, w):
    """Cut text to w columns, with an ellipsis when it doesn't fit."""
    if text_width(text) <= w:
        return text
    out, used = "", 0
    for c in text:
        cw = text_width(c)
        if used + cw > w - 1:
            break
        out, used = out + c, used + cw
    return out + "…" if w > 0 else ""


def sgr(code, text, color):
    return f"\x1b[{code}m{text}\x1b[0m" if color and text else text


def task_line(repo, t, every=None, idw=0, width=None, color=False, age=True, detail=True, at=None):
    """One task as the board says it: mark, ID, title, age, then the board's sentence.
    Cut to width columns when given (the title keeps room for the age); in colour with
    color, as on the board: the mark in its status's colour, the rest dim."""
    mark, tid = MARKS.get(t["status"], " "), f"{t['id']:<{idw}}"
    when = ago(t, at) if age else ""
    text = board_detail(repo, t, every, at) if detail else ""
    head = 2 + len(tid) + 2
    title = t["title"] if width is None else fit(t["title"], max(width - head - (len(when) + 2 if when else 0), 1))
    if width is not None:
        room = width - head - text_width(title) - (len(when) + 2 if when else 0) - 2
        text = fit(text, room) if room >= 4 else ""
    title_dim = t["status"] in CLOSED or t["status"] == "triage"
    parts = [sgr(mark_color(t), mark, color) + " " + sgr("2", tid, color) + "  " + sgr("2" if title_dim else "0", title, color)]
    if when:
        parts.append(sgr("1;31" if stale(t, at) else "2", when, color))
    if text:
        parts.append(sgr("2", text, color))
    return "  ".join(parts)


def wrap(text, w, lines):
    """text in at most `lines` lines of w columns, the last ending in " …" when cut."""
    out = textwrap.wrap(text, max(w, 1), break_on_hyphens=False) or [""]
    if len(out) > lines:
        out = out[:lines]
        out[-1] = fit(out[-1] + " …", w)
    return out


def task_rows(repo, t, every=None, idw=0, width=80, color=False, at=None, lines=None):
    """The board's rows for a task, for a terminal too narrow for task_line: mark, ID,
    title and the age at the right edge, then the board's sentence under the title in
    `lines` lines (by default the board's: two for one that needs you, else one)."""
    mark, tid, when = MARKS.get(t["status"], " "), f"{t['id']:<{idw}}", ago(t, at)
    w = width - 1  # never into the last column
    head = 2 + len(tid) + 2
    title = fit(t["title"], max(w - head - len(when) - 2, 1))
    pad = " " * max(w - head - text_width(title) - len(when), 2) if when else ""
    title_dim = t["status"] in CLOSED or t["status"] == "triage"
    rows = [sgr(mark_color(t), mark, color) + " " + sgr("2", tid, color) + "  " + sgr("2" if title_dim else "0", title, color)
            + pad + sgr("1;31" if stale(t, at) else "2", when, color)]
    text = board_detail(repo, t, every, at)
    n = lines or (2 if t["status"] in ("needs", "stopped", "finished") or is_stuck(t) or is_limited(t) or pr_phrase(t) else 1)
    rows += [" " * head + sgr("2", line, color) for line in wrap(text, w - head, n) if line]
    return rows


def term_width():
    """The terminal's width when stdout is one, else None: piped output is whole."""
    if not sys.stdout.isatty():
        return None
    import shutil  # here, not at the top: it imports the compression modules
    return shutil.get_terminal_size().columns


def colored():
    return sys.stdout.isatty() and not os.environ.get("NO_COLOR")


def is_stuck(t):
    """Has the agent the task is waiting on (its worker, or its reviewer in review)
    shown no progress for too long?"""
    s = t.get("stuck")
    return bool(s) and t["status"] == ("review" if s["who"] == "reviewer" else "working")


def stuck_phrase(t, at=None):
    """ "no progress for 25 min", or "" for a task that isn't stuck."""
    if not is_stuck(t):
        return ""
    m = int(seconds_since(t["stuck"]["since"], at) // 60)
    return f"no progress for {m} min" if m < 120 else f"no progress for {m // 60} h"


def sent_back_by_github(t):
    """Is the worker fixing failed checks or comments from its PR? Set by `send_back`,
    cleared by the next `ready` (or a reviewer's changes)."""
    return bool((t.get("gh") or {}).get("sent"))


def pr_phrase(t):
    """The PR's latest state in a few words, for the board and `show`, or "".
    A working task only shows what GitHub sent it back for: anything else is
    about the old push."""
    g = t.get("gh") or {}
    if g.get("state") in ("merged", "closed"):
        return f"{g['state']} on GitHub"
    if t.get("status") == "working" and not sent_back_by_github(t):
        return ""
    parts = []
    if g.get("checks"):
        parts.append(f"checks {g['checks']}")
    if g.get("new"):
        parts.append(f"{g['new']} new comment{'s' if g['new'] != 1 else ''}")
    return ", ".join(parts)
