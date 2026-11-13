"""`check`: each task's status against what its agent is doing now, and the events it missed."""

import re
from pathlib import Path

from .core import (
    CLOSED, DEFAULT_CONFIG, Repo, WAITING, at_prompt, clip, command, git, known_repos, now,
    repo_root, seconds_since, task_base_ref, write_json,
)
from .rex import program_status, rex_alive, screen
from .agents import waited_on
from .stops import escalate, overdue
from .events import Event, compacting, event, gone, shell_running


# A task's status follows its agent's events; a missed one (a watcher or Rex restart,
# a bug) leaves it out of step. `check` compares each task that has an agent in a
# pane with what Rex says that agent is doing right now, and `--fix` applies the
# event that was missed, through the same paths (and news) as the watcher.

SETTLE = 60  # seconds a new idle, blocked or error state is left alone: its event may still be on the way


def screen_state(text):
    """What Claude Code's screen shows: working (a spinner), blocked (a question),
    idle (its prompt), or None if it's none of those."""
    lines = [l.strip() for l in text.splitlines() if l.strip()][-12:]
    if any("esc to interrupt" in l for l in lines):
        return "working"
    if any(l.startswith("Do you want to") for l in lines):
        return "blocked"
    if any(l.startswith("❯") for l in lines):
        return "idle"
    return None


def agent_state(session, block, at=None):
    """What the agent in a pane is doing now: {"state", "kind", "msg", "age"}, with state
    one of gone, working, blocked, error, idle or shell (its turn ended, but a background
    shell of its own runs, so it takes another). age: seconds in that state, None if
    unknown (the screen doesn't say). None: unsure, or it just changed."""
    if not rex_alive(session, block):
        return {"state": "gone", "kind": "", "msg": "", "age": None}
    r = program_status(session, block)
    if r:
        age = seconds_since(r["updated_at"], at) if r.get("updated_at") else None
        state = r.get("state") or "idle"
        state = state if state in ("working", "blocked", "error") else "idle"
        if state != "working" and age is not None and age < SETTLE:
            # its event may still be on the way (the watcher holds an end of turn back).
            # Not working: the watcher forwards that at once, and a busy agent's record
            # is new every few seconds, as its message changes.
            return None
        out = {"state": state, "kind": r.get("kind") or "", "msg": r.get("msg") or "", "age": age}
    else:  # no record to read: go by the screen
        state = screen_state(screen(session, block))
        if not state:
            return None
        out = {"state": state, "kind": "", "msg": "", "age": None}
    if out["state"] == "idle" and shell_running(screen(session, block)):
        out["state"] = "shell"
    return out


def needs_label(why):
    """ "permission" for "Needs you (permission): ...", else the why in a few words."""
    m = re.match(r"(?:Reviewer )?[Nn]eeds you \(([^)]*)\)", why or "")
    if m:
        return m.group(1)
    return clip((why or "waiting")[:1].lower() + (why or "waiting")[1:], 60)


def doing(a, who="agent"):
    """What the agent is doing, in words: "its agent is idle at its prompt"."""
    return {"idle": f"its {who} is idle at its prompt",
            "blocked": f"its {who} is waiting on you ({a['kind'] or 'input'})",
            "error": f"its {who}'s turn failed",
            "working": f"its {who} is working again"}[a["state"]]


