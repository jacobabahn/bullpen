"""The usage limit: reading it off an agent's screen, and what holds until it resets."""

import datetime as dt
import re

from .core import (
    REX, STATE, WAITING, clip, now, read_json, run, seconds_since, set_status, write_json,
)
from .agents import is_limited, pane
from .news import news


# Every agent shares one subscription. An agent that reaches its usage limit stops
# with a line like "You've hit your 5-hour limit · resets 3pm": Claude Code reports
# that turn as `error` (or just goes idle), with the line as the record's msg or
# only on its screen. Nothing for the person to do: the task keeps its status,
# carries a `limit` and is continued after the reset. <state>/limit.json holds the
# limit for every repo while it lasts; `start` refuses meanwhile.

LIMIT_FILE = STATE / "limit.json"
# The patterns are compiled on first use (re caches them), not on import: most calls
# that import this module never read a limit.
LIMIT_RE = (r"(?i)you['’]ve hit your [\w' -]*limit|usage limit reached|limit will reset"
            r"|out of usage credits")
LIMIT_STATES = ("error", "idle", "done")  # how a turn ends; a blocked agent waits on the person
LIMIT_GRACE = 120          # seconds after the reset before continuing: Claude Code may do it itself
LIMIT_FORGET = 5 * 3600    # seconds a limit with no known reset time holds `start` back
LIMIT_CONTINUE = "The usage limit has reset. Carry on where you stopped."
DAYS = ("mon", "tue", "wed", "thu", "fri", "sat", "sun")
MONTHS = ("jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec")
RESET_RE = (
    r"(?i)resets?(?:\s+at)?\s+"
    rf"(?:(?P<day>{'|'.join(DAYS)})[a-z]*,?\s+)?"
    rf"(?:(?P<month>{'|'.join(MONTHS)})[a-z]*\.?\s+(?P<date>\d{{1,2}})(?:,|\s+at)?\s+)?"
    r"(?P<h>\d{1,2})(?::(?P<m>\d{2}))?\s*(?P<ap>[ap])?(?(ap)\.?m\b\.?)"
    r"(?:\s*\((?P<tz>[A-Za-z_]+(?:/[A-Za-z0-9_+-]+)*)\))?")


def limit_reset(text, at=None):
    """When a limit message says the limit resets, as a local time, or None if it
    doesn't say so in a way we can read. Reads "resets 3pm", "resets 3:45pm",
    "resets Mon 12:00am", "resets Oct 9, 3pm", "reset at 3pm (America/New_York)"
    and "resets 15:00"; a time with no date is the next time the clock shows it."""
    at = at or dt.datetime.now().astimezone()
    m = re.search(r"limit reached\|(\d{10})\b", text)  # older Claude Code: a Unix time
    if m:
        return dt.datetime.fromtimestamp(int(m.group(1))).astimezone()
    m = re.search(RESET_RE, text)
    if not m or not (m["ap"] or m["m"]):
        return None
    hour, minute = int(m["h"]), int(m["m"] or 0)
    if m["ap"]:
        if not 1 <= hour <= 12:
            return None
        hour = hour % 12 + (12 if m["ap"].lower() == "p" else 0)
    if hour > 23 or minute > 59:
        return None
    here = at
    if m["tz"]:
        try:
            import zoneinfo
            here = at.astimezone(zoneinfo.ZoneInfo(m["tz"]))
        except Exception:
            return None
    when = here.replace(hour=hour, minute=minute, second=0, microsecond=0)
    slack = dt.timedelta(minutes=5)  # a time just gone is now, not tomorrow
    if m["month"]:
        try:
            when = when.replace(month=MONTHS.index(m["month"].lower()) + 1, day=int(m["date"]))
        except ValueError:
            return None
        if when < here - dt.timedelta(days=1):
            when = when.replace(year=when.year + 1)
    elif m["day"]:
        when += dt.timedelta(days=(DAYS.index(m["day"].lower()) - when.weekday()) % 7)
        if when <= here - slack:
            when += dt.timedelta(days=7)
    elif when <= here - slack:
        when += dt.timedelta(days=1)
    return when.astimezone()


def clock(when, at=None):
    """ "3:00 PM", with the day when it isn't today: "Mon 12:00 AM", "Oct 9 3:00 PM"."""
    at = (at or dt.datetime.now().astimezone()).astimezone()
    when = when.astimezone()
    text = f"{when.hour % 12 or 12}:{when.minute:02d} {'AM' if when.hour < 12 else 'PM'}"
    days = (when.date() - at.date()).days
    if days == 0:
        return text
    return (f"{when:%a} " if 0 < days < 7 else f"{when:%b} {when.day} ") + text


