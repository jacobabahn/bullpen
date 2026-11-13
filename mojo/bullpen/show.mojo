"""`ls`, `show` and `diff --stat` (bullpen/show.py)."""

from std.os.path import exists

from bullpen.core import Repo, Task, repo_root, listed, known_repos, own_model, basename
from bullpen.json import Doc, dumps_list, quote, NULL
from bullpen.args import Args
from bullpen.rex import rex_alive
from bullpen.agents import is_limited
from bullpen.coordinators import owner
from bullpen.hooks import hook_log
from bullpen.start import statuses, waits_phrase
from bullpen.review import review_kind
from bullpen.task_line import (
    ago, board_detail, board_order, colored, is_stuck, pr_phrase, stuck_phrase, task_line,
    task_rows, term_width, py_str,
)
from bullpen.text import text_width, wrap, strip, splitlines


def cmd_ls(args: Args) raises:
    if args.flag("--all-repos"):
        return ls_repos(args)
    var repo = Repo(repo_root())
    var tasks = listed(repo, args.flag("--all"))
    if args.flag("--json"):
        print(task_list_json(tasks))
        return
    if len(tasks) == 0:
        print("no open tasks" + ("" if args.flag("--all") else " (--all shows finished ones)"))
    for line in ls_lines(repo, tasks, term_width(), colored()):
        print(line)


def task_list_json(tasks: List[Task]) -> String:
    var docs = List[Doc]()
    for t in tasks:
        docs.append(t.doc.copy())
    return dumps_list(docs)


comptime SENTENCE_ROOM = 24  # columns a cut line must leave for the board's sentence, or ls takes two rows


def ls_lines(repo: Repo, tasks: List[Task], width: Int = -1, color: Bool = False) raises -> List[String]:
    """`ls`'s lines, in the board's order: one task_line each, whole when piped (width
    -1), cut to the width on a terminal. When a line would leave under SENTENCE_ROOM
    columns for some task's sentence (a narrow terminal, a long title), the board's
    two rows each instead."""
    var every = statuses(repo)
    var idw = 0
    for t in tasks:
        idw = max(idw, t.id().byte_length())
    var ordered = board_order(tasks)
    var out = List[String]()
    if width < 0:
        for t in ordered:
            out.append(task_line(repo, t, every, idw, -1, color))
        return out^
    var one_row = True
    for t in ordered:
        var when = ago(t)
        var room = width - 1 - (2 + idw + 2) - text_width(t.s("title")) - (when.byte_length() + 2 if when else 0) - 2
        if not (room >= SENTENCE_ROOM or text_width(task_line(repo, t, every, idw)) < width):
            one_row = False
            break
    for t in ordered:
        if one_row:
            out.append(task_line(repo, t, every, idw, width - 1, color))
        else:
            out += task_rows(repo, t, every, idw, width, color)
    return out^


def ls_repos(args: Args) raises:
    """`ls --all-repos`: the open tasks of every repository bullpen knows, by repository.
    One with nothing open, or whose checkout is gone, is left out."""
    var json = args.flag("--json")
    var out = String()
    var n = 0
    var width = term_width()
    for r in known_repos():
        var ts = listed(r, args.flag("--all"))
        if len(ts) == 0:
            continue
        if not json:
            if n:
                print()
            print(basename(r.root) + "  (" + r.root + ")")
            for line in ls_lines(r, ts, width - 2 if width > 0 else width, colored()):
                print("  " + line)
            n += 1
            continue
        # json.dumps([{"repo": ..., "tasks": [...]}], indent=2)
        out += ("," if n else "") + '\n  {\n    "repo": '
        quote(r.root, out)
        out += ',\n    "tasks": ['
        for i in range(len(ts)):
            out += ("," if i else "") + "\n      " + ts[i].doc.dumps(0, 2, 3)
        out += "\n    ]\n  }"
        n += 1
    if json:
        print("[]" if n == 0 else "[" + out + "\n]")
    elif n == 0:
        print("no open tasks in any repository" + ("" if args.flag("--all") else " (--all shows finished ones)"))


def model_line(repo: Repo, t: Task) raises -> String:
    """ "opus (its own, over the config's sonnet)", and what runs now if that's another."""
    var cfg = repo.cfg.get(0, "worker_model")
    var line = own_model(repo, t) + " (its own, over the config's" + (" " + py_str(repo.cfg, cfg) if repo.cfg.is_true(cfg) else "") + ")"
    var now_on = t.s("model") if rex_alive(t.s("session"), t.s("block")) else ""
    if now_on and now_on != own_model(repo, t):
        line += "; running " + now_on + " until its next start or resume"
    return line^


def _key(k: String) -> String:
    """f"{k:>9}: " """
    return " " * max(9 - k.byte_length(), 0) + k + ": "


