"""The base of bullpen: its paths, the settings table, small helpers, a repo's config
and tasks, and a task's status (set_status). Every other module imports from here,
and this one imports none of them: what saving a task sets off elsewhere registers
with on_save.
"""

import datetime as dt
import json
import os
import re
import subprocess
from pathlib import Path


HOME = Path(__file__).resolve().parent.parent  # bullpen's checkout: bullpen/ sits beside bin/
PROMPTS = HOME / "prompts"
WATCH_LUA = HOME / "lua" / "watch.lua"
STATE = Path(os.environ.get("BULLPEN_STATE") or Path.home() / ".local/state/bullpen")
REX = os.environ.get("BULLPEN_REX", "rex")
# The entry script: what Lua, the key bindings and bullpen's own background jobs run.
SELF = str(HOME / "bin" / "bullpen")


class Setting:
    """One setting: its default, its group in `config`'s list, whether only a repository
    can hold it, and its kind, which gives `config` its flag and the flag's check:
    "file" (no flag: set in config.json by hand), "choice", "on/off", "number" (with a
    least value, and what to say when it is less), "text" (a "" clears it, when clears),
    or "model" (claude's --model). A text setting with a pattern must match it."""

    def __init__(self, name, default, group, kind, help="", repo_only=False, choices=None,
                 least=None, says=None, metavar=None, clears=False, none=None, pattern=None):
        self.name, self.default, self.group, self.kind, self.help = name, default, group, kind, help
        self.repo_only, self.choices, self.least, self.says = repo_only, choices, least, says
        self.metavar, self.clears, self.none, self.pattern = metavar, clears, none, pattern

    @property
    def flag(self):
        return "--" + self.name.replace("_", "-")

    def default_phrase(self):
        """The default, as config's help states it."""
        d = self.default
        if d is None:
            return self.none
        if isinstance(d, bool):
            return "on" if d else "off"
        return str(d)

    def full_help(self):
        phrase = self.default_phrase()
        return f"{self.help} (default {phrase})" if phrase else self.help


AGENT = ["claude", "--permission-mode", "auto"]
NOTIFY = ("all", "needs", "off")
PLACES = ("float", "tab", "tabs", "background")  # where a worker's or reviewer's pane can live (panes.py)

