"""Adding and starting tasks: what a task waits on, the run queue, an agent's pane, and
handing a worker a message.
"""

import re
from pathlib import Path

from .core import (
    CLOSED, Fail, PLACES, PROMPTS, REVIEW_MODES, Repo, SELF, WAITING,
    back_to_work, clip, command, git, home, index_block, is_claude, is_status, later, model_in, now,
    on_save, pr_short, repo_root, resolve, set_auto_accept, set_status, slug, status_words,
    unindex_block, with_model, write_json,
)
from .modes import task_base_ref
from .rex import rex_alive, rex_type, screen_end, type_to_worker
from .coordinators import calling_coordinator, primary, session_for
from .panes import place
from .news import news
from .limit import limit_active, limit_phrase
from .hooks import hook_log, set_up
from .rules import committed_rules_on, rule_sources, rules_text


def cmd_add(args):
    repo = Repo(repo_root())
    brief = Path(args.brief_file).read_text() if args.brief_file else args.brief
    if not brief:
        raise Fail("give the task's brief with --brief or --brief-file")
    tid = f"{repo.cfg['prefix']}-{repo.cfg['next']}"
    after = check_after(repo, tid, dep_ids(repo, args.after))
    repo.cfg["next"] += 1
    repo.save()
    t = {"id": tid, "title": args.title, "status": "todo", "created": now(),
         "base": args.base or repo.base(), "branch": None, "worktree": None,
         "block": None, "session": None, "pr": None, "why": None, "last": None}
    c = calling_coordinator(repo) or primary(repo)  # who hears its news
    if c:
        t["coordinator"] = c["id"]
    if after:
        t["after"] = after
    if args.model:
        t["worker_model"] = args.model  # its own, over the config's, from its first start
    if args.review_mode and args.review_mode != "auto":
        t["review_mode"] = args.review_mode
    set_auto_accept(t, args.auto_accept)
    set_placement(t, args.pane)
    (repo.dir / "briefs").mkdir(parents=True, exist_ok=True)
    (repo.dir / "briefs" / f"{tid}.md").write_text(brief.rstrip() + "\n")
    repo.save_task(t)
    print(tid)


# ---------- dependencies and the run queue ----------
#
# A task can wait on others (`add --after`, `after`): its "after" is a list of IDs.
# `start` refuses it until they're all done. When a task is accepted or dropped, or its
# worker hands it off (to review, ready or finished), the queue starts each todo task
# that has dependencies, all done, within max_running. Only working tasks take a slot:
# a reviewer is short and cheap. A task sent back from review works, and counts, again,
# even if that puts the count over max_running: it's finishing work already started,
# and the queue just starts nothing more until the count is under the limit.


def dep_ids(repo, values):
    """IDs from --after values, each one or several: "BP-8", "8,9", "BP-8 BP-9"."""
    out = []
    for v in values or []:
        for part in re.split(r"[,\s]+", v.strip()):
            if part and repo.norm(part) not in out:
                out.append(repo.norm(part))
    return out


def check_after(repo, tid, after):
    """after, if the task tid can wait on those tasks: each exists, isn't tid itself
    or dropped, and none of them waits on tid already (a cycle)."""
    tasks = {t["id"]: t for t in repo.tasks()}
    for d in after:
        if d == tid:
            raise Fail(f"{tid} can't wait on itself")
        if d not in tasks:
            raise Fail(f"no task {d} in {home(repo.root)}: `bullpen ls --all` lists them")
        if tasks[d]["status"] == "dropped":
            raise Fail(f"{d} was dropped: {tid} would wait on it forever")
        path = dep_path(tasks, d, tid)
        if path:
            raise Fail(f"{d} already waits on {tid} ({' → '.join(path)}): that would be a cycle")
    return after


def dep_path(tasks, start, goal, seen=None):
    """[start, ..., goal] if start waits on goal, directly or through others, else None."""
    seen = seen if seen is not None else set()
    if start == goal:
        return [goal]
    if start in seen:
        return None
    seen.add(start)
    for d in (tasks.get(start) or {}).get("after") or []:
        rest = dep_path(tasks, d, goal, seen)
        if rest:
            return [start, *rest]
    return None


def open_deps(repo, t, tasks=None):
    """The tasks t waits on that aren't done, as (id, status); status None: no such task."""
    tasks = tasks if tasks is not None else {o["id"]: o for o in repo.tasks()}
    return [(d, (tasks.get(d) or {}).get("status")) for d in t.get("after") or []
            if (tasks.get(d) or {}).get("status") != "done"]


