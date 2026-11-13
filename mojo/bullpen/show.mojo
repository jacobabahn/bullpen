"""`ls`, `show` and `diff --stat` (bullpen/show.py)."""

from bullpen.core import Repo, Task, repo_root, listed, known_repos
from bullpen.json import Doc, dumps_list, quote
from bullpen.args import Args


def cmd_ls(args: Args) raises:
    if args.flag("--all-repos"):
        return ls_repos(args)
    var repo = Repo(repo_root())
    var tasks = listed(repo, args.flag("--all"))
    if args.flag("--json"):
        print(task_list_json(tasks))
        return
    raise Error("__python__")  # not ported yet


def task_list_json(tasks: List[Task]) -> String:
    var docs = List[Doc]()
    for t in tasks:
        docs.append(t.doc.copy())
    return dumps_list(docs)


def ls_repos(args: Args) raises:
    if not args.flag("--json"):
        raise Error("__python__")  # not ported yet
    var out = String()
    var n = 0
    for r in known_repos():
        var ts = listed(r, args.flag("--all"))
        if len(ts) == 0:
            continue
        # json.dumps([{"repo": ..., "tasks": [...]}], indent=2)
        out += ("," if n else "") + '\n  {\n    "repo": '
        quote(r.root, out)
        out += ',\n    "tasks": '
        if len(ts) == 0:
            out += "[]"
        else:
            out += "["
            for i in range(len(ts)):
                out += ("," if i else "") + "\n      " + ts[i].doc.dumps(0, 2, 3)
            out += "\n    ]"
        out += "\n  }"
        n += 1
    print("[]" if n == 0 else "[" + out + "\n]")


def cmd_show(args: Args) raises:
    var repo = Repo(repo_root())
    var t = repo.task(args.pos[0])
    if args.flag("--json"):
        print(t.doc.dumps())
        return
    raise Error("__python__")  # not ported yet