# Every setting bullpen has, in the order `config` lists them.
SETTINGS = [
    Setting("prefix", "BP", "Tasks", "text", "the letters new task IDs start with (2 to 5 capitals)",
            repo_only=True, pattern=r"[A-Z]{2,5}", says="2 to 5 capital letters"),
    Setting("next", 1, None, "file", repo_only=True),  # the next task's number
    Setting("base", None, "Tasks", "text", "the branch tasks start from and open PRs against",
            repo_only=True, none="the repo's default branch"),
    Setting("max_running", 3, "Tasks", "number",
            "tasks working at once (review doesn't count); the queue starts more as they hand off",
            least=1, says="a number of tasks, 1 or more", metavar="N"),
    Setting("merge", "rebase", "Tasks", "choice", "how accept merges a PR",
            choices=("rebase", "squash", "merge")),
    Setting("rebase_others", True, "Tasks", "on/off", "accept rebases the other open tasks onto the new base"),
    Setting("push_remote", None, "Tasks", "text",
            "local mode: push the base branch to this remote (a backup, not origin) after "
            "each accept, and when `up` finds it behind (\"\" turns it off)",
            repo_only=True, metavar="REMOTE", clears=True, none="none"),
    Setting("auto_accept", False, "Tasks", "on/off", "accept (merge) a task by itself once review approves it"),
    Setting("setup", None, "Tasks", "text",
            "a command run in each new worktree before its worker starts, over .bullpen/setup.sh (\"\" clears it)",
            metavar="CMD", clears=True, none="none"),
    Setting("teardown", None, "Tasks", "text",
            "a command run in a worktree before accept, close or drop removes it, over "
            ".bullpen/teardown.sh (\"\" clears it)", metavar="CMD", clears=True, none="none"),
    Setting("gh", None, "Tasks", "text",
            "the gh program, if not the one on PATH; BULLPEN_GH wins (\"\" clears it)",
            metavar="PATH", clears=True, none="gh on PATH"),
    Setting("worker", list(AGENT), "Agents", "file"),           # the command each worker runs
    Setting("coordinator_cmd", list(AGENT), "Agents", "file"),  # the one a coordinator runs
    Setting("reviewer", list(AGENT), "Agents", "file"),         # the one a reviewer runs
    Setting("tour", ["claude"], "Agents", "file"),        # the one that writes tours, headless (claude -p)
    Setting("workers", "float", "Agents", "choice",
            "new workers as capsules in the coordinator's tab (float), as capsules in one workers "
            "tab (tab), a tab each (tabs), or in the background: in no tab until `go` shows one",
            choices=PLACES),
    Setting("reviewers", "background", "Agents", "choice",
            "new reviewers the same way (float, tab, tabs, background), or where workers go (same)",
            choices=("same", *PLACES)),
    Setting("worker_model", None, "Agents", "model",
            "claude's --model for workers (\"default\": none, so Claude Code picks)",
            none="the worker command's own"),
    Setting("reviewer_model", None, "Agents", "model", "the same, for reviewers (a full first review)",
            none="the reviewer command's own"),
    Setting("reviewer_followup_model", "sonnet", "Agents", "model",
            "the same, for later review rounds and light reviews"),
    Setting("tour_model", "sonnet", "Agents", "model",
            "claude's --model for the agent that writes guided review tours"),
    Setting("review", True, "Review", "on/off", "a reviewer agent checks each task before it's ready"),
    Setting("review_rounds", 2, "Review", "number",
            "reviews a task gets before it is ready anyway, findings listed",
            least=1, says="a number of rounds, 1 or more", metavar="N"),
    Setting("light_review_lines", 40, "Review", "number",
            "a change of at most this many lines in at most 2 files gets a light review",
            least=0, says="a number of lines, or 0 for docs-only changes alone", metavar="LINES"),
    Setting("board_layout", "grouped", "Board", "choice", "how the board lists tasks",
            choices=("grouped", "compact")),
    Setting("board_bottom_gap", None, "Board", "number",
            "empty rows at the board's bottom, under Rex's minimized-pane capsules (0: none)",
            least=0, says="rows, or 0 for none", metavar="ROWS",
            none="1 while workers or reviewers float, else 0"),
    Setting("board_repos", True, "Board", "on/off",
            "the board's top line counts the tasks that need you or are ready in other repositories"),
    Setting("news", "type", "News", "choice",
            "how news reaches the coordinator: typed into its pane (type), or read by its "
            "Monitor tool from `bullpen news --follow` (monitor)", repo_only=True, choices=("type", "monitor")),
    Setting("notify", "needs", "News", "choice",
            "native notifications: when a task needs you (needs), also when one is ready, "
            "finished or auto-accepted (all), or none (off)", choices=NOTIFY),
    Setting("stuck_after", 20, "News", "number",
            "news when a working agent shows no progress this long (0: never)",
            least=0, says="minutes, or 0 to turn it off", metavar="MINUTES"),
    Setting("triage_minutes", 10, "News", "number",
            "how long a task can wait on its coordinator before it needs the person",
            least=0, says="minutes", metavar="MINUTES"),
    # [{"id": "c1", "block": ..., "session": ..., "board": {block, session}}]
    Setting("coordinators", [], None, "file", repo_only=True),
    Setting("next_coordinator", 1, None, "file", repo_only=True),
]
SETTING = {s.name: s for s in SETTINGS}

DEFAULT_CONFIG = {s.name: s.default for s in SETTINGS}
# Settings that belong to one repository: the global config.json can't hold them.
REPO_ONLY = tuple(s.name for s in SETTINGS if s.repo_only)
# What bullpen notes for itself in a repo's config.json.
INTERNAL = ("root", "triage_migrated", "stops_migrated", "layered")

