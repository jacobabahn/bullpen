"""The person's notifications: when a task comes to need them, or (notify all) is ready or done."""

import contextlib
import fcntl
import os
import re
import subprocess
import time
import tty

from .core import (
    Fail, Repo, STATE, clip, command, load_config, now, on_save, read_json,
    write_json,
)
from .rex import pane_tty, rex_view
from .agents import pane, placed, waits_on
from .coordinators import home_of
from .news import NEWS_MARK, deliver


# A task that comes to need the person (a prompt, `ask`, a triage escalated), and with
# notify "all" one that is ready, finished or auto-accepted, gets a native notification:
# OSC 777 written to the terminal of the task's pane (else its board's, else its
# coordinator's), which Rex for Mac shows, and opens that pane's tab when clicked. It
# goes out when the change is saved (save_task), so once per change, from whatever made
# it: mostly the watcher's calls, and `ask`, `ready`, `verdict`.
#
# Several tasks that come to need the person at once (within NOTIFY_BURST seconds of
# a notification for another task) get one more between them, not one each: they wait
# in notify.json, and once none has come for NOTIFY_QUIET seconds the watcher's
# outbox round (`_flush`) sends "3 tasks need you: BP-3, BP-4, BP-5".

NOTIFY_BURST = 10  # seconds
NOTIFY_QUIET = 3


def notice(t, old):
    """(kind, text) of the notification the task's change from old (as it was saved)
    deserves, kind "needs" or "news", else None."""
    s, was = t["status"], old.get("status")
    asked = t.get("ask") and not (old.get("ask") and old.get("why") == t.get("why"))
    if s == "needs" and (was != "needs" or asked):
        why = t.get("why") or "it is stopped"
        m = re.match(r"(Reviewer )?[Nn]eeds you\b:?\s*", why)
        if m:
            who = f"{t['id']}'s reviewer" if m.group(1) else t["id"]
            rest = why[m.end():]
            return "needs", f"{who} needs you" + (f" {rest}" if rest.startswith("(") else f": {rest}" if rest else "")
        return "needs", f"{t['id']} needs you: {why}"
    if s == was:
        return None
    if s == "ready":
        return "news", f"{t['id']} is ready: {t['title']}"
    if s == "finished":
        return "news", f"{t['id']} finished: {t['title']}"
    if s == "done" and t.get("auto_accepted"):
        return "news", f"{t['id']} was auto-accepted: {t['title']}"
    return None


@on_save
def notify(repo, t, old):
    """Tell the person of the task's change, if notify says to and they aren't looking.
    Never fails: a notification that can't go out is noted in notify.log."""
    n = notice(t, old)
    level = repo.cfg["notify"]
    if not n or level == "off" or (level == "needs" and n[0] != "needs"):
        return
    try:
        if looking(repo, t):
            return log_notify(repo, t, n[1], "not sent: the person is looking")
        if n[0] == "needs" and held(repo, t, n[1]):
            return log_notify(repo, t, n[1], "held: another task needs you too")
        send_notification(repo, t, n[1])
    except (OSError, ValueError, subprocess.SubprocessError) as e:
        log_notify(repo, t, n[1], f"not sent: {e}")


def send_notification(repo, t, text):
    title = f"{NEWS_MARK}{repo.root.name}"
    for session, block in notify_panes(repo, t):
        tty = pane_tty(session, block)
        if tty and send_osc(tty, title, text):
            return log_notify(repo, t, text, f"sent to {block}")
    log_notify(repo, t, text, "not sent: no pane to send it from")


