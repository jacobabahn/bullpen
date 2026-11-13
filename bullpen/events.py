"""What the watcher (lua/watch.lua) reports: an agent's events, a pane that closed, an
agent that shows no progress, and the usage limit's reset.
"""

import collections
import datetime as dt
import re
from pathlib import Path

from .core import (
    CLOSED, Fail, Repo, STATE, WAITING, back_to_work, clip, command, git, load_config,
    lookup_block, mark_compacting, now, read_json, seconds_since, set_status,
    unindex_block,
)
from .modes import task_base_ref
from .rex import rex_alive, rex_type, screen
from .agents import is_limited, on_reviewer, pane, whose_turn
from .coordinators import coordinator_at, coordinator_gone
from .news import news, ready_news
from .limit import (
    LIMIT_CONTINUE, LIMIT_FILE, LIMIT_GRACE, LIMIT_STATES, limit_active, limit_hit,
    limit_over, limited,
)
from .stops import stop
from .start import hand_back
from .review import approval_kept, keep_approval, start_review


# What the watcher saw a pane's agent do: its state (working, idle, done, blocked or
# error), the kind of prompt it's blocked on and Rex's message. `check --fix` also
# gives shell_min, how long the agent's own background shell has run.
Event = collections.namedtuple("Event", "block state kind msg shell_min", defaults=(None, None, None))


def cmd_event(args):
    event(Event(args.block, args.state, args.kind, args.msg))


def event(ev):
    """A pane's agent changed state (lua/watch.lua, or `check --fix` for one it missed)."""
    repo, role = lookup_block(ev.block)
    if not repo or role in ("coordinator", "board"):
        return
    if role.endswith(" review"):
        return review_event(repo, repo.task(role.split()[0]), ev)
    t = repo.task(role)
    if t["block"] != ev.block or t["status"] in CLOSED:
        return
    state, msg = ev.state, (ev.msg or "").strip()
    key = f"{state}|{msg}"
    woke = unstick(t, "worker")
    if state in LIMIT_STATES and (limited(t, "worker") or limit_hit(repo, t, "worker", ev)):
        return
    if state == "working" and t["status"] != "working" and compacting(t.get("session"), t["block"], msg):
        # Claude Code compacting the conversation by itself: not a new turn
        if not t.get("compacting"):
            t["compacting"] = now()
            mark_compacting(ev.block, True)
        repo.save_task(t)
        return
    if t.pop("compacting", None):
        mark_compacting(ev.block, False)
        if state in ("done", "idle") and t["status"] != "working":
            # the compaction is over: its end is no end of a turn
            t["last"] = key
            repo.save_task(t)
            return
    if state == "working":
        if t["status"] in (*WAITING, "stopped") or limit_over(t):
            set_status(t, "working")
        elif t["status"] == "ready" and not (t.get("last") or "").startswith("working|"):
            back_to_work(t)  # it had ended its turn: this is a new one
            approved = (t.get("review") or {}).get("verdict") == "approved"
            if approved:
                t["from_ready"] = True  # an end of turn that changed no code makes it ready again
            news(repo, t, "working", f"{t['id']} is working again (was ready): it must run `ready` again" +
                 ("; its approval holds if the code doesn't change" if approved else ""))
        elif t["status"] == "finished" and not (t.get("last") or "").startswith("working|"):
            back_to_work(t)
            news(repo, t, "working", f"{t['id']} is working again (was finished): it must run `finish` "
                 f"(or `ready`) again")
        t.pop("ok", None)
        t["last"] = key
        repo.save_task(t)
        return
    if key == t.get("last"):
        if woke:
            repo.save_task(t)
        return
    t["last"] = key
    if state == "blocked":
        kind, msg = ev.kind or "input", prompt_msg(ev.kind or "input", msg)
        stop(repo, t, "worker", "prompt", f"Needs you ({kind})" + (f": {msg}" if msg else ""),
             prompt_news(t["id"], kind, msg), "needs-you")
    elif state == "error":
        stop(repo, t, "worker", "failed", "Failed" + (f": {msg}" if msg else ""),
             f"{t['id']}'s turn failed" + (f": {msg}" if msg else "") +
             f": read it with `bullpen peek {t['id']}`", "failed")
    elif state in ("done", "idle"):
        shell_min = ev.shell_min  # from `check`: its shell has run this long
        if t["status"] != "working" or (shell_min is None and shell_running(screen(t.get("session"), t["block"]))):
            # a background shell of its own still runs: it takes another turn when that ends
            repo.save_task(t)
            return
        if ready_again(repo, t):
            return
        if shell_min is None:
            line = f"{t['id']} ended its turn without `ready`: read it with `bullpen peek {t['id']}`"
        else:
            line = (f"{t['id']} ended its turn without `ready` {shell_min} min ago, and a background shell "
                    f"of its own still runs (a server, a watch?): read it with `bullpen peek {t['id']}`")
        stop(repo, t, "worker", "ended", "Ended its turn without saying ready", line, "quiet")
    else:
        repo.save_task(t)


