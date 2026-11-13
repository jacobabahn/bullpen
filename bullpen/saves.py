"""What Python's saves set off that has moved to Rust: the run queue. When a task frees a
slot or closes, `bullpen _queue` (Rust's: cli/src/start/queue.rs) starts the next queued
tasks, once the command's own work is done. It reads the tasks fresh from disk."""

import subprocess
import sys

from .core import CLOSED, SELF, later, on_save


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
