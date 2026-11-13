"""What's left of starting tasks in Python, as twins of cli/src/start/ for the commands
Python still runs: what a task waits on (task_line), a dropped dependency (close), an
agent's pane and handing a worker a message (accept, verdict, events, prs, tours). The
commands, and the run queue, are Rust's; Python's saves move the queue through saves.py.
"""

from .core import back_to_work, index_block, is_claude, model_in, resolve, set_status, unindex_block, with_model
from .rex import rex_alive, type_to_worker
from .coordinators import session_for
from .panes import place
from .news import news


# ---------- dependencies ----------
#
# A task can wait on others (`add --after`, `after`): its "after" is a list of IDs.
# `start` refuses it until they're all done (the run queue: cli/src/start/queue.rs).


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