def prompt_msg(kind, msg):
    """What Rex said of a prompt, unless it only names its kind ("permission prompt")."""
    msg = (msg or "").strip()
    return "" if msg.lower() in (kind.lower(), f"{kind.lower()} prompt") else msg


def prompt_news(who, kind, msg):
    """The news that a task's agent is at a prompt, which needs the person: "BP-3 is at
    a permission prompt: Allow rm?", "BP-3's reviewer is at a prompt (question): ..."."""
    at = "a permission prompt" if kind == "permission" else f"a prompt ({kind})"
    return f"{who} is at {at}" + (f": {msg}" if msg else "")


def ready_again(repo, t):
    """A ready task's worker took a turn and ended it without ready. If the code is as it
    was approved (committed, the same patch), it is ready again with its approval. True
    if so."""
    if not t.pop("from_ready", None) or not worktree_clean(t):
        return False
    ref = task_base_ref(repo, t)
    kept = approval_kept(t, ref)
    if not kept:
        return False
    keep_approval(t, ref)
    set_status(t, "ready")
    repo.save_task(t)
    ready_news(repo, t, [f"its worker's turn left the code as approved: {kept}"],
               f". `bullpen peek {t['id']}` if it said something", again=True)
    return True


def worktree_clean(t):
    wt = t.get("worktree")
    return bool(wt) and Path(wt).is_dir() and not git(wt, "status", "--porcelain", check=False).stdout.strip()


def review_event(repo, t, ev):
    """A reviewer's status: its coordinator hears if it stops, the person if it's at a prompt."""
    r = t.get("review") or {}
    if r.get("block") != ev.block or t["status"] not in ("review", *WAITING):
        return
    state, msg = ev.state, (ev.msg or "").strip()
    stuck = on_reviewer(t)
    woke = unstick(t, "reviewer")
    if state in LIMIT_STATES and (limited(t, "reviewer") or limit_hit(repo, t, "reviewer", ev)):
        return
    if state == "working" and stuck and compacting(*pane(t, "reviewer"), msg):
        # compacting by itself: it stays where its coordinator or the person has it
        if not r.get("compacting"):
            r["compacting"] = now()
            mark_compacting(ev.block, True)
        repo.save_task(t)
        return
    if r.pop("compacting", None):
        # the compaction is over. Its end changes nothing here: a reviewer stopped in
        # triage or needs stays there, and one in review (`ok`) ended a real turn.
        mark_compacting(ev.block, False)
        woke = True  # save the task
    if state == "working":
        back, was_ok = limit_over(t), t.pop("ok", None)
        if stuck or back:
            set_status(t, "review")
        if stuck or woke or back or was_ok:
            repo.save_task(t)
    elif state == "blocked":
        kind, msg = ev.kind or "input", prompt_msg(ev.kind or "input", msg)
        stop(repo, t, "reviewer", "prompt", f"Reviewer needs you ({kind})" + (f": {msg}" if msg else ""),
             prompt_news(f"{t['id']}'s reviewer", kind, msg), "needs-you")
    elif state in ("done", "idle", "error") and t["status"] == "review":
        if (state != "error" and ev.shell_min is None
                and shell_running(screen(*pane(t, "reviewer")))):
            # a background shell of its own still runs (the tests?): it takes another turn
            if woke:
                repo.save_task(t)
            return
        shell_min = ev.shell_min
        late = (f" {shell_min} min ago, and a background shell of its own still runs" if shell_min is not None else "")
        stop(repo, t, "reviewer", "ended", "Reviewer stopped without a verdict",
             f"{t['id']}'s reviewer stopped without a verdict{late}: `bullpen go {t['id']} --review` "
             f"shows it, `bullpen review {t['id']}` starts a new one", "quiet")
    elif woke:
        repo.save_task(t)


