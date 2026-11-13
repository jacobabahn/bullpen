"""News for the coordinators: a news line, and the outboxes the watcher types into
their panes. A twin of cli/src/news/ for the Python commands; `bullpen news` (and
--follow) is Rust's.
"""

import os
import re
import shlex

from .core import Fail, REX, Repo, SELF, STATE, now, repo_root, run, safe
from . import jobs
from .rex import rex_type, with_state
from .coordinators import alive, calling_coordinator, sees_to


NEWS_MARK = "bullpen · "
JOB_NEWS = {"id": "job"}  # the news log's task column for a job with no task behind it


def job_failed(cwd, tid, line):
    """A background job failed without saying so (jobs.py): news to the task's
    coordinator, or to everyone."""
    repo = Repo(repo_root(cwd))
    try:
        t = repo.task(tid) if tid else JOB_NEWS
    except Fail:
        t = JOB_NEWS
    news(repo, t, "job", line, everyone=t is JOB_NEWS)


jobs.FAILED.append(job_failed)


def news(repo, task, event, text, everyone=False, skip=None, typed=None, to=None, more=()):
    """One line of news, `bullpen · <what happened>`, for the coordinator that owns the
    task (everyone=True: for all of them, as news that isn't about one task is): logged,
    and typed into their prompts unless they follow the log (config --news monitor).
    skip: a coordinator not to tell (the one that ran the command). typed: what to type
    in place of the line (the person's own words, not bullpen's). to: the IDs of the
    coordinators it is for, whoever owns what. more: lines that go after it, as they
    are (a diff): logged after it, each starting with a tab, and typed with it as one
    message."""
    line = NEWS_MARK + " ".join(text.split())  # one line: a newline would send it early
    to, waits = (to, False) if to else news_to(repo, task, everyone)
    if skip and to and skip in to:
        to = [cid for cid in to if cid != skip]
        if not to:
            return  # only the coordinator that did it would hear it
    log = repo.dir / "news.log"
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open("a") as f:
        f.write(f"{now()}{' [' + ','.join(to) + ']' if to else ''} {task['id']} {event}: {line}\n"
                + "".join(f"\t{l}\n" for l in more))  # one write: a reader never sees half
    if repo.cfg.get("news") != "monitor":
        # With no coordinator alive the line waits once, in the shared outbox: one copy
        # per coordinator would reach the first to come back once for each.
        said = " ".join(typed.split()) if typed else line
        type_news(repo, LINE_BREAK.join([said, *more]), None if waits else to)


def news_to(repo, task, everyone=False):
    """(IDs, waits): the coordinators a line of news is for, those who see to the task
    (every live one, with everyone); with none alive, all of them (waits is True: it
    has to wait for one to come back). ([], True) when the repo has none."""
    cs = repo.cfg["coordinators"]
    to = [c for c in cs if alive(c)] if everyone else sees_to(repo, task)
    return [c["id"] for c in (to or cs)], not to


# An outbox holds one message a line; a message of several lines (news with a diff
# after it) keeps its line breaks as this, and gets them back when it is typed. Claude
# Code takes a newline that comes in a typed burst as part of the message (a paste), and
# only the Enter after it sends it.
LINE_BREAK = "\x1f"


def outbox_path(repo, cid=None):
    """Typed news waiting for coordinator cid. Without one: news that came while no
    coordinator was alive (or there was none), for every one alive when it's typed."""
    return repo.dir / (f"outbox-{safe(cid)}.txt" if cid else "outbox.txt")


def outboxes(d):
    return sorted(d.glob("outbox*.txt"))


def type_news(repo, line, to=None):
    """Type a line into these coordinators' prompts, now or once their input is empty."""
    repo.dir.mkdir(parents=True, exist_ok=True)
    for cid in to or [None]:
        with outbox_path(repo, cid).open("a") as f:
            f.write(line + "\n")
    if not deliver(repo):
        (STATE / "outbox").touch()  # the watcher retries while this exists


def tell_coordinator(repo, task, event, text):
    """News of something the person may have done elsewhere (the board, a shell).
    The coordinator whose pane ran it isn't told: it would only repeat what it just ran."""
    me = calling_coordinator(repo)
    news(repo, task, event, text, skip=me and me["id"])