TERMINAL = "com.superlogical.terminal.shell"
FLOAT_BOUNDS = [0.4, 0.35, 0.58, 0.62]  # bottom right of the window: x, y, w, h

CLOSED = ("done", "dropped")
REVIEW_MODES = ("full", "light", "auto")
# A task stopped on something: "triage" waits on its coordinator, "needs" on the person.
WAITING = ("needs", "triage")
# A task with a stop: waiting, or "stopped" (its worker's pane is gone).
STOPPED = (*WAITING, "stopped")


class Fail(Exception):
    pass


# ---------- small helpers ----------


def now():
    return dt.datetime.now().astimezone().isoformat(timespec="seconds")


def run(cmd, cwd=None, check=True, input=None):
    # Never let a program read our terminal: from the board, it would wait there
    # for keys meant for the board.
    stdin = None if input is not None else subprocess.DEVNULL
    p = subprocess.run(cmd, cwd=cwd, input=input, stdin=stdin, text=True, capture_output=True)
    if check and p.returncode != 0:
        msg = (p.stderr or p.stdout).strip()
        raise Fail(f"`{' '.join(map(str, cmd))}` failed: {msg}")
    return p


def git(repo, *args, check=True):
    return run(["git", "-C", str(repo), *args], check=check)


def head(wt):
    return git(wt, "rev-parse", "HEAD").stdout.strip()


def merge_base(wt, base_ref):
    return git(wt, "merge-base", base_ref, "HEAD", check=False).stdout.strip() or None


def read_json(path, default=None):
    try:
        return json.loads(Path(path).read_text())
    except FileNotFoundError:
        return default


def write_json(path, data):
    path = Path(path)
    import tempfile  # here, not at the top: most calls write nothing (start-up is most of a call)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".tmp-")
    with os.fdopen(fd, "w") as f:
        json.dump(data, f, indent=2)
        f.write("\n")
    os.replace(tmp, path)


def safe(name):
    return re.sub(r"[^A-Za-z0-9._-]", "_", name)


def slug(title, n=40):
    s = re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")
    return s[:n].rstrip("-") or "task"


def resolve(cmd):
    """Absolute path for the program, so Rex panes don't depend on PATH."""
    import shutil  # here, not at the top: it imports the compression modules
    exe = shutil.which(cmd[0])
    return [exe or cmd[0], *cmd[1:]]


def is_claude(cmd):
    return bool(cmd) and Path(cmd[0]).name == "claude"


def model_in(cmd):
    """The --model a command list gives, if any."""
    for i, a in enumerate(cmd):
        if a == "--model" and i + 1 < len(cmd):
            return cmd[i + 1]
        if a.startswith("--model="):
            return a.split("=", 1)[1]
    return None


def with_model(cmd, model):
    """cmd with claude's --model set to model, replacing any it has. No model: cmd as
    it is; "default": no --model, so Claude Code picks. Other agents are left alone."""
    if not model or not is_claude(cmd):
        return list(cmd)
    out, skip = [], False
    for a in cmd:
        if skip:
            skip = False
        elif a == "--model":
            skip = True
        elif not a.startswith("--model="):
            out.append(a)
    return out if model == "default" else out + ["--model", model]


def model_phrase(cmd, model):
    """What model an agent with this command and setting runs, for `config`."""
    if not is_claude(cmd):
        return f"not set by bullpen: the command is {cmd[0] if cmd else 'empty'}, not claude"
    return model_in(with_model(cmd, model)) or "default (Claude Code picks)"


def seconds_since(stamp, at=None):
    at = at or dt.datetime.now().astimezone()
    return (at - dt.datetime.fromisoformat(stamp)).total_seconds()


def clip(text, n=300):
    text = " ".join((text or "").split())
    return text if len(text) <= n else text[: n - 1] + "…"


