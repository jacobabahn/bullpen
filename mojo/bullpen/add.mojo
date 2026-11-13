"""The commands that only write a task's files (from bullpen/start.py): `add`, `after`,
`model` and `review-mode`. None of them touches a pane, a worktree or GitHub."""

from std.os.path import exists

from bullpen.args import Args, choice
from bullpen.core import Repo, Task, repo_root, closed, is_status, home, read_text, write_text, write_json, mkdirs
from bullpen.json import Doc, Node, OBJ, ARR, STR, NULL
from bullpen.timeutil import iso_now
from bullpen.coordinators import calling_coordinator, primary
from bullpen.rex import rex_alive


def dep_ids(repo: Repo, values: List[String]) -> List[String]:
    """IDs from --after values, each one or several: "BP-8", "8,9", "BP-8 BP-9"."""
    var out = List[String]()
    for v in values:
        for part in v.replace(",", " ").split():
            var id = repo.norm(String(part))
            var seen = False
            for o in out:
                if o == id:
                    seen = True
            if not seen:
                out.append(id)
    return out^


def find(tasks: List[Task], id: String) -> Int:
    for i in range(len(tasks)):
        if tasks[i].id() == id:
            return i
    return -1


def dep_path(tasks: List[Task], start: String, goal: String, mut seen: List[String]) -> List[String]:
    """[start, ..., goal] if start waits on goal, directly or through others, else []."""
    if start == goal:
        return [goal]
    for s in seen:
        if s == start:
            return List[String]()
    seen.append(start)
    var i = find(tasks, start)
    if i < 0:
        return List[String]()
    for d in tasks[i].doc.strs(0, "after"):
        var rest = dep_path(tasks, d, goal, seen)
        if len(rest):
            var out: List[String] = [start]
            out += rest^
            return out^
    return List[String]()


def check_after(repo: Repo, tid: String, var after: List[String]) raises -> List[String]:
    """after, if the task tid can wait on those tasks: each exists, isn't tid itself or
    dropped, and none of them waits on tid already (a cycle)."""
    var tasks = repo.tasks()
    for d in after:
        if d == tid:
            raise Error(tid + " can't wait on itself")
        var i = find(tasks, d)
        if i < 0:
            raise Error("no task " + d + " in " + home(repo.root) + ": `bullpen ls --all` lists them")
        if tasks[i].status() == "dropped":
            raise Error(d + " was dropped: " + tid + " would wait on it forever")
        var seen = List[String]()
        var path = dep_path(tasks, d, tid, seen)
        if len(path):
            raise Error(d + " already waits on " + tid + " (" + String(" → ").join(path) + "): that would be a cycle")
    return after^


def put_list(mut t: Task, key: String, values: List[String]):
    var a = t.doc.put(0, key, Node(ARR))
    for v in values:
        var n = t.doc.add(Node(STR, v))
        t.doc.nodes[a].kids.append(n)


