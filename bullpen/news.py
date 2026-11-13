"""News for the coordinators: a news line, the outboxes the watcher types into their
panes, and `bullpen news` for one that follows it by monitor.
"""

import argparse
import fcntl
import os
import re
import shlex
import signal
import sys
import time
from pathlib import Path

from .core import (
    Fail, REX, Repo, SELF, STATE, command, load_config, merge_base, now, repo_root, run, safe,
    task_base_ref, write_json,
)
from .rex import rex_type, with_state
from .coordinators import alive, calling_coordinator, coordinator_at, primary, sees_to


NEWS_MARK = "bullpen · "


def ready_news(repo, t, facts=(), then="", again=False):
    """The news that the task is ready: `BP-3 is ready: <title> (<facts>)<then>`, the
    title second, where a long line is cut last. A task that was ready before is
    "ready again", "after a rebase" if its base moved since; a line word for word the
    one before it about the task isn't sent again."""
    wt = t.get("worktree")
    base = merge_base(wt, task_base_ref(repo, t)) if wt and Path(wt).is_dir() else None
    told = t.get("ready_told")
    if told is not None or again:
        again = " again after a rebase" if told and base and told != base else " again"
    t["ready_told"] = base or ""
    write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change here
    facts = ", ".join(f for f in facts if f)
    text = f"{t['id']} is ready{again or ''}: {t['title']}" + (f" ({facts})" if facts else "") + then
    if last_news(repo, t["id"]) == NEWS_MARK + " ".join(text.split()):
        return
    news(repo, t, "ready", text)


def last_news(repo, tid):
    """The last line of news about task tid, as it was logged, or None."""
    log = repo.dir / "news.log"
    if not log.exists():
        return None
    with log.open("rb") as f:
        f.seek(max(0, log.stat().st_size - 65536))
        lines = f.read().decode(errors="replace").splitlines()
    for line in reversed(lines):
        m = re.match(r"\S+(?: \[[^\]]*\])? (\S+) [^ :]+: (.*)$", line)
        if m and m.group(1) == tid:
            return m.group(2)
    return None


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


# ---------- news by monitor: `bullpen news [--follow]` ----------


def cursor_path(repo, cid):
    return repo.dir / (f"news-{safe(cid)}.cursor" if cid else "news.cursor")


def news_cursor(repo, cid=None):
    """How far into news.log (in bytes) coordinator cid has read with `bullpen news`,
    or None: never. (news.cursor: the one cursor from before several coordinators, or
    the reader when there's no coordinator; c1 carries it on.)"""
    for path in (cursor_path(repo, cid), *([cursor_path(repo, None)] if cid == "c1" else [])):
        try:
            return int(path.read_text())
        except (FileNotFoundError, ValueError):
            pass
    return None


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


def news_for(text, cid, listed):
    """Is a logged line for coordinator cid? Lines name the coordinators they're for
    as `[c1,c2]` before the task ID; lines without, or for coordinators no longer
    listed, are for everyone."""
    head = text.split(NEWS_MARK, 1)[0].split()
    to = next((w[1:-1].split(",") for w in head if w.startswith("[") and w.endswith("]")), None)
    return not cid or not to or cid in to or not any(x in listed for x in to)


def print_news(repo, offset, cid=None):
    """Print coordinator cid's news logged after offset, moving its cursor past each
    line once it is out (or skipped: another coordinator's). A line's own lines after
    it (each starting with a tab: a diff) go with it. Returns the new offset. A line
    still being written waits."""
    size = news_size(repo)
    if offset > size:  # the log was removed or cut short: what's there now is new
        offset = 0
        set_news_cursor(repo, 0, cid)
    if offset == size:
        return offset
    listed = {c["id"] for c in load_config(repo.dir)["coordinators"]}
    with (repo.dir / "news.log").open("rb") as f:
        f.seek(offset)
        for raws in news_records(f):
            text = raws[0].decode(errors="replace").rstrip("\n")
            at = text.find(NEWS_MARK)
            # Printing and moving the cursor go together: being stopped in between
            # (a monitor expiring) would print the line again next time.
            held = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM, signal.SIGINT, signal.SIGHUP})
            try:
                # a tab-led line with no line before it (a cursor left between them) is skipped
                if not text.startswith("\t") and news_for(text, cid, listed):
                    rest = [r.decode(errors="replace")[1:].rstrip("\n") for r in raws[1:]]
                    print("\n".join([text[at:] if at >= 0 else text, *rest]), flush=True)
                offset += sum(len(r) for r in raws)
                set_news_cursor(repo, offset, cid)  # after printing: if nobody was reading, it comes again
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, held)
    return offset