def waits_phrase(repo, t, tasks=None):
    """ "Waits on BP-8, BP-9 (dropped)" for a todo task with open dependencies, else "". """
    if t["status"] != "todo":
        return ""
    deps = open_deps(repo, t, tasks)
    if not deps:
        return ""
    return "Waits on " + ", ".join(d + {"dropped": " (dropped)", None: " (missing)"}.get(st, "") for d, st in deps)


def cmd_after(args):
    """Set what a task waits on, or clear it with --none."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if args.none == bool(args.deps):
        raise Fail("give the tasks it waits on, or --none")
    if t["status"] in CLOSED:
        raise Fail(is_status(t))
    after = [] if args.none else check_after(repo, t["id"], dep_ids(repo, args.deps))
    if after:
        t["after"] = after
    else:
        t.pop("after", None)
    write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
    print(f"{t['id']} waits on {', '.join(after)}" if after else f"{t['id']} waits on nothing")


def cmd_model(args):
    """Set, change or clear the worker model a task starts and resumes with."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if args.clear == bool(args.model):
        raise Fail("give the model (an alias like opus, or a model ID), or --clear")
    if t["status"] in CLOSED:
        raise Fail(is_status(t))
    if args.clear:
        t.pop("worker_model", None)
    else:
        t["worker_model"] = args.model
    write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
    cfg = repo.cfg.get("worker_model")
    said = (f"{t['id']} uses the config's worker model ({cfg})" if args.clear and cfg else
            f"{t['id']} uses the worker command's own model" if args.clear else
            f"{t['id']} uses {args.model}")
    if rex_alive(t.get("session"), t.get("block")):
        said += f"; its worker keeps running on {t.get('model') or 'its model'}: this takes effect at its next start or resume"
    print(said)


def cmd_review_mode(args):
    """Set how a task's first review goes: full, light, or auto (by the size of the change)."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] in CLOSED:
        raise Fail(is_status(t))
    if args.mode == "auto":
        t.pop("review_mode", None)
    else:
        t["review_mode"] = args.mode
    write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
    r = t.get("review") or {}
    later = " (it has had a review already: later rounds follow up on it, whatever the mode)" if r.get("round") else ""
    print(f"{t['id']}'s first review: {args.mode}{later}")


def running_count(repo):
    """The tasks that take a max_running slot: those working (not in review)."""
    return sum(1 for t in repo.tasks() if t["status"] == "working")


FREES = ("review", "ready", "finished")  # leaving working for these frees a slot


def slot_freed(old, t):
    """How a save moves the run queue: "review", "ready" or "finished" for a task that
    left working for one of them, "accepted", "closed" or "dropped" for one that closed
    (its dependents may start), else None. A stop (needs, triage, stopped) frees
    nothing: its agent comes back to the task."""
    was, now_ = old.get("status"), t["status"]
    if now_ in CLOSED and was not in CLOSED:
        return "dropped" if now_ == "dropped" else "closed" if t.get("closed") else "accepted"
    if was == "working" and now_ in FREES:
        return now_
    return None


@on_save
def move_queue(repo, t, old):
    """The run queue hears of every status change: when a task frees a slot or closes,
    it starts the next queued tasks once the command's own work is done."""
    how = slot_freed(old, t)
    if how:
        by = dict(t)
        later(("queue", str(repo.root)), lambda: start_queued(repo, by, how))


def start_queued(repo, by, how):
    """After `by` was accepted, closed or dropped, or went to review, ready or finished
    (how): start each queued task, one with dependencies all done, in ID order, while
    fewer than max_running work. One news line per start. Plain todo tasks wait for the
    coordinator. Only move_queue calls it, from the status change."""
    tasks = {t["id"]: t for t in repo.tasks()}
    queued = [t for t in tasks.values() if t["status"] == "todo" and t.get("after")
              and not open_deps(repo, t, tasks)]
    if not queued:
        return
    why = {"accepted": f"{by['id']} was accepted", "closed": f"{by['id']} was closed",
           "review": f"{by['id']} went to review, freeing a slot",
           "ready": f"{by['id']} is ready, freeing a slot",
           "finished": f"{by['id']} finished, freeing a slot"}.get(
        how, f"{by['id']} was dropped, freeing a slot")
    lim = limit_active()
    if lim:
        ids = ", ".join(t["id"] for t in queued)
        news(repo, by, "queue", f"{ids} can start ({why}), but the usage limit is reached "
             f"({limit_phrase(lim.get('resets'))}): `bullpen start` them after the reset", everyone=True)
        return
    room = repo.cfg["max_running"] - running_count(repo)
    session = session_for(repo)
    for t in sorted(queued, key=lambda t: int(t["id"].rsplit("-", 1)[1]))[:max(room, 0)]:
        try:
            if not session:
                raise Fail("no Rex session: run `bullpen up`")
            if not start_task(repo, t, session):
                continue  # its setup failed: that has its own news
        except Fail as e:
            news(repo, t, "queue", f"{t['id']} couldn't start ({why}): {clip(str(e), 200)}")
            continue
        news(repo, t, "started", f"{t['id']} started ({why})")