def file_hash(path):
    import hashlib  # here, not at the top: it loads OpenSSL
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def commits(n):
    return f"{n} commit{'s' if n != 1 else ''}"


def command(sub, name, fn, group=None, **kw):
    """Add a command to bullpen's parser: the function that runs it, and its group in
    `bullpen --help` (a title in cli.GROUPS; none for an internal command, which --help
    and the unknown-command error leave out)."""
    p = sub.add_parser(name, **kw)
    p.set_defaults(fn=fn)
    p.group = group
    return p


# ---------- repo, config, tasks ----------


def repo_root(path=None):
    p = run(["git", "-C", str(path or os.getcwd()), "rev-parse", "--path-format=absolute",
             "--git-common-dir"], check=False)
    if p.returncode != 0:
        raise Fail("not in a git repository")
    common = Path(p.stdout.strip())
    return common.parent if common.name == ".git" else common


def global_config_path():
    """The config.json with every repository's defaults: $XDG_CONFIG_HOME/bullpen/config.json,
    else ~/.config/bullpen/config.json."""
    return Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config") / "bullpen" / "config.json"


def read_global_file():
    """The global config.json as it is ({} when there is none). Fail when it doesn't parse."""
    path = global_config_path()
    try:
        g = read_json(path, {})
    except ValueError as e:
        raise Fail(f"{path} doesn't parse ({e}): fix it, or move it away")
    except IsADirectoryError:
        raise Fail(f"{path} is a folder, not a file: move it away")
    if not isinstance(g, dict):
        raise Fail(f"{path} isn't a JSON object ({{...}}): fix it, or move it away")
    return g


def read_global():
    """The settings the global config.json gives every repository: what it holds,
    without the keys only a repository can have."""
    return {k: v for k, v in read_global_file().items() if k not in REPO_ONLY and k not in INTERNAL}


def inherited_config():
    """What a repository gets when its own config.json doesn't say: the global settings
    over the built-in defaults."""
    return {**DEFAULT_CONFIG, **read_global()}


def load_config(d):
    """A repo's settings: its own config.json over the global one, over the defaults.
    A config from before several coordinators has one {"coordinator": {block, session}}
    and one "board": it becomes coordinator c1."""
    cfg = {**inherited_config(), **read_json(d / "config.json", {})}
    if "coordinator" in cfg or "board" in cfg:
        old, board = cfg.pop("coordinator", None), cfg.pop("board", None)
        if old and not cfg["coordinators"]:
            cfg["coordinators"] = [{"id": "c1", "block": old.get("block"), "session": old.get("session"),
                                    "board": board}]
            cfg["next_coordinator"] = 2
    cfg["coordinators"] = [dict(c) for c in cfg["coordinators"]]  # never the default's own list
    return cfg


def config_source(repo, key):
    """Where a setting's value comes from: "repo", "global" or "default"."""
    if key in REPO_ONLY:  # always written down: it is the repo's own once it isn't the default
        return "repo" if key in repo.own and repo.cfg.get(key) != DEFAULT_CONFIG[key] else "default"
    if key in repo.own:
        return "repo"
    return "global" if key in read_global() else "default"


# What saving a task sets off, beyond writing it: each fn(repo, t, old), old the
# task as it was saved before. The modules that act on a change register here
# (on_save): notifications.py tells the person, prs.py marks a PR for the watcher to
# follow, and saves.py moves the run queue when a task frees a slot. That keeps this
# module below them: it never imports them.
SAVED = []
# Loads the modules that register here, before the first save calls them: cli.main sets
# it, since a command imports only its own module (and the ones it needs).
LOAD_SAVERS = []


def load_savers():
    """Import the modules that act on a save now, if the command hasn't yet. A command
    that may replace bullpen's own code under itself (accept, merging into the main
    checkout bullpen runs from) calls this before it does: a module imported after
    that would be the new code, mixed with the old the command already runs."""
    while LOAD_SAVERS:
        LOAD_SAVERS.pop()()