# ---------- agents that show no progress (called by lua/watch.lua) ----------


def cmd_stuck(args):
    """(watcher) a pane has said it's working, and nothing else, for this many minutes.
    News once per quiet spell; the task carries on as it is."""
    repo, role = lookup_block(args.block)
    if not repo or role in ("coordinator", "board") or not repo.cfg.get("stuck_after"):
        return
    who = "reviewer" if role.endswith(" review") else "worker"
    t = repo.task(role.split()[0])
    if pane(t, who)[1] != args.block or t.get("stuck") or t["status"] != ("review" if who == "reviewer" else "working"):
        return
    since = dt.datetime.now().astimezone() - dt.timedelta(minutes=args.minutes)
    t["stuck"] = {"who": who, "since": since.isoformat(timespec="seconds")}
    repo.save_task(t)
    if who == "reviewer":
        news(repo, t, "stuck", f"{t['id']}'s reviewer has shown no progress for {args.minutes} min: "
             f"`bullpen go {t['id']} --review` shows it")
    else:
        news(repo, t, "stuck", f"{t['id']} has shown no progress for {args.minutes} min: `bullpen peek {t['id']}`")


def unstick(t, who):
    """The agent gave a sign of life: forget that it was quiet. True if it was."""
    if (t.get("stuck") or {}).get("who") != who:
        return False
    del t["stuck"]
    return True


# ---------- a pane that closed (called by lua/watch.lua) ----------


def cmd_gone(args):
    gone(args.block)


def gone(block):
    """A pane bullpen knows closed (lua/watch.lua, or `check --fix` for one it missed)."""
    repo, role = lookup_block(block)
    if not repo:
        return
    unindex_block(block)
    if role == "coordinator":
        c = coordinator_at(repo, block, boards=False)
        if c:
            coordinator_gone(repo, c)
        return
    if role == "board":
        return
    if role.endswith(" review"):
        t = repo.task(role.split()[0])
        r = t.get("review") or {}
        if r.get("block") == block:
            r["block"] = None
            if limited(t, "reviewer"):
                pass  # started again when the limit resets
            elif whose_turn(t) == "reviewer" and not r.get("verdict"):
                stop(repo, t, "reviewer", "exited", "Reviewer exited without a verdict",
                     f"{t['id']}'s reviewer exited without a verdict: `bullpen review {t['id']}` starts a new one")
            repo.save_task(t)
        return
    t = repo.task(role)
    if t["block"] != block:
        return
    t["block"] = None
    if limited(t, "worker"):
        repo.save_task(t)  # resumed when the limit resets
    elif t["status"] in ("working", *WAITING):
        stop(repo, t, "worker", "exited", "Agent exited before saying ready",
             f"{t['id']} stopped before saying ready: `bullpen resume {t['id']}` continues it")
    else:
        repo.save_task(t)


# ---------- what an agent's screen says ----------