def dropped_dep(repo, t):
    """t was dropped: the tasks waiting on it stay todo, with one news line."""
    waiting = [o["id"] for o in repo.tasks() if o["status"] == "todo" and t["id"] in (o.get("after") or [])]
    if not waiting:
        return
    names = ", ".join(waiting)
    one = waiting[0]
    verb = "waits" if len(waiting) == 1 else "wait"
    stays = "it stays" if len(waiting) == 1 else "they stay"
    news(repo, t, "dropped-dependency",
         f"{names} {verb} on {t['id']}, which was dropped, so {stays} queued. Ask the person what to do: "
         f"`bullpen after {one} --none` (or other tasks) to go ahead without it, then "
         f"`bullpen start {one}`; or drop {'it' if len(waiting) == 1 else 'them'}", everyone=True)


def deps_section(repo, t):
    """For the brief of a task with dependencies: what each one landed."""
    if not t.get("after"):
        return ""
    tasks = {o["id"]: o for o in repo.tasks()}
    parts = []
    for d in t["after"]:
        o = tasks.get(d)
        if not o:
            continue
        state = f"{o['status']}, not accepted yet"
        if o["status"] == "done":
            state = "finished with nothing to merge, closed" if o.get("closed") else "accepted"
        where = pr_short(o.get("pr")) if o.get("pr") else (f"merged into {o['base']}" if o.get("merged") else "")
        lines = [f"### {d}: {o['title']} ({state}{', ' + where if where else ''})", ""]
        lines.append((o.get("summary") or "No summary.").strip())
        findings = sorted((repo.dir / "briefs").glob(f"{d}.review-*.findings.md"))
        if findings:
            lines += ["", "Review findings: " + ", ".join(str(f) for f in findings)]
        parts.append("\n".join(lines))
    if not parts:
        return ""
    return ("## What this builds on\n\nThis task waited on these tasks. Their work is on your base "
            "branch now (unless marked otherwise): read it before you start.\n\n"
            + "\n\n".join(parts) + "\n\n")


def footer(repo, t):
    return (PROMPTS / "worker-footer.md").read_text().format(
        id=t["id"], title=t["title"], worktree=t["worktree"], branch=t["branch"],
        base=t["base"], bin=SELF)


def cmd_start(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] not in ("todo", "stopped") and not (t.get("needs_setup") and t["status"] in WAITING):
        raise Fail(f"{is_status(t)}; use `resume` or `tell`")
    deps = [] if args.force or t["status"] != "todo" else open_deps(repo, t)
    if deps:
        raise Fail(f"{t['id']} waits on " + ", ".join(f"{d} ({status_words(st) if st else 'no such task'})" for d, st in deps)
                   + f": it starts by itself once they're accepted; `start {t['id']} --force` starts it now")
    lim = None if args.force else limit_active()
    if lim:
        raise Fail(f"the usage limit is reached ({limit_phrase(lim.get('resets'))}): a worker started "
                   f"now would stop at once. `start` works again after the reset, or once a "
                   f"limited worker is working again; `start {t['id']} --force` starts it anyway")
    session = session_for(repo, args)
    if not session:
        raise Fail("no Rex session: run `bullpen up` first or pass --session")
    set_auto_accept(t, args.auto_accept)
    set_placement(t, args.pane)
    if not start_task(repo, t, session, args.model):
        raise Fail(f"{t['id']}'s setup failed, so its worker didn't start: see {hook_log(repo, t, 'setup')}")
    print(f"{t['id']} started in {t['worktree']} (branch {t['branch']}, pane {t['block']})")


