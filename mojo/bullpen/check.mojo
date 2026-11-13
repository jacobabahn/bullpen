"""`check`: each task's status against what its agent is doing now (bullpen/check.py),
the report only. `--fix` applies the missed events through the watcher's paths and
news, so it stays with Python."""

from std.os.path import isdir

from bullpen.args import Args
from bullpen.core import Repo, Task, repo_root, known_repos, closed, waiting, clip, git, task_base_ref, stamp
from bullpen.rex import rex_alive, program_status, screen
from bullpen.agents import waited_on
from bullpen.stops import overdue
from bullpen.events import shell_running, compacting
from bullpen.timeutil import seconds_since
from bullpen.textutil import stripped_lines, py_strip, cps, from_cps

comptime SETTLE = 60.0  # seconds a new idle, blocked or error state is left alone
comptime NO_AGE: Float64 = -1e300  # an age not known (Python's None)


def screen_state(text: String) -> String:
    """What Claude Code's screen shows: working, blocked, idle, or "" for none of those."""
    var lines = stripped_lines(text, 12)
    for l in lines:
        if "esc to interrupt" in l:
            return "working"
    for l in lines:
        if l.startswith("Do you want to"):
            return "blocked"
    for l in lines:
        if l.startswith("❯"):
            return "idle"
    return ""


struct AgentState(Copyable, Movable):
    """agent_state's dict; state "" for None (unsure, or it just changed). age NO_AGE: unknown (None)."""

    var state: String
    var kind: String
    var msg: String
    var age: Float64

    def __init__(out self, state: String, kind: String = "", msg: String = "", age: Float64 = NO_AGE):
        self.state = state
        self.kind = kind
        self.msg = msg
        self.age = age


def agent_state(session: String, block: String) raises -> AgentState:
    """What the agent in a pane is doing now: gone, working, blocked, error, idle or shell."""
    if not rex_alive(session, block):
        return AgentState("gone")
    var r = program_status(session, block)
    var out: AgentState
    if len(r.nodes) > 0 and r.is_true(0):
        var upd = r.str(0, "updated_at")
        var age = seconds_since(upd) if r.truthy(0, "updated_at") else NO_AGE
        var state = r.str(0, "state")
        if not r.truthy(0, "state"):
            state = "idle"
        if state != "working" and state != "blocked" and state != "error":
            state = "idle"
        if state != "working" and age != NO_AGE and age < SETTLE:
            return AgentState("")  # its event may still be on the way
        out = AgentState(state, r.str(0, "kind"), r.str(0, "msg"), age)
    else:  # no record to read: go by the screen
        var state = screen_state(screen(session, block))
        if not state:
            return AgentState("")
        out = AgentState(state)
    if out.state == "idle" and shell_running(screen(session, block)):
        out.state = "shell"
    return out^


def needs_label(why: String) -> String:
    """ "permission" for "Needs you (permission): ...", else the why in a few words."""
    var w = String(why[byte=9:]) if why.startswith("Reviewer ") else why
    if w.startswith("Needs you (") or w.startswith("needs you ("):
        var rest = String(w[byte=11:])
        var close = rest.find(")")
        if close >= 0:
            return String(rest[byte=:close])
    var text = why if why else "waiting"
    var c = cps(text)
    var first = from_cps(c, 0, 1)
    return clip(first.lower() + from_cps(c, 1, len(c)), 60)


def doing(a: AgentState, who: String = "agent") -> String:
    if a.state == "idle":
        return "its " + who + " is idle at its prompt"
    if a.state == "blocked":
        return "its " + who + " is waiting on you (" + (a.kind if a.kind else "input") + ")"
    if a.state == "error":
        return "its " + who + "'s turn failed"
    return "its " + who + " is working again"