def cmd_add(args: Args) raises:
    if args.has_opt("--review"):
        choice(args.cmd, "--review", args.opt("--review"), "full light auto")
    var repo = Repo(repo_root())
    var brief = read_text(args.opt("--brief-file")) if args.has_opt("--brief-file") else args.opt("--brief")
    if not brief:
        raise Error("give the task's brief with --brief or --brief-file")
    var tid = repo.cfg_str("prefix") + "-" + String(repo.cfg.int(0, "next"))
    var after = check_after(repo, tid, dep_ids(repo, args.opts("--after")))
    _ = repo.cfg.put_int(0, "next", repo.cfg.int(0, "next") + 1)
    repo.save()
    var t = Task(Doc())
    _ = t.doc.add(Node(OBJ))
    _ = t.doc.put_str(0, "id", tid)
    _ = t.doc.put_str(0, "title", args.pos[0])
    _ = t.doc.put_str(0, "status", "todo")
    _ = t.doc.put_str(0, "created", iso_now())
    _ = t.doc.put_str(0, "base", args.opt("--base") if args.opt("--base") else repo.base())
    for k in ["branch", "worktree", "block", "session", "pr", "why", "last"]:
        _ = t.doc.put_null(0, k)
    var c = calling_coordinator(repo)
    if c < 0:
        c = primary(repo)
    if c >= 0:
        _ = t.doc.put_str(0, "coordinator", repo.cfg.str(c, "id"))
    if len(after):
        put_list(t, "after", after)
    if args.opt("--model"):
        _ = t.doc.put_str(0, "worker_model", args.opt("--model"))
    var mode = args.opt("--review")
    if mode and mode != "auto":
        _ = t.doc.put_str(0, "review_mode", mode)
    if args.flag("--auto-accept") or args.flag("--no-auto-accept"):  # the last one given wins, as argparse's
        _ = t.doc.put_bool(0, "auto_accept", args.last_flag("--auto-accept", "--no-auto-accept") == "--auto-accept")
    mkdirs(repo.dir + "/briefs")
    write_text(repo.dir + "/briefs/" + tid + ".md", String(brief.rstrip()) + "\n")
    repo.save_task(t)
    print(tid)


def cmd_after(args: Args) raises:
    """Set what a task waits on, or clear it with --none."""
    var repo = Repo(repo_root())
    var t = repo.task(args.pos[0])
    var deps = List[String]()
    for i in range(1, len(args.pos)):
        deps.append(args.pos[i])
    if args.flag("--none") == (len(deps) > 0):
        raise Error("give the tasks it waits on, or --none")
    if closed(t.status()):
        raise Error(is_status(t))
    var after = List[String]() if args.flag("--none") else check_after(repo, t.id(), dep_ids(repo, deps))
    if len(after):
        put_list(t, "after", after)
    else:
        t.doc.pop(0, "after")
    write_json(repo.task_path(t.id()), t.doc)  # not save_task: the status didn't change
    print(t.id() + " waits on " + String(", ").join(after) if len(after) else t.id() + " waits on nothing")


def cmd_model(args: Args) raises:
    """Set, change or clear the worker model a task starts and resumes with."""
    var repo = Repo(repo_root())
    var t = repo.task(args.pos[0])
    var model = args.pos[1] if len(args.pos) > 1 else String("")
    var clear = args.flag("--clear")
    if clear == (model.byte_length() > 0):
        raise Error("give the model (an alias like opus, or a model ID), or --clear")
    if closed(t.status()):
        raise Error(is_status(t))
    if clear:
        t.doc.pop(0, "worker_model")
    else:
        _ = t.doc.put_str(0, "worker_model", model)
    write_json(repo.task_path(t.id()), t.doc)  # not save_task: the status didn't change
    var cfg = repo.cfg_str("worker_model")
    var said: String
    if clear and cfg:
        said = t.id() + " uses the config's worker model (" + cfg + ")"
    elif clear:
        said = t.id() + " uses the worker command's own model"
    else:
        said = t.id() + " uses " + model
    if rex_alive(t.s("session"), t.s("block")):
        var m = t.s("model")
        said += "; its worker keeps running on " + (m if m else "its model") + ": this takes effect at its next start or resume"
    print(said)


def cmd_review_mode(args: Args) raises:
    """Set how a task's first review goes: full, light, or auto (by the size of the change)."""
    var mode = args.pos[1]
    choice(args.cmd, "mode", mode, "full light auto")
    var repo = Repo(repo_root())
    var t = repo.task(args.pos[0])
    if closed(t.status()):
        raise Error(is_status(t))
    if mode == "auto":
        t.doc.pop(0, "review_mode")
    else:
        _ = t.doc.put_str(0, "review_mode", mode)
    write_json(repo.task_path(t.id()), t.doc)  # not save_task: the status didn't change
    var r = t.get("review")
    var later = " (it has had a review already: later rounds follow up on it, whatever the mode)" if t.doc.truthy(r, "round") else ""
    print(t.id() + "'s first review: " + mode + later)