def on_save(fn):
    """Call fn(repo, t, old) after every save_task, in the order registered."""
    SAVED.append(fn)
    return fn


# Work a save sets off that waits for the command's own work to be done (the run
# queue starting the next task), so its news follows the command's. cli.main runs it
# when the command returns; a command that closes the pane it runs in runs it first
# (rex.close_last).
LATER = {}


def later(key, fn):
    """Run fn once the command's work is done (run_later). A later fn with the same key
    replaces it: it runs once."""
    LATER.pop(key, None)
    LATER[key] = fn


def run_later():
    while LATER:
        LATER.pop(next(iter(LATER)))()


class Repo:
    def __init__(self, root):
        self.root = Path(root)
        self.dir = STATE / "repos" / safe(str(self.root).strip("/").replace("/", "-"))
        self.cfg = load_config(self.dir)
        self.own = set(read_json(self.dir / "config.json", {}))  # the keys this repo sets itself
        if (self.dir / "config.json").exists() and not (self.cfg.get("triage_migrated") and
                                                         self.cfg.get("stops_migrated") and
                                                         self.cfg.get("layered")):
            if not self.cfg.get("triage_migrated"):
                migrate_triage(self)
            if not self.cfg.get("stops_migrated"):
                migrate_stops(self)
            if not self.cfg.get("layered"):
                self.migrate_layers()
            self.save()

    def migrate_layers(self):
        """A config.json from before the global one held every setting, the defaults too.
        Its values that are just the built-in default are dropped, so the global ones apply."""
        inherited = inherited_config()
        for k in list(self.own):
            if k in DEFAULT_CONFIG and k not in REPO_ONLY and self.cfg.get(k) == DEFAULT_CONFIG[k]:
                self.unset(k, inherited)

    def set(self, key, value):
        """This repo's own value for a setting, over the global one."""
        self.cfg[key] = value
        self.own.add(key)

    def unset(self, key, inherited=None):
        """Drop this repo's own value for a setting: the global one (or the default) applies."""
        inherited = inherited_config() if inherited is None else inherited
        self.own.discard(key)
        if key in inherited:
            self.cfg[key] = inherited[key]
        else:
            self.cfg.pop(key, None)

    def save(self):
        self.cfg["root"] = str(self.root)
        self.cfg["triage_migrated"] = True  # tasks made from here on know triage
        self.cfg["stops_migrated"] = True  # and record their stops
        self.cfg["layered"] = True  # and keep only their own settings: the rest comes from the global config
        # Only what this repo sets itself is written: the rest comes from the global
        # config.json and the defaults, as they are each time they're read.
        inherited = inherited_config()
        own = {k: v for k, v in self.cfg.items()
               if k in self.own or k in REPO_ONLY or k in INTERNAL or k not in inherited}
        self.own = set(own)
        write_json(self.dir / "config.json", own)

    def gh(self):
        return os.environ.get("BULLPEN_GH") or self.cfg.get("gh") or "gh"

    def has_origin(self):
        """No origin remote means local mode: tasks merge into the local base branch."""
        return git(self.root, "remote", "get-url", "origin", check=False).returncode == 0

    def base(self):
        if self.cfg.get("base"):
            return self.cfg["base"]
        p = git(self.root, "symbolic-ref", "--short", "refs/remotes/origin/HEAD", check=False)
        if p.returncode == 0:
            return p.stdout.strip().split("/", 1)[1]
        return git(self.root, "branch", "--show-current").stdout.strip() or "main"

    def task_path(self, tid):
        return self.dir / "tasks" / f"{tid}.json"

    def task(self, tid):
        tid = self.norm(tid)
        t = read_json(self.task_path(tid))
        if t is None:
            raise Fail(f"no task {tid} in {home(self.root)}: `bullpen ls --all` lists them")
        return t

    def norm(self, tid):
        tid = str(tid).upper()
        return tid if "-" in tid else f"{self.cfg['prefix']}-{tid}"

    def save_task(self, t):
        t["updated"] = now()
        old = read_json(self.task_path(t["id"])) or {}
        if old.get("status") != t["status"]:
            t["note"] = None  # what it needed then, not now
            t.pop("stuck", None)
            t.pop("auto_wait", None)  # only an approval that is still current accepts it
        if old.get("status") != t["status"] or not t.get("since"):
            # when it entered its current status; fine-grained so `go --next` can order it
            t["since"] = dt.datetime.now().astimezone().isoformat(timespec="microseconds")
        write_json(self.task_path(t["id"]), t)
        load_savers()
        for fn in SAVED:
            fn(self, t, old)

    def tasks(self):
        d = self.dir / "tasks"
        out = [read_json(p) for p in d.glob("*.json")] if d.exists() else []
        return sorted(out, key=lambda t: int(t["id"].rsplit("-", 1)[1]))

    def worktree_dir(self, t):
        return self.root.parent / f"{self.root.name}-bullpen" / t["id"]