def has_draft(session, block):
    """Is something half-typed in the coordinator's input? Typing news there would
    send it along. (Reads Claude Code's input box: the last line starting ❯. Faint
    text there is Claude Code's suggestion or placeholder, not the person's: a
    suggestion can show for as long as the pane is idle, so it mustn't hold news.)"""
    p = run([REX, "capture", *(["-s", session] if session else []), "-b", block, "--format=vt"],
            check=False)
    state = {"faint": False}
    lines = [styled_line(l, state) for l in p.stdout.splitlines()]
    lines = [l for l in lines if "".join(t for t, _ in l).strip()]
    for line in reversed(lines[-10:]):
        text = "".join(t for t, _ in line).strip()
        if text.startswith("❯"):
            typed = "".join(t for t, faint in line if not faint)
            return bool(typed.split("❯", 1)[-1].strip())
    return False


VT_ESC = re.compile(r"\x1b\[([0-9;:]*)m|\x1b\[(\d*)C|\x1b\[[0-9;:?<=>]*[ -/]*[@-~]|\x1b[@-_]")


def styled_line(raw, state):
    """One line of a `rex capture --format=vt` screen as (text, faint) pieces. Faint
    (SGR 2) carries on from line to line until reset, so state is shared: pass the
    lines of one capture in order."""
    out, pos = [], 0
    for m in VT_ESC.finditer(raw):
        out.append((raw[pos:m.start()], state["faint"]))
        pos = m.end()
        if m.group(1) is not None:
            state["faint"] = sgr_faint(m.group(1), state["faint"])
        elif m.group(2) is not None:
            out.append((" " * int(m.group(2) or 1), state["faint"]))  # cursor forward: a gap
    out.append((raw[pos:].replace("\r", ""), state["faint"]))
    return [(t, f) for t, f in out if t]


def sgr_faint(params, faint):
    """Is text faint after this SGR (`\\x1b[<params>m`)?"""
    ps = params.split(";") if params else ["0"]
    i = 0
    while i < len(ps):
        p = ps[i].split(":")[0]
        if p in ("", "0", "22"):
            faint = False
        elif p == "2":
            faint = True
        elif p in ("38", "48", "58") and ":" not in ps[i] and i + 1 < len(ps):
            i += {"5": 2, "2": 4}.get(ps[i + 1], 1)  # a colour's own numbers
        i += 1
    return faint


def take_lines(path):
    lines = path.read_text().splitlines() if path.exists() else []
    path.unlink(missing_ok=True)
    return lines


def add_lines(path, lines):
    if lines:
        with path.open("a") as f:
            f.write("".join(l + "\n" for l in lines))


def deliver(repo):
    """Type each coordinator's waiting news into its pane, oldest first. News for a
    coordinator whose pane is gone goes to the live ones instead. False if some has
    to wait: no live coordinator, or the person is typing there."""
    files = outboxes(repo.dir)
    if not files:
        return True
    cs = repo.cfg["coordinators"]
    live = [c for c in cs if alive(c)]
    if not live:
        return False
    for path in files:
        cid = path.stem[len("outbox-"):] if path.stem != "outbox" else None
        if cid and any(c["id"] == cid for c in live):
            continue
        # shared, or for a coordinator that's gone: the live ones have it
        lines = take_lines(path)
        for c in live:
            add_lines(outbox_path(repo, c["id"]), lines)
    done = True
    for c in live:
        path = outbox_path(repo, c["id"])
        if not path.exists():
            continue
        if has_draft(c.get("session"), c["block"]):
            done = False
            continue
        for line in take_lines(path):
            rex_type(c.get("session"), c["block"], line.replace(LINE_BREAK, "\n"))
    return done


# ---------- news by monitor: `bullpen news [--follow]` is Rust's (cli/src/news/read.rs) ----------


def cursor_path(repo, cid):
    return repo.dir / (f"news-{safe(cid)}.cursor" if cid else "news.cursor")


def set_news_cursor(repo, offset, cid=None):
    repo.dir.mkdir(parents=True, exist_ok=True)
    path = cursor_path(repo, cid)
    tmp = path.with_name(f".{path.name}.tmp")
    tmp.write_text(f"{offset}\n")
    os.replace(tmp, path)


def news_size(repo):
    try:
        return (repo.dir / "news.log").stat().st_size
    except FileNotFoundError:
        return 0


def follow_cmd():
    return shlex.join(with_state([SELF, "news", "--follow"]))


def news_mode_line(repo):
    """One line saying how news reaches the coordinator."""
    if repo.cfg.get("news") == "monitor":
        return (f"news: by monitor. Nothing is typed: the coordinator must run the Monitor "
                f"tool on `{follow_cmd()}` (its guide says how).")
    return ("news: typed into the coordinator's pane "
            "(`bullpen config --news monitor`: by Claude Code's Monitor tool instead).")