def shell_minutes(a, limit):
    """Minutes an agent has been idle with a background shell running, once that's
    past limit (minutes): a server or a watch it left running, not a test run. Else None."""
    if a["state"] != "shell" or a["age"] is None or a["age"] < limit * 60:
        return None
    return int(a["age"] // 60)


def worker_drift(t, a, limit):
    """(line, fix) when the worker's task status disagrees with what it does, else None.
    fix is the event the watcher missed: "gone", "shell" (idle for long with a shell
    running), or the agent's state."""
    tid, st = t["id"], t["status"]
    if a["state"] == "gone":
        return f"{tid} is marked {st} but its pane is gone", "gone"
    if a["state"] == "shell":
        m = shell_minutes(a, limit)
        if st == "working" and m is not None:
            return (f"{tid} is marked working but its agent has been idle at its prompt for {m} min, "
                    f"with a background shell still running: `bullpen peek` it"), "shell"
        return None  # busy: it takes another turn when its shell ends
    if st == "working" and a["state"] != "working":
        if t.get("ok") and a["state"] != "blocked":
            return None  # its coordinator said it's fine (`bullpen ok`)
        tail = ": `bullpen peek` it" if a["state"] == "idle" else ""
        return f"{tid} is marked working but {doing(a)}{tail}", a["state"]
    if st not in WAITING or t.get("ask"):
        return None  # an ask waits on the person, whatever its agent does
    if a["state"] == "working":
        return f"{tid} is marked {st} ({needs_label(t.get('why'))}) but {doing(a)}", "working"
    if a["state"] == "blocked" and not at_prompt(t):
        return f"{tid} is marked {st} ({needs_label(t.get('why'))}) but {doing(a)}", "blocked"
    return None


def reviewer_drift(t, a, limit):
    """The same for a task waiting on its reviewer."""
    tid, st = t["id"], t["status"]
    if a["state"] == "shell":
        if st != "review" or shell_minutes(a, limit) is None or t.get("ok"):
            return None  # busy: it takes another turn when its shell ends
        # its shell has run too long to count as busy
        return f"{tid} is in review but its reviewer is idle without a verdict", "shell"
    if a["state"] == "gone":
        return f"{tid} is {'in review' if st == 'review' else 'marked ' + st} but its reviewer's pane is gone", "gone"
    if st == "review" and a["state"] in ("idle", "error") and not t.get("ok"):
        return f"{tid} is in review but its reviewer is idle without a verdict", a["state"]
    if st == "review" and a["state"] == "blocked":
        return f"{tid} is in review but {doing(a, 'reviewer')}", "blocked"
    if st in WAITING and a["state"] == "working":
        return f"{tid} is marked {st} ({needs_label(t.get('why'))}) but {doing(a, 'reviewer')}", "working"
    return None


def unready_hint(repo, t, a):
    """A line for an idle worker whose work looks finished but never ran ready, or None."""
    if a["state"] != "idle" or t["status"] not in ("working", *WAITING):
        return None
    wt = t.get("worktree")
    if not wt or not Path(wt).is_dir() or git(wt, "status", "--porcelain", check=False).stdout.strip():
        return None
    ahead = git(wt, "rev-list", "--count", f"{task_base_ref(repo, t)}..HEAD", check=False).stdout.strip()
    if not ahead.isdigit() or int(ahead) == 0:
        return None
    return (f"{t['id']} is idle with commits and a clean worktree but never ran ready: "
            f"`bullpen peek` it, then `bullpen tell` it to run ready if it's done")


def fix_drift(repo, t, who, block, fix, a):
    """Apply the event the watcher missed, through its own path (and news)."""
    if fix == "gone":
        return gone(block)
    if who == "worker":
        t = repo.task(t["id"])
        t["last"] = None  # the event did come: don't take it for a repeat
        write_json(repo.task_path(t["id"]), t)
    if fix == "shell":
        return event(Event(block, "idle", a["kind"], a["msg"], shell_minutes(a, 0)))
    event(Event(block, fix, a["kind"], a["msg"]))


def check_repo(repo, ids, fix, out):
    """Check the repo's tasks (those in ids, if given). Returns how many disagreed."""
    drift = 0
    for t in repo.tasks():
        if t["status"] in CLOSED or (ids and t["id"] not in ids):
            continue
        late = overdue(repo, t)
        if late:
            drift += 1
            if fix:
                escalate(repo, t, late)
                t = repo.task(t["id"])
            out(f"{t['id']} waits on its coordinator, but {late[1]}: it needs the person"
                + (" (fixed)" if fix else ""))
        if seconds_since(t.get("since") or t.get("updated") or now()) < SETTLE:
            continue  # bullpen just changed it (start, tell, ready): its agent has yet to say
        agent = waited_on(t)
        if not agent:
            continue
        who, session, block = agent
        a = agent_state(session, block)
        if not a:
            continue
        if a["state"] == "working" and compacting(session, block, a["msg"]):
            continue  # compacting by itself: no turn, so no drift
        # How long an idle agent's shell may run before it is drift. stuck_after 0 turns
        # off only the "no progress" news: this check still runs, on the default.
        limit = repo.cfg["stuck_after"] or DEFAULT_CONFIG["stuck_after"]
        found = (worker_drift if who == "worker" else reviewer_drift)(t, a, limit)
        if found:
            drift += 1
            line, what = found
            if fix:
                fix_drift(repo, t, who, block, what, a)
                line += " (fixed)"
            out(line)
        hint = unready_hint(repo, repo.task(t["id"]), a) if who == "worker" else None
        if hint:
            out(hint)
    return drift


def cmd_check(args):
    """Compare each task's status with what its agent is doing now; --fix catches up."""
    out = (lambda line: None) if args.quiet else print
    if args.all_repos:
        repos = known_repos()
    else:
        repos = [Repo(repo_root())]
    drift = 0
    for repo in repos:
        ids = {repo.norm(i) for i in args.ids}
        for i in ids:
            repo.task(i)  # an unknown ID is an error, not "all agree"
        drift += check_repo(repo, ids, args.fix, out)
    return 1 if drift and not args.fix else 0


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "check", cmd_check, "Coordinator",
                help="compare each task's status with what its agent is doing now")
    p.add_argument("ids", nargs="*", metavar="ID", help="only these tasks")
    p.add_argument("--fix", action="store_true",
                   help="apply the updates the missed events would have made (news included)")
    p.add_argument("--quiet", action="store_true", help="print nothing")
    p.add_argument("--all-repos", action="store_true", help="every repo bullpen knows (the watcher's)")