# Before triage, every stop was "needs". Reasons the coordinator can see to become
# triage, once per repo; prompts stay needs.
TRIAGE_WHY = ("Failed", "Ended its turn", "Review findings didn't reach", "PR ",
              "Couldn't be continued", "Reviewer stopped", "Reviewer exited")


def migrate_triage(repo):
    for p in sorted((repo.dir / "tasks").glob("*.json")):
        t = read_json(p) or {}
        if t.get("status") == "needs" and (t.get("why") or "").startswith(TRIAGE_WHY):
            t["status"] = "triage"
            write_json(p, t)  # not save_task: when it stopped stays as it was


# Before stops were recorded, a task's why said who stopped and how; once per repo,
# stopped tasks get the stop their why says. Longer prefixes first.
OLD_STOPS = (("Reviewer needs you", "reviewer", "prompt"), ("Reviewer stopped", "reviewer", "ended"),
             ("Reviewer exited", "reviewer", "exited"), ("Needs you", "worker", "prompt"),
             ("Failed", "worker", "failed"), ("Ended its turn", "worker", "ended"),
             ("Agent exited", "worker", "exited"), ("Its pane was gone", "worker", "pane_gone"),
             ("Review findings didn't reach", "worker", "not_sent"), ("PR ", "worker", "not_sent"),
             ("Couldn't be continued", "worker", "limit"))


def stop_from_why(why):
    """The stop an old task's why says it had, or None."""
    if not why:
        return None
    for prefix, by, kind in OLD_STOPS:
        if why.startswith(prefix):
            return {"by": by, "kind": kind}
    return {"by": "worker", "kind": "other"}


def migrate_stops(repo):
    for p in sorted((repo.dir / "tasks").glob("*.json")):
        t = read_json(p) or {}
        if t.get("status") not in STOPPED or t.get("stop"):
            continue
        # an asked task's why is the question: ask_of kept what stopped it
        s = stop_from_why(t.get("ask_of") if t.get("ask") else t.get("why"))
        t.pop("ask_of", None)
        if s:
            t["stop"] = s
        write_json(p, t)


# ---------- task lifecycle ----------
#
# A task's status changes only through set_status. A stopped task (STOPPED) has a
# stop, {"by": "worker" or "reviewer", "kind": ...}, saying who stopped and how:
#   prompt     at a prompt: it needs the person
#   failed     its turn failed
#   ended      it ended its turn without ready (a reviewer: without a verdict)
#   exited     its pane closed before ready (a reviewer: before a verdict)
#   pane_gone  its pane wasn't found again after a Rex restart (a worker is stopped)
#   not_sent   review findings or PR feedback didn't reach the worker
#   limit      it couldn't be continued after the usage limit
#   setup      its worktree's setup failed, so its worker never started
#   other      a stop from before stops were recorded, of no known kind
# why is the stop in words, for people and the board: nothing goes by its wording.
# ask and ok are said of the status they were set in, and end with it.