SHELL_RE = re.compile(r"\b\d+ (?:background )?shells?\b|shells? still running", re.I)


def shell_running(text):
    """Does Claude Code's footer say a background shell of its own still runs?"""
    return any(SHELL_RE.search(l) for l in text.splitlines()[-6:])


COMPACT_RE = re.compile(r"^\W{0,2}\s*(?:Compacting conversation|Running Pre-?Compact hooks)")


def compacting(session, block, msg=""):
    """Is Claude Code compacting its conversation (auto-compaction)? Its status record
    says only that it's working, so this goes by its spinner line: "✽ Compacting
    conversation…" (or the PreCompact hooks it runs first)."""
    if re.match(r"(?:Compacting conversation|Running Pre-?Compact hooks)", msg or ""):
        return True
    lines = [l.strip() for l in screen(session, block).splitlines() if l.strip()][-12:]
    return any(COMPACT_RE.match(l) for l in lines)


# ---------- the usage limit's reset (called by lua/watch.lua) ----------


def cmd_limit(args):
    """(watcher) once the limit has reset, continue the agents that stopped at it."""
    if not LIMIT_FILE.exists():
        return
    waiting = False
    for cfg in (STATE / "repos").glob("*/config.json"):
        root = read_json(cfg, {}).get("root")
        if not root or not Path(root).is_dir():
            continue
        repo = Repo(root)
        repo.dir = cfg.parent
        repo.cfg = load_config(cfg.parent)
        done = []
        for t in repo.tasks():
            resets = (t.get("limit") or {}).get("resets")
            if not is_limited(t) or not resets:
                continue  # no known reset time: the person (or coordinator) continues it
            if seconds_since(resets) < LIMIT_GRACE:
                waiting = True
                continue
            done.append((t, continue_limited(repo, t)))
        if done:
            news(repo, done[0][0], "limit-reset",
                 "the usage limit has reset: continued " + ", ".join(said for _, said in done), everyone=True)
    if not waiting and not limit_active():
        LIMIT_FILE.unlink(missing_ok=True)


def continue_limited(repo, t):
    """Tell the task's limited agent to carry on, in a new pane if its old one is
    gone. Returns what to call it in the news."""
    lim = t.pop("limit")
    t.update(limit_seen=lim["text"], why=None)
    r = t.get("review") or {}
    try:
        if lim["who"] == "reviewer":
            if rex_alive(*pane(t, "reviewer")):
                rex_type(*pane(t, "reviewer"), LIMIT_CONTINUE)
                repo.save_task(t)
                return f"{t['id']}'s reviewer"
            r["round"] = max(0, r.get("round", 1) - 1)  # the round it didn't finish
            start_review(repo, t, task_base_ref(repo, t))
            return f"{t['id']}'s reviewer (its pane was gone: started again)"
        alive = rex_alive(t.get("session"), t.get("block"))
        failed = hand_back(repo, t, LIMIT_CONTINUE)
        if failed:
            raise Fail(failed)
        return t["id"] if alive else f"{t['id']} (its pane was gone: resumed in a new one)"
    except Fail as e:
        stop(repo, t, lim["who"], "limit", f"Couldn't be continued after the usage limit: {clip(str(e), 120)}")
        who = "its coordinator" if t["status"] == "triage" else "the person"
        return f"{t['id']} (not continued, for {who} to see to: {clip(str(e), 120)})"


def commands(sub):
    """This module's commands, for bullpen's parser."""
    # The internal commands below (the watcher's and bullpen's own) have no help and no
    p = command(sub, "_event", cmd_event)
    p.add_argument("block")
    p.add_argument("state")
    p.add_argument("kind", nargs="?")
    p.add_argument("msg", nargs="?")

    p = command(sub, "_stuck", cmd_stuck)
    p.add_argument("block")
    p.add_argument("minutes", type=int)

    command(sub, "_limit", cmd_limit)

    p = command(sub, "_gone", cmd_gone)
    p.add_argument("block")