@contextlib.contextmanager
def notify_state(repo):
    """notify.json, locked against the other bullpen processes the same burst runs:
    {"at": when the last went out, "ids": the tasks it and those held since were
    for, "held": [[ID, text], ...] waiting, "last": when the last was held}."""
    repo.dir.mkdir(parents=True, exist_ok=True)
    with (repo.dir / "notify.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        path = repo.dir / "notify.json"
        st = read_json(path, {}) or {}
        yield st
        write_json(path, st)


def held(repo, t, text):
    """Hold a task's needs-you notification if one for another task went out in the
    last NOTIFY_BURST seconds; else note that this one goes out now. True if held."""
    with notify_state(repo) as st:
        clock = time.time()
        ids = st.get("ids") or []
        if clock - st.get("at", 0) < NOTIFY_BURST and any(i != t["id"] for i in ids):
            st["held"] = [h for h in st.get("held") or [] if h[0] != t["id"]] + [[t["id"], text]]
            st.update(ids=ids + [t["id"]] * (t["id"] not in ids), last=clock)
            (STATE / "outbox").touch()  # the watcher's outbox round sends it
            return True
        if clock - st.get("at", 0) >= NOTIFY_BURST:
            ids = []
        st.update(at=clock, ids=ids + [t["id"]] * (t["id"] not in ids))
        return False


def flush_notifications(repo):
    """Send the held notifications once none has come for NOTIFY_QUIET seconds: one,
    for all the tasks of the burst. False while some still wait."""
    if not (repo.dir / "notify.json").exists():
        return True
    with notify_state(repo) as st:
        hold = st.get("held") or []
        if not hold:
            return True
        if time.time() - st.get("last", 0) < NOTIFY_QUIET:
            return False
        ids = st.get("ids") or []
        text = hold[0][1] if len(hold) == 1 else f"{len(ids)} tasks need you: {', '.join(ids)}"
        st.update(held=[], at=time.time())
    try:
        t = repo.task(hold[-1][0])
        send_notification(repo, t, text)
    except (Fail, OSError, ValueError, subprocess.SubprocessError) as e:
        with (repo.dir / "notify.log").open("a") as f:
            f.write(f"{now()} {hold[-1][0]} not sent: {e}: {text}\n")
    return True


def notify_panes(repo, t):
    """(session, block) of the panes a notification can come from, best first: the task's
    reviewer if it is what needs the person, its worker, its coordinator's board, its
    coordinator. Clicking it opens the pane's tab: a pane in the background has none, so
    it's passed over (the board's Enter, or `go --next`, brings it up)."""
    c = home_of(repo, t)
    b = c.get("board") or {}
    agents = [who for who in ("reviewer", "worker") if placed(t, who) != "background"
              and (who == "worker" or waits_on(t) == "reviewer")]
    out = [pane(t, who) for who in agents] + [(b.get("session"), b.get("block")), (c.get("session"), c.get("block"))]
    return [p for p in out if p[1]]


def looking(repo, t):
    """Has the task's worker or reviewer pane, or its coordinator's board, focus in its
    session's active window? Then the person sees it already. (Rex can't say whether
    its app is in front, so this goes by the session alone.)"""
    b = home_of(repo, t).get("board") or {}
    panes = [pane(t, "worker"), pane(t, "reviewer"), (b.get("session"), b.get("block"))]
    blocks = {blk for _, blk in panes if blk}
    for session in {s for s, blk in panes if blk}:
        v = rex_view(session) or {}
        for w in v.get("windows", []):
            if w.get("window_id") == v.get("active_window_id") and w.get("focused_block_id") in blocks:
                return True
    return False


def send_osc(tty, title, body):
    """Write a notification (OSC 777) to a terminal device. False if it can't be opened."""
    def clean(text, n, also=""):
        return clip(re.sub(f"[\\x00-\\x1f\\x7f{also}]", " ", text), n)
    # the title ends at the first ";": the body may have them
    data = f"\x1b]777;notify;{clean(title, 60, ';')};{clean(body, 240)}\x07".encode()
    try:
        fd = os.open(tty, os.O_WRONLY | os.O_NOCTTY | os.O_NONBLOCK)
    except OSError:
        return False
    try:
        os.write(fd, data)  # in one write, so it lands between two of the program's own
    finally:
        os.close(fd)
    return True


def log_notify(repo, t, text, how):
    with (repo.dir / "notify.log").open("a") as f:
        f.write(f"{now()} {t['id']} {how}: {text}\n")


def cmd_flush(args):
    """(watcher) deliver news, and send notifications, that had to wait."""
    waiting = False
    for d in {p.parent for p in (STATE / "repos").glob("*/outbox*.txt")}:
        repo = Repo(read_json(d / "config.json", {}).get("root") or "/")
        repo.dir = d
        repo.cfg = load_config(d)
        if not deliver(repo):
            waiting = True
    for d in {p.parent for p in (STATE / "repos").glob("*/notify.json")}:
        repo = Repo(read_json(d / "config.json", {}).get("root") or "/")
        repo.dir = d
        repo.cfg = load_config(d)
        if not flush_notifications(repo):
            waiting = True
    if not waiting:
        (STATE / "outbox").unlink(missing_ok=True)


def commands(sub):
    """This module's commands, for bullpen's parser."""
    command(sub, "_flush", cmd_flush)