def set_placement(t, pane):
    """--pane: where the task's worker pane goes from its next start on, over the
    `workers` setting (panes.for_new reads it). Its reviewer's too, with `--reviewers same`."""
    if pane:
        t["placement"] = pane


def start_task(repo, t, session, model=None):
    """Make the task's worktree, set it up, write its brief and start its worker.
    Returns False, with the task stopped (saved, news sent), if its setup failed."""
    if not t["worktree"]:
        t["needs_setup"] = True  # until its setup succeeds: a retry runs it again
        base = t["base"]
        start_point = base
        if git(repo.root, "remote", "get-url", "origin", check=False).returncode == 0:
            git(repo.root, "fetch", "-q", "origin", base, check=False)
            if git(repo.root, "rev-parse", "-q", "--verify", f"origin/{base}", check=False).returncode == 0:
                start_point = f"origin/{base}"
        t["branch"] = f"bullpen/{t['id'].lower()}-{slug(t['title'])}"
        wt = repo.worktree_dir(t)
        wt.parent.mkdir(parents=True, exist_ok=True)
        git(repo.root, "worktree", "add", "-q", "--no-track", "-b", t["branch"], str(wt), start_point)
        t["worktree"] = str(wt)
        repo.save_task(t)  # setup can take minutes: if bullpen dies in it, a retry knows the worktree
    if t.get("needs_setup") and not set_up(repo, t):
        return False
    brief = (repo.dir / "briefs" / f"{t['id']}.md").read_text()
    full = repo.dir / "briefs" / f"{t['id']}.full.md"
    # committed on its base, not the worktree's copy: a restarted task may have edited it
    rules = rules_text(rule_sources(repo, committed_rules_on(repo, task_base_ref(repo, t)), ""))
    saved = repo.dir / "briefs" / f"{t['id']}.rules.md"
    saved.write_text(rules + "\n" if rules else "")
    section = f"## Rules for this repository\n\n{rules}\n\n" if rules else ""
    full.write_text(f"# {t['id']}: {t['title']}\n\n{brief}\n{deps_section(repo, t)}{section}{footer(repo, t)}")
    first = f"You are the worker for bullpen task {t['id']}. Read your brief at {full} and follow it."
    if model:
        t["worker_model"] = model  # this task's own, over the config's
    launch(repo, t, session, first, resume=False)
    return True


def launch(repo, t, session, first, resume):
    cmd = with_model(repo.cfg["worker"], t.get("worker_model") or repo.cfg.get("worker_model"))
    t["model"] = model_in(cmd) if is_claude(cmd) else None
    argv = resolve(cmd) + (["--continue"] if resume else []) + \
        ["-n", f"{t['id']}: {t['title']}", first]
    unindex_block(t.get("block"))
    label = f"bullpen: {t['id']}"
    block, where = place(repo, t, session, label, argv)
    index_block(block, repo, t["id"])
    set_status(t, "working")
    t.update(block=block, session=session, last=None, pane=where)
    repo.save_task(t)


def hand_back(repo, t, msg):
    """Give the task's worker msg to act on: typed into its pane, or, with its pane gone,
    resumed in a new one with msg. Once the worker has it the task is working (saved),
    and its next event is never taken for a repeat. Returns None, or why the message
    didn't reach the worker: the task is left as it was, unsaved. Raises Fail when a
    new pane can't be started."""
    if not rex_alive(t.get("session"), t.get("block")):
        launch(repo, t, session_for(repo) or t.get("session"), msg, resume=True)
        return None
    failed = type_to_worker(t, msg)
    if failed:
        return failed
    if t["status"] != "working":
        back_to_work(t)
    t["last"] = None
    repo.save_task(t)
    return None


