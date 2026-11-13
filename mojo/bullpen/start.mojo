"""What a queued task waits on (bullpen/start.py's open_deps and waits_phrase).

Python passes `tasks` as {id: task}; these need only each task's status, so the port
passes {id: status} (`statuses(repo)`)."""

from bullpen.core import Repo, Task


def statuses(repo: Repo) raises -> Dict[String, String]:
    """Every task's status, by ID: what Python's {o["id"]: o for o in repo.tasks()} is used for."""
    var out = Dict[String, String]()
    for t in repo.tasks():
        out[t.id()] = t.status()
    return out^


def open_deps(t: Task, tasks: Dict[String, String]) -> List[String]:
    """The tasks t waits on that aren't done, as "id" then its status ("" for no such task)."""
    var out = List[String]()
    for d in t.doc.strs(0, "after"):
        var st = tasks.get(d).or_else("")
        if st != "done":
            out.append(d)
            out.append(st)
    return out^


def waits_phrase(t: Task, tasks: Dict[String, String]) -> String:
    """ "Waits on BP-8, BP-9 (dropped)" for a todo task with open dependencies, else "". """
    if t.status() != "todo":
        return ""
    var deps = open_deps(t, tasks)
    if len(deps) == 0:
        return ""
    var out = String("Waits on ")
    for i in range(0, len(deps), 2):
        out += (", " if i else "") + deps[i]
        if deps[i + 1] == "dropped":
            out += " (dropped)"
        elif deps[i + 1] == "":
            out += " (missing)"
    return out^
