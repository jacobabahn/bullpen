"""One task line, as the board says it, for `ls` and `show` (bullpen/task_line.py).

`ls` and `show` say a task the way the board does (board/src/model.rs): its mark, ID,
title, age and the board's sentence for it, in the board's order. Change one, change
the others.

Python passes `every` as {id: task}; only the statuses are read, so here it is
{id: status} (start.statuses). A width of -1 is Python's None: no width, whole lines.
Python's `at` (a fixed now, for tests) isn't ported: each use reads the clock."""

from std.os import getenv
from std.os.path import exists
from std.ffi import external_call, c_int
from std.sys.info import CompilationTarget

from bullpen.core import Repo, Task, closed, own_model, pr_short, stamp, state_dir, safe
from bullpen.json import Doc, NULL, TRUE, FALSE, NUM, STR
from bullpen.timeutil import now, parse_iso, clock
from bullpen.agents import is_limited
from bullpen.start import waits_phrase
from bullpen.text import text_width, fit, wrap, strip, split_words, splitlines


comptime STALE = 300  # seconds a task can need you before its age turns red, as on the board
comptime DOING_FOR = 30 * 60  # how old a note of what an agent is doing can be and still show


def py_str(d: Doc, n: Int) -> String:
    """Python's str() of a scalar JSON value n in d (missing or null: "None")."""
    var k = d.kind(n)
    if n < 0 or k == NULL:
        return "None"
    if k == TRUE:
        return "True"
    if k == FALSE:
        return "False"
    if k == NUM or k == STR:
        return d.nodes[n].text
    return d.dumps(n, 0)  # a list or dict: not what Python's repr says, and not in bullpen's records


def cap(s: String) -> String:
    """s with its first letter upper case (an ASCII one, as every caller's is)."""
    if s.byte_length() == 0:
        return s
    return String(s[byte=0:1]).upper() + String(s[byte=1:])


def seconds_since(stamp: String) raises -> Float64:
    return now() - parse_iso(stamp)


def mark(status: String) -> String:
    if status == "needs":
        return "●"
    if status == "stopped":
        return "■"
    if status == "triage":
        return "◎"
    if status == "ready":
        return "✔"
    if status == "finished":
        return "◆"
    if status == "done":
        return "·"
    if status == "review":
        return "◐"
    if status == "working":
        return "○"
    if status == "todo":
        return "◌"
    if status == "dropped":
        return "–"
    return " "


def board_group(status: String) -> Int:
    """The board's groups, in order: what you do next first (BOARD_ORDER)."""
    if status == "needs" or status == "stopped":
        return 0
    if status == "triage":
        return 1
    if status == "ready":
        return 2
    if status == "finished":
        return 3
    if status == "review":
        return 4
    if status == "working":
        return 5
    if status == "todo":
        return 6
    if status == "done" or status == "dropped":
        return 7
    return 8


def board_order(tasks: List[Task]) -> List[Task]:
    """Tasks as the board lists them: by group (needs you first), then by number."""
    var out = tasks.copy()
    for i in range(1, len(out)):
        var j = i
        while j > 0 and (board_group(out[j - 1].status()), out[j - 1].num()) > (board_group(out[j].status()), out[j].num()):
            out.swap_elements(j - 1, j)
            j -= 1
    return out^


def status_color(status: String) -> String:
    if status == "needs":
        return "1;33"
    if status == "stopped":
        return "1;31"
    if status == "ready" or status == "finished":
        return "1;32"
    if status == "review":
        return "1;34"
    return "2"


def mark_color(t: Task) -> String:
    """The SGR colour of a task's mark: a quiet agent's in the colour of one that needs you."""
    return status_color("needs") if is_stuck(t) else status_color(t.status())