def set_status(t, status, why=None, stop=None):
    """The one place a task's status changes. A new status ends ask and ok. stop: why
    it stopped now; a task that stays stopped without a new one (an ask, an escalation)
    keeps the one it had."""
    if status != t.get("status"):
        t.pop("ok", None)
        t.pop("ask", None)
        t.pop("asked_from", None)
    t.update(status=status, why=why)
    if stop:
        t["stop"] = stop
    elif status not in STOPPED:
        t.pop("stop", None)


def ask(t, question):
    """The task needs the person, for question. A stopped task keeps its stop: whether
    its worker is mid-turn still goes by it. One that wasn't stopped (working, in
    review, ready...) remembers its status and why (asked_from): the answer puts them
    back."""
    was, why, asked_from = t["status"], t.get("why"), t.get("asked_from")
    set_status(t, "needs", question)
    t["ask"] = True
    if was == "needs":
        if asked_from:
            t["asked_from"] = asked_from  # asked again: still from where it was first
    elif was not in STOPPED:
        t["asked_from"] = {"status": was, "why": why}


def ok(t):
    """Its coordinator saw to the task's stop: back to working, or to review for its
    reviewer's. ok: the idle agent isn't a stop again until its next turn."""
    set_status(t, "review" if reviewers_stop(t) else "working")
    t["ok"] = True


def back_to_work(t):
    """The task's worker is at it again, and has to run ready again. Its approval stays on
    record: that ready keeps it if the code didn't change (approval_kept)."""
    t.pop("from_ready", None)
    set_status(t, "working")


def reviewers_stop(t):
    """Is the task's stop its reviewer's? (One that couldn't be continued after the usage
    limit is seen to as its worker's.)"""
    s = t.get("stop") or {}
    return s.get("by") == "reviewer" and s.get("kind") != "limit"


def at_prompt(t):
    """Is the task's worker stopped at a prompt?"""
    return t.get("stop") == {"by": "worker", "kind": "prompt"}


def worker_idle(t):
    """Is the worker between turns, so a message typed into it lands as a new turn?
    Not when it's blocked on a prompt, or failed mid-turn."""
    if t["status"] not in WAITING:
        return True
    return not (at_prompt(t) or (t.get("stop") or {}).get("kind") in ("failed", "limit"))


# ---------- panes bullpen knows (<state>/blocks), and finding the repo ----------


def index_block(block, repo, role):
    # the watcher writes what the pane's agent is doing into doing/, for the board
    (STATE / "doing").mkdir(parents=True, exist_ok=True)
    # stuck_after is a copy of the repo's setting, where the watcher can read it
    write_json(STATE / "blocks" / f"{safe(block)}.json",
               {"repo": str(repo.root), "role": role, "stuck_after": repo.cfg["stuck_after"]})


def reindex(repo):
    """Give the repo's panes its settings again (after `config` changed them)."""
    for p in (STATE / "blocks").glob("*.json"):
        entry = read_json(p) or {}
        if entry.get("repo") == str(repo.root):
            write_json(p, {**entry, "stuck_after": repo.cfg["stuck_after"]})


def mark_compacting(block, on):
    """Note in the pane's index entry that its agent is compacting, so the watcher
    passes on its next working event (the one that may start a real turn)."""
    path = STATE / "blocks" / f"{safe(block)}.json"
    entry = read_json(path) if block else None
    if entry and bool(entry.get("compacting")) != on:
        entry.pop("compacting", None)
        write_json(path, {**entry, "compacting": True} if on else entry)


def unindex_block(block):
    if block:
        (STATE / "blocks" / f"{safe(block)}.json").unlink(missing_ok=True)
        (STATE / "doing" / f"{safe(block)}.json").unlink(missing_ok=True)


def lookup_block(block):
    entry = read_json(STATE / "blocks" / f"{safe(block)}.json") if block else None
    if not entry:
        return None, None
    return Repo(entry["repo"]), entry["role"]