def limit_phrase(resets, at=None):
    """ "resets 3:00 PM" for a stored reset time, "reset time unknown" for none."""
    return f"resets {clock(dt.datetime.fromisoformat(resets), at)}" if resets else "reset time unknown"


def limit_active(at=None):
    """The usage limit in force ({"at", "resets", "text"}), or None. It holds until
    its reset time; one with no known reset time, for LIMIT_FORGET seconds."""
    lim = read_json(LIMIT_FILE)
    if not lim:
        return None
    if lim.get("resets"):
        return lim if seconds_since(lim["resets"], at) < 0 else None
    return lim if seconds_since(lim["at"], at) < LIMIT_FORGET else None


def limited(t, who):
    """Is this agent of the task (its "worker" or its "reviewer") stopped at the limit?"""
    return is_limited(t) and t["limit"]["who"] == who


def limit_why(lim, at=None):
    return f"Usage limit, {limit_phrase(lim.get('resets'), at)}"


def limit_text(session, block, state, msg, seen=None):
    """The limit message an agent's turn ended with, or None. It is the record's msg,
    or (for a turn that failed or went idle, not one that finished) among the last
    lines of its screen. A screen still showing the message we've already acted on
    (`seen`) says nothing new."""
    if state not in LIMIT_STATES:
        return None
    if re.search(LIMIT_RE, msg):
        return msg
    if state == "done":
        return None
    p = run([REX, "capture", *(["-s", session] if session else []), "-b", block], check=False)
    lines = [" ".join(l.split()) for l in p.stdout.splitlines() if l.strip()][-15:]
    for i in range(len(lines) - 1, -1, -1):
        if re.search(LIMIT_RE, lines[i]):
            text = clip(" ".join(lines[i:i + 2]), 200)  # with the next line: it may have wrapped
            return None if text == seen else text
    return None


def limit_hit(repo, t, who, ev):
    """Did this event say the agent stopped at the usage limit? Then note it on the
    task and for `start`, with one line of news, and return True. ev: the event
    (events.Event)."""
    session, _ = pane(t, who)
    text = limit_text(session, ev.block, ev.state, (ev.msg or "").strip(), t.get("limit_seen"))
    if not text:
        return False
    text = clip(text, 200)
    before = limit_active()
    when = limit_reset(text)
    resets = when.isoformat(timespec="seconds") if when else (before or {}).get("resets")
    write_json(LIMIT_FILE, {"at": now(), "resets": resets, "text": text})
    name = f"{t['id']}'s reviewer" if who == "reviewer" else t["id"]
    waits = t["status"] in (("review", *WAITING) if who == "reviewer" else ("working", *WAITING, "stopped"))
    if waits:  # else the task doesn't wait on this agent: nothing to continue later
        lim = {"who": who, "at": now(), "resets": resets, "text": text}
        set_status(t, "review" if who == "reviewer" else "working", limit_why(lim))
        t["limit"] = lim
        t["last"] = f"{ev.state}|{(ev.msg or '').strip()}" if who == "worker" else t.get("last")
        repo.save_task(t)
    if resets:
        line = f"{name} stopped at the usage limit, {limit_phrase(resets)}: nothing to do" + \
            (", it is continued then" if waits else "")
    elif who == "reviewer":
        line = (f"{name} stopped at the usage limit, and its reset time couldn't be read, so it won't "
                f"be continued by itself: `bullpen go {t['id']} --review` shows it")
    else:
        line = (f"{name} stopped at the usage limit, and its reset time couldn't be read, so it won't "
                f"be continued by itself: `bullpen tell {t['id']} continue` once the limit has reset")
    if not before:
        line += ". `bullpen start` refuses meanwhile (`--force` overrides)"
    news(repo, t, "limit", line)
    return True


def limit_over(t):
    """The agent the task waits on is working again: forget its limit, and let
    `start` through. True if it was limited. limit.json stays, as a limit that has
    reset: other tasks may still wait to be continued (`cmd_limit` removes it)."""
    if not is_limited(t):
        return False
    t["limit_seen"] = t.pop("limit")["text"]
    lim = read_json(LIMIT_FILE)
    if lim:
        write_json(LIMIT_FILE, {**lim, "resets": now()})
    return True
