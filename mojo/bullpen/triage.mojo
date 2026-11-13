"""The coordinator's `ok` and `note` (bullpen/triage.py). `ask` stays with Python: it
notifies the person and types news to the coordinators, which the port doesn't do."""

from bullpen.args import Args
from bullpen.core import Repo, repo_root, is_status, ok, write_json
from bullpen.agents import waits_on, running


def cmd_ok(args: Args) raises:
    """A task's stop is seen to: back to what it was, typing nothing."""
    var repo = Repo(repo_root())
    var t = repo.task(args.pos[0])
    if t.status() != "triage":
        raise Error(is_status(t) + ", not waiting on its coordinator")
    if waits_on(t) == "reviewer":
        if not running(t):
            raise Error(t.id() + "'s reviewer isn't running: `bullpen review " + t.id() + "` starts a new one, or `bullpen ask` the person")
    elif not running(t):
        raise Error(t.id() + "'s worker isn't running: `bullpen resume " + t.id() + "` restarts it")
    ok(t)  # `check` leaves its agent idle; its next turn clears that
    repo.save_task(t)
    var status = t.status()
    print(t.id() + " is " + ("in review" if status == "review" else status) + " again (nothing typed to its agent)")


def cmd_note(args: Args) raises:
    """An informational line on the board about a task (cleared when its status changes)."""
    var repo = Repo(repo_root())
    var t = repo.task(args.pos[0])
    var words = String(" ").join(args.pos[1:]).split()
    _ = t.doc.put_str(0, "note", String(" ").join(words))
    write_json(repo.task_path(t.id()), t.doc)  # not save_task: the status didn't change
    print("noted on " + t.id())