def news_records(f):
    """The whole news lines left in f, each with the tab-led lines that follow it, as
    lists of raw lines. One still being written isn't given."""
    rec = []
    for raw in f:
        if not raw.endswith(b"\n"):
            return
        if rec and not raw.startswith(b"\t"):
            yield rec
            rec = []
        rec.append(raw)
    if rec:
        yield rec


def news_reader(repo):
    """Whose news `bullpen news` prints: the coordinator in whose pane it runs, else
    the primary one. None (all news) when the repo has no coordinator."""
    c = calling_coordinator(repo) or primary(repo)
    return c and c["id"]


def cmd_news(args):
    """Print the news logged for this coordinator since the last time, once each;
    --follow keeps going. Each coordinator has its own cursor."""
    repo = Repo(repo_root())
    repo.dir.mkdir(parents=True, exist_ok=True)
    cid = news_reader(repo)
    if not_a_reader(repo, cid):
        if args.follow:  # the Monitor tool shows what's printed: say it there
            return print(NOT_A_COORDINATOR, flush=True)
        raise Fail(NOT_A_COORDINATOR.removeprefix(NEWS_MARK))
    lock = (repo.dir / (f"news-{safe(cid)}.lock" if cid else "news.lock")).open("w")
    try:
        # One reader at a time, or lines would come twice. A second follower waits
        # for the first to go (a monitor being replaced); a one-off read doesn't.
        fcntl.flock(lock, fcntl.LOCK_EX if args.follow else fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise Fail("news is being followed already (`bullpen news --follow` is running)")
    offset = news_cursor(repo, cid)
    if offset is None:  # first time: old news stays old
        offset = news_size(repo)
        set_news_cursor(repo, offset, cid)
    try:
        while True:
            if args.follow and not news_by_monitor(repo):
                # Typed news is logged too: following it would give every line twice.
                print(NEWS_MARK + "news is typed into your pane now (`bullpen config --news type`): "
                      "don't start the monitor again", flush=True)
                return
            if args.follow and not_a_reader(repo, cid):  # dropped while following
                return print(NOT_A_COORDINATOR, flush=True)
            offset = print_news(repo, offset, cid)
            if not args.follow:
                return
            time.sleep(args.every)
    except BrokenPipeError:  # whoever was reading is gone; the cursor stays before the lost line
        os.dup2(os.open(os.devnull, os.O_WRONLY), sys.stdout.fileno())
    except KeyboardInterrupt:
        pass


NOT_A_COORDINATOR = (NEWS_MARK + "this pane isn't one of this repo's coordinators, so it doesn't read "
                     "their news: don't start the monitor again (`bullpen up --here` here makes it one)")


def not_a_reader(repo, cid):
    """Is `bullpen news` running in a Rex pane other than coordinator cid's? Each line
    is printed once, so a reader there (an agent that used to be the coordinator, a
    shell) would take lines from the coordinator's own monitor. Outside Rex there's
    no telling: it reads the primary's news, as before."""
    block = os.environ.get("REX_BLOCK")
    if not block or not cid:
        return False
    repo.cfg = load_config(repo.dir)  # read again: it can change while we follow
    me = coordinator_at(repo, block, boards=False)
    return not me or me["id"] != cid


def news_by_monitor(repo):
    """Is the setting `monitor` right now? Read again: it can change while we follow."""
    return load_config(repo.dir).get("news") == "monitor"


def follow_cmd():
    return shlex.join(with_state([SELF, "news", "--follow"]))


def news_mode_line(repo):
    """One line saying how news reaches the coordinator."""
    if repo.cfg.get("news") == "monitor":
        return (f"news: by monitor. Nothing is typed: the coordinator must run the Monitor "
                f"tool on `{follow_cmd()}` (its guide says how).")
    return ("news: typed into the coordinator's pane "
            "(`bullpen config --news monitor`: by Claude Code's Monitor tool instead).")


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "news", cmd_news, "Coordinator",
                help="print news logged since the last time; --follow keeps printing")
    p.add_argument("--follow", action="store_true", help="keep running and print each line as it is logged")
    p.add_argument("--every", type=float, default=0.5, help=argparse.SUPPRESS)  # seconds between looks