def ago(t: Task) raises -> String:
    """ "3m", "2h", "1d", "now", or "" with no time."""
    if not stamp(t):
        return ""
    var s = max(Int(seconds_since(stamp(t))), 0)
    if s >= 86400:
        return String(s // 86400) + "d"
    if s >= 3600:
        return String(s // 3600) + "h"
    if s >= 60:
        return String(s // 60) + "m"
    return "now"


def stale(t: Task) raises -> Bool:
    return t.status() == "needs" and stamp(t).byte_length() > 0 and seconds_since(stamp(t)) >= STALE


def doing_now(t: Task) raises -> String:
    """What the agent the task waits on last said it's doing, on one line, as the board
    shows it in place of its sentence: "" when it hasn't said, isn't working, said it
    before the task's status (less 30 s), over DOING_FOR ago, or is quiet or limited."""
    var block = String()
    if t.status() == "review":
        block = t.doc.str(t.get("review"), "block")
    elif t.status() == "working":
        block = t.s("block")
    if not block or is_stuck(t) or is_limited(t) or t.truthy("note"):
        return ""
    var path = state_dir() + "/doing/" + safe(block) + ".json"
    if not exists(path):
        return ""
    var d = Doc.read(path)
    var at = d.get(0, "at")
    if d.str(0, "state") != "working" or d.kind(at) != NUM:
        return ""
    var when = d.num(at)
    if stamp(t) and when < parse_iso(stamp(t)) - 30:
        return ""
    if now() - when > DOING_FOR:
        return ""
    var text = d.get(0, "text")
    return String(" ").join(split_words(py_str(d, text) if d.is_true(text) else ""))


def board_kind(kind: String) -> String:
    """BOARD_KINDS, or "" for a kind it hasn't."""
    if kind == "full":
        return "full review"
    if kind == "light":
        return "light review"
    if kind == "followup":
        return "follow-up review"
    if kind == "tour":
        return "review of your comments"
    if kind == "rebase":
        return "rebase check"
    return ""


def first_sentence(summary: String) -> String:
    """ "Recorded the demo." from "# Recorded the demo", or "" when there's none."""
    for raw in splitlines(summary):
        var line = strip(String(strip(String(raw)).lstrip("#")))
        if line:
            return line if line.endswith(".") or line.endswith("!") or line.endswith("?") else line + "."
    return ""


def limit_detail(t: Task) raises -> String:
    var lim = t.get("limit")
    var reviewer = t.doc.str(lim, "who") == "reviewer"
    var who = "Reviewer stopped at the usage limit" if reviewer else "Usage limit"
    if t.doc.truthy(lim, "resets"):
        return who + ", resets " + clock(parse_iso(t.doc.str(lim, "resets")), now()) + ". Continues by itself then."
    return who + ", reset time unknown. " + ("⏎ opens it." if reviewer else "t tells it to continue.")


def status_detail(repo: Repo, t: Task, every: Dict[String, String]) raises -> String:
    """The board's sentence for where the task stands."""
    var r = t.get("review")
    var rnd = py_str(t.doc, t.doc.get(r, "round")) if t.doc.truthy(r, "round") else "1"
    var why = t.s("why")
    var st = t.status()
    var pr = pr_short(t.s("pr"))
    if not pr and t.truthy("local"):
        pr = "Local branch " + t.s("branch")
    if t.truthy("note"):
        return py_str(t.doc, t.get("note"))
    if is_stuck(t):
        var quiet = stuck_phrase(t)
        if t.doc.str(t.get("stuck"), "who") == "reviewer":
            return "Reviewer has shown " + quiet + ". ⏎ opens it."
        return cap(quiet) + ". p shows its screen."
    if is_limited(t):
        return limit_detail(t)
    if (st == "needs" or st == "triage") and why == "Ended its turn without saying ready":
        return "Stopped mid-task, probably with a question. p shows its screen." if st == "needs" else "Ended its turn without ready."
    if st == "needs" or st == "triage":
        return why if why else ("Waiting for you." if st == "needs" else "Waiting for its coordinator.")
    if st == "stopped":
        return "Its agent exited. r resumes it."
    var kind = t.doc.str(r, "kind")
    if st == "review":
        var word = board_kind(kind)
        if not word:
            word = "review" if t.doc.truthy(r, "kind") else "reviewer"
        return cap(word) + " on round " + rnd + " of " + py_str(repo.cfg, repo.cfg.get(0, "review_rounds")) + "."
    if st == "ready" and why.startswith("Nothing to merge"):
        return "Done, with nothing to merge."
    if st == "finished":
        var line = first_sentence(t.s("summary"))
        return "Finished, nothing to merge: " + line + " a closes it." if line else "Finished, nothing to merge. a closes it."
    if st == "ready":
        var verdict = t.doc.str(r, "verdict")
        var text: String
        if verdict == "approved":
            var word = board_kind(kind)
            text = pr + ", approved by " + (word if word else "review") + "."
        elif verdict == "changes":
            text = pr + ". Review still asks for changes."
        else:
            text = pr + "." if pr else "Done."
        return text + (" Rebased onto " + py_str(t.doc, t.get("rebased")) + "." if t.truthy("rebased") else "")
    if st == "working" and sent_back_by_github(t):
        return "Fixing what GitHub said."
    if st == "working" and t.doc.str(r, "verdict") == "changes":
        return "Fixing what review round " + rnd + " found."
    if st == "todo":
        var waits = waits_phrase(t, every)
        return waits + "." if waits else "Not started. s starts it."
    if st == "done" and t.truthy("local") and not t.truthy("pr"):
        if t.truthy("merged") and t.truthy("base"):
            return "Accepted. Merged into " + py_str(t.doc, t.get("base")) + "."
        return "Accepted."
    if st == "done" and t.truthy("closed"):
        return "Closed, nothing to merge."
    if st == "done":
        return "Accepted. " + pr + " merged." if pr else "Accepted."
    return "Dropped." if st == "dropped" else ""


def auto_detail(repo: Repo, t: Task) -> String:
    """What auto-accept does with it, when that isn't what the config says for all."""
    var on = repo.cfg.truthy(0, "auto_accept")
    var own = t.doc.kind(t.get("auto_accept"))
    if closed(t.status()):
        return ""
    if t.truthy("auto_wait") and t.status() == "ready":
        return "Auto-accepts once its checks pass."
    if on and t.has("auto_accept") and own == FALSE:
        return "Kept for you: no auto-accept."
    if not on and own == TRUE:
        return "Auto-accepts once approved."
    return ""


def board_detail(repo: Repo, t: Task, every: Dict[String, String]) raises -> String:
    """The board's sentence for a task: what its agent is doing now, else where it
    stands, what GitHub said, what auto-accept does with it and its own model."""
    var doing = doing_now(t)
    if doing:
        return doing
    var text = status_detail(repo, t, every)
    var gh = "" if closed(t.status()) or t.truthy("note") else pr_phrase(t)
    if gh:
        text = strip(text + " " + cap(gh) + ".")
    var model = own_model(repo, t) if not closed(t.status()) else ""
    var parts = List[String]()
    for p in [text, auto_detail(repo, t), "Model: " + model + "." if model else ""]:
        if p:
            parts.append(p)
    return String(" ").join(parts)


def sgr(code: String, text: String, color: Bool) -> String:
    return "\x1b[" + code + "m" + text + "\x1b[0m" if color and text else text


def _pad(id: String, w: Int) -> String:
    return id + " " * max(w - id.byte_length(), 0)


def task_line(repo: Repo, t: Task, every: Dict[String, String], idw: Int = 0, width: Int = -1, color: Bool = False) raises -> String:
    """One task as the board says it: mark, ID, title, age, then the board's sentence.
    Cut to width columns when given (the title keeps room for the age); in colour with
    color, as on the board: the mark in its status's colour, the rest dim."""
    var tid = _pad(t.id(), idw)
    var when = ago(t)
    var text = board_detail(repo, t, every)
    var head = 2 + tid.byte_length() + 2
    var age_w = when.byte_length() + 2 if when else 0
    var title = t.s("title")
    if width >= 0:
        title = fit(title, max(width - head - age_w, 1))
        var room = width - head - text_width(title) - age_w - 2
        text = fit(text, room) if room >= 4 else ""
    var title_dim = closed(t.status()) or t.status() == "triage"
    var line = sgr(mark_color(t), mark(t.status()), color) + " " + sgr("2", tid, color) + "  " + sgr("2" if title_dim else "0", title, color)
    if when:
        line += "  " + sgr("1;31" if stale(t) else "2", when, color)
    if text:
        line += "  " + sgr("2", text, color)
    return line^


def task_rows(repo: Repo, t: Task, every: Dict[String, String], idw: Int = 0, width: Int = 80, color: Bool = False, lines: Int = 0) raises -> List[String]:
    """The board's rows for a task, for a terminal too narrow for task_line: mark, ID,
    title and the age at the right edge, then the board's sentence under the title in
    `lines` lines (by default, 0, the board's: two for one that needs you, else one)."""
    var tid = _pad(t.id(), idw)
    var when = ago(t)
    var w = width - 1  # never into the last column
    var head = 2 + tid.byte_length() + 2
    var title = fit(t.s("title"), max(w - head - when.byte_length() - 2, 1))
    var pad = " " * max(w - head - text_width(title) - when.byte_length(), 2) if when else ""
    var title_dim = closed(t.status()) or t.status() == "triage"
    var rows = List[String]()
    rows.append(sgr(mark_color(t), mark(t.status()), color) + " " + sgr("2", tid, color) + "  "
                + sgr("2" if title_dim else "0", title, color) + pad + sgr("1;31" if stale(t) else "2", when, color))
    var text = board_detail(repo, t, every)
    var st = t.status()
    var n = lines
    if not n:
        n = 2 if st == "needs" or st == "stopped" or st == "finished" or is_stuck(t) or is_limited(t) or pr_phrase(t) else 1
    for line in wrap(text, w - head, n):
        if line:
            rows.append(" " * head + sgr("2", line, color))
    return rows^


def isatty() -> Bool:
    return external_call["isatty", c_int](c_int(1)) == 1


def term_width() -> Int:
    """The terminal's width when stdout is one, else -1 (None): piped output is whole.
    As shutil.get_terminal_size: COLUMNS first, then the terminal's own size, then 80."""
    if not isatty():
        return -1
    var cols: Int
    try:
        cols = Int(String(getenv("COLUMNS").strip()))
    except:
        cols = 0
    if cols > 0:
        return cols
    var ws = List[UInt16](length=4, fill=0)  # struct winsize: rows, columns, x and y pixels
    comptime TIOCGWINSZ = 0x40087468 if CompilationTarget.is_macos() else 0x5413
    if external_call["ioctl", c_int](c_int(1), UInt(TIOCGWINSZ), Int(ws.unsafe_ptr())) != 0:
        return 80
    return Int(ws[1]) if ws[1] else 80


def colored() -> Bool:
    return isatty() and not getenv("NO_COLOR")


def is_stuck(t: Task) -> Bool:
    """Has the agent the task is waiting on (its worker, or its reviewer in review)
    shown no progress for too long?"""
    var s = t.get("stuck")
    if not t.doc.is_true(s):
        return False
    return t.status() == ("review" if t.doc.str(s, "who") == "reviewer" else "working")


def stuck_phrase(t: Task) raises -> String:
    """ "no progress for 25 min", or "" for a task that isn't stuck."""
    if not is_stuck(t):
        return ""
    var secs = seconds_since(t.doc.str(t.get("stuck"), "since"))
    var m = Int(secs // 60)
    return "no progress for " + String(m) + " min" if m < 120 else "no progress for " + String(m // 60) + " h"


def sent_back_by_github(t: Task) -> Bool:
    """Is the worker fixing failed checks or comments from its PR?"""
    return t.doc.truthy(t.get("gh"), "sent")


def pr_phrase(t: Task) -> String:
    """The PR's latest state in a few words, for the board and `show`, or "".
    A working task only shows what GitHub sent it back for: anything else is
    about the old push."""
    var g = t.get("gh")
    var state = t.doc.str(g, "state")
    if state == "merged" or state == "closed":
        return state + " on GitHub"
    if t.status() == "working" and not sent_back_by_github(t):
        return ""
    var parts = List[String]()
    if t.doc.truthy(g, "checks"):
        parts.append("checks " + py_str(t.doc, t.doc.get(g, "checks")))
    if t.doc.truthy(g, "new"):
        var new = t.doc.get(g, "new")
        parts.append(py_str(t.doc, new) + " new comment" + ("s" if t.doc.num(new) != 1 else ""))
    return String(", ").join(parts)