def cmd_resume(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if rex_alive(t["session"], t["block"]):
        raise Fail(f"{t['id']}'s agent is still running; use `tell`")
    if not t["worktree"]:
        raise Fail(f"{t['id']} never started; use `start`")
    set_placement(t, args.pane)
    if t.get("needs_setup"):  # its worker never ran: set it up and start it afresh
        session = session_for(repo, args) or t["session"]
        if not start_task(repo, t, session):
            raise Fail(f"{t['id']}'s setup failed again: see {hook_log(repo, t, 'setup')}")
        print(f"{t['id']} set up and started in pane {t['block']}")
        return
    msg = args.message or "You were stopped before finishing. Check where you were and carry on with your brief."
    launch(repo, t, session_for(repo, args) or t["session"], msg, resume=True)
    print(f"{t['id']} resumed in pane {t['block']}")


def cmd_tell(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if not rex_alive(t["session"], t["block"]):
        raise Fail(f"{t['id']}'s agent isn't running; use `resume {t['id']} \"...\"`")
    rex_type(t["session"], t["block"], " ".join(args.message))
    if t["status"] in (*WAITING, "ready", "finished"):
        back_to_work(t)
        t["last"] = None
        repo.save_task(t)
    print(f"sent to {t['id']}")


def cmd_peek(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if not rex_alive(t["session"], t["block"]):
        raise Fail(f"{t['id']}'s agent isn't running")
    print("\n".join(screen_end(t["session"], t["block"], args.lines, args.vt)))


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "add", cmd_add, "Coordinator", help="add a task; prints its ID")
    p.add_argument("title", help="a short name for the task")
    p.add_argument("--brief", help="what the worker should do, as text")
    p.add_argument("--brief-file", metavar="FILE", help="the same, read from a file (over --brief)")
    p.add_argument("--base", help="branch to start from and open the PR against")
    p.add_argument("--after", action="append", metavar="ID",
                   help="a task this one waits on (repeat, or comma-separated): it starts once they're accepted")
    p.add_argument("--model", help="claude's --model for this task's worker, over the config's, "
                   "whenever it starts (by itself too) or resumes")
    p.add_argument("--review", choices=REVIEW_MODES, dest="review_mode",
                   help="its first review: full, light, or auto (light for a small or docs-only change; the default)")
    p.add_argument("--auto-accept", dest="auto_accept", action="store_true", default=None,
                   help="accept it by itself once review approves it, whatever config --auto-accept says")
    p.add_argument("--no-auto-accept", dest="auto_accept", action="store_false",
                   help="keep it for you to accept even with config --auto-accept on")
    p.add_argument("--pane", choices=PLACES,
                   help="where its worker's pane goes, over config --workers: float, tab, tabs or background")

    p = command(sub, "after", cmd_after, "Coordinator",
                help="set the tasks a task waits on (--none: nothing)")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("deps", nargs="*", metavar="ID", help="the tasks it waits on: it starts once they're accepted")
    p.add_argument("--none", action="store_true", help="it waits on nothing")

    p = command(sub, "model", cmd_model, "Coordinator",
                help="set the worker model a task starts and resumes with (--clear: the config's)")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("model", nargs="?", help="an alias like opus, or a model ID; default: let Claude Code pick")
    p.add_argument("--clear", action="store_true", help="use the config's worker model again")

    p = command(sub, "review-mode", cmd_review_mode, "Coordinator",
                help="set how a task's first review goes: full, light or auto")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("mode", choices=REVIEW_MODES, help="light: a quick look; auto: light for a small or docs-only change")

    p = command(sub, "start", cmd_start, "Coordinator", help="make a task's worktree and start its worker")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--model", help="claude's --model for this task's worker, over the config's")
    p.add_argument("--session", help="the Rex session for its pane (default: the coordinator's)")
    p.add_argument("--force", action="store_true",
                   help="start even while the usage limit is reached or what it waits on isn't done")
    p.add_argument("--auto-accept", dest="auto_accept", action="store_true", default=None,
                   help="accept it by itself once review approves it, whatever config --auto-accept says")
    p.add_argument("--no-auto-accept", dest="auto_accept", action="store_false",
                   help="keep it for you to accept even with config --auto-accept on")
    p.add_argument("--pane", choices=PLACES,
                   help="where its worker's pane goes, over config --workers: float, tab, tabs or background")

    p = command(sub, "resume", cmd_resume, "Coordinator",
                help="restart a stopped worker, continuing its conversation")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("message", nargs="?", help="what to type to it first (default: carry on with your brief)")
    p.add_argument("--session", help="the Rex session for its pane (default: the coordinator's)")
    p.add_argument("--pane", choices=PLACES,
                   help="where its worker's pane goes, over config --workers: float, tab, tabs or background")

    p = command(sub, "tell", cmd_tell, "Coordinator", help="type a message to a task's worker")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("message", nargs="+", help="the words to type")

    p = command(sub, "peek", cmd_peek, "Coordinator", help="print the end of a worker's screen")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--lines", type=int, default=40, help="how many lines (default 40)")
    p.add_argument("--vt", action="store_true", help="keep the screen's colours, as terminal escape codes")