def shell_minutes(a: AgentState, limit: Float64) -> Int:
    """Minutes idle with a background shell running, once past limit; else -1."""
    if a.state != "shell" or a.age == NO_AGE or a.age < limit * 60:
        return -1
    return Int(a.age // 60)


def at_prompt(t: Task) -> Bool:
    """Is the task's worker stopped at a prompt? (its stop is exactly that)"""
    var s = t.get("stop")
    return t.doc.kind(s) == 6 and len(t.doc.nodes[s].keys) == 2 and t.sub(s, "by") == "worker" and t.sub(s, "kind") == "prompt"


struct Drift(Copyable, Movable):
    var line: String  # "" for none
    var fix: String

    def __init__(out self, line: String = "", fix: String = ""):
        self.line = line
        self.fix = fix


def worker_drift(t: Task, a: AgentState, limit: Float64) -> Drift:
    var tid = t.id()
    var st = t.status()
    if a.state == "gone":
        return Drift(tid + " is marked " + st + " but its pane is gone", "gone")
    if a.state == "shell":
        var m = shell_minutes(a, limit)
        if st == "working" and m >= 0:
            return Drift(tid + " is marked working but its agent has been idle at its prompt for " + String(m)
                         + " min, with a background shell still running: `bullpen peek` it", "shell")
        return Drift()
    if st == "working" and a.state != "working":
        if t.truthy("ok") and a.state != "blocked":
            return Drift()
        var tail = ": `bullpen peek` it" if a.state == "idle" else ""
        return Drift(tid + " is marked working but " + doing(a) + tail, a.state)
    if not waiting(st) or t.truthy("ask"):
        return Drift()
    if a.state == "working":
        return Drift(tid + " is marked " + st + " (" + needs_label(t.s("why")) + ") but " + doing(a), "working")
    if a.state == "blocked" and not at_prompt(t):
        return Drift(tid + " is marked " + st + " (" + needs_label(t.s("why")) + ") but " + doing(a), "blocked")
    return Drift()


def reviewer_drift(t: Task, a: AgentState, limit: Float64) -> Drift:
    var tid = t.id()
    var st = t.status()
    if a.state == "shell":
        if st != "review" or shell_minutes(a, limit) < 0 or t.truthy("ok"):
            return Drift()
        return Drift(tid + " is in review but its reviewer is idle without a verdict", "shell")
    if a.state == "gone":
        return Drift(tid + " is " + ("in review" if st == "review" else "marked " + st) + " but its reviewer's pane is gone", "gone")
    if st == "review" and (a.state == "idle" or a.state == "error") and not t.truthy("ok"):
        return Drift(tid + " is in review but its reviewer is idle without a verdict", a.state)
    if st == "review" and a.state == "blocked":
        return Drift(tid + " is in review but " + doing(a, "reviewer"), "blocked")
    if waiting(st) and a.state == "working":
        return Drift(tid + " is marked " + st + " (" + needs_label(t.s("why")) + ") but " + doing(a, "reviewer"), "working")
    return Drift()


def _isdigit(s: String) -> Bool:
    if s.byte_length() == 0:
        return False
    for b in s.as_bytes():
        if Int(b) < ord("0") or Int(b) > ord("9"):
            return False
    return True


def unready_hint(repo: Repo, t: Task, a: AgentState) raises -> String:
    """A line for an idle worker whose work looks finished but never ran ready, or ""."""
    if a.state != "idle" or not (t.status() == "working" or waiting(t.status())):
        return ""
    var wt = t.s("worktree")
    if not wt or not isdir(wt) or py_strip(git(wt, ["status", "--porcelain"], False).out).byte_length() > 0:
        return ""
    var ahead = py_strip(git(wt, ["rev-list", "--count", task_base_ref(repo, t) + "..HEAD"], False).out)
    if not _isdigit(ahead) or Int(ahead) == 0:
        return ""
    return (t.id() + " is idle with commits and a clean worktree but never ran ready: "
            + "`bullpen peek` it, then `bullpen tell` it to run ready if it's done")


def check_repo(repo: Repo, ids: List[String], quiet: Bool) raises -> Int:
    """Check the repo's tasks (those in ids, if given). Returns how many disagreed."""
    var drift = 0
    for t in repo.tasks():
        if closed(t.status()) or (len(ids) > 0 and t.id() not in ids):
            continue
        var late = overdue(repo, t)
        if late.why:
            drift += 1
            if not quiet:
                print(t.id() + " waits on its coordinator, but " + late.why + ": it needs the person")
        var since = stamp(t)
        if (seconds_since(since) if since else 0.0) < SETTLE:
            continue  # bullpen just changed it: its agent has yet to say
        var agent = waited_on(t)
        if not agent.who:
            continue
        var a = agent_state(agent.session, agent.block)
        if not a.state:
            continue
        if a.state == "working" and compacting(agent.session, agent.block, a.msg):
            continue  # compacting by itself: no turn, so no drift
        var limit = repo.cfg.num(repo.cfg.get(0, "stuck_after"))
        if limit == 0:
            limit = 20
        var found = worker_drift(t, a, limit) if agent.who == "worker" else reviewer_drift(t, a, limit)
        if found.line:
            drift += 1
            if not quiet:
                print(found.line)
        var hint = unready_hint(repo, repo.task(t.id()), a) if agent.who == "worker" else String("")
        if hint and not quiet:
            print(hint)
    return drift


def cmd_check(args: Args) raises -> Int:
    """Compare each task's status with what its agent is doing now; --fix (Python) catches up."""
    if args.flag("--fix"):
        raise Error("__python__")
    var repos = List[Repo]()
    if args.flag("--all-repos"):
        repos = known_repos()
    else:
        repos.append(Repo(repo_root()))
    var drift = 0
    for r in range(len(repos)):
        ref repo = repos[r]
        var ids = List[String]()
        for i in args.pos:
            var id = repo.norm(i)
            if id not in ids:
                ids.append(id)
        for i in ids:
            _ = repo.task(i)  # an unknown ID is an error, not "all agree"
        drift += check_repo(repo, ids, args.flag("--quiet"))
    return 1 if drift else 0