def find_repo(from_block=None):
    """The repo a command is about: the one owning from_block, else the one we're in,
    else (for key bindings, which run in the Rex server's directory) the bullpen repo
    whose coordinator most recently started."""
    repo, _ = lookup_block(from_block)
    if repo:
        return repo
    try:
        repo = Repo(repo_root())
        if (repo.dir / "config.json").exists():
            return repo
    except Fail:
        pass
    best = None
    for cfg in (STATE / "repos").glob("*/config.json"):
        for c in load_config(cfg.parent)["coordinators"]:
            if not c.get("block"):
                continue
            idx = STATE / "blocks" / f"{safe(c['block'])}.json"
            entry = read_json(idx)
            if entry and (best is None or idx.stat().st_mtime > best[0]):
                best = (idx.stat().st_mtime, entry["repo"])
    if best:
        return Repo(best[1])
    raise Fail("no bullpen here: run `bullpen up` in a repository first")


def known_repos():
    """Every repository bullpen has state for (<state>/repos/*) whose checkout is still
    there, by state folder."""
    out = []
    for cfg in sorted((STATE / "repos").glob("*/config.json")):
        try:
            root = (read_json(cfg, {}) or {}).get("root")
        except ValueError:
            continue  # a config.json caught half-written, or broken: that repo's own commands say so
        if root and Path(root).is_dir():
            out.append(Repo(root))
    return out


# ---------- a task in words, and its own settings ----------


def pr_short(url):
    m = re.search(r"/pull/(\d+)", url or "")
    return f"PR #{m.group(1)}" if m else (url or "")


def where(t):
    """What to call the task's work in news: its PR, else "" (a local branch, in a repo
    with no remote, goes without saying)."""
    return pr_short(t["pr"]) if t.get("pr") and not t.get("local") else ""


def own_model(repo, t):
    """The worker model a task chose for itself, if it isn't the config's."""
    m = t.get("worker_model")
    return m if m and m != repo.cfg.get("worker_model") else None


def set_auto_accept(t, on):
    """The task's own auto-accept (on: True or False), over the config's; None leaves it."""
    if on is not None:
        t["auto_accept"] = on


def auto_accepts(repo, t):
    own = t.get("auto_accept")
    return bool(repo.cfg.get("auto_accept") if own is None else own)


def stamp(t):
    """When it entered its current status."""
    return t.get("since") or t.get("updated")


# The board's group for each status, so `ls` uses the same words.
BOARD_WORDS = {"needs": "needs you", "stopped": "needs you", "triage": "coordinator on it",
               "ready": "ready to accept", "finished": "finished", "review": "in review",
               "working": "working", "todo": "queued", "done": "closed", "dropped": "closed"}


def status_words(status):
    """The board's words for a status, with the status itself when they don't say it:
    "needs you", "needs you (stopped)", "coordinator on it (triage)"."""
    words = BOARD_WORDS.get(status, status)
    return words if status in words else f"{words} ({status})"


def is_status(t):
    """ "BP-3 is queued (todo)", "BP-3 needs you (stopped)": a task's status in the
    board's words, for errors."""
    words = status_words(t["status"])
    if words.startswith("coordinator"):
        return f"{t['id']}'s {words.replace(' on it', ' is on it', 1)}"
    return f"{t['id']} {words}" if words.startswith("needs") else f"{t['id']} is {words}"


def homes(text):
    """Text with each path under $HOME written with ~, for people to read."""
    h = str(Path.home())
    return text if h == "/" else re.sub(re.escape(h) + r"(?![\w.-])", "~", text)


def home(path):
    """A path with $HOME as ~, for people to read."""
    path, h = str(path), str(Path.home())
    return "~" + path[len(h):] if h != "/" and (path == h or path.startswith(h + "/")) else path


def listed(tasks, args):
    """Of a repo's tasks, the ones `ls` lists: the open ones, or all with --all."""
    return [t for t in tasks if args.all or t["status"] not in CLOSED]