def cmd_show(args: Args) raises:
    var repo = Repo(repo_root())
    var t = repo.task(args.pos[0])
    if args.flag("--json"):
        print(t.doc.dumps())
        return
    var every = statuses(repo)
    var detail = board_detail(repo, t, every)
    var width = term_width()
    if width < 0 or text_width(task_line(repo, t, every)) < width:
        print(task_line(repo, t, every, color=colored()))
    else:
        print(String("\n").join(task_rows(repo, t, every, width=width, color=colored(), lines=99)))
    for k in ["why", "base", "branch", "worktree", "block", "model", "pr", "diffstat", "summary"]:
        if k == "model" and own_model(repo, t):
            print(_key(k) + model_line(repo, t))
        elif k == "why" and t.truthy(k) and t.s(k) in detail:
            continue  # the first line says it
        elif t.truthy(k):
            var lines = List[String]()
            for l in splitlines(strip(py_str(t.doc, t.get(k)))):
                lines.append((" " * 11 if l else "") + l)
            print(_key(k) + strip(String("\n").join(lines), trail=False))
        elif k == "pr" and t.truthy("local"):
            print(_key(k) + "none; local branch, merged into " + py_str(t.doc, t.get("base")) + " by accept")
    if t.truthy("after"):
        print("    after: " + String(", ").join(t.doc.strs(0, "after")))
    var c = owner(repo, t)
    if repo.cfg.len(repo.cfg.get(0, "coordinators")) > 1 or (t.truthy("coordinator") and c < 0):
        if c >= 0:
            print("    coord: " + py_str(repo.cfg, repo.cfg.get(c, "id")) + " (" + py_str(repo.cfg, repo.cfg.get(c, "block")) + ")")
        else:
            print("    coord: " + (t.s("coordinator") if t.truthy("coordinator") else "none")
                  + ", gone: every running coordinator sees to it")
    if t.has("auto_accept") and t.doc.kind(t.get("auto_accept")) != NULL:
        print("     auto: " + ("accepts by itself once approved" if t.truthy("auto_accept") else "kept for you (--no-auto-accept)"))
    var waits = waits_phrase(t, every)
    if waits:
        print("    waits: " + waits)
    var gh = pr_phrase(t)
    if gh and not (gh.lower() in detail.lower()):
        print("   github: " + gh)
    if is_stuck(t):
        print("    quiet: its " + t.doc.str(t.get("stuck"), "who") + " has shown " + stuck_phrase(t))
    var r = t.get("review")
    if t.truthy("review_mode"):
        print("     mode: " + t.s("review_mode") + " first review (its own)")
    if t.doc.is_true(r):
        var state = t.doc.str(r, "verdict") if t.doc.truthy(r, "verdict") else "in progress"
        if t.doc.str(r, "verdict") == "approved" and t.doc.truthy(r, "kept"):
            var kept = t.doc.get(r, "kept")
            state += ", kept " + py_str(t.doc, kept) + " time" + ("s" if t.doc.num(kept) != 1 else "") + " as the code didn't change"
        var kind = review_kind(t.doc, r) + (" on " + t.doc.str(r, "model") if t.doc.truthy(r, "model") else "")
        print("   review: round " + py_str(t.doc, t.doc.get(r, "round")) + " (" + kind + "), " + state
              + ("; findings: " + t.doc.str(r, "findings") if t.doc.truthy(r, "findings") else ""))
    for name in ["setup", "teardown"]:
        var log = hook_log(repo, t, name)
        if exists(log):
            var failed = " (failed: the worker didn't start)" if name == "setup" and t.truthy("needs_setup") else ""
            print(_key(name) + log + failed)
    print("    brief: " + repo.dir + "/briefs/" + t.id() + ".md")
    var nxt = next_steps(t, every)
    print("     next: " + (String("\n" + " " * 11).join(wrap(nxt, width - 12, 99)) if width > 0 else nxt))


def next_steps(t: Task, every: Dict[String, String]) -> String:
    """What you can do with the task now: the commands, with the board's keys."""
    var tid = t.id()
    var st = t.status()
    if st == "needs" and t.truthy("ask"):
        return "`bullpen reply " + tid + " <answer>`, or i on the board"
    if st == "needs" and t.doc.str(t.get("stop"), "kind") == "prompt":
        return "`bullpen answer " + tid + " <option>` (`bullpen peek " + tid + "` shows the prompt), or i on the board"
    if st == "needs":
        return "`bullpen go " + tid + "` opens it (⏎ on the board), or `bullpen tell " + tid + " <message>` (t)"
    if st == "stopped":
        return "`bullpen resume " + tid + "` (r on the board), or `bullpen drop " + tid + "` (d)"
    if st == "triage":
        return "nothing: its coordinator is on it. `bullpen go " + tid + "` opens it (⏎ on the board)"
    if st == "ready":
        return "`bullpen accept " + tid + "` (a on the board); `bullpen diff " + tid + "` shows the change (v)"
    if st == "finished":
        return "`bullpen close " + tid + "` (a on the board), or `bullpen drop " + tid + "` (d)"
    if st == "review":
        return "nothing until its review is done; `bullpen go " + tid + " --review` opens the reviewer (⏎ on the board)"
    if st == "working" and (is_stuck(t) or (is_limited(t) and not t.doc.truthy(t.get("limit"), "resets"))):
        return "`bullpen peek " + tid + "` shows its screen (p on the board), `bullpen tell " + tid + " <message>` (t)"
    if st == "working":
        return "nothing until it's ready; `bullpen tell " + tid + " <message>` steers it (t on the board)"
    if st == "todo" and waits_phrase(t, every):
        return "nothing: it starts by itself once what it waits on is accepted; `bullpen start " + tid + " --force` starts it now"
    if st == "todo":
        return "`bullpen start " + tid + "` (s on the board), or `bullpen drop " + tid + "` (d)"
    return "nothing: it's closed"
