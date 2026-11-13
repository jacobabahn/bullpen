"""What Python's saves set off that has moved to Rust: the run queue and the PR to
follow. When a task frees a slot or closes, `bullpen _queue` (Rust's:
cli/src/start/queue.rs) starts the next queued tasks, once the command's own work is
done. It reads the tasks fresh from disk. A task whose PR the watcher looks at marks it
to follow (cli/src/saves.rs's twin)."""

import subprocess
import sys

from .core import CLOSED, SELF, STATE, WAITING, later, on_save


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
        root, by = str(repo.root), t["id"]
        later(("queue", root), lambda: run_queue(root, by, how))


def run_queue(root, by, how):
    """`bullpen _queue`: start what the queue can, now that task by freed a slot (how)."""
    sys.stdout.flush()  # its output comes after ours
    try:
        subprocess.run([str(SELF), "_queue", "--repo", root, "--by", by, "--how", how], cwd=root,
                       stdin=subprocess.DEVNULL)
    except OSError as e:
        print(f"bullpen: couldn't move the run queue: {e}", file=sys.stderr)


# ---------- the PR to follow (Rust's `_prs` follows it: cli/src/prs.rs) ----------


FOLLOWED = ("review", "ready", *WAITING)  # statuses whose PR the watcher looks at


def followed(t):
    """Does the watcher look at this task's PR? Only an open PR of a task that waits
    on review or the person: a working task is about to push anyway."""
    return bool(t.get("pr")) and not t.get("local") and t["status"] in FOLLOWED \
        and (t.get("gh") or {}).get("state", "open") == "open"


@on_save
def follow_saved(repo, t, old):
    """A saved task whose PR the watcher looks at marks it to follow: <state>/prs exists
    while some task's PR may need a look, and the watcher only calls `_prs` then."""
    if followed(t):
        STATE.mkdir(parents=True, exist_ok=True)
        (STATE / "prs").touch()
