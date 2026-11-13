"""The core of bullpen: repo, config and tasks, the task lifecycle, Rex, news, review,
accept and the watcher's events. Stage 2 of the split (BP-79) breaks it up; until then
the feature modules beside it import what they need from here.
"""

import argparse
import contextlib
import datetime as dt
import difflib
import fcntl
import hashlib
import json
import os
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
import unicodedata
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
            "new workers as capsules in the coordinator's tab (float) or as tabs", choices=("float", "tabs")),
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
    Setting("board_bottom_gap", 1, "Board", "number",
            "empty rows at the board's bottom, under Rex's minimized-pane capsules (0: none)",
            least=0, says="rows, or 0 for none", metavar="ROWS"),
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


def read_json(path, default=None):
    try:
        return json.loads(Path(path).read_text())
    except FileNotFoundError:
        return default


def write_json(path, data):
    path = Path(path)
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


def task_base_ref(repo, t):
    """What the task's branch is compared with: origin's copy of its base branch when
    there is one, else the local base branch (local mode)."""
    if repo.has_origin() and git(t["worktree"], "rev-parse", "-q", "--verify", f"origin/{t['base']}",
                                 check=False).returncode == 0:
        return f"origin/{t['base']}"
    return t["base"]


def seconds_since(stamp, at=None):
    at = at or dt.datetime.now().astimezone()
    return (at - dt.datetime.fromisoformat(stamp)).total_seconds()


def clip(text, n=300):
    text = " ".join((text or "").split())
    return text if len(text) <= n else text[: n - 1] + "…"


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
        if followed(t):
            mark_following()
        notify(self, t, old)

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
#   pane_gone  its pane wasn't found again after a Rex restart
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


def stop(repo, t, by, kind, why, lines=(), event="stopped"):
    """The task's worker or reviewer (by) stopped (kind; why in words). A prompt needs
    the person. A worker whose pane is gone is stopped: resume restarts it. Anything
    else waits on its coordinator, or on the person when no coordinator is running to
    see to it. Saves the task, then sends each of lines as news, worded for whoever has
    it now. Returns the task's new status."""
    if kind == "prompt":
        t.pop("ask", None)  # the prompt is what it needs the person for now
        status, say = "needs", (lambda line: line)
    elif by == "worker" and kind in ("exited", "pane_gone"):
        status, say = "stopped", (lambda line: line)
    elif sees_to(repo, t):
        m = triage_minutes(repo)
        status, say = "triage", (lambda line: f"For the coordinator: {line} (still here in {m} min, "
                                              f"it goes to the person)")
    else:
        status, say = "needs", (lambda line: f"{line}. No coordinator is running to see to it, "
                                             f"so it needs the person")
    set_status(t, status, why, {"by": by, "kind": kind})
    repo.save_task(t)
    for line in [lines] if isinstance(lines, str) else lines:
        news(repo, t, event, say(line))
    return status


def escalate(repo, t, late):
    """Pass a task its coordinator didn't settle on to the person, with one news line.
    late: (what its why gets, why it goes now), from overdue."""
    why = (t.get("why") or "Stopped") + late[0]
    set_status(t, "needs", why)
    repo.save_task(t)
    news(repo, t, "needs-you", f"{t['id']} needs the person: {why}")


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


def reviewers_stop(t):
    """Is the task's stop its reviewer's? (One that couldn't be continued after the usage
    limit is seen to as its worker's.)"""
    s = t.get("stop") or {}
    return s.get("by") == "reviewer" and s.get("kind") != "limit"


def on_reviewer(t):
    """Is the task stopped on its reviewer (a prompt, or no verdict)?"""
    return t["status"] in WAITING and reviewers_stop(t)


def at_prompt(t):
    """Is the task's worker stopped at a prompt?"""
    return t.get("stop") == {"by": "worker", "kind": "prompt"}


def worker_idle(t):
    """Is the worker between turns, so a message typed into it lands as a new turn?
    Not when it's blocked on a prompt, or failed mid-turn."""
    if t["status"] not in WAITING:
        return True
    return not (at_prompt(t) or (t.get("stop") or {}).get("kind") in ("failed", "limit"))

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


# ---------- Rex ----------

def with_state(argv):
    """Panes Rex starts don't inherit our environment: carry bullpen's own settings
    (BULLPEN_BOARD too, so a board runs the program doctor and `up` expect of it)."""
    env = [f"{k}={os.environ[k]}" for k in ("BULLPEN_STATE", "BULLPEN_GH", "BULLPEN_REX", "BULLPEN_BOARD")
           if os.environ.get(k)]
    return ["env", *env, *argv] if env else argv


def rex_run(session, cwd, label, argv, focus=False):
    cmd = [REX, "run", "--json", f"--focus={'true' if focus else 'false'}", "--label", label,
           "-c", str(cwd)]
    if session:
        cmd += ["-s", session]
    p = run([*cmd, "--", *with_state(argv)])
    return json.loads(p.stdout)["block_ids"][0]


def rex_alive(session, block):
    if not block:
        return False
    cmd = [REX, "block", "inspect", block] + (["-s", session] if session else [])
    return run(cmd, check=False).returncode == 0


def rex_type(session, block, text):
    """Type a message (a line, or several) into a terminal and press Enter."""
    s = ["-s", session] if session else []
    run([REX, "send", *s, "-b", block, text])
    run([REX, "send-key", *s, "-b", block, "enter"])


def type_to_worker(t, msg):
    """Type msg into the task's worker. None if it went through, else why not
    (the caller leaves the task where it was and says so)."""
    s = ["-s", t["session"]] if t.get("session") else []
    for cmd in ([REX, "send", *s, "-b", t["block"], msg], [REX, "send-key", *s, "-b", t["block"], "enter"]):
        p = run(cmd, check=False)
        if p.returncode != 0:
            return clip(" ".join((p.stderr or p.stdout).split()) or "typing into its pane failed", 160)
    return None


def rex_focus(session, block):
    run([REX, "focus", *(["-s", session] if session else []), block])


def rex_api(session, method, args=None, check=True):
    """Call a Rex server API method; -s fills in session_id."""
    cmd = [REX, "api", "call", *(["-s", session] if session else []), method, json.dumps(args or {})]
    p = run(cmd, check=check)
    if p.returncode != 0 or not p.stdout.strip():
        return None
    return json.loads(p.stdout)


def rex_view(session):
    """The session's windows, layers and blocks, or None if Rex can't say."""
    v = rex_api(session, "session.view", check=False)
    return v if v and "windows" in v else None


# A worker's or reviewer's floating pane says how to put it away (lua/keys.lua's
# default key). Its label is then "bullpen: BP-3 · ctrl+esc hides"; it's found by
# "bullpen: BP-3" all the same, as are panes started before the hint.
HIDE_HINT = " · ctrl+esc hides"


def is_label(b, label):
    return bool(label) and b.get("label") in (label, label + HIDE_HINT)


def find_pane(view, block=None, label=None):
    """(window_id, layer, block_id) of a placed block, by ID, else by label."""
    for w in (view or {}).get("windows", []):
        for layer in w.get("layers", []):
            for b in layer.get("blocks", []):
                if (block and b.get("block_id") == block) or (not block and is_label(b, label)):
                    return w["window_id"], layer, b["block_id"]
    return None


def find_panes(view, label):
    """(window_id, layer, block_id) of every placed block with this label."""
    return [(w["window_id"], layer, b["block_id"])
            for w in (view or {}).get("windows", []) for layer in w.get("layers", [])
            for b in layer.get("blocks", []) if is_label(b, label)]


def set_layer(session, layer_id, state, focus=False):
    args = {"layer_id": layer_id, "state": state}
    if focus:
        args["focus"] = True
    return rex_api(session, "session.set_layer_state", args, check=False) is not None


def rex_close(session, block):
    if block:
        run([REX, "block", "close", block] + (["-s", session] if session else []), check=False)


def pr_short(url):
    m = re.search(r"/pull/(\d+)", url or "")
    return f"PR #{m.group(1)}" if m else (url or "")


def where(t):
    """What to call the task's work in news: its PR, else "" (a local branch, in a repo
    with no remote, goes without saying)."""
    return pr_short(t["pr"]) if t.get("pr") and not t.get("local") else ""


NEWS_MARK = "bullpen · "


def ready_news(repo, t, facts=(), then="", again=False):
    """The news that the task is ready: `BP-3 is ready: <title> (<facts>)<then>`, the
    title second, where a long line is cut last. A task that was ready before is
    "ready again", "after a rebase" if its base moved since; a line word for word the
    one before it about the task isn't sent again."""
    wt = t.get("worktree")
    base = merge_base(wt, task_base_ref(repo, t)) if wt and Path(wt).is_dir() else None
    told = t.get("ready_told")
    if told is not None or again:
        again = " again after a rebase" if told and base and told != base else " again"
    t["ready_told"] = base or ""
    write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change here
    facts = ", ".join(f for f in facts if f)
    text = f"{t['id']} is ready{again or ''}: {t['title']}" + (f" ({facts})" if facts else "") + then
    if last_news(repo, t["id"]) == NEWS_MARK + " ".join(text.split()):
        return
    news(repo, t, "ready", text)


def last_news(repo, tid):
    """The last line of news about task tid, as it was logged, or None."""
    log = repo.dir / "news.log"
    if not log.exists():
        return None
    with log.open("rb") as f:
        f.seek(max(0, log.stat().st_size - 65536))
        lines = f.read().decode(errors="replace").splitlines()
    for line in reversed(lines):
        m = re.match(r"\S+(?: \[[^\]]*\])? (\S+) [^ :]+: (.*)$", line)
        if m and m.group(1) == tid:
            return m.group(2)
    return None


def news(repo, task, event, text, everyone=False, skip=None, typed=None, to=None, more=()):
    """One line of news, `bullpen · <what happened>`, for the coordinator that owns the
    task (everyone=True: for all of them, as news that isn't about one task is): logged,
    and typed into their prompts unless they follow the log (config --news monitor).
    skip: a coordinator not to tell (the one that ran the command). typed: what to type
    in place of the line (the person's own words, not bullpen's). to: the IDs of the
    coordinators it is for, whoever owns what. more: lines that go after it, as they
    are (a diff): logged after it, each starting with a tab, and typed with it as one
    message."""
    line = NEWS_MARK + " ".join(text.split())  # one line: a newline would send it early
    to, waits = (to, False) if to else news_to(repo, task, everyone)
    if skip and to and skip in to:
        to = [cid for cid in to if cid != skip]
        if not to:
            return  # only the coordinator that did it would hear it
    log = repo.dir / "news.log"
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open("a") as f:
        f.write(f"{now()}{' [' + ','.join(to) + ']' if to else ''} {task['id']} {event}: {line}\n"
                + "".join(f"\t{l}\n" for l in more))  # one write: a reader never sees half
    if repo.cfg.get("news") != "monitor":
        # With no coordinator alive the line waits once, in the shared outbox: one copy
        # per coordinator would reach the first to come back once for each.
        said = " ".join(typed.split()) if typed else line
        type_news(repo, LINE_BREAK.join([said, *more]), None if waits else to)


def news_to(repo, task, everyone=False):
    """(IDs, waits): the coordinators a line of news is for, those who see to the task
    (every live one, with everyone); with none alive, all of them (waits is True: it
    has to wait for one to come back). ([], True) when the repo has none."""
    cs = repo.cfg["coordinators"]
    to = [c for c in cs if alive(c)] if everyone else sees_to(repo, task)
    return [c["id"] for c in (to or cs)], not to


# An outbox holds one message a line; a message of several lines (news with a diff
# after it) keeps its line breaks as this, and gets them back when it is typed. Claude
# Code takes a newline that comes in a typed burst as part of the message (a paste), and
# only the Enter after it sends it.
LINE_BREAK = "\x1f"


def outbox_path(repo, cid=None):
    """Typed news waiting for coordinator cid. Without one: news that came while no
    coordinator was alive (or there was none), for every one alive when it's typed."""
    return repo.dir / (f"outbox-{safe(cid)}.txt" if cid else "outbox.txt")


def outboxes(d):
    return sorted(d.glob("outbox*.txt"))


def type_news(repo, line, to=None):
    """Type a line into these coordinators' prompts, now or once their input is empty."""
    repo.dir.mkdir(parents=True, exist_ok=True)
    for cid in to or [None]:
        with outbox_path(repo, cid).open("a") as f:
            f.write(line + "\n")
    if not deliver(repo):
        (STATE / "outbox").touch()  # the watcher retries while this exists


# ---------- notifications for the person ----------
#
# A task that comes to need the person (a prompt, `ask`, a triage escalated), and with
# notify "all" one that is ready, finished or auto-accepted, gets a native notification:
# OSC 777 written to the terminal of the task's pane (else its board's, else its
# coordinator's), which Rex for Mac shows, and opens that pane's tab when clicked. It
# goes out when the change is saved (save_task), so once per change, from whatever made
# it: mostly the watcher's calls, and `ask`, `ready`, `verdict`.
#
# Several tasks that come to need the person at once (within NOTIFY_BURST seconds of
# a notification for another task) get one more between them, not one each: they wait
# in notify.json, and once none has come for NOTIFY_QUIET seconds the watcher's
# outbox round (`_flush`) sends "3 tasks need you: BP-3, BP-4, BP-5".

NOTIFY_BURST = 10  # seconds
NOTIFY_QUIET = 3


def notice(t, old):
    """(kind, text) of the notification the task's change from old (as it was saved)
    deserves, kind "needs" or "news", else None."""
    s, was = t["status"], old.get("status")
    asked = t.get("ask") and not (old.get("ask") and old.get("why") == t.get("why"))
    if s == "needs" and (was != "needs" or asked):
        why = t.get("why") or "it is stopped"
        m = re.match(r"(Reviewer )?[Nn]eeds you\b:?\s*", why)
        if m:
            who = f"{t['id']}'s reviewer" if m.group(1) else t["id"]
            rest = why[m.end():]
            return "needs", f"{who} needs you" + (f" {rest}" if rest.startswith("(") else f": {rest}" if rest else "")
        return "needs", f"{t['id']} needs you: {why}"
    if s == was:
        return None
    if s == "ready":
        return "news", f"{t['id']} is ready: {t['title']}"
    if s == "finished":
        return "news", f"{t['id']} finished: {t['title']}"
    if s == "done" and t.get("auto_accepted"):
        return "news", f"{t['id']} was auto-accepted: {t['title']}"
    return None


def notify(repo, t, old):
    """Tell the person of the task's change, if notify says to and they aren't looking.
    Never fails: a notification that can't go out is noted in notify.log."""
    n = notice(t, old)
    level = repo.cfg["notify"]
    if not n or level == "off" or (level == "needs" and n[0] != "needs"):
        return
    try:
        if looking(repo, t):
            return log_notify(repo, t, n[1], "not sent: the person is looking")
        if n[0] == "needs" and held(repo, t, n[1]):
            return log_notify(repo, t, n[1], "held: another task needs you too")
        send_notification(repo, t, n[1])
    except (OSError, ValueError, subprocess.SubprocessError) as e:
        log_notify(repo, t, n[1], f"not sent: {e}")


def send_notification(repo, t, text):
    title = f"{NEWS_MARK}{repo.root.name}"
    for session, block in notify_panes(repo, t):
        tty = pane_tty(session, block)
        if tty and send_osc(tty, title, text):
            return log_notify(repo, t, text, f"sent to {block}")
    log_notify(repo, t, text, "not sent: no pane to send it from")


@contextlib.contextmanager
def notify_state(repo):
    """notify.json, locked against the other bullpen processes the same burst runs:
    {"at": when the last went out, "ids": the tasks it and those held since were
    for, "held": [[ID, text], ...] waiting, "last": when the last was held}."""
    repo.dir.mkdir(parents=True, exist_ok=True)
    with (repo.dir / "notify.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        path = repo.dir / "notify.json"
        st = read_json(path, {}) or {}
        yield st
        write_json(path, st)


def held(repo, t, text):
    """Hold a task's needs-you notification if one for another task went out in the
    last NOTIFY_BURST seconds; else note that this one goes out now. True if held."""
    with notify_state(repo) as st:
        clock = time.time()
        ids = st.get("ids") or []
        if clock - st.get("at", 0) < NOTIFY_BURST and any(i != t["id"] for i in ids):
            st["held"] = [h for h in st.get("held") or [] if h[0] != t["id"]] + [[t["id"], text]]
            st.update(ids=ids + [t["id"]] * (t["id"] not in ids), last=clock)
            (STATE / "outbox").touch()  # the watcher's outbox round sends it
            return True
        if clock - st.get("at", 0) >= NOTIFY_BURST:
            ids = []
        st.update(at=clock, ids=ids + [t["id"]] * (t["id"] not in ids))
        return False


def flush_notifications(repo):
    """Send the held notifications once none has come for NOTIFY_QUIET seconds: one,
    for all the tasks of the burst. False while some still wait."""
    if not (repo.dir / "notify.json").exists():
        return True
    with notify_state(repo) as st:
        hold = st.get("held") or []
        if not hold:
            return True
        if time.time() - st.get("last", 0) < NOTIFY_QUIET:
            return False
        ids = st.get("ids") or []
        text = hold[0][1] if len(hold) == 1 else f"{len(ids)} tasks need you: {', '.join(ids)}"
        st.update(held=[], at=time.time())
    try:
        t = repo.task(hold[-1][0])
        send_notification(repo, t, text)
    except (Fail, OSError, ValueError, subprocess.SubprocessError) as e:
        with (repo.dir / "notify.log").open("a") as f:
            f.write(f"{now()} {hold[-1][0]} not sent: {e}: {text}\n")
    return True


def notify_panes(repo, t):
    """(session, block) of the panes a notification can come from, best first: the task's
    reviewer if it is what needs the person, its worker, its coordinator's board, its
    coordinator. Clicking it opens the pane's tab."""
    r = t.get("review") or {}
    c = home_of(repo, t)
    b = c.get("board") or {}
    out = [(r.get("session") or t.get("session"), r.get("block")) if on_reviewer(t) else (None, None),
           (t.get("session"), t.get("block")), (b.get("session"), b.get("block")),
           (c.get("session"), c.get("block"))]
    return [p for p in out if p[1]]


def looking(repo, t):
    """Has the task's worker or reviewer pane, or its coordinator's board, focus in its
    session's active window? Then the person sees it already. (Rex can't say whether
    its app is in front, so this goes by the session alone.)"""
    r = t.get("review") or {}
    b = home_of(repo, t).get("board") or {}
    panes = [(t.get("session"), t.get("block")), (r.get("session") or t.get("session"), r.get("block")),
             (b.get("session"), b.get("block"))]
    blocks = {blk for _, blk in panes if blk}
    for session in {s for s, blk in panes if blk}:
        v = rex_view(session) or {}
        for w in v.get("windows", []):
            if w.get("window_id") == v.get("active_window_id") and w.get("focused_block_id") in blocks:
                return True
    return False


def pane_tty(session, block):
    """The terminal device of a pane's program, or None."""
    p = run([REX, "block", "call", *(["-s", session] if session else []), "-b", block, "process"], check=False)
    if p.returncode != 0 or not p.stdout.strip():
        return None
    pid = (json.loads(p.stdout).get("child") or {}).get("pid")
    if not pid:
        return None
    tty = run(["ps", "-o", "tty=", "-p", str(pid)], check=False).stdout.strip()
    return None if not tty or tty.startswith("?") else (tty if tty.startswith("/") else f"/dev/{tty}")


def send_osc(tty, title, body):
    """Write a notification (OSC 777) to a terminal device. False if it can't be opened."""
    def clean(text, n, also=""):
        return clip(re.sub(f"[\\x00-\\x1f\\x7f{also}]", " ", text), n)
    # the title ends at the first ";": the body may have them
    data = f"\x1b]777;notify;{clean(title, 60, ';')};{clean(body, 240)}\x07".encode()
    try:
        fd = os.open(tty, os.O_WRONLY | os.O_NOCTTY | os.O_NONBLOCK)
    except OSError:
        return False
    try:
        os.write(fd, data)  # in one write, so it lands between two of the program's own
    finally:
        os.close(fd)
    return True


def log_notify(repo, t, text, how):
    with (repo.dir / "notify.log").open("a") as f:
        f.write(f"{now()} {t['id']} {how}: {text}\n")


def calling_coordinator(repo):
    """The coordinator whose own pane ran this command, or None. Rex gives each pane
    its block ID as REX_BLOCK. Unsure (no REX_BLOCK, or a stored block that is out of
    date) counts as none."""
    return coordinator_at(repo, os.environ.get("REX_BLOCK"), boards=False)


def tell_coordinator(repo, task, event, text):
    """News of something the person may have done elsewhere (the board, a shell).
    The coordinator whose pane ran it isn't told: it would only repeat what it just ran."""
    me = calling_coordinator(repo)
    news(repo, task, event, text, skip=me and me["id"])


def has_draft(session, block):
    """Is something half-typed in the coordinator's input? Typing news there would
    send it along. (Reads Claude Code's input box: the last line starting ❯. Faint
    text there is Claude Code's suggestion or placeholder, not the person's: a
    suggestion can show for as long as the pane is idle, so it mustn't hold news.)"""
    p = run([REX, "capture", *(["-s", session] if session else []), "-b", block, "--format=vt"],
            check=False)
    state = {"faint": False}
    lines = [styled_line(l, state) for l in p.stdout.splitlines()]
    lines = [l for l in lines if "".join(t for t, _ in l).strip()]
    for line in reversed(lines[-10:]):
        text = "".join(t for t, _ in line).strip()
        if text.startswith("❯"):
            typed = "".join(t for t, faint in line if not faint)
            return bool(typed.split("❯", 1)[-1].strip())
    return False


VT_ESC = re.compile(r"\x1b\[([0-9;:]*)m|\x1b\[(\d*)C|\x1b\[[0-9;:?<=>]*[ -/]*[@-~]|\x1b[@-_]")


def styled_line(raw, state):
    """One line of a `rex capture --format=vt` screen as (text, faint) pieces. Faint
    (SGR 2) carries on from line to line until reset, so state is shared: pass the
    lines of one capture in order."""
    out, pos = [], 0
    for m in VT_ESC.finditer(raw):
        out.append((raw[pos:m.start()], state["faint"]))
        pos = m.end()
        if m.group(1) is not None:
            state["faint"] = sgr_faint(m.group(1), state["faint"])
        elif m.group(2) is not None:
            out.append((" " * int(m.group(2) or 1), state["faint"]))  # cursor forward: a gap
    out.append((raw[pos:].replace("\r", ""), state["faint"]))
    return [(t, f) for t, f in out if t]


def sgr_faint(params, faint):
    """Is text faint after this SGR (`\\x1b[<params>m`)?"""
    ps = params.split(";") if params else ["0"]
    i = 0
    while i < len(ps):
        p = ps[i].split(":")[0]
        if p in ("", "0", "22"):
            faint = False
        elif p == "2":
            faint = True
        elif p in ("38", "48", "58") and ":" not in ps[i] and i + 1 < len(ps):
            i += {"5": 2, "2": 4}.get(ps[i + 1], 1)  # a colour's own numbers
        i += 1
    return faint


def take_lines(path):
    lines = path.read_text().splitlines() if path.exists() else []
    path.unlink(missing_ok=True)
    return lines


def add_lines(path, lines):
    if lines:
        with path.open("a") as f:
            f.write("".join(l + "\n" for l in lines))


def deliver(repo):
    """Type each coordinator's waiting news into its pane, oldest first. News for a
    coordinator whose pane is gone goes to the live ones instead. False if some has
    to wait: no live coordinator, or the person is typing there."""
    files = outboxes(repo.dir)
    if not files:
        return True
    cs = repo.cfg["coordinators"]
    live = [c for c in cs if alive(c)]
    if not live:
        return False
    for path in files:
        cid = path.stem[len("outbox-"):] if path.stem != "outbox" else None
        if cid and any(c["id"] == cid for c in live):
            continue
        # shared, or for a coordinator that's gone: the live ones have it
        lines = take_lines(path)
        for c in live:
            add_lines(outbox_path(repo, c["id"]), lines)
    done = True
    for c in live:
        path = outbox_path(repo, c["id"])
        if not path.exists():
            continue
        if has_draft(c.get("session"), c["block"]):
            done = False
            continue
        for line in take_lines(path):
            rex_type(c.get("session"), c["block"], line.replace(LINE_BREAK, "\n"))
    return done


def cmd_flush(args):
    """(watcher) deliver news, and send notifications, that had to wait."""
    waiting = False
    for d in {p.parent for p in (STATE / "repos").glob("*/outbox*.txt")}:
        repo = Repo(read_json(d / "config.json", {}).get("root") or "/")
        repo.dir = d
        repo.cfg = load_config(d)
        if not deliver(repo):
            waiting = True
    for d in {p.parent for p in (STATE / "repos").glob("*/notify.json")}:
        repo = Repo(read_json(d / "config.json", {}).get("root") or "/")
        repo.dir = d
        repo.cfg = load_config(d)
        if not flush_notifications(repo):
            waiting = True
    if not waiting:
        (STATE / "outbox").unlink(missing_ok=True)


# ---------- coordinators ----------
#
# A repo can have several coordinators, each an agent in its own pane with its own
# board beside it. cfg["coordinators"] lists them, oldest (the primary) first:
# {"id": "c1", "block", "session", "board": {"block", "session"}, "board_others"}.
# Its board shows only its own tasks (and those with no owner, or a gone one) unless
# board_others (the board's `o`) shows the others' too. The ID stays when
# a Rex restart gives the pane a new block. A task belongs to the coordinator that
# added it (t["coordinator"], its ID). Who sees to it (sees_to) is that one alone while
# its pane is running, else every running coordinator: they get its news, its stops
# and its panes, and their boards show it. A coordinator whose pane closes is dropped
# (coordinator_gone). `bullpen adopt` moves a task to another.


def coordinator(repo, cid):
    return next((c for c in repo.cfg["coordinators"] if cid and c["id"] == cid), None)


def coordinator_at(repo, block, boards=True):
    """The coordinator whose pane is block (or whose board is, with boards)."""
    if not block:
        return None
    for c in repo.cfg["coordinators"]:
        if c.get("block") == block or (boards and (c.get("board") or {}).get("block") == block):
            return c
    return None


def on_board(repo, c, t):
    """Does coordinator c's board show the task? Only one c sees to, or one nobody sees
    to, while c's board_others is off. With one coordinator (or c None), every task."""
    if not c or c.get("board_others") or len(repo.cfg["coordinators"]) < 2:
        return True
    sees = sees_to(repo, t)
    return not sees or any(o["id"] == c["id"] for o in sees)


def alive(c):
    return bool(c) and rex_alive(c.get("session"), c.get("block"))


def owner(repo, t):
    """The task's coordinator, if it is still one (an older task may have none)."""
    return coordinator(repo, t.get("coordinator"))


def primary(repo):
    cs = repo.cfg["coordinators"]
    return cs[0] if cs else None


def sees_to(repo, t):
    """The coordinators who see to a task: its owner while the owner's pane is running,
    else every running coordinator, else none (the task needs the person)."""
    own = owner(repo, t)
    if alive(own):
        return [own]
    return [c for c in repo.cfg["coordinators"] if alive(c)]


def home_of(repo, t):
    """Where a task's panes live: with the coordinator who sees to it, else (none
    running) with its owner, else the primary coordinator."""
    sees = sees_to(repo, t)
    return sees[0] if sees else owner(repo, t) or primary(repo) or {}


def add_coordinator(repo, block, session):
    n = repo.cfg["next_coordinator"]
    c = {"id": f"c{n}", "block": block, "session": session, "board": None}
    repo.cfg["next_coordinator"] = n + 1
    repo.cfg["coordinators"].append(c)
    index_block(block, repo, "coordinator")
    return c


def prune(repo):
    """Forget coordinators whose panes are gone, and close their boards. Their tasks'
    news goes to the others."""
    for c in list(repo.cfg["coordinators"]):
        if not alive(c):
            drop_coordinator(repo, c)


def drop_coordinator(repo, c):
    """Forget coordinator c, whose pane is gone, and close its board."""
    b = c.get("board") or {}
    if b.get("block"):
        rex_close(b.get("session"), b["block"])
        unindex_block(b["block"])
    unindex_block(c.get("block"))
    repo.cfg["coordinators"].remove(c)
    print(f"coordinator {c['id']}: its pane is gone, so it is dropped")


GONE_WAIT = 5  # seconds before a look at whether a coordinator's pane really closed


def coordinator_gone(repo, c):
    """The watcher said coordinator c's pane is gone: a few seconds later, look in the
    background, and drop c if its pane really closed."""
    wait = os.environ.get("BULLPEN_GONE_WAIT")  # the tests: no wait, and wait for the look
    p = subprocess.Popen([SELF, "_coordinator_gone", c["id"], "--wait", wait or str(GONE_WAIT)],
                         cwd=repo.root, start_new_session=True, stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if wait:
        p.wait()


def cmd_coordinator_gone(args):
    """(background) Drop a coordinator whose pane closed: only while Rex still answers
    for its session but not for its pane, and has no pane of bullpen's coordinator
    label that no coordinator has. So a Rex shutdown or restart drops nobody: `up`
    finds the panes again."""
    time.sleep(args.wait)
    repo = Repo(repo_root())
    c = coordinator(repo, args.id)
    if not c or alive(c):
        return
    view = rex_view(c.get("session"))
    if view is None:
        return  # Rex is down, or restarted with a new session: nobody can tell
    taken = {o.get("block") for o in repo.cfg["coordinators"]}
    if any(h[2] not in taken for h in find_panes(view, coordinator_label(repo))):
        return  # a restart brought it back under a new ID
    drop_coordinator(repo, c)
    repo.save()


# ---------- news by monitor: `bullpen news [--follow]` ----------

def cursor_path(repo, cid):
    return repo.dir / (f"news-{safe(cid)}.cursor" if cid else "news.cursor")


def news_cursor(repo, cid=None):
    """How far into news.log (in bytes) coordinator cid has read with `bullpen news`,
    or None: never. (news.cursor: the one cursor from before several coordinators, or
    the reader when there's no coordinator; c1 carries it on.)"""
    for path in (cursor_path(repo, cid), *([cursor_path(repo, None)] if cid == "c1" else [])):
        try:
            return int(path.read_text())
        except (FileNotFoundError, ValueError):
            pass
    return None


def set_news_cursor(repo, offset, cid=None):
    repo.dir.mkdir(parents=True, exist_ok=True)
    path = cursor_path(repo, cid)
    tmp = path.with_name(f".{path.name}.tmp")
    tmp.write_text(f"{offset}\n")
    os.replace(tmp, path)


def news_size(repo):
    try:
        return (repo.dir / "news.log").stat().st_size
    except FileNotFoundError:
        return 0


def news_for(text, cid, listed):
    """Is a logged line for coordinator cid? Lines name the coordinators they're for
    as `[c1,c2]` before the task ID; lines without, or for coordinators no longer
    listed, are for everyone."""
    head = text.split(NEWS_MARK, 1)[0].split()
    to = next((w[1:-1].split(",") for w in head if w.startswith("[") and w.endswith("]")), None)
    return not cid or not to or cid in to or not any(x in listed for x in to)


def print_news(repo, offset, cid=None):
    """Print coordinator cid's news logged after offset, moving its cursor past each
    line once it is out (or skipped: another coordinator's). A line's own lines after
    it (each starting with a tab: a diff) go with it. Returns the new offset. A line
    still being written waits."""
    size = news_size(repo)
    if offset > size:  # the log was removed or cut short: what's there now is new
        offset = 0
        set_news_cursor(repo, 0, cid)
    if offset == size:
        return offset
    listed = {c["id"] for c in load_config(repo.dir)["coordinators"]}
    with (repo.dir / "news.log").open("rb") as f:
        f.seek(offset)
        for raws in news_records(f):
            text = raws[0].decode(errors="replace").rstrip("\n")
            at = text.find(NEWS_MARK)
            # Printing and moving the cursor go together: being stopped in between
            # (a monitor expiring) would print the line again next time.
            held = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM, signal.SIGINT, signal.SIGHUP})
            try:
                # a tab-led line with no line before it (a cursor left between them) is skipped
                if not text.startswith("\t") and news_for(text, cid, listed):
                    rest = [r.decode(errors="replace")[1:].rstrip("\n") for r in raws[1:]]
                    print("\n".join([text[at:] if at >= 0 else text, *rest]), flush=True)
                offset += sum(len(r) for r in raws)
                set_news_cursor(repo, offset, cid)  # after printing: if nobody was reading, it comes again
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, held)
    return offset


def news_records(f):
    """The whole news lines left in f, each with the tab-led lines that follow it, as
    lists of raw lines. One still being written isn't given."""
    rec = []
    for raw in f:
        if not raw.endswith(b"\n"):
            return
        if rec and not raw.startswith(b"\t"):
            yield rec
            rec = []
        rec.append(raw)
    if rec:
        yield rec


def news_reader(repo):
    """Whose news `bullpen news` prints: the coordinator in whose pane it runs, else
    the primary one. None (all news) when the repo has no coordinator."""
    c = calling_coordinator(repo) or primary(repo)
    return c and c["id"]


def cmd_news(args):
    """Print the news logged for this coordinator since the last time, once each;
    --follow keeps going. Each coordinator has its own cursor."""
    repo = Repo(repo_root())
    repo.dir.mkdir(parents=True, exist_ok=True)
    cid = news_reader(repo)
    if not_a_reader(repo, cid):
        if args.follow:  # the Monitor tool shows what's printed: say it there
            return print(NOT_A_COORDINATOR, flush=True)
        raise Fail(NOT_A_COORDINATOR.removeprefix(NEWS_MARK))
    lock = (repo.dir / (f"news-{safe(cid)}.lock" if cid else "news.lock")).open("w")
    try:
        # One reader at a time, or lines would come twice. A second follower waits
        # for the first to go (a monitor being replaced); a one-off read doesn't.
        fcntl.flock(lock, fcntl.LOCK_EX if args.follow else fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise Fail("news is being followed already (`bullpen news --follow` is running)")
    offset = news_cursor(repo, cid)
    if offset is None:  # first time: old news stays old
        offset = news_size(repo)
        set_news_cursor(repo, offset, cid)
    try:
        while True:
            if args.follow and not news_by_monitor(repo):
                # Typed news is logged too: following it would give every line twice.
                print(NEWS_MARK + "news is typed into your pane now (`bullpen config --news type`): "
                      "don't start the monitor again", flush=True)
                return
            if args.follow and not_a_reader(repo, cid):  # dropped while following
                return print(NOT_A_COORDINATOR, flush=True)
            offset = print_news(repo, offset, cid)
            if not args.follow:
                return
            time.sleep(args.every)
    except BrokenPipeError:  # whoever was reading is gone; the cursor stays before the lost line
        os.dup2(os.open(os.devnull, os.O_WRONLY), sys.stdout.fileno())
    except KeyboardInterrupt:
        pass


NOT_A_COORDINATOR = (NEWS_MARK + "this pane isn't one of this repo's coordinators, so it doesn't read "
                     "their news: don't start the monitor again (`bullpen up --here` here makes it one)")


def not_a_reader(repo, cid):
    """Is `bullpen news` running in a Rex pane other than coordinator cid's? Each line
    is printed once, so a reader there (an agent that used to be the coordinator, a
    shell) would take lines from the coordinator's own monitor. Outside Rex there's
    no telling: it reads the primary's news, as before."""
    block = os.environ.get("REX_BLOCK")
    if not block or not cid:
        return False
    repo.cfg = load_config(repo.dir)  # read again: it can change while we follow
    me = coordinator_at(repo, block, boards=False)
    return not me or me["id"] != cid


def news_by_monitor(repo):
    """Is the setting `monitor` right now? Read again: it can change while we follow."""
    return load_config(repo.dir).get("news") == "monitor"


def follow_cmd():
    return shlex.join(with_state([SELF, "news", "--follow"]))


def news_mode_line(repo):
    """One line saying how news reaches the coordinator."""
    if repo.cfg.get("news") == "monitor":
        return (f"news: by monitor. Nothing is typed: the coordinator must run the Monitor "
                f"tool on `{follow_cmd()}` (its guide says how).")
    return ("news: typed into the coordinator's pane "
            "(`bullpen config --news monitor`: by Claude Code's Monitor tool instead).")


def guide_text():
    """The coordinator's guide, with this bullpen's own path in its commands."""
    text = (PROMPTS / "coordinator.md").read_text()
    return text.replace("{{follow}}", follow_cmd())


def guide_file(repo):
    """The guide as a file, for the coordinator's system prompt."""
    path = repo.dir / "coordinator.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(guide_text())
    return path


# ---------- telling running coordinators that their guide changed ----------
#
# A coordinator gets its guide once, in its system prompt or by reading `bullpen guide`
# (`up --here`), and can't load it again. Each records the hash of the guide it was
# given (c["guide"]), and the text itself is kept in <state>/guides/<hash>.md: one
# folder for every repo, since they all run this bullpen's guide. `up` and `accept`
# (a merge or a pull may have changed prompts/coordinator.md) compare each running
# coordinator's hash, in every repo, with the guide now. A coordinator whose guide is
# out of date gets one news line, only it, with the diff after the line when it is
# short, and its hash moves on, so each change is told once. One from before hashes,
# or whose old text is gone, is told to read the guide again, with no diff.

GUIDES = STATE / "guides"
GUIDE_NEWS = {"id": "guide"}  # the news log's task column for these lines
GUIDE_DIFF_LINES = 40  # a longer diff is summed up by its sections instead


def guide_hash(text):
    return hashlib.sha256(text.encode()).hexdigest()


def give_guide(c, text):
    """Record that coordinator c has this guide: its hash, and the text, for the diff
    when it changes."""
    h = guide_hash(text)
    path = GUIDES / f"{h}.md"
    if not path.exists():
        GUIDES.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
        tmp.write_text(text)
        os.replace(tmp, path)
    c["guide"] = h


def old_guide(h):
    """The guide text with hash h, if it was kept."""
    try:
        return (GUIDES / f"{h}.md").read_text() if h else None
    except OSError:
        return None


def guide_sections(old, new):
    """The headings of the guide's sections that changed, in order: those around changed
    lines in the new guide, or in the old one for a section that's gone."""
    a, b = old.splitlines(), new.splitlines()

    def heading(lines, i):
        for l in reversed(lines[:i + 1]):
            if l.startswith("#"):
                return l.lstrip("#").strip()
        return "the top"

    out = []
    for op, i1, i2, j1, j2 in difflib.SequenceMatcher(None, a, b, autojunk=False).get_opcodes():
        if op == "equal":
            continue
        hs = [heading(b, j) for j in range(j1, max(j2, j1 + 1))] if b else []
        hs += [heading(a, i) for i in range(i1, i2) if a[i].startswith("#")]  # removed sections
        for h in hs:
            if h not in out:
                out.append(h)
    return out


def guide_news(old, new):
    """(line, lines after it) telling a coordinator given the guide old (None: not
    known) that it is now new."""
    follow = "follow it from now on"
    if old is None:
        return (f"Your coordinator guide may have changed since you were given it: run "
                f"`bullpen guide` to read it all, and {follow}.", [])
    diff = list(difflib.unified_diff(old.splitlines(), new.splitlines(),
                                     "guide (yours)", "guide (now)", lineterm=""))
    where = guide_sections(old, new)
    where = f" ({'section' if len(where) == 1 else 'sections'}: {', '.join(where)})" if where else ""
    if len(diff) <= GUIDE_DIFF_LINES:
        return f"Your coordinator guide changed{where}. Read the change below and {follow}:", diff
    return (f"Your coordinator guide changed{where}, {len(diff)} lines of diff: run `bullpen guide` "
            f"to read it all, and {follow}.", [])


def tell_guide_changes(repo):
    """Tell each running coordinator, in this repo and every other, whose guide isn't
    the current one what changed. Never fails the command that called it."""
    try:
        text = guide_text()
        h = guide_hash(text)
        for r in [repo] + [o for o in known_repos() if o.dir != repo.dir]:
            told = [c for c in r.cfg["coordinators"] if c.get("guide") != h and alive(c)]
            for c in told:
                line, more = guide_news(old_guide(c.get("guide")), text)
                news(r, GUIDE_NEWS, "guide", line, to=[c["id"]], more=more)
                give_guide(c, text)
                print(f"coordinator {c['id']}{'' if r is repo else ' of ' + str(r.root)}: "
                      f"told that its guide changed")
            if told:
                r.save()
    except (Fail, OSError, ValueError) as e:
        print(f"bullpen: couldn't tell the coordinators about their guide: {e}", file=sys.stderr)


def coordinator_label(repo):
    return f"bullpen: {repo.root.name}"


def board_label(repo):
    return f"bullpen: {repo.root.name} board"


def session_for(repo, args=None):
    """The Rex session to work in: --session, else the primary coordinator's while it
    exists (a Rex restart gives sessions new IDs), else the one we're running in."""
    s = getattr(args, "session", None) if args else None
    c = (primary(repo) or {}).get("session")
    if s:
        return s
    if c and (c == os.environ.get("REX_SESSION") or rex_view(c) is not None):
        return c
    return os.environ.get("REX_SESSION") or c


# ---------- commands ----------

def cmd_up(args):
    from .board_build import rebuild_board_if_stale  # the leaves import core: not at the top
    from .doctor import up_hint
    from .push_remote import catch_up_push
    repo = Repo(repo_root())
    session = session_for(repo, args)
    if not session:
        raise Fail("not inside Rex: pass --session")
    relink(repo, session)
    prune(repo)
    live = list(repo.cfg["coordinators"])  # pruned: only live ones are left
    said = []  # (what happened, to what, block IDs): one line, or a line each with --verbose
    if args.here:
        block = os.environ.get("REX_BLOCK")
        if not block:
            raise Fail("--here needs REX_BLOCK (run it from the agent's own Rex pane)")
        c = coordinator_at(repo, block, boards=False)
        if c:
            c["session"] = session
            print(f"coordinator {c['id']}: this pane ({block}), already.", end=" ")
        else:
            c = add_coordinator(repo, block, session)
            others = ", ".join(f"{o['id']} ({o['block']})" for o in live)
            print(f"coordinator {c['id']}: this pane ({block})."
                  + (f" The other coordinators stay: {others}. Tasks you add are yours." if live else ""),
                  end=" ")
        give_guide(c, guide_text())  # it's told to read it now
        print(f"Run `{SELF} guide` to read the coordinator's guide."
              + (" News comes by monitor: start it first, as the guide says."
                 if repo.cfg.get("news") == "monitor" else ""))
        mine = [c]
    elif live:
        said.append(("already running", "coordinators " + " and ".join(c["id"] for c in live)
                     if len(live) > 1 else "coordinator", ", ".join(f"{c['id']} ({c['block']})" for c in live)))
        mine = live
    else:
        argv = resolve(repo.cfg["coordinator_cmd"]) + [
            "-n", coordinator_label(repo),
            "--append-system-prompt-file", str(guide_file(repo)),
            f"You are the bullpen coordinator for {repo.root}. The bullpen CLI is {SELF}. "
            + ("News comes by monitor: start it first, as your guide says. "
               if repo.cfg.get("news") == "monitor" else "")
            + f"Run `{SELF} ls`, then say in one short line that you are ready.",
        ]
        block = rex_run(session, repo.root, coordinator_label(repo), argv, focus=True)
        c = add_coordinator(repo, block, session)
        give_guide(c, guide_text())
        said.append(("started", "coordinator", block))
        mine = [c]
    repo.save()
    tell_guide_changes(repo)  # the others (all of them, without --here), after a pull
    if not args.no_board:
        for c in mine:
            ensure_board(repo, session, c, said=said)
    gather(repo, session)
    ensure_watcher(session, said=said)
    if args.verbose:
        for what, name, ids in said:
            print(f"{name.split()[0]} {what}: {ids}")
    elif said:
        print(up_line(said, fresh=not args.here and not live))
    pushed = catch_up_push(repo)
    if pushed:
        print(pushed)
    rebuilt = rebuild_board_if_stale(repo)
    if rebuilt:
        print(rebuilt)
    hint = up_hint()
    if hint:
        print(hint)
    if args.verbose or repo.cfg.get("news") == "monitor":
        print(news_mode_line(repo))


def up_line(said, fresh):
    """`up`'s one line for people: "Coordinator, board and watcher started. Tell the
    coordinator what you want." on a first run, else what started or was running
    already. said: (what happened, to what, its block IDs); --verbose prints the IDs."""
    by = {}
    for what, name, _ in said:
        by.setdefault(what, []).append(name)
    parts = [(names[0] if len(names) == 1 else ", ".join(names[:-1]) + " and " + names[-1]) + " " + what
             for what, names in by.items()]
    line = " ".join(p[0].upper() + p[1:] + "." for p in parts)
    if fresh and "coordinator" in by.get("started", []):
        line += " Tell the coordinator what you want."
    return line


def ensure_board(repo, session, c, out=sys.stdout, said=None):
    """Coordinator c's task board, in a narrow pane to its right. A board running an
    older build than `bullpen board` runs now (old_board) is replaced: it never reloads."""
    b = c.get("board")
    old = None
    if b and rex_alive(b.get("session"), b.get("block")):
        if not old_board(b):
            return
        old = b
    p = run([REX, "split", "--json", "--focus=false", "-s", session, "-b", c["block"],
             "--split=right", "--ratio", "35", "--label", board_label(repo),
             "-c", str(repo.root), "--", *with_state([SELF, "board", "--coordinator", c["id"]])])
    block = json.loads(p.stdout)["block_ids"][0]
    if b:
        unindex_block(b.get("block"))
    c["board"] = {"block": block, "session": session}
    index_block(block, repo, "board")
    repo.save()
    if old:  # only once the new one is up
        rex_close(old.get("session"), old["block"])
    what = "restarted (it ran an older build)" if old else "started"
    if said is None:
        print(f"board {what}: {block}", file=out)
    else:
        said.append((what, "board", block))


def board_program(b):
    """The program a board's pane runs, as it was started (the path a board reloads
    from), or None when Rex can't say or it isn't the board yet."""
    p = run([REX, "block", "call", *(["-s", b["session"]] if b.get("session") else []), "-b", b["block"],
             "process"], check=False)
    if p.returncode != 0:
        return None
    try:
        child = json.loads(p.stdout).get("child") or {}
    except (ValueError, AttributeError):
        return None
    prog = child.get("invoked_path") or child.get("argv0") or ""
    if not os.path.isabs(prog):
        prog = child.get("path") or ""
    if not prog or Path(prog).name not in ("bullpen-board", board_exe().name):
        return None  # still `bullpen board`, about to exec it, or Rex doesn't say
    return prog


def old_board(b):
    """A running board's program, when it isn't the one `bullpen board` runs now: an
    older install's path, which a rebuild never replaces, so the board never reloads.
    (A board on the right path whose file was rebuilt reloads by itself.) Else None."""
    prog = board_program(b)
    if prog and os.path.realpath(prog) != os.path.realpath(board_exe()):
        return prog
    return None


def file_hash(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def code_hash():
    """A hash of bullpen's Python code: the entry script and the package's modules."""
    h = hashlib.sha256(Path(SELF).read_bytes())
    for f in sorted(Path(__file__).parent.glob("*.py")):
        h.update(f.name.encode() + b"\0" + f.read_bytes())
    return h.hexdigest()


def watcher_stamp():
    """What a watcher runs: a hash of watch.lua and of bullpen's code. A watcher started
    from other contents is stale."""
    return {"watch": file_hash(WATCH_LUA), "bullpen": code_hash()}


def ensure_watcher(session, said=None):
    """Start the watcher unless a current one runs; one started from an older
    watch.lua or bullpen script is closed and replaced."""
    w = read_json(STATE / "watcher.json", {})
    stamp = watcher_stamp()
    running = rex_alive(w.get("session"), w.get("block"))
    if running and w.get("stamp") == stamp:
        return
    argv = [shutil.which(REX) or REX, "do", "--all", str(WATCH_LUA),
            f"bin={SELF}", f"state={STATE}"]
    # Detached: it runs without a tab of its own.
    r = rex_api(session, "session.new_block", {
        "flavor": TERMINAL, "label": "bullpen: watcher",
        "options": {"command": with_state(argv), "cwd": str(Path.home())}})
    if not r or not r.get("block_id"):
        raise Fail("couldn't start the watcher")
    block = r["block_id"]
    if running:  # only once the new one is up
        rex_close(w.get("session"), w["block"])
    write_json(STATE / "watcher.json", {"block": block, "session": session, "stamp": stamp})
    what = "restarted (watch.lua or bullpen changed)" if running else "started"
    if said is None:
        print(f"watcher {what}: {block}")
    else:
        said.append((what, "watcher", block))


def refresh_watcher(repo):
    """After a merge that may have changed the watcher: make sure a current one runs.
    Never fails the command that called it."""
    try:
        session = session_for(repo)
        if session:
            ensure_watcher(session)
    except Fail as e:
        print(f"bullpen: {e}", file=sys.stderr)


def cmd_add(args):
    repo = Repo(repo_root())
    brief = Path(args.brief_file).read_text() if args.brief_file else args.brief
    if not brief:
        raise Fail("give the task's brief with --brief or --brief-file")
    tid = f"{repo.cfg['prefix']}-{repo.cfg['next']}"
    after = check_after(repo, tid, dep_ids(repo, args.after))
    repo.cfg["next"] += 1
    repo.save()
    t = {"id": tid, "title": args.title, "status": "todo", "created": now(),
         "base": args.base or repo.base(), "branch": None, "worktree": None,
         "block": None, "session": None, "pr": None, "why": None, "last": None}
    c = calling_coordinator(repo) or primary(repo)  # who hears its news
    if c:
        t["coordinator"] = c["id"]
    if after:
        t["after"] = after
    if args.model:
        t["worker_model"] = args.model  # its own, over the config's, from its first start
    if args.review_mode and args.review_mode != "auto":
        t["review_mode"] = args.review_mode
    set_auto_accept(t, args)
    (repo.dir / "briefs").mkdir(parents=True, exist_ok=True)
    (repo.dir / "briefs" / f"{tid}.md").write_text(brief.rstrip() + "\n")
    repo.save_task(t)
    print(tid)


def cmd_adopt(args):
    """Make the coordinator in this pane the task's own: its news comes here."""
    repo = Repo(repo_root())
    c = calling_coordinator(repo)
    if not c:
        raise Fail("run `adopt` from a coordinator's own pane (`bullpen up --here` makes one)")
    for tid in args.ids:
        t = repo.task(tid)
        was = t.get("coordinator")
        if was == c["id"]:
            print(f"{t['id']} is yours already")
            continue
        t["coordinator"] = c["id"]
        repo.save_task(t)
        print(f"{t['id']} is yours now (was {was or 'nobody'}'s): its news comes here")
    session = session_for(repo)
    if session:
        gather(repo, session)  # its panes move to this coordinator's tab


# ---------- dependencies and the run queue ----------
#
# A task can wait on others (`add --after`, `after`): its "after" is a list of IDs.
# `start` refuses it until they're all done. When a task is accepted or dropped, or its
# worker hands it off (to review, ready or finished), the queue starts each todo task
# that has dependencies, all done, within max_running. Only working tasks take a slot:
# a reviewer is short and cheap. A task sent back from review works, and counts, again,
# even if that puts the count over max_running: it's finishing work already started,
# and the queue just starts nothing more until the count is under the limit.


def dep_ids(repo, values):
    """IDs from --after values, each one or several: "BP-8", "8,9", "BP-8 BP-9"."""
    out = []
    for v in values or []:
        for part in re.split(r"[,\s]+", v.strip()):
            if part and repo.norm(part) not in out:
                out.append(repo.norm(part))
    return out


def check_after(repo, tid, after):
    """after, if the task tid can wait on those tasks: each exists, isn't tid itself
    or dropped, and none of them waits on tid already (a cycle)."""
    tasks = {t["id"]: t for t in repo.tasks()}
    for d in after:
        if d == tid:
            raise Fail(f"{tid} can't wait on itself")
        if d not in tasks:
            raise Fail(f"no task {d} in {home(repo.root)}: `bullpen ls --all` lists them")
        if tasks[d]["status"] == "dropped":
            raise Fail(f"{d} was dropped: {tid} would wait on it forever")
        path = dep_path(tasks, d, tid)
        if path:
            raise Fail(f"{d} already waits on {tid} ({' → '.join(path)}): that would be a cycle")
    return after


def dep_path(tasks, start, goal, seen=None):
    """[start, ..., goal] if start waits on goal, directly or through others, else None."""
    seen = seen if seen is not None else set()
    if start == goal:
        return [goal]
    if start in seen:
        return None
    seen.add(start)
    for d in (tasks.get(start) or {}).get("after") or []:
        rest = dep_path(tasks, d, goal, seen)
        if rest:
            return [start, *rest]
    return None


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


def cmd_after(args):
    """Set what a task waits on, or clear it with --none."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if args.none == bool(args.deps):
        raise Fail("give the tasks it waits on, or --none")
    if t["status"] in CLOSED:
        raise Fail(is_status(t))
    after = [] if args.none else check_after(repo, t["id"], dep_ids(repo, args.deps))
    if after:
        t["after"] = after
    else:
        t.pop("after", None)
    write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
    print(f"{t['id']} waits on {', '.join(after)}" if after else f"{t['id']} waits on nothing")


def own_model(repo, t):
    """The worker model a task chose for itself, if it isn't the config's."""
    m = t.get("worker_model")
    return m if m and m != repo.cfg.get("worker_model") else None


def cmd_model(args):
    """Set, change or clear the worker model a task starts and resumes with."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if args.clear == bool(args.model):
        raise Fail("give the model (an alias like opus, or a model ID), or --clear")
    if t["status"] in CLOSED:
        raise Fail(is_status(t))
    if args.clear:
        t.pop("worker_model", None)
    else:
        t["worker_model"] = args.model
    write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
    cfg = repo.cfg.get("worker_model")
    said = (f"{t['id']} uses the config's worker model ({cfg})" if args.clear and cfg else
            f"{t['id']} uses the worker command's own model" if args.clear else
            f"{t['id']} uses {args.model}")
    if rex_alive(t.get("session"), t.get("block")):
        said += f"; its worker keeps running on {t.get('model') or 'its model'}: this takes effect at its next start or resume"
    print(said)


def running_count(repo):
    """The tasks that take a max_running slot: those working (not in review)."""
    return sum(1 for t in repo.tasks() if t["status"] == "working")


def start_queued(repo, by, how):
    """After `by` was accepted, closed or dropped, or went to review, ready or finished
    (how): start each queued task, one with dependencies all done, in ID order, while
    fewer than max_running work. One news line per start. Plain todo tasks wait for the
    coordinator."""
    tasks = {t["id"]: t for t in repo.tasks()}
    queued = [t for t in tasks.values() if t["status"] == "todo" and t.get("after")
              and not open_deps(repo, t, tasks)]
    if not queued:
        return
    why = {"accepted": f"{by['id']} was accepted", "closed": f"{by['id']} was closed",
           "review": f"{by['id']} went to review, freeing a slot",
           "ready": f"{by['id']} is ready, freeing a slot",
           "finished": f"{by['id']} finished, freeing a slot"}.get(
        how, f"{by['id']} was dropped, freeing a slot")
    lim = limit_active()
    if lim:
        ids = ", ".join(t["id"] for t in queued)
        news(repo, by, "queue", f"{ids} can start ({why}), but the usage limit is reached "
             f"({limit_phrase(lim.get('resets'))}): `bullpen start` them after the reset", everyone=True)
        return
    room = repo.cfg["max_running"] - running_count(repo)
    session = session_for(repo)
    for t in sorted(queued, key=lambda t: int(t["id"].rsplit("-", 1)[1]))[:max(room, 0)]:
        try:
            if not session:
                raise Fail("no Rex session: run `bullpen up`")
            if not start_task(repo, t, session):
                continue  # its setup failed: that has its own news
        except Fail as e:
            news(repo, t, "queue", f"{t['id']} couldn't start ({why}): {clip(str(e), 200)}")
            continue
        news(repo, t, "started", f"{t['id']} started ({why})")


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


def deps_section(repo, t):
    """For the brief of a task with dependencies: what each one landed."""
    if not t.get("after"):
        return ""
    tasks = {o["id"]: o for o in repo.tasks()}
    parts = []
    for d in t["after"]:
        o = tasks.get(d)
        if not o:
            continue
        state = f"{o['status']}, not accepted yet"
        if o["status"] == "done":
            state = "finished with nothing to merge, closed" if o.get("closed") else "accepted"
        where = pr_short(o.get("pr")) if o.get("pr") else (f"merged into {o['base']}" if o.get("merged") else "")
        lines = [f"### {d}: {o['title']} ({state}{', ' + where if where else ''})", ""]
        lines.append((o.get("summary") or "No summary.").strip())
        findings = sorted((repo.dir / "briefs").glob(f"{d}.review-*.findings.md"))
        if findings:
            lines += ["", "Review findings: " + ", ".join(str(f) for f in findings)]
        parts.append("\n".join(lines))
    if not parts:
        return ""
    return ("## What this builds on\n\nThis task waited on these tasks. Their work is on your base "
            "branch now (unless marked otherwise): read it before you start.\n\n"
            + "\n\n".join(parts) + "\n\n")


def footer(repo, t):
    return (PROMPTS / "worker-footer.md").read_text().format(
        id=t["id"], title=t["title"], worktree=t["worktree"], branch=t["branch"],
        base=t["base"], bin=SELF)


RULES_FILE = ".bullpen/rules.md"


def personal_rules(repo):
    return repo.dir / "rules.md"


def rule_sources(repo, committed, where):
    """[(where it came from, text)]: the repo's rules file first, then the personal one."""
    out = []
    if committed and committed.strip():
        out.append((f"{RULES_FILE} {where}".strip(), committed.strip()))
    p = personal_rules(repo)
    if p.exists() and p.read_text().strip():
        out.append((str(p), p.read_text().strip()))
    return out


def committed_rules_on(repo, ref):
    """The repo's rules file as committed on ref, or None."""
    p = git(repo.root, "show", f"{ref}:{RULES_FILE}", check=False)
    return p.stdout if p.returncode == 0 else None


def rules_text(sources):
    return "\n\n".join(text for _, text in sources)


def task_rules(repo, t):
    """The rules the task's worker was given; for a task started before rules
    were saved, the ones on its base now."""
    saved = repo.dir / "briefs" / f"{t['id']}.rules.md"
    if saved.exists():
        return saved.read_text().strip()
    return rules_text(rule_sources(repo, committed_rules_on(repo, task_base_ref(repo, t)), ""))


def cmd_rules(args):
    repo = Repo(repo_root())
    if args.edit:
        p = personal_rules(repo)
        p.parent.mkdir(parents=True, exist_ok=True)
        editor = os.environ.get("VISUAL") or os.environ.get("EDITOR") or "vi"
        subprocess.call([*editor.split(), str(p)])
        return
    base = repo.base()
    ref = f"origin/{base}" if repo.has_origin() and git(
        repo.root, "rev-parse", "-q", "--verify", f"origin/{base}", check=False).returncode == 0 else base
    sources = rule_sources(repo, committed_rules_on(repo, ref), f"on {ref}")
    if not sources:
        print(f"No rules. Commit them as {RULES_FILE}, or keep your own in "
              f"{personal_rules(repo)} (`bullpen rules --edit`).")
        return
    print("\n\n".join(f"# From {where}\n\n{text}" for where, text in sources))


def cmd_start(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] not in ("todo", "stopped") and not (t.get("needs_setup") and t["status"] in WAITING):
        raise Fail(f"{is_status(t)}; use `resume` or `tell`")
    deps = [] if args.force or t["status"] != "todo" else open_deps(repo, t)
    if deps:
        raise Fail(f"{t['id']} waits on " + ", ".join(f"{d} ({status_words(st) if st else 'no such task'})" for d, st in deps)
                   + f": it starts by itself once they're accepted; `start {t['id']} --force` starts it now")
    lim = None if args.force else limit_active()
    if lim:
        raise Fail(f"the usage limit is reached ({limit_phrase(lim.get('resets'))}): a worker started "
                   f"now would stop at once. `start` works again after the reset, or once a "
                   f"limited worker is working again; `start {t['id']} --force` starts it anyway")
    session = session_for(repo, args)
    if not session:
        raise Fail("no Rex session: run `bullpen up` first or pass --session")
    set_auto_accept(t, args)
    if not start_task(repo, t, session, args.model):
        raise Fail(f"{t['id']}'s setup failed, so its worker didn't start: see {hook_log(repo, t, 'setup')}")
    print(f"{t['id']} started in {t['worktree']} (branch {t['branch']}, pane {t['block']})")


def start_task(repo, t, session, model=None):
    """Make the task's worktree, set it up, write its brief and start its worker.
    Returns False, with the task stopped (saved, news sent), if its setup failed."""
    if not t["worktree"]:
        t["needs_setup"] = True  # until its setup succeeds: a retry runs it again
        base = t["base"]
        start_point = base
        if git(repo.root, "remote", "get-url", "origin", check=False).returncode == 0:
            git(repo.root, "fetch", "-q", "origin", base, check=False)
            if git(repo.root, "rev-parse", "-q", "--verify", f"origin/{base}", check=False).returncode == 0:
                start_point = f"origin/{base}"
        t["branch"] = f"bullpen/{t['id'].lower()}-{slug(t['title'])}"
        wt = repo.worktree_dir(t)
        wt.parent.mkdir(parents=True, exist_ok=True)
        git(repo.root, "worktree", "add", "-q", "--no-track", "-b", t["branch"], str(wt), start_point)
        t["worktree"] = str(wt)
        repo.save_task(t)  # setup can take minutes: if bullpen dies in it, a retry knows the worktree
    if t.get("needs_setup") and not set_up(repo, t):
        return False
    brief = (repo.dir / "briefs" / f"{t['id']}.md").read_text()
    full = repo.dir / "briefs" / f"{t['id']}.full.md"
    # committed on its base, not the worktree's copy: a restarted task may have edited it
    rules = rules_text(rule_sources(repo, committed_rules_on(repo, task_base_ref(repo, t)), ""))
    saved = repo.dir / "briefs" / f"{t['id']}.rules.md"
    saved.write_text(rules + "\n" if rules else "")
    section = f"## Rules for this repository\n\n{rules}\n\n" if rules else ""
    full.write_text(f"# {t['id']}: {t['title']}\n\n{brief}\n{deps_section(repo, t)}{section}{footer(repo, t)}")
    first = f"You are the worker for bullpen task {t['id']}. Read your brief at {full} and follow it."
    if model:
        t["worker_model"] = model  # this task's own, over the config's
    launch(repo, t, session, first, resume=False)
    return True


def launch(repo, t, session, first, resume):
    cmd = with_model(repo.cfg["worker"], t.get("worker_model") or repo.cfg.get("worker_model"))
    t["model"] = model_in(cmd) if is_claude(cmd) else None
    argv = resolve(cmd) + (["--continue"] if resume else []) + \
        ["-n", f"{t['id']}: {t['title']}", first]
    unindex_block(t.get("block"))
    label = f"bullpen: {t['id']}"
    if repo.cfg.get("workers") == "tabs":
        block = rex_run(session, t["worktree"], label, argv)
    else:
        block = float_pane(repo, session, label + HIDE_HINT, t["worktree"], argv, t)
    index_block(block, repo, t["id"])
    set_status(t, "working")
    t.update(block=block, session=session, last=None)
    repo.save_task(t)


def float_pane(repo, session, label, cwd, argv, t):
    """Start argv in a minimized floating pane in the window of task t's coordinator:
    it shows as a capsule at the bottom right until `bullpen go` brings it up."""
    args = {"bounds": FLOAT_BOUNDS, "layout": {"block": {
        "flavor": TERMINAL, "label": label,
        "options": {"command": with_state(argv), "cwd": str(cwd)}}}}
    where = find_pane(rex_view(session), block=home_of(repo, t).get("block"))
    if where:
        args["window_id"] = where[0]
    r = rex_api(session, "session.new_layer", args)
    set_layer(session, r["layer_id"], "minimized")
    return r["block_ids"][0]


# ---------- worktree hooks: setup and teardown ----------

def hook_log(repo, t, name):
    return repo.dir / "briefs" / f"{t['id']}.{name}.log"


def hook(repo, t, name):
    """(argv, where it came from, the other source it wins over or None) for the task's
    setup or teardown, or None: the repo's config.json first, then .bullpen/<name>.sh
    as committed on the task's base."""
    ref = task_base_ref(repo, t)
    path = f".bullpen/{name}.sh"
    p = git(repo.root, "show", f"{ref}:{path}", check=False)
    committed = p.stdout if p.returncode == 0 and p.stdout.strip() else None
    cmd = repo.cfg.get(name)
    if cmd:
        return ["sh", "-c", cmd], f"config.json's {name}", (f"{path} on {ref}" if committed else None)
    if not committed:
        return None
    script = repo.dir / "briefs" / f"{t['id']}.{name}.sh"
    script.write_text(committed)
    script.chmod(0o755)
    return ([str(script)] if committed.startswith("#!") else ["sh", str(script)]), f"{path} on {ref}", None


def run_hook(repo, t, name):
    """Run the task's setup or teardown in its worktree, its output in <ID>.<name>.log.
    Returns None when there is none, else (exit code, log, where it came from, what it won over)."""
    h = hook(repo, t, name)
    if not h:
        return None
    argv, source, over = h
    log = hook_log(repo, t, name)
    env = {**os.environ, "BULLPEN_TASK": t["id"], "BULLPEN_WORKTREE": t["worktree"],
           "BULLPEN_MAIN": str(repo.root), "BULLPEN_BRANCH": t["branch"]}
    with log.open("w") as f:
        f.write(f"# {name} for {t['id']} at {now()}: {source}"
                + (f" (over {over})" if over else "") + "\n")
        f.flush()
        try:
            code = subprocess.run(argv, cwd=t["worktree"], env=env, stdin=subprocess.DEVNULL,
                                  stdout=f, stderr=subprocess.STDOUT).returncode
        except OSError as e:
            f.write(f"{e}\n")
            code = 127
        f.write(f"# exit {code}\n")
    return code, log, source, over


def set_up(repo, t):
    """Run the task's setup in its new worktree. On failure the task stops (saved, news
    sent) and this returns False; its next start or resume sets it up again."""
    r = run_hook(repo, t, "setup")
    if r and r[3]:
        print(f"{t['id']}: running {r[2]}, over {r[3]}", file=sys.stderr)
    if r and r[0] != 0:
        code, log = r[0], r[1]
        stop(repo, t, "worker", "setup", f"Setup failed (exit {code}): see {log}",
             f"{t['id']}'s setup failed (exit {code}), so its worker didn't start: see {log}. "
             f"`bullpen start {t['id']}` sets it up again")
        return False
    t.pop("needs_setup", None)
    return True


def tear_down(repo, t):
    """Run the task's teardown before its worktree goes. A failure is logged and said,
    never in the way. Returns whether one ran."""
    if not (t.get("worktree") and Path(t["worktree"]).exists()):
        return False
    try:
        r = run_hook(repo, t, "teardown")
    except (Fail, OSError) as e:
        print(f"{t['id']}: teardown couldn't run: {e}", file=sys.stderr)
        return False
    if r and r[0] != 0:
        print(f"{t['id']}: teardown failed (exit {r[0]}): see {r[1]}; removing the worktree anyway",
              file=sys.stderr)
    return bool(r)


def relink(repo, session):
    """A Rex restart brings panes back with new block IDs: find bullpen's own again
    by their labels. Panes that are gone are forgotten (a working task is stopped)."""
    view = rex_view(session)
    if view is None:
        return
    known = {}

    def ours(stored):
        """Is a pane recorded in session `stored` one to look for here? Yes if it's
        this session, or if that session no longer exists (a restart renamed it)."""
        if stored in (None, session, view.get("session_id"), view.get("label")):
            return True
        if stored not in known:
            known[stored] = rex_view(stored) is None
        return known[stored]

    cs = repo.cfg["coordinators"]
    # A pane bullpen started is found again by its label; one made a coordinator with
    # `up --here` isn't (its label is its agent's): it's forgotten.
    taken = {c.get("block") for c in cs} | {(c.get("board") or {}).get("block") for c in cs}
    for c in list(cs):
        if find_pane(view, block=c.get("block")):
            c["session"] = session
            continue
        if not ours(c.get("session")) or alive(c):
            continue
        unindex_block(c.get("block"))
        hit = next((h for h in find_panes(view, coordinator_label(repo)) if h[2] not in taken), None)
        if hit:
            c.update(block=hit[2], session=session)
            taken.add(hit[2])
            index_block(hit[2], repo, "coordinator")
            print(f"coordinator {c['id']}: found again ({hit[2]})")
        else:
            b = c.get("board") or {}
            if find_pane(view, block=b.get("block")):
                rex_close(session, b["block"])  # its coordinator is gone
            unindex_block(b.get("block"))
            cs.remove(c)
            print(f"coordinator {c['id']}: its pane is gone, so it is dropped")
    for c in cs:
        b = c.get("board")
        if not b:
            continue
        if find_pane(view, block=b.get("block")):
            b["session"] = session
            continue
        if not ours(b.get("session")):
            continue
        unindex_block(b.get("block"))
        # the board beside its coordinator, else any board nobody has
        home = find_pane(view, block=c.get("block"))
        hits = [h for h in find_panes(view, board_label(repo)) if h[2] not in taken]
        hit = next((h for h in hits if home and h[0] == home[0]), hits[0] if hits else None)
        c["board"] = {"block": hit[2], "session": session} if hit else None
        if hit:
            taken.add(hit[2])
            index_block(hit[2], repo, "board")
            print(f"board of {c['id']}: found again ({hit[2]})")
    repo.save()
    for t in repo.tasks():
        if t["status"] in CLOSED or not t.get("block") or find_pane(view, block=t["block"]):
            continue
        if not ours(t.get("session")):
            continue
        unindex_block(t["block"])
        hit = find_pane(view, label=f"bullpen: {t['id']}")
        if hit:
            t.update(block=hit[2], session=session)
            index_block(hit[2], repo, t["id"])
            print(f"{t['id']}: found again ({hit[2]})")
        else:
            t["block"] = None
            if t["status"] in ("working", *WAITING) and not is_limited(t):  # a limited one is resumed later
                set_status(t, "stopped", "Its pane was gone", {"by": "worker", "kind": "pane_gone"})
        repo.save_task(t)
    relink_reviewers(repo, session, view)


def gather(repo, session):
    """Keep every worker and reviewer pane in its coordinator's tab, as capsules (the
    primary coordinator's, when its own is gone). A coordinator that was restarted
    opens in a new tab: move the panes there and close any tab that moving them left
    empty."""
    if repo.cfg.get("workers") == "tabs":
        return
    view = rex_view(session)
    fallback = primary(repo) or {}
    emptied = set()
    for t in repo.tasks():
        if t["status"] in CLOSED:
            continue
        c = home_of(repo, t)
        home = find_pane(view, block=c.get("block"))
        if not home:
            c, home = fallback, find_pane(view, block=fallback.get("block"))
        if not home:
            continue
        for b in (t.get("block"), (t.get("review") or {}).get("block")):
            where = find_pane(view, block=b) if b else None
            if not where or where[0] == home[0]:
                continue
            if rex_api(session, "session.move_block", {"block_id": b, "anchor_block_id": c["block"]},
                       check=False) is None:
                continue
            r = rex_api(session, "session.float_block", {"block_id": b, "bounds": FLOAT_BOUNDS}, check=False)
            if r:
                set_layer(session, r["layer_id"], "minimized")
            emptied.add(where[0])
    if emptied:
        for w in (rex_view(session) or {}).get("windows", []):
            if w["window_id"] in emptied and not any(l.get("blocks") for l in w.get("layers", [])):
                rex_api(session, "session.close_window", {"window_id": w["window_id"]}, check=False)


def relink_reviewers(repo, session, view):
    for t in repo.tasks():
        r = t.get("review") or {}
        if t["status"] in CLOSED or not r.get("block") or find_pane(view, block=r["block"]):
            continue
        unindex_block(r["block"])
        hit = find_pane(view, label=review_label(t))
        r.update(block=hit[2] if hit else None, session=session)
        if hit:
            index_block(hit[2], repo, f"{t['id']} review")
        repo.save_task(t)


def cmd_resume(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if rex_alive(t["session"], t["block"]):
        raise Fail(f"{t['id']}'s agent is still running; use `tell`")
    if not t["worktree"]:
        raise Fail(f"{t['id']} never started; use `start`")
    if t.get("needs_setup"):  # its worker never ran: set it up and start it afresh
        session = session_for(repo, args) or t["session"]
        if not start_task(repo, t, session):
            raise Fail(f"{t['id']}'s setup failed again: see {hook_log(repo, t, 'setup')}")
        print(f"{t['id']} set up and started in pane {t['block']}")
        return
    msg = args.message or "You were stopped before finishing. Check where you were and carry on with your brief."
    launch(repo, t, session_for(repo, args) or t["session"], msg, resume=True)
    print(f"{t['id']} resumed in pane {t['block']}")


def cmd_tell(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if not rex_alive(t["session"], t["block"]):
        raise Fail(f"{t['id']}'s agent isn't running; use `resume {t['id']} \"...\"`")
    rex_type(t["session"], t["block"], " ".join(args.message))
    if t["status"] in (*WAITING, "ready", "finished"):
        back_to_work(t)
        t["last"] = None
        repo.save_task(t)
    print(f"sent to {t['id']}")


def cmd_peek(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if not rex_alive(t["session"], t["block"]):
        raise Fail(f"{t['id']}'s agent isn't running")
    print("\n".join(screen_end(t["session"], t["block"], args.lines, args.vt)))


def screen_end(session, block, n, vt=False):
    """The last n lines of a pane's screen, without the blank ones at the end. vt keeps
    its colours, as escape codes; a gap the screen skips over (cursor forward) is
    spaces, as a reader of the codes would otherwise run its words together."""
    s = ["-s", session] if session else []
    out = run([REX, "capture", *(["--format=vt"] if vt else []), *s, "-b", block]).stdout
    if vt:
        out = re.sub(r"\x1b\[(\d*)C", lambda m: " " * int(m.group(1) or 1), out)
    lines = [l.rstrip("\r") for l in out.split("\n")]
    while lines and not re.sub(r"\x1b\[[0-9;:]*m", "", lines[-1]).strip():
        lines.pop()
    return lines[-n:]


# ---------- answering from the board ----------
#
# A task whose agent is at a prompt can be answered from the board: `prompt` reads the
# question and its options off the pane's screen, and `answer` presses one of them,
# once it has read the screen again and found the same question there. A task the
# coordinator asked the person about (`ask`) is answered with `reply`: the answer goes
# to the coordinator, never to the worker.

# Claude Code's menus: "❯ 1. Yes", "  2. No" (the ❯ marks the chosen one). Its old
# boxed layout puts a │ at each edge.
OPTION_RE = re.compile(r"^(\s*)(❯|›|>)?\s*(\d{1,2})\.\s+(\S.*)$")
RULE_RE = re.compile(r"^\s*[╭╰]?[─━╌┄-]{3,}[╮╯]?\s*$")
YES_NO_RE = re.compile(r"[(\[]\s*y\s*/\s*n\s*[)\]]", re.I)
HINT_LINES = 3  # lines a menu can have below it: "Esc to cancel · Tab to amend", a rule


def unboxed(line):
    return re.sub(r"^\s*│|│\s*$", "", line).rstrip()


def prompt_id(lines):
    """A short ID for a prompt from its lines: the same prompt on screen again, not
    just the same question ("Do you want to proceed?" asks about every command)."""
    text = "\n".join(l.strip() for l in lines if l.strip() and not RULE_RE.match(l))
    return hashlib.sha1(text.encode()).hexdigest()[:12]


PROMPT_CONTEXT = 8  # at most this many lines above a question are its prompt's: the command it asks about
TOP_RULE_RE = re.compile(r"^\s*╭?─{3,}╮?\s*$")  # a prompt's top edge (a dashed ╌ rule is inside it)


def prompt_top(lines, i):
    """Where the prompt whose question is line i starts: below its top edge."""
    top = max(0, i - PROMPT_CONTEXT)
    for j in range(i - 1, top - 1, -1):
        if TOP_RULE_RE.match(lines[j]):
            return j + 1
    return top


def read_prompt(text):
    """The question Claude Code's screen ends with, as {"question", "options": [[key,
    label], ...], "id"}, or None when it doesn't end with one. A numbered menu with its
    ❯ on an option is answered by its number; a "(y/n)" question by y or n. A menu's
    "Type something." is left out: it wants typing in the pane. id: prompt_id of the
    question, what's above it and its options."""
    lines = [unboxed(l) for l in text.splitlines()]
    while lines and not lines[-1].strip():
        lines.pop()
    filled = [i for i, l in enumerate(lines) if l.strip() and not RULE_RE.match(l)]
    for i in reversed(filled[-HINT_LINES:]):
        if YES_NO_RE.search(lines[i]):
            return {"question": lines[i].strip(), "options": [["y", "Yes"], ["n", "No"]],
                    "id": prompt_id(lines[prompt_top(lines, i):i + 1])}
    # the last option, with at most a few hint lines under it, and no input box
    last = None
    for i in reversed(filled):
        if OPTION_RE.match(lines[i]):
            last = i
            break
        if lines[i].lstrip().startswith(("❯", "›")):
            return None  # the agent's own input box: it isn't asking
        if len(filled) - filled.index(i) > HINT_LINES:
            return None
    if last is None:
        return None
    options, marked, question, top = [], False, None, 0
    indent = len(OPTION_RE.match(lines[last]).group(1))
    for i in range(last, -1, -1):
        line = lines[i]
        m = OPTION_RE.match(line)
        if m:
            options.append((m.group(3), m.group(4).strip()))
            marked = marked or bool(m.group(2))
            indent = min(indent, len(m.group(1)))
        elif not line.strip() or RULE_RE.match(line) or len(line) - len(line.lstrip()) > indent + 1:
            continue  # a gap, a rule, or the option above's description or wrapped label
        else:
            question, top = line.strip(), prompt_top(lines, i)
            break
    options.reverse()
    if not marked or [k for k, _ in options] != [str(n) for n in range(1, len(options) + 1)]:
        return None  # a numbered list in what the agent wrote, not a menu
    options = [[k, label] for k, label in options if not label.startswith("Type something")]
    return {"question": question or "", "options": options, "id": prompt_id(lines[top:last + 1])}


def prompt_pane(t):
    """(who, session, block) of the agent the task is stopped on at a prompt. Fails
    when it isn't at one, or its pane is gone."""
    s = t.get("stop") or {}
    if t["status"] != "needs" or s.get("kind") != "prompt":
        raise Fail(f"{t['id']} isn't waiting at a prompt")
    if s.get("by") == "reviewer":
        r = t.get("review") or {}
        who, session, block = "reviewer", r.get("session") or t.get("session"), r.get("block")
    else:
        who, session, block = "worker", t.get("session"), t.get("block")
    if not rex_alive(session, block):
        raise Fail(f"{t['id']}'s {who} isn't running")
    return who, session, block


def cmd_prompt(args):
    """The prompt a task's agent is at, with the end of its screen (in colour), as JSON
    for the board. "question" is null when the screen shows none now."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    who, session, block = prompt_pane(t)
    p = read_prompt(screen(session, block)) or {"question": None, "options": [], "id": None}
    print(json.dumps({"who": who, **p, "screen": screen_end(session, block, args.lines, vt=True)}))


def cmd_answer(args):
    """Press one of the options of the prompt a task's agent is at, as the person would
    in its pane. Refuses unless its screen still shows that prompt (--prompt, its id
    from `prompt`)."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    who, session, block = prompt_pane(t)
    p = read_prompt(screen(session, block))
    if not p or (args.prompt is not None and p["id"] != args.prompt):
        raise Fail(f"{t['id']}'s {who} isn't at that prompt any more: nothing was sent")
    label = dict(map(tuple, p["options"])).get(args.key.lower())
    if label is None:
        keys = ", ".join(k for k, _ in p["options"])
        raise Fail(f"{args.key} isn't one of the prompt's options ({keys}): nothing was sent")
    run([REX, "send", *(["-s", session] if session else []), "-b", block, args.key.lower()])
    print(f"answered {t['id']}'s {who}: {args.key.lower()} ({label})")


def agent_running(t):
    """Is the pane of the agent the task is stopped on (its reviewer's, for its stop)
    alive?"""
    if reviewers_stop(t):
        r = t.get("review") or {}
        return rex_alive(r.get("session") or t.get("session"), r.get("block"))
    return rex_alive(t.get("session"), t.get("block"))


def cmd_reply(args):
    """The person's answer to what the coordinator asked about a task (`ask`). It goes
    to the task's coordinator as "BP-3: <answer>": typed into its pane, or as news with
    config --news monitor. A task asked about while it wasn't stopped goes back to the
    status it had then. A stopped one leaves needs as `ok` would, or goes to its
    coordinator when its agent isn't running."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] != "needs" or not t.get("ask"):
        raise Fail(f"{t['id']} has no question from its coordinator to answer")
    text = " ".join(" ".join(args.answer).split())
    if not text:
        raise Fail("give the answer")
    if not sees_to(repo, t):
        raise Fail(f"no coordinator is running to take the answer: `bullpen tell {t['id']}` "
                   f"tells its worker instead")
    question = t.get("why") or ""
    if t.get("asked_from"):
        was = t["asked_from"]
        set_status(t, was["status"], was.get("why"))  # it wasn't stopped: back to what it was doing
    elif agent_running(t):
        ok(t)
    else:
        set_status(t, "triage", clip(f"The person answered: {text}", 160))
    repo.save_task(t)
    news(repo, t, "answer", f"{t['id']}: the person answers \"{clip(question, 120)}\": {text}",
         typed=f"{t['id']}: {text}")
    print(f"sent {t['id']}'s answer to its coordinator")


def summary_text(args, what, required=True):
    """The text of --summary-file (or --summary). An empty file is refused: it is a
    write that failed (a full disk, /tmp out of inodes), not "nothing to say"."""
    if not args.summary_file:
        text = (args.summary or "").strip()
        if required and not text:
            raise Fail(f"give your {what} with --summary-file (or --summary)")
        return text
    try:
        text = Path(args.summary_file).read_text().strip()
    except OSError as e:
        raise Fail(f"can't read {args.summary_file}: {e.strerror or e}")
    if not text:
        raise Fail(f"{args.summary_file} is empty, so writing your {what} to it probably failed "
                   f"(a full disk, or /tmp out of inodes). Write it again, somewhere else if need "
                   f"be, and rerun this command")
    return text


def cmd_ready(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    was = t["status"]
    ready(repo, t, args)
    t = repo.task(t["id"])
    # its slot is free now; not when its approval carried over and auto-accept takes it
    # from here: that accept starts them
    accepting = auto_accepts(repo, t) and (t.get("review") or {}).get("verdict") == "approved"
    if was == "working" and (t["status"] == "review" or t["status"] == "ready" and not accepting):
        start_queued(repo, t, t["status"])


def ready(repo, t, args):
    wt = t["worktree"]
    if not wt:
        raise Fail(f"{t['id']} has no worktree")
    if git(wt, "status", "--porcelain").stdout.strip():
        raise Fail("uncommitted changes: commit (or discard) them, then run ready again")
    summary = summary_text(args, "summary", required=False)
    if t.get("gh"):
        t["gh"].pop("sent", None)  # whatever GitHub sent it back for is answered now
    t.pop("rebased", None)  # what the board says of a rebase after an accept
    if (t.get("tour") or {}).get("state") == "written":
        from .tours import tour_path, tour_stale  # tours imports core: not at the top
        tour = read_json(tour_path(repo, t))
        if tour and tour_stale(repo, t, tour):  # new code: not toured yet, as the board says
            t["tour"].pop("opened", None)
            t["tour"]["stale"] = True
    t.pop("from_ready", None)
    local = not repo.has_origin()
    if not repo.cfg["review"]:
        t.pop("tour_sent", None)  # no reviewer checks the person's comments with review off
    base_ref = task_base_ref(repo, t)
    ahead = int(git(wt, "rev-list", "--count", f"{base_ref}..HEAD").stdout.strip())
    if ahead == 0:
        set_status(t, "ready", "Nothing to merge (no commits)")
        t.update(summary=summary.strip() or None, diffstat=None)
        repo.save_task(t)
        ready_news(repo, t, ["nothing to merge"])
        print("no commits: marked ready with nothing to merge")
        return
    t["diffstat"] = shortstat(wt, base_ref)  # for the board, worked out once per ready
    if local:
        t["local"] = True
        t.update(why=None, summary=summary.strip() or t.get("summary"), commits=ahead)
        if repo.cfg["review"]:
            said = review_at_ready(repo, t, base_ref)
            print(f"local branch {t['branch']} ({commits(ahead)} ahead of {base_ref}; no remote, "
                  f"so nothing pushed). {said}")
            return
        set_status(t, "ready")
        repo.save_task(t)
        ready_news(repo, t, [commits(ahead)])
        print(f"local branch {t['branch']} is ready ({commits(ahead)} ahead of {base_ref})")
        return
    git(wt, "push", "-q", "--force-with-lease", "-u", "origin", f"HEAD:refs/heads/{t['branch']}")
    if t.get("gh"):
        t["gh"].update(new=0, checks=None)  # what it says about the new push comes later
    if not t.get("pr"):
        body = (summary.strip() or t["title"]) + f"\n\nBullpen task {t['id']}.\n"
        p = run([repo.gh(), "pr", "create", "--base", t["base"], "--head", t["branch"],
                 "--title", f"{t['title']} ({t['id']})", "--body-file", "-"], cwd=wt, input=body)
        t["pr"] = p.stdout.strip().splitlines()[-1]
    t.update(why=None, summary=summary.strip() or t.get("summary"), commits=ahead)
    if repo.cfg["review"]:
        said = review_at_ready(repo, t, base_ref)
        print(f"pushed {t['branch']}; pull request: {t['pr']}. {said}")
        return
    set_status(t, "ready")
    repo.save_task(t)
    ready_news(repo, t, [commits(ahead), pr_short(t["pr"])])
    print(f"pushed {t['branch']}; pull request: {t['pr']}")


# ---------- finish and close: a task with nothing to merge ----------
#
# Some tasks leave nothing in the repo (a video, research notes). Their worker runs
# `finish` instead of `ready`: no PR and no review. The person closes the task, or
# auto-accept does it at once.

def cmd_finish(args):
    """(worker) the task is done and leaves nothing to merge."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] in CLOSED:
        raise Fail(is_status(t))
    wt = t["worktree"]
    if not wt:
        raise Fail(f"{t['id']} has no worktree")
    if git(wt, "status", "--porcelain").stdout.strip():
        raise Fail("uncommitted changes: commit them and run `ready`, or discard them and run finish again")
    base_ref = task_base_ref(repo, t)
    if git(wt, "diff", "--quiet", f"{base_ref}...HEAD", check=False).returncode != 0:
        raise Fail(f"your branch changes {shortstat(wt, base_ref) or 'files'} against {base_ref}, so "
                   f"there is something to merge: run `ready` instead")
    summary = summary_text(args, "summary")
    close_reviewer(t)
    if t.get("gh"):
        t["gh"].pop("sent", None)
    t.pop("rebased", None)
    set_status(t, "finished")
    t.update(summary=summary, diffstat=None)
    repo.save_task(t)
    first = summary.splitlines()[0].strip().lstrip("#").strip()  # a Markdown title is fine
    if auto_accepts(repo, t):
        print(f"{t['id']} finished, with nothing to merge; auto-accept closes it now. End your turn.")
        sys.stdout.flush()
        close_task(repo, t, auto=True, first=first)  # last: this closes the pane we run in
        return
    news(repo, t, "finished", f"{t['id']} finished (nothing to merge): {first}")
    start_queued(repo, t, "finished")
    print(f"{t['id']} finished, with nothing to merge. The person closes it: end your turn.")


def cmd_close(args):
    """(the person's call) close a finished task: its worktree, branch and pane go.
    `accept` does the same on a finished task; this stays as another name for it."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] != "finished":
        hint = f": `accept {t['id']}` merges it" if t["status"] == "ready" else ""
        raise Fail(f"{is_status(t)}, not finished{hint}")
    close_task(repo, t)


def close_task(repo, t, auto=False, first=None):
    """Remove a finished task's worktree and branch (no warning: it has nothing to
    merge), close its panes and any PR, and mark it done."""
    os.chdir(repo.root)  # we may be running in the worktree this removes
    worker = (t.get("session"), t.get("block"))
    unindex_block(t.get("block"))
    close_reviewer(t)
    if t.get("pr"):
        run([repo.gh(), "pr", "close", t["pr"]], cwd=repo.root, check=False)
        git(repo.root, "push", "-q", "origin", "--delete", t["branch"], check=False)
    remove_worktree(repo, t, force=True)
    set_status(t, "done", t.get("why"))
    t.update(closed=True, block=None, worktree=None, auto_accepted=auto)
    repo.save_task(t)
    if auto:
        news(repo, t, "closed", f"{t['id']} finished (nothing to merge) and was closed by auto-accept: {first}")
    else:
        print(f"{t['id']} closed")
        tell_coordinator(repo, t, "closed", f"{t['id']} closed (finished, nothing to merge)")
    start_queued(repo, t, "closed")
    sys.stdout.flush()
    rex_close(*worker)  # last: this may close the pane we run in


def commits(n):
    return f"{n} commit{'s' if n != 1 else ''}"


def review_at_ready(repo, t, base_ref):
    """ready with review on: keep an approval that still covers the code, leave a reviewer
    that is on this very commit, or start a review. Returns what to tell the worker."""
    r = t.get("review") or {}
    if t.get("tour_sent"):
        # the person sent comments back after a tour: a review checks them, whatever the code
        start_review(repo, t, base_ref)
        return (f"A reviewer checks that the person's comments are addressed now: end your turn, and "
                f"you'll hear back if it asks for changes.")
    check = rebase_check(t, base_ref)
    kept = check[1] if check and check[0] == "kept" else None
    if kept:
        keep_approval(t, base_ref)
        set_status(t, "ready")
        repo.save_task(t)
        if auto_accepts(repo, t):
            # not here: accepting closes the worker's pane, which this runs in
            subprocess.Popen([SELF, "_auto_accept", t["id"]], cwd=repo.root, start_new_session=True,
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        else:
            ready_news(repo, t, [commits(t.get("commits") or 0), where(t), f"its approval carried over: {kept}"])
        return f"Its review approval carries over ({kept}), so there's no new review: end your turn."
    if (t["status"] == "review" and r.get("block") and r.get("commit") == head(t["worktree"])
            and rex_alive(r.get("session"), r["block"])):
        repo.save_task(t)
        return ("A reviewer is already checking this commit: end your turn, and you'll hear back "
                "if it asks for changes.")
    start_review(repo, t, base_ref, rebase=check[1] if check else None)
    return (f"A reviewer checks it now ({review_kind(t['review'])}): end your turn, and you'll "
            f"hear back if it asks for changes.")


def cmd_auto_accept(args):
    """(bullpen) the auto-accept of an approval carried over at ready, away from the worker's pane."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] == "ready" and (t.get("review") or {}).get("verdict") == "approved":
        auto_accept(repo, t)


# ---------- what a review covers ----------
#
# A review records what it looked at: the commit, the base it was on (merge-base), a
# patch-id of the task's diff against that base, and the task's own lines (own_lines:
# what it added and removed, file by file, without context or line numbers). An
# approval holds while the code doesn't change: the same commit, the same patch-id
# (reworded commits), or the same own lines after a rebase (only the base moved, even
# where it changed the context, or where a conflict was resolved by keeping both
# sides). After a rebase that conflicted, own lines that changed only in the files
# that conflicted get a rebase check (a short review of the resolution, which doesn't
# count as a round); a change anywhere else gets a follow-up review.


def head(wt):
    return git(wt, "rev-parse", "HEAD").stdout.strip()


def patch_id(wt, base_ref):
    """The stable patch-id of the branch's change against its base, or None if there's none."""
    diff = git(wt, "diff", "--binary", f"{base_ref}...HEAD").stdout
    if not diff.strip():
        return None
    out = run(["git", "-C", str(wt), "patch-id", "--stable"], input=diff).stdout.split()
    return out[0] if out else None


def own_lines(wt, base_ref):
    """The branch's own added and removed lines, file by file ({path: hash}), without
    context or line numbers: what stays the same when only the base moved under it."""
    out = git(wt, "diff", "-U0", "--binary", "--no-color", "--no-ext-diff", "--no-renames",
              f"{base_ref}...HEAD").stdout
    files, h = {}, None
    for line in out.split("\n"):
        if line.startswith("diff --git "):
            pair = line[len("diff --git "):]  # "a/<path> b/<path>", the same path twice
            h = files[pair[2:2 + (len(pair) - 5) // 2]] = hashlib.sha1()
        elif h and not line.startswith(("index ", "@@")):
            h.update(line.encode() + b"\n")
    return {path: h.hexdigest() for path, h in files.items()}


def merge_base(wt, base_ref):
    return git(wt, "merge-base", base_ref, "HEAD", check=False).stdout.strip() or None


def rebase_check(t, base_ref):
    """What the task's approval needs now: ("kept", why) when it still covers the code,
    ("rebase", files) when its own lines changed only in the files a rebase conflicted
    in (a rebase check looks at the resolution), or None (a normal review)."""
    r = t.get("review") or {}
    wt = t.get("worktree")
    if r.get("verdict") != "approved" or not r.get("patch") or not wt:
        return None
    if head(wt) == r.get("commit"):
        return "kept", "the same commit"
    moved = merge_base(wt, base_ref) != r.get("base")
    if not r.get("conflict") and patch_id(wt, base_ref) == r["patch"]:
        return "kept", (f"the same patch after a clean rebase onto {base_ref}" if moved
                        else "the same patch, in other commits")
    if r.get("own") is None:
        return None  # reviewed before bullpen kept own lines
    own = own_lines(wt, base_ref)
    changed = {p for p in set(own) | set(r["own"]) if own.get(p) != r["own"].get(p)}
    if not changed:
        return "kept", "only the base moved" if moved else "the same lines, in other commits"
    conflicted = r["conflict"] if isinstance(r.get("conflict"), list) else []
    if moved and changed <= set(conflicted):
        return "rebase", sorted(changed)
    return None


def approval_kept(t, base_ref):
    """Why the task's approval still covers its code, in a few words, or None if it doesn't."""
    check = rebase_check(t, base_ref)
    return check[1] if check and check[0] == "kept" else None


def keep_approval(t, base_ref):
    """The approval now covers the task's current commit."""
    r, wt = t["review"], t["worktree"]
    r.update(commit=head(wt), base=merge_base(wt, base_ref), patch=patch_id(wt, base_ref),
             own=own_lines(wt, base_ref))
    r.pop("conflict", None)
    r.pop("conflict_diff", None)
    r["kept"] = r.get("kept", 0) + 1


def is_doc(path):
    return path.startswith(("docs/", "prompts/")) or path.endswith(".md")


def change_size(wt, base_ref):
    """(files, lines, docs_only) of the branch's change; lines is None if a binary file changed."""
    out = git(wt, "diff", "--numstat", "--no-renames", f"{base_ref}...HEAD").stdout
    files, lines, docs = 0, 0, True
    for row in out.splitlines():
        added, deleted, path = row.split("\t", 2)
        files += 1
        lines = None if lines is None or added == "-" else lines + int(added) + int(deleted)
        docs = docs and is_doc(path)
    return files, lines, docs and files > 0


def first_kind(repo, t, base_ref):
    """("full" or "light", why) for a task's first review: its own review mode, else auto."""
    mode = t.get("review_mode") or "auto"
    if mode != "auto":
        return mode, f"set for {t['id']}"
    files, lines, docs = change_size(t["worktree"], base_ref)
    limit = repo.cfg["light_review_lines"]
    if docs:
        return "light", "docs only"
    if lines is not None and lines <= limit and files <= 2:
        return "light", f"{lines} line{'s' if lines != 1 else ''} in {files} file{'s' if files != 1 else ''}"
    return "full", None


def review_kind(r):
    """ "full review", "light review (docs only)", "follow-up review"."""
    kind = r.get("kind")
    if not kind:
        return "review"
    word = {"full": "full review", "light": "light review", "followup": "follow-up review",
            "tour": "review of the person's comments", "rebase": "rebase check"}[kind]
    return word + (f" ({r['why']})" if r.get("why") else "")


def review_model(repo, kind):
    if kind == "full":
        return repo.cfg.get("reviewer_model")
    return repo.cfg.get("reviewer_followup_model") or repo.cfg.get("reviewer_model")


MAX_DIFF_LINES = 800  # longer, and a follow-up's brief gives the command instead


def since_last(wt, base_ref, prev):
    """(command, diff) for what changed since the reviewed commit prev["commit"]: a plain
    diff when it's still in the branch's history, else (a rebase) a range-diff."""
    c = prev["commit"]
    if git(wt, "merge-base", "--is-ancestor", c, "HEAD", check=False).returncode == 0:
        args = ["diff", f"{c[:12]}..HEAD"]
    else:
        args = ["range-diff", f"{(prev.get('base') or c)[:12]}..{c[:12]}", f"{base_ref}..HEAD"]
    p = git(wt, *args, check=False)
    text = p.stdout if p.returncode == 0 else ""
    cmd = f"git -C {wt} {' '.join(args)}"
    if not text.strip():
        return cmd, "(no changes)" if p.returncode == 0 else "(git couldn't show it: run the command)"
    lines = text.rstrip("\n").split("\n")
    if len(lines) > MAX_DIFF_LINES:
        return cmd, f"({len(lines)} lines, too long to include: run the command)"
    return cmd, "\n".join(lines)


def clipped_diff(text, cmd):
    """A diff for a reviewer's brief, or what to run when it's empty or too long."""
    if not text.strip():
        return "(no changes)"
    lines = text.rstrip("\n").split("\n")
    if len(lines) > MAX_DIFF_LINES:
        return f"({len(lines)} lines, too long to include: run `{cmd}`)"
    return "\n".join(lines)


def conflict_text(path):
    """The conflicts a rebase met, as saved when it stopped, for a rebase check."""
    f = Path(path) if path else None
    if not f or not f.is_file():
        return "(not saved: the range-diff below shows how they were resolved)"
    return clipped_diff(f.read_text(), f"cat {f}")


def shortstat(wt, base_ref):
    """The branch's change against its base in a few words: "3 files, +120 −14"."""
    out = git(wt, "diff", "--shortstat", f"{base_ref}...HEAD").stdout
    n = {k: int(v) for v, k in re.findall(r"(\d+) (file|insertion|deletion)", out)}
    if not n.get("file"):
        return None
    files = f"{n['file']} file{'s' if n['file'] != 1 else ''}"
    return f"{files}, +{n.get('insertion', 0)} −{n.get('deletion', 0)}"


def review_label(t):
    return f"bullpen: {t['id']} review"


def start_review(repo, t, base_ref, rebase=None):
    """Start a reviewer agent on the task's branch, in its own capsule. The first round is
    a full review, or a light one for a small change; later rounds follow up on the last
    verdict: its findings and what changed since. Comments the person sent back after a
    tour (tour_sent) get a review of their own, and an approved change whose own lines
    changed only where a rebase conflicted (rebase: those files) a rebase check of the
    resolution. Both keep the round they follow: they don't count toward review_rounds."""
    old = t.get("review") or {}
    close_reviewer(t)
    sent = t.get("tour_sent")
    rebase = None if sent else rebase
    n = old.get("round", 0) + (0 if sent or rebase else 1)
    rounds = repo.cfg["review_rounds"]
    briefs = repo.dir / "briefs"
    wt = t["worktree"]
    # the round the next one follows up on: the last one with a verdict
    prev = ({k: old.get(k) for k in ("round", "commit", "base", "findings", "verdict")}
            if old.get("verdict") else old.get("prev"))
    if sent:
        kind, why = "tour", None
    elif rebase:
        kind, why = "rebase", None
    elif prev and prev.get("commit"):
        kind, why = "followup", None
    else:
        prev = None
        kind, why = first_kind(repo, t, base_ref)
    vals = dict(id=t["id"], title=t["title"], round=n, rounds=rounds, brief=briefs / f"{t['id']}.md",
                worktree=wt, branch=t["branch"], base_ref=base_ref, bin=SELF,
                summary=t.get("summary") or "(none)", rules=review_rules(repo, t), why=why or "")
    if sent:
        since, diff = since_last(wt, base_ref, sent)
        f = Path(sent["file"])
        vals.update(since=since, diff=diff, comments_file=f, count=sent["count"],
                    comments=f.read_text().strip() if f.is_file() else "(the file is gone)")
    elif prev:
        since, diff = since_last(wt, base_ref, prev)
        f = Path(prev["findings"]) if prev.get("findings") else None
        vals.update(prev_round=prev["round"], since=since, diff=diff, findings_file=f or "(none)",
                    findings=(f.read_text().strip() if f and f.is_file() else "(none)"),
                    prev_said="approved the change. Its notes" if prev["verdict"] == "approved"
                    else "asked for changes. Its findings")
    if rebase:
        vals.update(conflict_files=", ".join(f"`{f}`" for f in rebase),
                    conflicts=conflict_text(old.get("conflict_diff")),
                    now=clipped_diff(git(wt, "diff", f"{base_ref}...HEAD", "--", *rebase, check=False).stdout,
                                     f"git -C {wt} diff {base_ref}...HEAD -- {' '.join(rebase)}"))
    look = (PROMPTS / f"review-{kind}.md").read_text().format(**vals)
    prompt = briefs / (f"{t['id']}.tour-{sent['n']}.review.md" if sent else
                       f"{t['id']}.review-{n}.rebase.md" if rebase else f"{t['id']}.review-{n}.md")
    prompt.write_text((PROMPTS / "reviewer.md").read_text().format(
        **vals, kind=review_kind({"kind": kind, "why": why}), look=look))
    first = f"You are the reviewer for bullpen task {t['id']}. Read {prompt} and follow it."
    model = review_model(repo, kind)
    argv = resolve(with_model(repo.cfg["reviewer"], model)) + ["-n", f"{t['id']} review", first]
    session = t.get("session") or session_for(repo)
    if repo.cfg.get("workers") == "tabs":
        block = rex_run(session, t["worktree"], review_label(t), argv)
    else:
        block = float_pane(repo, session, review_label(t) + HIDE_HINT, t["worktree"], argv, t)
    index_block(block, repo, f"{t['id']} review")
    set_status(t, "review")
    t.update(review={
        "round": n, "block": block, "session": session, "verdict": None, "findings": None,
        "kind": kind, "why": why, "model": model, "prev": prev, **({"tour": sent["n"]} if sent else {}),
        **({"rebase": rebase} if rebase else {}),
        "commit": head(wt), "base": merge_base(wt, base_ref), "patch": patch_id(wt, base_ref),
        "own": own_lines(wt, base_ref)})
    repo.save_task(t)


def review_rules(repo, t):
    rules = task_rules(repo, t)
    if not rules:
        return ""
    return ("## Rules for this repository\n\nThe worker was given these rules too. "
            f"Check that the work follows them.\n\n{rules}\n\n")


def close_reviewer(t):
    r = t.get("review") or {}
    if r.get("block"):
        unindex_block(r["block"])
        rex_close(r.get("session"), r["block"])
        r["block"] = None


def cmd_verdict(args):
    """(reviewer) approve the task, or send findings back to its worker."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    r = t.get("review") or {}
    if t["status"] not in ("review", *WAITING) or not r.get("block"):
        raise Fail(f"{t['id']} isn't in review")
    text = summary_text(args, "findings")
    tour, rebase = r.get("kind") == "tour", r.get("kind") == "rebase"
    findings = repo.dir / "briefs" / (f"{t['id']}.tour-{r['tour']}.review.findings.md" if tour else
                                      f"{t['id']}.review-{r['round']}.rebase.findings.md" if rebase
                                      else f"{t['id']}.review-{r['round']}.findings.md")
    findings.write_text(text + "\n")
    rounds = repo.cfg["review_rounds"]
    r.update(verdict="approved" if args.verdict == "approve" else "changes", findings=str(findings))
    reviewer = (r.get("session"), r.get("block"))
    unindex_block(r["block"])
    r["block"] = None
    pr = where(t)
    t.pop("auto_wait", None)  # a new verdict decides again
    if tour:
        t.pop("tour_sent", None)  # reviewed: the next review follows up on this one
    if args.verdict == "approve":
        set_status(t, "ready")
        repo.save_task(t)
        if auto_accepts(repo, t):
            auto_accept(repo, t)
        else:
            why = f": {r['why']}" if r.get("why") else ""  # last, so not in brackets in brackets
            ready_news(repo, t, [commits(t.get("commits") or 0), pr,
                                 "a rebase check approved the conflict resolution" if rebase else
                                 f"approved by {review_kind({**r, 'why': None})}{why}"])
    elif tour or rebase or r["round"] < rounds:
        said = ("The reviewer says the person's comments aren't all addressed yet" if tour else
                "A rebase check says the conflict resolution isn't right" if rebase else
                f"The reviewer asked for changes (review round {r['round']} of {rounds})")
        msg = (f"{said}. Read {findings}, fix what you agree with, commit, and run your ready command "
               f"again. If you disagree with a finding, say why in your ready summary.")
        if t.get("gh"):
            t["gh"].pop("sent", None)  # it's the review it is fixing now, not GitHub
        try:
            failed = hand_back(repo, t, msg)
        except Fail as e:
            failed = clip(str(e), 160)
        if failed:
            # nobody is on it: the coordinator passes the findings on
            stop(repo, t, "worker", "not_sent", "Review findings didn't reach its worker",
                 f"{t['id']}'s reviewer asked for changes, but the message didn't reach its worker "
                 f"({failed}). The findings are in {findings}: `bullpen tell {t['id']}` it to read them",
                 "not-sent")
    else:
        set_status(t, "ready", f"Review still asks for changes after {rounds} rounds")
        repo.save_task(t)
        ready_news(repo, t, [pr], f", but review still asks for changes after {rounds} rounds: "
                   f"findings in {findings}")
    print(f"verdict recorded: {args.verdict}")
    sys.stdout.flush()
    rex_close(*reviewer)  # last: this closes the pane we may be running in


# ---------- auto-accept ----------
#
# With `config --auto-accept on`, an approve verdict runs the normal accept itself:
# the person said in advance that an approval is enough. A task's own "auto_accept"
# (`add`/`start --no-auto-accept`, or `--auto-accept`) wins over the config's. In PR
# mode it waits for the PR's checks ("auto_wait"), which the PR follow-up looks at.


def set_auto_accept(t, args):
    if getattr(args, "auto_accept", None) is not None:
        t["auto_accept"] = args.auto_accept


def auto_accepts(repo, t):
    own = t.get("auto_accept")
    return bool(repo.cfg.get("auto_accept") if own is None else own)


def auto_accept(repo, t):
    """An approved task: accept it now, or, with a PR whose checks are still running,
    once they pass. Anything that stops it leaves the task ready, with news."""
    if t.get("pr") and not t.get("local"):
        state = pr_checks_now(repo, t)
        if state == "failing":
            ready_news(repo, t, [pr_short(t["pr"]), "approved by review"],
                       ", but not auto-accepted: its PR's checks are failing")
            start_queued(repo, t, "ready")  # its worker is done: its slot is free
            return
        if state == "running":
            t["auto_wait"] = True
            write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
            ready_news(repo, t, [pr_short(t["pr"]), "approved by review"],
                       "; it auto-accepts once its PR's checks pass")
            start_queued(repo, t, "ready")
            return
    run_auto_accept(repo, t)


def pr_checks_now(repo, t):
    """"failing", "running", or "passing" (no checks counts as passing). Unsure (gh
    failed) counts as running: the PR follow-up looks again."""
    p = run([repo.gh(), "pr", "view", t["pr"], "--json", PR_FIELDS], cwd=repo.root, check=False)
    try:
        summary, _ = checks(json.loads(p.stdout)) if p.returncode == 0 else ("running", [])
    except ValueError:
        return "running"
    return summary or "passing"


def run_auto_accept(repo, t):
    """The normal accept, as if the person ran it. A refused merge leaves the task ready."""
    os.chdir(repo.root)  # we may be running in the worktree accept removes
    try:
        cmd_accept(argparse.Namespace(id=t["id"], auto=True, repo=repo))
    except Fail as e:
        t = repo.task(t["id"])
        t.pop("auto_wait", None)
        write_json(repo.task_path(t["id"]), t)
        ready_news(repo, t, [where(t), "approved by review"],
                   f", but the auto-accept was refused: {e}. Send it back to rebase")
        start_queued(repo, t, "ready")


def cmd_note(args):
    """An informational line on the board about a task (cleared when its status changes).
    A question for the person is `ask`."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    t["note"] = " ".join(" ".join(args.text).split())
    write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
    print(f"noted on {t['id']}")


def remove_worktree(repo, t, force=False):
    # what a teardown leaves behind mustn't keep the worktree (drop checked it was clean before)
    force = tear_down(repo, t) or force
    if t.get("worktree") and Path(t["worktree"]).exists():
        git(repo.root, "worktree", "remove", *(["--force"] if force else []), t["worktree"])
    if t.get("branch"):
        git(repo.root, "branch", "-D", t["branch"], check=False)


def cmd_accept(args):
    from .board_build import rebuild_board_if_stale  # the leaves import core: not at the top
    from .push_remote import push_base
    repo = getattr(args, "repo", None) or Repo(repo_root())
    t = repo.task(args.id)
    merged_outside = (t.get("gh") or {}).get("state") == "merged"
    if t["status"] == "finished" and not merged_outside:
        return close_task(repo, t)  # the person's yes on work with nothing to merge
    if t["status"] != "ready" and not merged_outside:
        raise Fail(f"{is_status(t)}, not ready to accept or finished")
    keys = keys_hash()
    touched = changed_files(repo, t)  # before the merge: what the others are checked against
    if t.get("pr"):
        p = None if merged_outside else run(
            [repo.gh(), "pr", "merge", t["pr"], f"--{repo.cfg['merge']}"],
            cwd=repo.root, check=False)
        if p and p.returncode != 0:
            raise Fail(f"merge refused: {(p.stderr or p.stdout).strip()}")
        if t.get("branch"):
            git(repo.root, "push", "-q", "origin", "--delete", t["branch"], check=False)
    elif t.get("branch") and (t.get("local") or not repo.has_origin()):
        # t["local"] too: a remote added after ready mustn't make accept skip the merge
        merge_locally(repo, t)  # raises, leaving the task as it was, if it can't
    rex_close(t["session"], t["block"])
    unindex_block(t["block"])
    close_reviewer(t)
    remove_worktree(repo, t, force=True)
    set_status(t, "done", t.get("why"))
    t.update(block=None, worktree=None, auto_accepted=bool(getattr(args, "auto", False)))
    repo.save_task(t)
    mode = repo.cfg["merge"]
    if t.get("pr"):
        print(f"{t['id']} done (merged {t['pr']})")
        how = (f"{pr_short(t['pr'])} was merged outside bullpen" if merged_outside
               else f"merged {pr_short(t['pr'])}, {mode}")
    elif t.get("merged"):
        print(f"{t['id']} done (merged into {t['base']}: {t['merged'][:12]})")
        how = f"merged into {t['base']}, {mode}"
    else:
        print(f"{t['id']} done")
        how = "nothing to merge"
    if getattr(args, "auto", False):
        news(repo, t, "accepted", f"{t['id']} approved by review and auto-accepted ({how})")
    else:
        tell_coordinator(repo, t, "accepted", f"{t['id']} accepted ({how})")
    if t.get("merged") and not t.get("pr"):  # local mode: back up the base branch
        said = push_base(repo, t["base"], t)
        if said:
            print(said)
    if how != "nothing to merge":
        rebase_others(repo, t, touched)
    start_queued(repo, t, "accepted")
    tell_guide_changes(repo)  # the merge may have changed the coordinator's guide
    refresh_watcher(repo)  # the merge may have changed the watcher itself
    if repo.root.resolve() == HOME.resolve():  # bullpen's own checkout: maybe the board too
        said = rebuild_board_if_stale(repo)
        if said:
            print(said)
    if keys_hash() != keys:  # last: the line the board shows
        news(repo, t, "keys", KEYS_CHANGED, everyone=True)
        print(KEYS_CHANGED)


KEYS_LUA = HOME / "lua" / "keys.lua"
KEYS_CHANGED = "Keys changed: run `rex config reload`"


def keys_hash():
    """keys.lua as it is, which Rex reads only when its config loads: an accept that
    changes it says to reload."""
    try:
        return file_hash(KEYS_LUA)
    except OSError:
        return None


def checked_out_at(repo, branch):
    """The worktree (the main checkout or another) that has branch checked out, or None."""
    p = git(repo.root, "worktree", "list", "--porcelain")
    path = None
    for line in p.stdout.splitlines():
        if line.startswith("worktree "):
            path = line[len("worktree "):]
        elif line == f"branch refs/heads/{branch}":
            return path
    return None


def merge_locally(repo, t):
    """Local mode: merge the task's branch into its local base branch, as config --merge
    says. On any failure, raise with the reason and leave the task, its worktree and its
    branch as they were (a rebase that went through stays: the branch is still intact)."""
    base, branch, wt = t["base"], t["branch"], t.get("worktree")
    mode = repo.cfg["merge"]
    if git(repo.root, "rev-parse", "-q", "--verify", f"refs/heads/{branch}", check=False).returncode != 0:
        raise Fail(f"merge refused: {t['id']}'s branch {branch} is gone")
    old = git(repo.root, "rev-parse", f"refs/heads/{base}").stdout.strip()
    if int(git(repo.root, "rev-list", "--count", f"{old}..refs/heads/{branch}").stdout.strip()) == 0:
        return  # nothing to merge
    if mode == "rebase":
        if not wt or not Path(wt).exists():
            raise Fail(f"merge refused: {t['id']} has no worktree to rebase in")
        if git(wt, "status", "--porcelain").stdout.strip():
            raise Fail(f"merge refused: {t['id']} has uncommitted changes in {wt}")
        p = git(wt, "rebase", "-q", old, check=False)
        if p.returncode != 0:
            git(wt, "rebase", "--abort", check=False)
            raise Fail(f"merge refused: {branch} doesn't rebase cleanly onto {base}; "
                       f"rebase it by hand in {wt}, then accept again")
        new = git(repo.root, "rev-parse", f"refs/heads/{branch}").stdout.strip()
    else:
        p = git(repo.root, "merge-tree", "--write-tree", "--no-messages", old, f"refs/heads/{branch}",
                check=False)
        if p.returncode != 0:
            raise Fail(f"merge refused: {branch} conflicts with {base}; "
                       f"rebase it onto {base} in {wt}, then accept again")
        tree = p.stdout.split()[0]
        if mode == "squash":
            msg = f"{t['title']} ({t['id']})"
            if t.get("summary"):
                msg += "\n\n" + t["summary"].strip()
            parents = ["-p", old]
        else:
            msg = f"Merge {branch}: {t['title']} ({t['id']})"
            parents = ["-p", old, "-p", f"refs/heads/{branch}"]
        new = run(["git", "-C", str(repo.root), "commit-tree", tree, *parents, "-F", "-"],
                  input=msg + "\n").stdout.strip()
    move_base(repo, base, old, new)
    t["merged"] = new


def move_base(repo, base, old, new):
    """Fast-forward the local base branch from old to new. Where it's checked out, do it
    with `merge --ff-only` there, which keeps uncommitted changes to other files and
    refuses if the incoming commits touch one of them."""
    at = checked_out_at(repo, base)
    if at:
        p = git(at, "merge", "-q", "--ff-only", new, check=False)
        if p.returncode != 0:
            msg = " ".join((p.stderr or p.stdout).split())
            raise Fail(f"merge refused: can't fast-forward {base} in {at}: {msg}")
        return
    if git(repo.root, "merge-base", "--is-ancestor", old, new, check=False).returncode != 0:
        raise Fail(f"merge refused: {base} can't be fast-forwarded")
    git(repo.root, "update-ref", "-m", "bullpen accept", f"refs/heads/{base}", new, old)


def changed_files(repo, t):
    """The files a task's branch changes against its base (both paths of a rename), or
    None when that can't be told (no worktree, or a base that already has the branch)."""
    wt = t.get("worktree")
    if not wt or not Path(wt).is_dir():
        return None
    p = git(wt, "diff", "-z", "--name-only", "--no-renames", f"{task_base_ref(repo, t)}...HEAD", check=False)
    return (set(p.stdout.split("\0")) - {""} or None) if p.returncode == 0 else None


def rebase_others(repo, accepted, touched=None):
    """After an accept, bring the other open tasks on the same base up to date: tell a
    working agent to rebase when the accepted change (the files in touched) conflicts
    with its work, rebase one between turns, and leave a running review be. One news
    line sums it up. Off with `config --rebase-others off`."""
    if not repo.cfg["rebase_others"]:
        return
    base = accepted["base"]
    others = [o for o in repo.tasks() if o["id"] != accepted["id"] and o["status"] not in (*CLOSED, "finished")
              and o.get("base") == base and o.get("worktree") and Path(o["worktree"]).is_dir()]
    if not others:
        return
    if repo.has_origin():
        git(repo.root, "fetch", "-q", "origin", base, check=False)
    said = []
    for o in others:
        try:
            line = rebase_other(repo, o, accepted, touched)
        except Fail as e:
            line = f"{o['id']} not rebased ({e})"
        if line:
            said.append(line)
    if said:
        news(repo, accepted, "rebased", f"After {accepted['id']}: {'; '.join(said)}.", everyone=True)


def rebase_other(repo, t, accepted, touched=None):
    """Bring one task up to date with its base; say how in a few words, or None when
    there was nothing to do."""
    wt, ref = t["worktree"], task_base_ref(repo, t)
    if git(wt, "merge-base", "--is-ancestor", ref, "HEAD", check=False).returncode == 0:
        return None  # already on top of it
    r = t.get("review") or {}
    if t["status"] == "review" or (r.get("block") and not r.get("verdict")):
        return f"{t['id']} left in review"  # a reviewer is on it; its next ready rebases it
    if t["status"] == "working" and rex_alive(t.get("session"), t.get("block")):
        return tell_to_rebase(t, accepted, ref, touched)
    if not worker_idle(t):
        who = "the person" if t["status"] == "needs" else "its coordinator"
        return f"{t['id']} waiting on {who}, not rebased"  # mid-turn: on a prompt, or failed
    if git(wt, "status", "--porcelain").stdout.strip():
        return f"{t['id']} has uncommitted changes, not rebased"
    p = git(wt, "rebase", "-q", ref, check=False)
    both = []  # files where bullpen kept both sides of a conflict
    while p.returncode != 0:
        files = git(wt, "diff", "--name-only", "--diff-filter=U", check=False).stdout.split()
        shown = git(wt, "diff", check=False).stdout  # the conflicts, before anything is resolved
        if not files or not all(keep_both_sides(wt, f) for f in files):
            git(wt, "rebase", "--abort", check=False)
            return rebase_conflict(repo, t, accepted, ref, files, shown)
        git(wt, "add", "--", *files)
        both += [f for f in files if f not in both]
        save_conflicts(repo, t, files, shown)
        p = subprocess.run(["git", "-C", str(wt), "rebase", "--continue"], env={**os.environ, "GIT_EDITOR": "true"},
                           stdin=subprocess.DEVNULL, text=True, capture_output=True)
    if t.get("pr"):
        p = git(wt, "push", "-q", "--force-with-lease", "origin", f"HEAD:refs/heads/{t['branch']}",
                check=False)
        if p.returncode != 0:
            return f"{t['id']} rebased, but the push failed: {' '.join((p.stderr or p.stdout).split())}"
    t["rebased"] = t["base"]
    if t["status"] == "ready" and t.get("diffstat"):
        t["diffstat"] = shortstat(wt, ref)
    rebased = f"{t['id']} rebased" + (f", keeping both sides of a conflict in {', '.join(both)}" if both else "")
    if (t.get("review") or {}).get("verdict") == "approved":
        check = rebase_check(t, ref)
        if check and check[0] == "kept":
            keep_approval(t, ref)
            repo.save_task(t)
            return f"{rebased}; its approval carried over ({check[1]})"
        if t["status"] == "ready" and repo.cfg["review"]:
            start_review(repo, t, ref, rebase=check[1] if check else None)
            if check:
                return f"{rebased}; a rebase check looks at the resolution"
            return f"{rebased}, but its patch changed: a reviewer checks it again"
    repo.save_task(t)  # same status: it stays where it was
    return rebased


# ---------- rebasing the others after an accept ----------

def rebase_need(wt, ref, touched):
    """Whether a working task's work meets the accepted change (the files in touched;
    None: whatever its base gained since the task branched off). Returns ("conflict",
    files) for committed work that doesn't merge cleanly, ("uncommitted", files) for
    uncommitted changes in files the change touched, or (None, why) to leave it alone."""
    names = lambda *a: set(git(wt, *a, "-z", check=False).stdout.split("\0")) - {""}
    if touched is None:
        touched = names("diff", "--name-only", "--no-renames", f"HEAD...{ref}")
    committed = names("diff", "--name-only", "--no-renames", f"{ref}...HEAD")
    uncommitted = (names("diff", "--name-only", "--no-renames", "HEAD") |
                   names("ls-files", "--others", "--exclude-standard"))
    if not touched & (committed | uncommitted):
        return None, "no overlap"
    conflicts = set()
    if touched & committed:
        p = git(wt, "merge-tree", "--write-tree", "--name-only", "--no-messages", "-z", ref, "HEAD",
                check=False)
        if p.returncode == 1:  # the tree, then the conflicted files
            conflicts = set(p.stdout.split("\0")[1:]) - {""}
        elif p.returncode != 0:  # it couldn't tell: say so to the worker rather than guess
            conflicts = touched & committed
    if conflicts:
        return "conflict", sorted(conflicts)
    if touched & uncommitted:
        return "uncommitted", sorted(touched & uncommitted)
    return None, "no conflict"


def tell_to_rebase(t, accepted, ref, touched):
    """A working task: type into its pane that the base moved, only when that meets its
    work (a conflict, or uncommitted changes in the files the accepted change touched)."""
    need, files = rebase_need(t["worktree"], ref, touched)
    if not need:
        return f"{t['id']} left alone ({files})"
    names = ", ".join(files)
    if need == "conflict":
        what, said = f"your branch conflicts with it in {names}", f"conflicts in {names}"
    else:
        what = f"it changed {names}, where you have uncommitted changes"
        said = f"uncommitted changes in {names}"
    rex_type(t["session"], t["block"],
             f"{t['base']} moved: {accepted['id']} was just accepted, and {what}. At a good point, "
             f"{onto(ref)} and fix the conflicts.")
    return f"{t['id']} told to rebase ({said})"


def onto(ref):
    """How a worker rebases onto ref, in words for its prompt."""
    if ref.startswith("origin/"):
        return f"run `git fetch origin`, then rebase your branch onto {ref}"
    return f"rebase your branch onto {ref}"


def save_conflicts(repo, t, files, shown):
    """Note on the task's review the files a rebase conflicted in (adding to any noted
    before), and keep the conflicts as git showed them, for a rebase check."""
    r = t.get("review")
    if not r:
        return
    r["conflict"] = sorted(set(r["conflict"] if isinstance(r.get("conflict"), list) else []) | set(files))
    f = repo.dir / "briefs" / f"{t['id']}.rebase-conflicts.diff"
    old = f.read_text() if r.get("conflict_diff") and f.is_file() else ""
    f.write_text(old + shown)
    r["conflict_diff"] = str(f)


CONFLICT = re.compile(r"^(<{7}|\|{7}|={7}|>{7})( |$)")


def keep_both_sides(wt, path):
    """Resolve a trivial conflict mid-rebase: in a doc or test file that both sides
    changed, every conflict is lines both sides added at the same place (nothing in
    the base between them). Writes the file with the base's lines, then the task's, and
    returns True; returns False, leaving it be, for anything else."""
    if not (is_doc(path) or path.startswith("tests/") or "/tests/" in path or
            Path(path).name.startswith("test_")):
        return False
    stages = {row.split()[2] for row in git(wt, "ls-files", "-u", "--", path).stdout.splitlines()}
    if stages != {"1", "2", "3"}:
        return False  # added on both sides, deleted on one, ...
    if git(wt, "checkout", "--conflict=diff3", "--", path, check=False).returncode != 0:
        return False
    file = Path(wt) / path
    try:
        lines = file.read_text().splitlines(keepends=True)
    except (OSError, UnicodeDecodeError):
        return False
    out, part, sides, found = [], None, {}, False
    for line in lines:
        m = CONFLICT.match(line.rstrip("\r\n"))
        mark = m.group(1)[0] if m else None
        if part is None:
            if mark == "<":
                part, sides, found = "ours", {"ours": [], "base": [], "theirs": []}, True
            else:
                out.append(line)  # outside a conflict, "=======" is just a line
        elif mark is None:
            sides[part].append(line)
        elif (part, mark) == ("ours", "|"):
            part = "base"
        elif (part, mark) == ("base", "="):
            part = "theirs"
        elif (part, mark) == ("theirs", ">"):
            if sides["base"]:
                return False  # one side changed lines the other kept or changed
            out += sides["ours"] + sides["theirs"]
            part = None
        else:
            return False  # a marker out of place
    if part or not found:
        return False
    file.write_text("".join(out))
    return True


def rebase_conflict(repo, t, accepted, ref, files, shown=""):
    """The task's branch doesn't rebase onto the moved base: it's left as it was, and
    the conflict goes to its worker, like review findings do."""
    names = ", ".join(files) or "some files"
    # resolved by hand: its next ready keeps the approval if its own lines are the same,
    # gets a rebase check if they changed only in these files, else a follow-up review
    save_conflicts(repo, t, files, shown)
    msg = (f"{t['base']} moved ({accepted['id']} was accepted) and your branch doesn't rebase "
           f"cleanly onto it: conflicts in {names}. Your branch is as it was. Now "
           f"{onto(ref)}, fix the conflicts, rerun the tests, commit, and run your ready "
           f"command again.")
    failed = hand_back(repo, t, msg)
    if failed:  # left where it was
        return (f"{t['id']} conflicts in {names}, but the message didn't reach its worker "
                f"({failed}): `bullpen tell {t['id']}` it to rebase onto {ref} and fix them")
    return f"{t['id']} conflicts in {names}, sent back"


# ---------- drop ----------

def cmd_drop(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t.get("worktree") and Path(t["worktree"]).exists() and not args.force:
        if git(t["worktree"], "status", "--porcelain").stdout.strip():
            raise Fail(f"{t['id']} has uncommitted changes in {t['worktree']}; pass --force to discard them")
    rex_close(t["session"], t["block"])
    unindex_block(t["block"])
    close_reviewer(t)
    if t.get("pr"):
        run([repo.gh(), "pr", "close", t["pr"]], cwd=repo.root, check=False)
    remove_worktree(repo, t, force=args.force)
    set_status(t, "dropped", t.get("why"))
    t.update(block=None, worktree=None)
    repo.save_task(t)
    print(f"{t['id']} dropped")
    tell_coordinator(repo, t, "dropped", f"{t['id']} dropped")
    dropped_dep(repo, t)
    start_queued(repo, t, "dropped")


# ---------- one task line, as the board says it ----------
#
# `ls`, the picker and `show` say a task the way the board does (board/src/model.rs):
# its mark, ID, title, age and the board's sentence for it, in the board's order.
# Change one, change the other.

MARKS = {"needs": "●", "stopped": "■", "triage": "◎", "ready": "✔", "finished": "◆", "done": "·",
         "review": "◐", "working": "○", "todo": "◌", "dropped": "–"}
# the board's groups, in order: what you do next first
BOARD_ORDER = {"needs": 0, "stopped": 0, "triage": 1, "ready": 2, "finished": 3, "review": 4,
               "working": 5, "todo": 6, "done": 7, "dropped": 7}
MARK_COLORS = {"needs": "1;33", "stopped": "1;31", "ready": "1;32", "finished": "1;32", "review": "1;34"}
MARK_SET = set(MARKS.values())
STALE = 300  # seconds a task can need you before its age turns red, as on the board


def task_num(t):
    return int(t["id"].rsplit("-", 1)[1])


def board_order(tasks):
    """Tasks as the board lists them: by group (needs you first), then by number."""
    return sorted(tasks, key=lambda t: (BOARD_ORDER.get(t["status"], 8), task_num(t)))


def mark_color(t):
    """The SGR colour of a task's mark: a quiet agent's in the colour of one that needs you."""
    return MARK_COLORS["needs"] if is_stuck(t) else MARK_COLORS.get(t["status"], "2")


def stamp(t):
    """When it entered its current status."""
    return t.get("since") or t.get("updated")


def ago(t, at=None):
    """ "3m", "2h", "1d", "now", or "" with no time."""
    if not stamp(t):
        return ""
    s = max(int(seconds_since(stamp(t), at)), 0)
    for unit, n in (("d", 86400), ("h", 3600), ("m", 60)):
        if s >= n:
            return f"{s // n}{unit}"
    return "now"


def stale(t, at=None):
    return t["status"] == "needs" and bool(stamp(t)) and seconds_since(stamp(t), at) >= STALE


DOING_FOR = 30 * 60  # how old a note of what an agent is doing can be and still show


def doing_now(t, at=None):
    """What the agent the task waits on last said it's doing, on one line, as the board
    shows it in place of its sentence: "" when it hasn't said, isn't working, said it
    before the task's status (less 30 s), over DOING_FOR ago, or is quiet or limited."""
    block = (t.get("review") or {}).get("block") if t["status"] == "review" else \
        t.get("block") if t["status"] == "working" else None
    if not block or is_stuck(t) or is_limited(t) or t.get("note"):
        return ""
    d = read_json(STATE / "doing" / f"{safe(block)}.json", {})
    if not isinstance(d, dict) or d.get("state") != "working" or not isinstance(d.get("at"), (int, float)):
        return ""
    at = at or dt.datetime.now().astimezone()
    since = dt.datetime.fromisoformat(stamp(t)).timestamp() - 30 if stamp(t) else float("-inf")
    if d["at"] < since or at.timestamp() - d["at"] > DOING_FOR:
        return ""
    return " ".join(str(d.get("text") or "").split())


BOARD_KINDS = {"full": "full review", "light": "light review", "followup": "follow-up review",
               "tour": "review of your comments", "rebase": "rebase check"}


def first_sentence(summary):
    """ "Recorded the demo." from "# Recorded the demo", or "" when there's none."""
    for line in (summary or "").splitlines():
        line = line.strip().lstrip("#").strip()
        if line:
            return line if line.endswith((".", "!", "?")) else line + "."
    return ""


def limit_detail(lim, at=None):
    reviewer = lim.get("who") == "reviewer"
    who = "Reviewer stopped at the usage limit" if reviewer else "Usage limit"
    if lim.get("resets"):
        return f"{who}, resets {clock(dt.datetime.fromisoformat(lim['resets']), at)}. Continues by itself then."
    return f"{who}, reset time unknown. {'⏎ opens it.' if reviewer else 't tells it to continue.'}"


def status_detail(repo, t, every, at=None):
    """The board's sentence for where the task stands."""
    r = t.get("review") or {}
    rnd, why, st = r.get("round") or 1, t.get("why") or "", t["status"]
    pr = pr_short(t.get("pr")) or (f"Local branch {t.get('branch') or ''}" if t.get("local") else "")
    if t.get("note"):
        return t["note"]
    if is_stuck(t):
        quiet = stuck_phrase(t, at)
        if t["stuck"]["who"] == "reviewer":
            return f"Reviewer has shown {quiet}. ⏎ opens it."
        return f"{quiet[0].upper()}{quiet[1:]}. p shows its screen."
    if is_limited(t):
        return limit_detail(t["limit"], at)
    if st in ("needs", "triage") and why == "Ended its turn without saying ready":
        return "Stopped mid-task, probably with a question. p shows its screen." if st == "needs" \
            else "Ended its turn without ready."
    if st in ("needs", "triage"):
        return why or ("Waiting for you." if st == "needs" else "Waiting for its coordinator.")
    if st == "stopped":
        return "Its agent exited. r resumes it."
    if st == "review":
        kind = BOARD_KINDS.get(r.get("kind"), "review" if r.get("kind") else "reviewer")
        return f"{kind[0].upper()}{kind[1:]} on round {rnd} of {repo.cfg['review_rounds']}."
    if st == "ready" and why.startswith("Nothing to merge"):
        return "Done, with nothing to merge."
    if st == "finished":
        line = first_sentence(t.get("summary"))
        return f"Finished, nothing to merge: {line} a closes it." if line else "Finished, nothing to merge. a closes it."
    if st == "ready":
        text = {"approved": f"{pr}, approved by {BOARD_KINDS.get(r.get('kind'), 'review')}.",
                "changes": f"{pr}. Review still asks for changes."}.get(r.get("verdict")) or (f"{pr}." if pr else "Done.")
        return text + (f" Rebased onto {t['rebased']}." if t.get("rebased") else "")
    if st == "working" and sent_back_by_github(t):
        return "Fixing what GitHub said."
    if st == "working" and r.get("verdict") == "changes":
        return f"Fixing what review round {rnd} found."
    if st == "todo":
        waits = waits_phrase(repo, t, every)
        return f"{waits}." if waits else "Not started. s starts it."
    if st == "done" and t.get("local") and not t.get("pr"):
        return f"Accepted. Merged into {t['base']}." if t.get("merged") and t.get("base") else "Accepted."
    if st == "done" and t.get("closed"):
        return "Closed, nothing to merge."
    if st == "done":
        return f"Accepted. {pr} merged." if pr else "Accepted."
    return "Dropped." if st == "dropped" else ""


def auto_detail(repo, t):
    """What auto-accept does with it, when that isn't what the config says for all."""
    on = bool(repo.cfg.get("auto_accept"))
    if t["status"] in CLOSED:
        return ""
    if t.get("auto_wait") and t["status"] == "ready":
        return "Auto-accepts once its checks pass."
    if on and t.get("auto_accept") is False:
        return "Kept for you: no auto-accept."
    if not on and t.get("auto_accept") is True:
        return "Auto-accepts once approved."
    return ""


def board_detail(repo, t, every=None, at=None):
    """The board's sentence for a task: what its agent is doing now, else where it
    stands, what GitHub said, what auto-accept does with it and its own model."""
    doing = doing_now(t, at)
    if doing:
        return doing
    every = every if every is not None else {o["id"]: o for o in repo.tasks()}
    text = status_detail(repo, t, every, at)
    gh = "" if t["status"] in CLOSED or t.get("note") else pr_phrase(t)
    if gh:
        text = f"{text} {gh[0].upper()}{gh[1:]}.".strip()
    model = own_model(repo, t) if t["status"] not in CLOSED else None
    return " ".join(p for p in (text, auto_detail(repo, t), f"Model: {model}." if model else "") if p)


def text_width(text):
    return sum(2 if unicodedata.east_asian_width(c) in "WF" else 0 if unicodedata.combining(c) else 1
               for c in text)


def fit(text, w):
    """Cut text to w columns, with an ellipsis when it doesn't fit."""
    if text_width(text) <= w:
        return text
    out, used = "", 0
    for c in text:
        cw = text_width(c)
        if used + cw > w - 1:
            break
        out, used = out + c, used + cw
    return out + "…" if w > 0 else ""


def sgr(code, text, color):
    return f"\x1b[{code}m{text}\x1b[0m" if color and text else text


def task_line(repo, t, every=None, idw=0, width=None, color=False, age=True, detail=True, at=None):
    """One task as the board says it: mark, ID, title, age, then the board's sentence.
    Cut to width columns when given (the title keeps room for the age); in colour with
    color, as on the board: the mark in its status's colour, the rest dim."""
    mark, tid = MARKS.get(t["status"], " "), f"{t['id']:<{idw}}"
    when = ago(t, at) if age else ""
    text = board_detail(repo, t, every, at) if detail else ""
    head = 2 + len(tid) + 2
    title = t["title"] if width is None else fit(t["title"], max(width - head - (len(when) + 2 if when else 0), 1))
    if width is not None:
        room = width - head - text_width(title) - (len(when) + 2 if when else 0) - 2
        text = fit(text, room) if room >= 4 else ""
    title_dim = t["status"] in CLOSED or t["status"] == "triage"
    parts = [sgr(mark_color(t), mark, color) + " " + sgr("2", tid, color) + "  " + sgr("2" if title_dim else "0", title, color)]
    if when:
        parts.append(sgr("1;31" if stale(t, at) else "2", when, color))
    if text:
        parts.append(sgr("2", text, color))
    return "  ".join(parts)


def wrap(text, w, lines):
    """text in at most `lines` lines of w columns, the last ending in " …" when cut."""
    out = textwrap.wrap(text, max(w, 1), break_on_hyphens=False) or [""]
    if len(out) > lines:
        out = out[:lines]
        out[-1] = fit(out[-1] + " …", w)
    return out


def task_rows(repo, t, every=None, idw=0, width=80, color=False, at=None, lines=None):
    """The board's rows for a task, for a terminal too narrow for task_line: mark, ID,
    title and the age at the right edge, then the board's sentence under the title in
    `lines` lines (by default the board's: two for one that needs you, else one)."""
    mark, tid, when = MARKS.get(t["status"], " "), f"{t['id']:<{idw}}", ago(t, at)
    w = width - 1  # never into the last column
    head = 2 + len(tid) + 2
    title = fit(t["title"], max(w - head - len(when) - 2, 1))
    pad = " " * max(w - head - text_width(title) - len(when), 2) if when else ""
    title_dim = t["status"] in CLOSED or t["status"] == "triage"
    rows = [sgr(mark_color(t), mark, color) + " " + sgr("2", tid, color) + "  " + sgr("2" if title_dim else "0", title, color)
            + pad + sgr("1;31" if stale(t, at) else "2", when, color)]
    text = board_detail(repo, t, every, at)
    n = lines or (2 if t["status"] in ("needs", "stopped", "finished") or is_stuck(t) or is_limited(t) or pr_phrase(t) else 1)
    rows += [" " * head + sgr("2", line, color) for line in wrap(text, w - head, n) if line]
    return rows


def term_width():
    """The terminal's width when stdout is one, else None: piped output is whole."""
    return shutil.get_terminal_size().columns if sys.stdout.isatty() else None


def colored():
    return sys.stdout.isatty() and not os.environ.get("NO_COLOR")


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


def cmd_ls(args):
    if args.all_repos:
        return ls_repos(args)
    repo = Repo(repo_root())
    tasks = listed(repo, args)
    if args.json:
        print(json.dumps(tasks, indent=2))
        return
    if not tasks:
        print("no open tasks" + ("" if args.all else " (--all shows finished ones)"))
    for line in ls_lines(repo, tasks, term_width(), colored()):
        print(line)


def listed(repo, args):
    """The tasks `ls` lists: the open ones, or all with --all."""
    return [t for t in repo.tasks() if args.all or t["status"] not in CLOSED]


SENTENCE_ROOM = 24  # columns a cut line must leave for the board's sentence, or ls takes two rows


def ls_lines(repo, tasks, width=None, color=False):
    """`ls`'s lines, in the board's order: one task_line each, whole when piped (width
    None), cut to the width on a terminal. When a line would leave under SENTENCE_ROOM
    columns for some task's sentence (a narrow terminal, a long title), the board's
    two rows each instead."""
    every = {t["id"]: t for t in repo.tasks()}
    idw = max((len(t["id"]) for t in tasks), default=0)
    tasks = board_order(tasks)
    if width is None:
        return [task_line(repo, t, every, idw, color=color) for t in tasks]
    room = lambda t: width - 1 - (2 + idw + 2) - text_width(t["title"]) - (len(ago(t)) + 2 if ago(t) else 0) - 2
    if all(room(t) >= SENTENCE_ROOM or text_width(task_line(repo, t, every, idw)) < width for t in tasks):
        return [task_line(repo, t, every, idw, width - 1, color) for t in tasks]
    return [row for t in tasks for row in task_rows(repo, t, every, idw, width, color)]


def ls_repos(args):
    """`ls --all-repos`: the open tasks of every repository bullpen knows, by repository.
    One with nothing open, or whose checkout is gone, is left out."""
    groups = [(r, ts) for r in known_repos() for ts in [listed(r, args)] if ts]
    if args.json:
        print(json.dumps([{"repo": str(r.root), "tasks": ts} for r, ts in groups], indent=2))
        return
    if not groups:
        print("no open tasks in any repository" + ("" if args.all else " (--all shows finished ones)"))
    width = term_width()
    for i, (r, ts) in enumerate(groups):
        if i:
            print()
        print(f"{r.root.name}  ({r.root})")
        for line in ls_lines(r, ts, width and width - 2, colored()):
            print("  " + line)


def model_line(repo, t):
    """ "opus (its own, over the config's sonnet)", and what runs now if that's another."""
    cfg = repo.cfg.get("worker_model")
    line = f"{own_model(repo, t)} (its own, over the config's{' ' + cfg if cfg else ''})"
    now_on = t.get("model") if rex_alive(t.get("session"), t.get("block")) else None
    if now_on and now_on != own_model(repo, t):
        line += f"; running {now_on} until its next start or resume"
    return line


def cmd_show(args):
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if args.json:
        print(json.dumps(t, indent=2))
        return
    detail, width = board_detail(repo, t), term_width()
    if width is None or text_width(task_line(repo, t)) < width:
        print(task_line(repo, t, color=colored()))
    else:
        print("\n".join(task_rows(repo, t, width=width, color=colored(), lines=99)))
    for k in ("why", "base", "branch", "worktree", "block", "model", "pr", "diffstat", "summary"):
        if k == "model" and own_model(repo, t):
            print(f"{k:>9}: {model_line(repo, t)}")
        elif k == "why" and t.get(k) and t[k] in detail:
            continue  # the first line says it
        elif t.get(k):
            print(f"{k:>9}: " + "\n".join((" " * 11 if l else "") + l for l in str(t[k]).strip().splitlines()).lstrip())
        elif k == "pr" and t.get("local"):
            print(f"{k:>9}: none; local branch, merged into {t['base']} by accept")
    if t.get("after"):
        print(f"    after: {', '.join(t['after'])}")
    if len(repo.cfg["coordinators"]) > 1 or (t.get("coordinator") and not owner(repo, t)):
        c = owner(repo, t)
        print(f"    coord: " + (f"{c['id']} ({c['block']})" if c else
                                 f"{t.get('coordinator') or 'none'}, gone: every running coordinator sees to it"))
    if t.get("auto_accept") is not None:
        print(f"     auto: {'accepts by itself once approved' if t['auto_accept'] else 'kept for you (--no-auto-accept)'}")
    if waits_phrase(repo, t):
        print(f"    waits: {waits_phrase(repo, t)}")
    if pr_phrase(t) and pr_phrase(t).lower() not in detail.lower():
        print(f"   github: {pr_phrase(t)}")
    if is_stuck(t):
        print(f"    quiet: its {t['stuck']['who']} has shown {stuck_phrase(t)}")
    r = t.get("review") or {}
    if t.get("review_mode"):
        print(f"     mode: {t['review_mode']} first review (its own)")
    if r:
        state = r.get("verdict") or "in progress"
        if r.get("verdict") == "approved" and r.get("kept"):
            state += f", kept {r['kept']} time{'s' if r['kept'] != 1 else ''} as the code didn't change"
        kind = review_kind(r) + (f" on {r['model']}" if r.get("model") else "")
        print(f"   review: round {r.get('round')} ({kind}), {state}"
              + (f"; findings: {r['findings']}" if r.get("findings") else ""))
    for name in ("setup", "teardown"):
        log = hook_log(repo, t, name)
        if log.exists():
            failed = " (failed: the worker didn't start)" if name == "setup" and t.get("needs_setup") else ""
            print(f"{name:>9}: {log}{failed}")
    print(f"    brief: {repo.dir / 'briefs' / (t['id'] + '.md')}")
    nxt = next_steps(repo, t)
    print("     next: " + ("\n" + " " * 11).join(wrap(nxt, width - 12, 99) if width else [nxt]))


def next_steps(repo, t):
    """What you can do with the task now: the commands, with the board's keys."""
    tid, st = t["id"], t["status"]
    if st == "needs" and t.get("ask"):
        return f"`bullpen reply {tid} <answer>`, or i on the board"
    if st == "needs" and (t.get("stop") or {}).get("kind") == "prompt":
        return f"`bullpen answer {tid} <option>` (`bullpen peek {tid}` shows the prompt), or i on the board"
    if st == "needs":
        return f"`bullpen go {tid}` opens it (⏎ on the board), or `bullpen tell {tid} <message>` (t)"
    if st == "stopped":
        return f"`bullpen resume {tid}` (r on the board), or `bullpen drop {tid}` (d)"
    if st == "triage":
        return f"nothing: its coordinator is on it. `bullpen go {tid}` opens it (⏎ on the board)"
    if st == "ready":
        return f"`bullpen accept {tid}` (a on the board); `bullpen diff {tid}` shows the change (v)"
    if st == "finished":
        return f"`bullpen close {tid}` (a on the board), or `bullpen drop {tid}` (d)"
    if st == "review":
        return f"nothing until its review is done; `bullpen go {tid} --review` opens the reviewer (⏎ on the board)"
    if st == "working" and (is_stuck(t) or (is_limited(t) and not t["limit"].get("resets"))):
        return f"`bullpen peek {tid}` shows its screen (p on the board), `bullpen tell {tid} <message>` (t)"
    if st == "working":
        return f"nothing until it's ready; `bullpen tell {tid} <message>` steers it (t on the board)"
    if st == "todo" and waits_phrase(repo, t):
        return f"nothing: it starts by itself once what it waits on is accepted; `bullpen start {tid} --force` starts it now"
    if st == "todo":
        return f"`bullpen start {tid}` (s on the board), or `bullpen drop {tid}` (d)"
    return "nothing: it's closed"


def diff_label(t):
    return f"bullpen: {t['id']} diff"


def cmd_diff(args):
    """A task's changes against its base, through a pager; --stat just the files."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    wt = t.get("worktree")
    if not wt or not Path(wt).is_dir():
        raise Fail(f"{is_status(t)}, with no worktree, so there's nothing to diff")
    if args.pane:
        return diff_pane(repo, t, args)
    base_ref = task_base_ref(repo, t)
    if args.stat:
        print(diff_stat(wt, base_ref, t["id"]))
        return
    delta = shutil.which("delta")
    tty = sys.stdout.isatty()
    color = ["--color=always"] if tty and not delta else ["--no-color"]
    text = git(wt, "diff", *color, f"{base_ref}...HEAD").stdout
    loose = git(wt, "diff", *color, "HEAD").stdout
    untracked = git(wt, "ls-files", "--others", "--exclude-standard").stdout.splitlines()
    if loose or untracked:
        text += ("\n" if text else "") + f"{UNCOMMITTED}\n\n" + loose + "".join(f"untracked: {f}\n" for f in untracked)
    if not text:
        text = f"{t['id']} has no changes against {base_ref}.\n"
    page(text, delta if tty else None, tty)
    if args.back:
        b = home_of(repo, t).get("board") or {}
        if rex_alive(b.get("session"), b.get("block")):
            rex_focus(b.get("session"), b["block"])


UNCOMMITTED = "Uncommitted (not in any commit yet):"


def diff_stat(wt, base_ref, tid):
    """The files the branch changed since its base, then any not committed yet."""
    parts = []
    committed = git(wt, "diff", "--stat", f"{base_ref}...HEAD").stdout.rstrip()
    parts.append(committed or f"{tid} has no commits against {base_ref}.")
    loose = git(wt, "diff", "--stat", "HEAD").stdout.rstrip()
    untracked = git(wt, "ls-files", "--others", "--exclude-standard").stdout.splitlines()
    if loose or untracked:
        parts.append("\n".join([UNCOMMITTED] + ([loose] if loose else []) +
                               [f" untracked: {f}" for f in untracked]))
    return "\n\n".join(parts)


def page(text, delta, tty):
    """Show text in delta, else `less -R`; straight to stdout when that's no terminal.
    The pager never quits by itself on a short diff: in a pane, that would close it at once."""
    if not tty:
        sys.stdout.write(text)
        return
    pager = [delta, "--paging=always"] if delta else ["less", "-R", "-+F"]
    try:
        subprocess.run(pager, input=text, text=True)
    except FileNotFoundError:
        sys.stdout.write(text)


def diff_pane(repo, t, args):
    """Show the diff in a floating pane over the board (a tab with --workers tabs);
    quitting the pager closes it and focuses the board again."""
    session = session_for(repo, args) or t.get("session")
    old = find_pane(rex_view(session), label=diff_label(t))
    if old:
        rex_close(session, old[2])  # one diff pane per task, showing the latest
    argv = [SELF, "diff", t["id"], "--back"]
    if repo.cfg.get("workers") == "tabs":
        block = rex_run(session, t["worktree"], diff_label(t), argv, focus=True)
    else:
        block = float_pane(repo, session, diff_label(t), t["worktree"], argv, t)
    show_pane(repo, session, block)


REVIEW_MODES = ("full", "light", "auto")


def cmd_review_mode(args):
    """Set how a task's first review goes: full, light, or auto (by the size of the change)."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] in CLOSED:
        raise Fail(is_status(t))
    if args.mode == "auto":
        t.pop("review_mode", None)
    else:
        t["review_mode"] = args.mode
    write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
    r = t.get("review") or {}
    later = " (it has had a review already: later rounds follow up on it, whatever the mode)" if r.get("round") else ""
    print(f"{t['id']}'s first review: {args.mode}{later}")


# The flags `config` takes that are per repository, for messages: the repo-only
# settings', and --board-others with its --coordinator (per board).
PER_REPO_FLAGS = tuple(s.name for s in SETTINGS if s.repo_only and s.kind != "file") + \
    ("board_others", "coordinator")


def config_flags(p):
    """config's flag for each setting that has one, from SETTINGS."""
    for s in SETTINGS:
        if s.kind == "file":
            continue
        if s.kind == "choice":
            p.add_argument(s.flag, choices=s.choices, help=s.full_help())
        elif s.kind == "on/off":
            p.add_argument(s.flag, choices=["on", "off"], help=s.full_help())
        elif s.kind == "number":
            p.add_argument(s.flag, type=int, metavar=s.metavar, help=s.full_help())
        else:  # text, model
            p.add_argument(s.flag, metavar=s.metavar or ("MODEL" if s.kind == "model" else None),
                           help=s.full_help())


def setting_value(s, v):
    """The value config's flag gives setting s, checked."""
    if s.kind == "on/off":
        return v == "on"
    if s.kind == "number" and v < s.least:
        raise Fail(f"{s.flag} takes {s.says}")
    if s.pattern and not re.fullmatch(s.pattern, v):
        raise Fail(f"{s.name} must be {s.says}")
    if s.clears:
        return v.strip() or None
    return v


def config_changes(args):
    """{key: value} for the settings config's flags set, each checked."""
    ch = {}
    for s in SETTINGS:
        v = getattr(args, s.name, None) if s.kind != "file" else None
        if v is not None and (v != "" or s.clears):
            ch[s.name] = setting_value(s, v)
    if args.model:  # --model is each role's, unless its own flag says otherwise
        for key in ("worker_model", "reviewer_model", "reviewer_followup_model"):
            ch.setdefault(key, args.model)  # "default" too: it drops the command's own --model
    return ch


def check_unset(keys, repo=None):
    """The keys --unset names, checked: settings bullpen has (or this repo's config.json
    has), and not one only a repository has, unless the repo's own is just switched
    off (base, push_remote)."""
    droppable = [k for k in DEFAULT_CONFIG if k not in REPO_ONLY or (repo and DEFAULT_CONFIG[k] is None)]
    for k in keys:
        if k in REPO_ONLY and k not in droppable:
            raise Fail(f"{k} is this repository's own: " + (
                "it can't be in the global config.json" if not repo else f"change it with --{k.replace('_', '-')}"))
        if k in INTERNAL or (k not in DEFAULT_CONFIG and not (repo and k in repo.own)
                             and k not in read_global_file()):
            raise Fail(f"no setting {k}: it is one of " + ", ".join(
                k for k in DEFAULT_CONFIG if k not in REPO_ONLY or DEFAULT_CONFIG[k] is None))
    return keys


def cmd_config(args):
    if args.global_:
        return config_global(args)
    repo = Repo(repo_root())
    ch = config_changes(args)
    for k in check_unset(args.unset or [], repo):
        repo.unset(k)
    if "news" in ch and ch["news"] == "monitor" and repo.cfg.get("news") != "monitor":
        for cid in [None] + [c["id"] for c in repo.cfg["coordinators"]]:
            set_news_cursor(repo, news_size(repo), cid)  # what was typed already isn't news again
        me = calling_coordinator(repo)
        told = [c["id"] for c in repo.cfg["coordinators"] if c is not me and alive(c)]
        if told:
            # The last typed line: running coordinators have to hear of the switch.
            type_news(repo, NEWS_MARK + "news now comes by monitor and is no longer typed: "
                      f"start the Monitor tool on `{follow_cmd()}` now, as your guide says", told)
    if args.board_others:
        c = coordinator(repo, args.coordinator) if args.coordinator else \
            coordinator_at(repo, os.environ.get("REX_BLOCK"))
        if not c:
            raise Fail(f"no coordinator {args.coordinator}" if args.coordinator else
                       "--board-others is per board: run it in a coordinator's pane or board, or add --coordinator ID")
        c["board_others"] = args.board_others == "on"
    for k, v in ch.items():
        repo.set(k, v)
    if "news" in ch:
        print(news_mode_line(repo), file=sys.stderr)
    repo.save()
    reindex(repo)
    show_config(repo, args.json)


def config_global(args):
    """`config --global`: change the global config.json, which every repository's
    settings fall back on."""
    per_repo = [f"--{f.replace('_', '-')}" for f in PER_REPO_FLAGS if getattr(args, f) is not None
                and getattr(args, f) is not False]
    if per_repo:
        raise Fail(f"{', '.join(per_repo)} {'is' if len(per_repo) == 1 else 'are'} per repository, "
                   "so not for the global config: run it without --global, in that repository")
    ch = config_changes(args)
    keys = check_unset(args.unset or [])
    path = global_config_path()
    g = read_global_file()
    if ch or keys:
        for k in keys:
            g.pop(k, None)
        g.update(ch)
        write_json(path, g)
        for cfg in (STATE / "repos").glob("*/config.json"):
            root = (read_json(cfg, {}) or {}).get("root") if cfg.exists() else None
            if root:
                reindex(Repo(root))  # stuck_after can come from here
    try:
        repo = Repo(repo_root())
    except Fail:
        repo = None
    show_config(repo, args.json)


def config_value(v):
    if isinstance(v, list) and all(isinstance(a, str) for a in v):
        return shlex.join(v)
    return v if isinstance(v, str) else json.dumps(v)


# The groups `config` lists the settings in, in SETTINGS's order; a setting not in
# one goes under "Other".
CONFIG_GROUPS = [(g, [s.name for s in SETTINGS if s.group == g])
                 for g in dict.fromkeys(s.group for s in SETTINGS if s.group)]


def config_groups(keys):
    """[(title, keys)]: keys in CONFIG_GROUPS's groups and order, the rest under Other."""
    named = {k for _, ks in CONFIG_GROUPS for k in ks}
    out = [(title, [k for k in ks if k in keys]) for title, ks in CONFIG_GROUPS]
    out.append(("Other", [k for k in keys if k not in named]))
    return [(title, ks) for title, ks in out if ks]


def show_config(repo, as_json=False):
    """Each setting, with where its value comes from: this repo's config.json ("repo"),
    the global one ("global") or bullpen's own default ("default"). Without a repo,
    what a repository gets when it sets nothing itself."""
    path = global_config_path()
    if repo:
        cfg = repo.cfg
        sources = {k: config_source(repo, k) for k in cfg if k not in INTERNAL}
    else:
        glob = read_global()
        cfg = {k: v for k, v in inherited_config().items() if k not in REPO_ONLY}
        sources = {k: "global" if k in glob else "default" for k in cfg}
    models = {role: model_phrase(cfg[role], cfg.get(f"{role}_model")) for role in ("worker", "reviewer")}
    if as_json:
        print(json.dumps({**({"repo": str(repo.root), "state": str(repo.dir)} if repo else {}), **cfg,
                          "models": models, "global": str(path), "sources": sources}, indent=2))
        return
    head = [("repo", home(repo.root)), ("state", home(repo.dir / "config.json"))] if repo else \
        [("repo", "none: not in a git repository")]
    head.append(("global", home(path) + ("" if path.exists() else " (none yet)")))
    for k, v in head:
        print(f"{k + ':':<8}{v}")
    keys = [k for k in sources if k not in ("next", "next_coordinator", "coordinators")]
    cols = term_width()
    for title, group in config_groups(keys):
        print(f"\n{title}")
        width = max(map(len, group))
        vals = {k: homes(str(config_value(cfg[k]))) for k in group}
        # on a terminal, a value too long to fit beside the others isn't lined up with them
        room = cols - width - 6 - max(len(sources[k]) for k in group) if cols else None
        vwidth = max((len(v) for v in vals.values() if room is None or len(v) <= room), default=0)
        for k in group:
            print(f"  {k:<{width}}  {vals[k]:<{vwidth}}  {sources[k]}")
    print()
    print(f"models: worker {models['worker']}; reviewer {models['reviewer']}")
    if repo:
        cs = ", ".join(f"{c['id']} ({c.get('block')})" for c in cfg["coordinators"])
        print(f"coordinators: {cs or 'none: `bullpen up` starts one'}")


def cmd_guide(args):
    print(guide_text())


# ---------- getting around: go, board ----------

def go_target(repo, args):
    """(repo, session, block, name) of the pane `go` should focus."""
    if args.next:
        # only over the tasks the board of the pane's coordinator shows
        c = pane_coordinator(repo, args.from_block)
        needs = [(repo, t) for t in repo.tasks() if t["status"] == "needs" and t.get("block")
                 and on_board(repo, c, t)]
        if args.all_repos:  # and every other repository's, whoever's board they're on
            needs += [(r, t) for r in known_repos() if r.dir != repo.dir
                      for t in r.tasks() if t["status"] == "needs" and t.get("block")]
        needs.sort(key=lambda rt: rt[1].get("since") or rt[1].get("updated") or "")
        if not needs:
            raise Fail("nothing needs you" + (" in any repository" if args.all_repos else ""))
        blocks = [t["block"] for _, t in needs]
        here = args.from_block
        r, t = needs[(blocks.index(here) + 1) % len(needs) if here in blocks else 0]
        return r, t["session"], t["block"], t["id"] if r.dir == repo.dir else f"{r.root.name} {t['id']}"
    return (repo, *pane_target(repo, args))


def next_to_accept(repo, args):
    """`go --next` once nothing needs you: (repo, task) of the task ready to accept, else
    finished, that has waited longest, on a board (the pane's coordinator's, in this
    repository), or None."""
    c = pane_coordinator(repo, args.from_block)
    found = [(repo, t) for t in repo.tasks() if t["status"] in ("ready", "finished") and on_board(repo, c, t)]
    if args.all_repos:
        found += [(r, t) for r in known_repos() if r.dir != repo.dir
                  for t in r.tasks() if t["status"] in ("ready", "finished")]

    def boarded(r, t):
        try:
            return bool(task_board(r, t))
        except Fail:
            return False
    found = [(r, t) for r, t in found if boarded(r, t)]
    found.sort(key=lambda rt: (rt[1]["status"] != "ready", rt[1].get("since") or rt[1].get("updated") or ""))
    return found[0] if found else None


def pane_target(repo, args):
    """(session, block, name) of a task's pane, its reviewer's, the coordinator's or the board's."""
    if args.id and args.board:
        _, b = task_board(repo, repo.task(args.id))
        return b.get("session"), b["block"], "board"
    if args.id and args.review:
        t = repo.task(args.id)
        r = t.get("review") or {}
        if not r.get("block"):
            raise Fail(f"{t['id']} has no reviewer running")
        return r.get("session") or t["session"], r["block"], f"{t['id']}'s reviewer"
    if args.id:
        t = repo.task(args.id)
        if not t.get("block"):
            hint = f"`bullpen resume {t['id']}` starts it again" if t["status"] == "stopped" else \
                is_status(t).replace(f"{t['id']} is ", "it's ", 1).replace(f"{t['id']} ", "it ", 1)
            raise Fail(f"{t['id']} has no pane: {hint}")
        return t["session"], t["block"], t["id"]
    c = coordinator_for(repo, args.from_block)
    if not c:
        raise Fail("no coordinator: run `bullpen up`")
    if args.board:
        b = c.get("board") or next((o["board"] for o in repo.cfg["coordinators"] if o.get("board")), None)
        if not b:
            raise Fail("no board: run `bullpen up`")
        return b.get("session"), b["block"], "board"
    return c.get("session"), c["block"], "coordinator"


def task_board(repo, t):
    """(coordinator, board) of a board that shows the task: its owner's, else any
    other that shows it (one showing other coordinators' tasks too)."""
    own = home_of(repo, t)
    boards = [c for c in [own] + repo.cfg["coordinators"] if c.get("board")]
    if not boards:
        raise Fail("no board: run `bullpen up`")
    shows = [c for c in boards if on_board(repo, c, t)]
    # a board that's running first: one that was quit would only fail
    c = next((c for c in shows if rex_alive(c["board"].get("session"), c["board"].get("block"))),
             shows[0] if shows else None)
    if not c:
        raise Fail(f"no board shows {t['id']}: it is {own['id']}'s, which has no board (o on a board shows it)")
    return c, c["board"]


def select_on_board(repo, tid):
    """Ask the board the task is on to select it. The board reads select/<its
    coordinator> when the state directory changes, selects the task and removes the
    file; it ignores one older than 30 seconds (no board read it then)."""
    c, _ = task_board(repo, repo.task(tid))
    path = repo.dir / "select" / safe(c.get("id") or "board")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(repo.task(tid)["id"] + "\n")


def coordinator_for(repo, block):
    """The coordinator a pane goes with (pane_coordinator), else the primary one."""
    return pane_coordinator(repo, block) or primary(repo)


def pane_coordinator(repo, block):
    """The coordinator a pane goes with: itself, the one beside its board, or the one
    owning its task; None for any other pane."""
    c = coordinator_at(repo, block)
    if not c and block:
        entry = read_json(STATE / "blocks" / f"{safe(block)}.json") or {}
        role = entry.get("role") or ""
        if entry.get("repo") == str(repo.root) and role not in ("coordinator", "board"):
            try:
                c = owner(repo, repo.task(role.split()[0]))
            except Fail:
                pass
    return c


def cmd_go(args):
    if args.all_repos and not args.next:
        raise Fail("--all-repos goes with --next")
    home = go_home(args)
    if args.board and not args.id:
        home = board_for_go(home, args)
    try:
        repo, session, block, name = go_target(home, args)
    except Fail as e:
        if not args.next or not str(e).startswith("nothing needs you"):
            raise
        boards = any(c.get("board") for c in home.cfg["coordinators"])
        if boards:  # a board that was quit starts again, as `go --board` does
            home = board_for_go(home, args)
        up = next_to_accept(home, args)
        if up:
            # nothing needs you, but something is yours to accept or close: the board, on it
            r, t = up
            name = t["id"] if r.dir == home.dir else f"{r.root.name} {t['id']}"
            print(f"bullpen: {e}; {name} is {t['status']}, on the board", file=sys.stderr)
            args.next, args.board, args.id, home = False, True, t["id"], r
        elif boards:
            # A key that does nothing looks broken: show the board, where that's plain.
            print(f"bullpen: {e}; showing the board", file=sys.stderr)
            args.next, args.board = False, True
        else:
            raise
        repo, session, block, name = go_target(home, args)
    if not rex_alive(session, block):
        # after a Rex restart its ID changed (and maybe its session's)
        relink(repo, args.session or os.environ.get("REX_SESSION") or session)
        home = Repo(home.root)
        repo, session, block, name = go_target(home, args)
        if not rex_alive(session, block):
            raise Fail(f"{name}'s pane is gone")
    if args.json:
        print(json.dumps({"session": session, "block": block, "name": name}))
    else:
        if args.board and args.id:
            select_on_board(repo, args.id)
        show_pane(repo, session, block)


def board_for_go(repo, args):
    """`go --board`: when the board it would show isn't running, start the pane's
    coordinator's (as `up` would), rather than refuse. The repo, read again if a Rex
    restart had to be caught up with."""
    c = coordinator_for(repo, args.from_block)
    if c and not alive(c):  # after a Rex restart its ID changed (and maybe its session's)
        relink(repo, args.session or os.environ.get("REX_SESSION") or c.get("session"))
        repo = Repo(repo.root)
        c = coordinator_for(repo, args.from_block)
    if not alive(c):
        return repo  # go_target says why
    b = c.get("board") or next((o["board"] for o in repo.cfg["coordinators"] if o.get("board")), None)
    if not (b and rex_alive(b.get("session"), b.get("block"))):
        ensure_board(repo, c["session"], c, out=sys.stderr)
    return repo


def go_home(args):
    """The repository `go` is about: --repo, else the one the pane or folder belongs to."""
    if not args.repo:
        return find_repo(args.from_block)
    repo = Repo(Path(args.repo).resolve())
    if not (repo.dir / "config.json").exists():
        raise Fail(f"bullpen has no tasks for {args.repo}")
    return repo


def cmd_hide(args):
    """Put the worker or reviewer pane you're in back into its capsule and go back
    to the board. Exits 3 if that pane isn't one of bullpen's floating panes, so a
    key binding can let the key through."""
    repo, role = lookup_block(args.from_block)
    if not repo or role in ("coordinator", "board"):
        sys.exit(3)
    t = repo.task(role.split()[0])
    session = args.session or (t.get("review") or {}).get("session") or t.get("session")
    where = find_pane(rex_view(session), block=args.from_block)
    if not where or where[1].get("kind") != "floating":
        sys.exit(3)
    set_layer(session, where[1]["layer_id"], "minimized")
    c = home_of(repo, t)
    for p in (c.get("board") or {}, c):
        if rex_alive(p.get("session"), p.get("block")):
            rex_focus(p.get("session"), p["block"])
            break


def show_pane(repo, session, block):
    """Focus a pane. A worker's floating pane comes up from its capsule; any other
    worker pane that's up goes back to being a capsule."""
    gather(repo, session)
    view = rex_view(session)
    if view is None:
        return rex_focus(session, block)
    mine = {b for t in repo.tasks() for b in (t.get("block"), (t.get("review") or {}).get("block")) if b}
    target = find_pane(view, block=block)
    for w in view["windows"]:
        for layer in w.get("layers", []):
            if layer.get("kind") != "floating" or layer.get("state") == "minimized":
                continue
            if target and layer["layer_id"] == target[1]["layer_id"]:
                continue
            if any(b.get("block_id") in mine for b in layer.get("blocks", [])):
                set_layer(session, layer["layer_id"], "minimized")
    if target and target[1].get("kind") == "floating":
        if set_layer(session, target[1]["layer_id"], "normal", focus=True):
            return
    rex_focus(session, block)


BOARD_SRC = HOME / "board"
TARGET = BOARD_SRC / "target"
# What boards run: a copy of cargo's output, renamed into place once it's whole, so a
# board never runs a half-written file (cargo's own output is a hard link it rewrites).
BOARD = TARGET / "bullpen-board"
CARGO_BOARD = TARGET / "release" / "bullpen-board"
# {"source": the source hash BOARD was built from, "failed": a hash whose build failed,
#  "skipped": one not built because board/ had uncommitted changes}
BOARD_STAMP = TARGET / "bullpen-board.json"
BOARD_LOG = TARGET / "bullpen-board.log"
BOARD_LOCK = TARGET / "bullpen-board.lock"


def board_exe():
    """The board program `bullpen board` runs: BULLPEN_BOARD, else the copy bullpen
    builds, else cargo's own output (built by an older install.sh)."""
    if os.environ.get("BULLPEN_BOARD"):
        return Path(os.environ["BULLPEN_BOARD"])
    return BOARD if BOARD.exists() or not CARGO_BOARD.exists() else CARGO_BOARD


def cmd_board(args):
    """The board is a Rust program (board/): it reads the repo's state directory and
    does everything through this command. It execs itself again when its program is
    replaced (a rebuild), keeping what it shows."""
    repo = Repo(repo_root())
    exe = str(board_exe())
    if not os.access(exe, os.X_OK):
        raise Fail(f"the board isn't built: run {HOME / 'install.sh'} (needs cargo)")
    mine = ["--coordinator", args.coordinator] if args.coordinator else []
    os.execv(exe, [exe, "--dir", str(repo.dir), "--root", str(repo.root), "--bullpen", SELF, *mine])


# ---------- triage: stops the coordinator sees to ----------
#
# A task that stopped on something its coordinator can handle (a failed turn, a turn
# ended without ready, a message that didn't reach its worker, a reviewer without a
# verdict) is "triage": its coordinator hears of it, and settles it with tell, resume,
# `review`, `ok` or `ask`. Still there after triage_minutes, or with no coordinator
# running, it needs the person ("needs"), as prompts and `ask` do at once.

def triage_minutes(repo):
    return repo.cfg["triage_minutes"]


def overdue(repo, t, at=None):
    """(what the reason gets, why) when a task waiting on its coordinator goes to the
    person now, else None."""
    if t["status"] != "triage":
        return None
    if not sees_to(repo, t):
        return " (no coordinator is running)", "no coordinator is running"
    m = triage_minutes(repo)
    if seconds_since(t.get("since") or t.get("updated") or now(), at) >= m * 60:
        return " (coordinator didn't resolve it)", f"its coordinator didn't resolve it in {m} min"
    return None


def cmd_ask(args):
    """(coordinator) the task needs the person: the question shows on the board."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] in CLOSED:
        raise Fail(is_status(t))
    question = " ".join(" ".join(args.question).split())
    if not question:
        raise Fail("give the question to ask")
    ask(t, question)
    repo.save_task(t)
    tell_coordinator(repo, t, "ask", f"{t['id']} needs the person: {question}")
    print(f"{t['id']} needs the person now: {question}")


def cmd_ok(args):
    """(coordinator) a task's stop is seen to: back to what it was, typing nothing."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] != "triage":
        raise Fail(f"{is_status(t)}, not waiting on its coordinator")
    if reviewers_stop(t):
        r = t.get("review") or {}
        if not rex_alive(r.get("session") or t.get("session"), r.get("block")):
            raise Fail(f"{t['id']}'s reviewer isn't running: `bullpen review {t['id']}` starts a "
                       f"new one, or `bullpen ask` the person")
    elif not rex_alive(t.get("session"), t.get("block")):
        raise Fail(f"{t['id']}'s worker isn't running: `bullpen resume {t['id']}` restarts it")
    ok(t)  # `check` leaves its agent idle; its next turn clears that
    repo.save_task(t)
    status = t["status"]
    print(f"{t['id']} is {'in review' if status == 'review' else status} again (nothing typed to its agent)")


def cmd_review(args):
    """(coordinator) a new reviewer on the task's current commit, in place of one that
    stopped or exited without a verdict."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    r = t.get("review") or {}
    if t["status"] in CLOSED or not t.get("worktree"):
        raise Fail(is_status(t) + ("" if t.get("worktree") else ", with no worktree"))
    if not r:
        raise Fail(f"{t['id']} has had no review: its worker starts one with `ready`")
    if r.get("verdict") or not (t["status"] == "review" or on_reviewer(t)):
        raise Fail(f"{is_status(t)}, not waiting on a reviewer")
    rounds = repo.cfg["review_rounds"]
    if r.get("round", 0) >= rounds and r.get("kind") != "tour":
        raise Fail(f"{t['id']} has used its {rounds} review rounds: `bullpen ask {t['id']}` the person")
    start_review(repo, t, task_base_ref(repo, t))
    print(f"{t['id']}: a new reviewer started (round {t['review']['round']} of {rounds})")


# ---------- watcher events (called by lua/watch.lua) ----------

def cmd_event(args):
    repo, role = lookup_block(args.block)
    if not repo or role in ("coordinator", "board"):
        return
    if role.endswith(" review"):
        return review_event(repo, repo.task(role.split()[0]), args)
    t = repo.task(role)
    if t["block"] != args.block or t["status"] in CLOSED:
        return
    state, msg = args.state, (args.msg or "").strip()
    key = f"{state}|{msg}"
    woke = unstick(t, "worker")
    if state in LIMIT_STATES and (limited(t, "worker") or limit_hit(repo, t, "worker", args)):
        return
    if state == "working" and t["status"] != "working" and compacting(t.get("session"), t["block"], msg):
        # Claude Code compacting the conversation by itself: not a new turn
        if not t.get("compacting"):
            t["compacting"] = now()
            mark_compacting(args.block, True)
        repo.save_task(t)
        return
    if t.pop("compacting", None):
        mark_compacting(args.block, False)
        if state in ("done", "idle") and t["status"] != "working":
            # the compaction is over: its end is no end of a turn
            t["last"] = key
            repo.save_task(t)
            return
    if state == "working":
        if t["status"] in (*WAITING, "stopped") or limit_over(t):
            set_status(t, "working")
        elif t["status"] == "ready" and not (t.get("last") or "").startswith("working|"):
            back_to_work(t)  # it had ended its turn: this is a new one
            approved = (t.get("review") or {}).get("verdict") == "approved"
            if approved:
                t["from_ready"] = True  # an end of turn that changed no code makes it ready again
            news(repo, t, "working", f"{t['id']} is working again (was ready): it must run `ready` again" +
                 ("; its approval holds if the code doesn't change" if approved else ""))
        elif t["status"] == "finished" and not (t.get("last") or "").startswith("working|"):
            back_to_work(t)
            news(repo, t, "working", f"{t['id']} is working again (was finished): it must run `finish` "
                 f"(or `ready`) again")
        t.pop("ok", None)
        t["last"] = key
        repo.save_task(t)
        return
    if key == t.get("last"):
        if woke:
            repo.save_task(t)
        return
    t["last"] = key
    if state == "blocked":
        kind, msg = args.kind or "input", prompt_msg(args.kind or "input", msg)
        stop(repo, t, "worker", "prompt", f"Needs you ({kind})" + (f": {msg}" if msg else ""),
             prompt_news(t["id"], kind, msg), "needs-you")
    elif state == "error":
        stop(repo, t, "worker", "failed", "Failed" + (f": {msg}" if msg else ""),
             f"{t['id']}'s turn failed" + (f": {msg}" if msg else "") +
             f": read it with `bullpen peek {t['id']}`", "failed")
    elif state in ("done", "idle"):
        shell_min = getattr(args, "shell_min", None)  # from `check`: its shell has run this long
        if t["status"] != "working" or (shell_min is None and shell_running(screen(t.get("session"), t["block"]))):
            # a background shell of its own still runs: it takes another turn when that ends
            repo.save_task(t)
            return
        if ready_again(repo, t):
            return
        if shell_min is None:
            line = f"{t['id']} ended its turn without `ready`: read it with `bullpen peek {t['id']}`"
        else:
            line = (f"{t['id']} ended its turn without `ready` {shell_min} min ago, and a background shell "
                    f"of its own still runs (a server, a watch?): read it with `bullpen peek {t['id']}`")
        stop(repo, t, "worker", "ended", "Ended its turn without saying ready", line, "quiet")
    else:
        repo.save_task(t)


def prompt_msg(kind, msg):
    """What Rex said of a prompt, unless it only names its kind ("permission prompt")."""
    msg = (msg or "").strip()
    return "" if msg.lower() in (kind.lower(), f"{kind.lower()} prompt") else msg


def prompt_news(who, kind, msg):
    """The news that a task's agent is at a prompt, which needs the person: "BP-3 is at
    a permission prompt: Allow rm?", "BP-3's reviewer is at a prompt (question): ..."."""
    at = "a permission prompt" if kind == "permission" else f"a prompt ({kind})"
    return f"{who} is at {at}" + (f": {msg}" if msg else "")


def ready_again(repo, t):
    """A ready task's worker took a turn and ended it without ready. If the code is as it
    was approved (committed, the same patch), it is ready again with its approval. True
    if so."""
    if not t.pop("from_ready", None) or not worktree_clean(t):
        return False
    ref = task_base_ref(repo, t)
    kept = approval_kept(t, ref)
    if not kept:
        return False
    keep_approval(t, ref)
    set_status(t, "ready")
    repo.save_task(t)
    ready_news(repo, t, [f"its worker's turn left the code as approved: {kept}"],
               f". `bullpen peek {t['id']}` if it said something", again=True)
    start_queued(repo, t, "ready")  # it worked again, and took a slot: now it's free
    return True


def worktree_clean(t):
    wt = t.get("worktree")
    return bool(wt) and Path(wt).is_dir() and not git(wt, "status", "--porcelain", check=False).stdout.strip()


def review_event(repo, t, args):
    """A reviewer's status: its coordinator hears if it stops, the person if it's at a prompt."""
    r = t.get("review") or {}
    if r.get("block") != args.block or t["status"] not in ("review", *WAITING):
        return
    state, msg = args.state, (args.msg or "").strip()
    stuck = on_reviewer(t)
    woke = unstick(t, "reviewer")
    if state in LIMIT_STATES and (limited(t, "reviewer") or limit_hit(repo, t, "reviewer", args)):
        return
    if state == "working" and stuck and compacting(r.get("session") or t.get("session"), r["block"], msg):
        # compacting by itself: it stays where its coordinator or the person has it
        if not r.get("compacting"):
            r["compacting"] = now()
            mark_compacting(args.block, True)
        repo.save_task(t)
        return
    if r.pop("compacting", None):
        # the compaction is over. Its end changes nothing here: a reviewer stopped in
        # triage or needs stays there, and one in review (`ok`) ended a real turn.
        mark_compacting(args.block, False)
        woke = True  # save the task
    if state == "working":
        back, was_ok = limit_over(t), t.pop("ok", None)
        if stuck or back:
            set_status(t, "review")
        if stuck or woke or back or was_ok:
            repo.save_task(t)
    elif state == "blocked":
        kind, msg = args.kind or "input", prompt_msg(args.kind or "input", msg)
        stop(repo, t, "reviewer", "prompt", f"Reviewer needs you ({kind})" + (f": {msg}" if msg else ""),
             prompt_news(f"{t['id']}'s reviewer", kind, msg), "needs-you")
    elif state in ("done", "idle", "error") and t["status"] == "review":
        if (state != "error" and getattr(args, "shell_min", None) is None
                and shell_running(screen(r.get("session") or t.get("session"), r["block"]))):
            # a background shell of its own still runs (the tests?): it takes another turn
            if woke:
                repo.save_task(t)
            return
        shell_min = getattr(args, "shell_min", None)
        late = (f" {shell_min} min ago, and a background shell of its own still runs" if shell_min is not None else "")
        stop(repo, t, "reviewer", "ended", "Reviewer stopped without a verdict",
             f"{t['id']}'s reviewer stopped without a verdict{late}: `bullpen go {t['id']} --review` "
             f"shows it, `bullpen review {t['id']}` starts a new one", "quiet")
    elif woke:
        repo.save_task(t)


# ---------- agents that show no progress (called by lua/watch.lua) ----------

def cmd_stuck(args):
    """(watcher) a pane has said it's working, and nothing else, for this many minutes.
    News once per quiet spell; the task carries on as it is."""
    repo, role = lookup_block(args.block)
    if not repo or role in ("coordinator", "board") or not repo.cfg.get("stuck_after"):
        return
    who = "reviewer" if role.endswith(" review") else "worker"
    t = repo.task(role.split()[0])
    block = (t.get("review") or {}).get("block") if who == "reviewer" else t["block"]
    if block != args.block or t.get("stuck") or t["status"] != ("review" if who == "reviewer" else "working"):
        return
    since = dt.datetime.now().astimezone() - dt.timedelta(minutes=args.minutes)
    t["stuck"] = {"who": who, "since": since.isoformat(timespec="seconds")}
    repo.save_task(t)
    if who == "reviewer":
        news(repo, t, "stuck", f"{t['id']}'s reviewer has shown no progress for {args.minutes} min: "
             f"`bullpen go {t['id']} --review` shows it")
    else:
        news(repo, t, "stuck", f"{t['id']} has shown no progress for {args.minutes} min: `bullpen peek {t['id']}`")


def unstick(t, who):
    """The agent gave a sign of life: forget that it was quiet. True if it was."""
    if (t.get("stuck") or {}).get("who") != who:
        return False
    del t["stuck"]
    return True


def is_stuck(t):
    """Has the agent the task is waiting on (its worker, or its reviewer in review)
    shown no progress for too long?"""
    s = t.get("stuck")
    return bool(s) and t["status"] == ("review" if s["who"] == "reviewer" else "working")


def stuck_phrase(t, at=None):
    """ "no progress for 25 min", or "" for a task that isn't stuck."""
    if not is_stuck(t):
        return ""
    m = int(seconds_since(t["stuck"]["since"], at) // 60)
    return f"no progress for {m} min" if m < 120 else f"no progress for {m // 60} h"


def cmd_gone(args):
    repo, role = lookup_block(args.block)
    if not repo:
        return
    unindex_block(args.block)
    if role == "coordinator":
        c = coordinator_at(repo, args.block, boards=False)
        if c:
            coordinator_gone(repo, c)
        return
    if role == "board":
        return
    if role.endswith(" review"):
        t = repo.task(role.split()[0])
        r = t.get("review") or {}
        if r.get("block") == args.block:
            r["block"] = None
            if limited(t, "reviewer"):
                pass  # started again when the limit resets
            elif (t["status"] == "review" or on_reviewer(t)) and not r.get("verdict"):
                stop(repo, t, "reviewer", "exited", "Reviewer exited without a verdict",
                     f"{t['id']}'s reviewer exited without a verdict: `bullpen review {t['id']}` starts a new one")
            repo.save_task(t)
        return
    t = repo.task(role)
    if t["block"] != args.block:
        return
    t["block"] = None
    if limited(t, "worker"):
        repo.save_task(t)  # resumed when the limit resets
    elif t["status"] in ("working", *WAITING):
        stop(repo, t, "worker", "exited", "Agent exited before saying ready",
             f"{t['id']} stopped before saying ready: `bullpen resume {t['id']}` continues it")
    else:
        repo.save_task(t)


# ---------- status drift: `bullpen check` ----------
#
# A task's status follows its agent's events; a missed one (a watcher or Rex restart,
# a bug) leaves it out of step. `check` compares each task that has an agent in a
# pane with what Rex says that agent is doing right now, and `--fix` applies the
# event that was missed, through the same paths (and news) as the watcher.

SETTLE = 60  # seconds a new idle, blocked or error state is left alone: its event may still be on the way
SHELL_RE = re.compile(r"\b\d+ (?:background )?shells?\b|shells? still running", re.I)


def screen(session, block):
    """The text on a pane's screen, or "" if Rex can't say."""
    p = run([REX, "capture", *(["-s", session] if session else []), "-b", block], check=False)
    return p.stdout if p.returncode == 0 else ""


def shell_running(text):
    """Does Claude Code's footer say a background shell of its own still runs?"""
    return any(SHELL_RE.search(l) for l in text.splitlines()[-6:])


COMPACT_RE = re.compile(r"^\W{0,2}\s*(?:Compacting conversation|Running Pre-?Compact hooks)")


def compacting(session, block, msg=""):
    """Is Claude Code compacting its conversation (auto-compaction)? Its status record
    says only that it's working, so this goes by its spinner line: "✽ Compacting
    conversation…" (or the PreCompact hooks it runs first)."""
    if re.match(r"(?:Compacting conversation|Running Pre-?Compact hooks)", msg or ""):
        return True
    lines = [l.strip() for l in screen(session, block).splitlines() if l.strip()][-12:]
    return any(COMPACT_RE.match(l) for l in lines)


def program_status(session, block):
    """The program's own (root) status record in the pane, as Rex keeps it, or None."""
    cmd = [REX, "block", "call", *(["-s", session] if session else []), "-b", block, "program_status"]
    p = run(cmd, check=False)
    try:
        records = json.loads(p.stdout).get("records") if p.returncode == 0 else None
    except ValueError:
        return None
    return next((r for r in records or [] if not r.get("id")), None)


def screen_state(text):
    """What Claude Code's screen shows: working (a spinner), blocked (a question),
    idle (its prompt), or None if it's none of those."""
    lines = [l.strip() for l in text.splitlines() if l.strip()][-12:]
    if any("esc to interrupt" in l for l in lines):
        return "working"
    if any(l.startswith("Do you want to") for l in lines):
        return "blocked"
    if any(l.startswith("❯") for l in lines):
        return "idle"
    return None


def agent_state(session, block, at=None):
    """What the agent in a pane is doing now: {"state", "kind", "msg", "age"}, with state
    one of gone, working, blocked, error, idle or shell (its turn ended, but a background
    shell of its own runs, so it takes another). age: seconds in that state, None if
    unknown (the screen doesn't say). None: unsure, or it just changed."""
    if not rex_alive(session, block):
        return {"state": "gone", "kind": "", "msg": "", "age": None}
    r = program_status(session, block)
    if r:
        age = seconds_since(r["updated_at"], at) if r.get("updated_at") else None
        state = r.get("state") or "idle"
        state = state if state in ("working", "blocked", "error") else "idle"
        if state != "working" and age is not None and age < SETTLE:
            # its event may still be on the way (the watcher holds an end of turn back).
            # Not working: the watcher forwards that at once, and a busy agent's record
            # is new every few seconds, as its message changes.
            return None
        out = {"state": state, "kind": r.get("kind") or "", "msg": r.get("msg") or "", "age": age}
    else:  # no record to read: go by the screen
        state = screen_state(screen(session, block))
        if not state:
            return None
        out = {"state": state, "kind": "", "msg": "", "age": None}
    if out["state"] == "idle" and shell_running(screen(session, block)):
        out["state"] = "shell"
    return out


def needs_label(why):
    """ "permission" for "Needs you (permission): ...", else the why in a few words."""
    m = re.match(r"(?:Reviewer )?[Nn]eeds you \(([^)]*)\)", why or "")
    if m:
        return m.group(1)
    return clip((why or "waiting")[:1].lower() + (why or "waiting")[1:], 60)


def doing(a, who="agent"):
    """What the agent is doing, in words: "its agent is idle at its prompt"."""
    return {"idle": f"its {who} is idle at its prompt",
            "blocked": f"its {who} is waiting on you ({a['kind'] or 'input'})",
            "error": f"its {who}'s turn failed",
            "working": f"its {who} is working again"}[a["state"]]


def shell_minutes(a, limit):
    """Minutes an agent has been idle with a background shell running, once that's
    past limit (minutes): a server or a watch it left running, not a test run. Else None."""
    if a["state"] != "shell" or a["age"] is None or a["age"] < limit * 60:
        return None
    return int(a["age"] // 60)


def worker_drift(t, a, limit):
    """(line, fix) when the worker's task status disagrees with what it does, else None.
    fix is the event the watcher missed: "gone", "shell" (idle for long with a shell
    running), or the agent's state."""
    tid, st = t["id"], t["status"]
    if a["state"] == "gone":
        return f"{tid} is marked {st} but its pane is gone", "gone"
    if a["state"] == "shell":
        m = shell_minutes(a, limit)
        if st == "working" and m is not None:
            return (f"{tid} is marked working but its agent has been idle at its prompt for {m} min, "
                    f"with a background shell still running: `bullpen peek` it"), "shell"
        return None  # busy: it takes another turn when its shell ends
    if st == "working" and a["state"] != "working":
        if t.get("ok") and a["state"] != "blocked":
            return None  # its coordinator said it's fine (`bullpen ok`)
        tail = ": `bullpen peek` it" if a["state"] == "idle" else ""
        return f"{tid} is marked working but {doing(a)}{tail}", a["state"]
    if st not in WAITING or t.get("ask"):
        return None  # an ask waits on the person, whatever its agent does
    if a["state"] == "working":
        return f"{tid} is marked {st} ({needs_label(t.get('why'))}) but {doing(a)}", "working"
    if a["state"] == "blocked" and not at_prompt(t):
        return f"{tid} is marked {st} ({needs_label(t.get('why'))}) but {doing(a)}", "blocked"
    return None


def reviewer_drift(t, a, limit):
    """The same for a task waiting on its reviewer."""
    tid, st = t["id"], t["status"]
    if a["state"] == "shell":
        if st != "review" or shell_minutes(a, limit) is None or t.get("ok"):
            return None  # busy: it takes another turn when its shell ends
        # its shell has run too long to count as busy
        return f"{tid} is in review but its reviewer is idle without a verdict", "shell"
    if a["state"] == "gone":
        return f"{tid} is {'in review' if st == 'review' else 'marked ' + st} but its reviewer's pane is gone", "gone"
    if st == "review" and a["state"] in ("idle", "error") and not t.get("ok"):
        return f"{tid} is in review but its reviewer is idle without a verdict", a["state"]
    if st == "review" and a["state"] == "blocked":
        return f"{tid} is in review but {doing(a, 'reviewer')}", "blocked"
    if st in WAITING and a["state"] == "working":
        return f"{tid} is marked {st} ({needs_label(t.get('why'))}) but {doing(a, 'reviewer')}", "working"
    return None


def waited_on(t):
    """("worker" or "reviewer", session, block) for the agent the task waits on now,
    or None: nothing is running for it, or it waits on the usage limit."""
    r = t.get("review") or {}
    if is_limited(t):
        return None
    if t["status"] == "review" or on_reviewer(t):
        return ("reviewer", r.get("session") or t.get("session"), r["block"]) if r.get("block") else None
    if t["status"] in ("working", *WAITING) and t.get("block"):
        return "worker", t.get("session"), t["block"]
    return None


def unready_hint(repo, t, a):
    """A line for an idle worker whose work looks finished but never ran ready, or None."""
    if a["state"] != "idle" or t["status"] not in ("working", *WAITING):
        return None
    wt = t.get("worktree")
    if not wt or not Path(wt).is_dir() or git(wt, "status", "--porcelain", check=False).stdout.strip():
        return None
    ahead = git(wt, "rev-list", "--count", f"{task_base_ref(repo, t)}..HEAD", check=False).stdout.strip()
    if not ahead.isdigit() or int(ahead) == 0:
        return None
    return (f"{t['id']} is idle with commits and a clean worktree but never ran ready: "
            f"`bullpen peek` it, then `bullpen tell` it to run ready if it's done")


def fix_drift(repo, t, who, block, fix, a):
    """Apply the event the watcher missed, through its own path (and news)."""
    if fix == "gone":
        return cmd_gone(argparse.Namespace(block=block))
    if who == "worker":
        t = repo.task(t["id"])
        t["last"] = None  # the event did come: don't take it for a repeat
        write_json(repo.task_path(t["id"]), t)
    args = argparse.Namespace(block=block, state=fix, kind=a["kind"], msg=a["msg"])
    if fix == "shell":
        args.state, args.shell_min = "idle", shell_minutes(a, 0)
    cmd_event(args)


def check_repo(repo, ids, fix, out):
    """Check the repo's tasks (those in ids, if given). Returns how many disagreed."""
    drift = 0
    for t in repo.tasks():
        if t["status"] in CLOSED or (ids and t["id"] not in ids):
            continue
        late = overdue(repo, t)
        if late:
            drift += 1
            if fix:
                escalate(repo, t, late)
                t = repo.task(t["id"])
            out(f"{t['id']} waits on its coordinator, but {late[1]}: it needs the person"
                + (" (fixed)" if fix else ""))
        if seconds_since(t.get("since") or t.get("updated") or now()) < SETTLE:
            continue  # bullpen just changed it (start, tell, ready): its agent has yet to say
        agent = waited_on(t)
        if not agent:
            continue
        who, session, block = agent
        a = agent_state(session, block)
        if not a:
            continue
        if a["state"] == "working" and compacting(session, block, a["msg"]):
            continue  # compacting by itself: no turn, so no drift
        # How long an idle agent's shell may run before it is drift. stuck_after 0 turns
        # off only the "no progress" news: this check still runs, on the default.
        limit = repo.cfg["stuck_after"] or DEFAULT_CONFIG["stuck_after"]
        found = (worker_drift if who == "worker" else reviewer_drift)(t, a, limit)
        if found:
            drift += 1
            line, what = found
            if fix:
                fix_drift(repo, t, who, block, what, a)
                line += " (fixed)"
            out(line)
        hint = unready_hint(repo, repo.task(t["id"]), a) if who == "worker" else None
        if hint:
            out(hint)
    return drift


def cmd_check(args):
    """Compare each task's status with what its agent is doing now; --fix catches up."""
    out = (lambda line: None) if args.quiet else print
    if args.all_repos:
        repos = known_repos()
    else:
        repos = [Repo(repo_root())]
    drift = 0
    for repo in repos:
        ids = {repo.norm(i) for i in args.ids}
        for i in ids:
            repo.task(i)  # an unknown ID is an error, not "all agree"
        drift += check_repo(repo, ids, args.fix, out)
    return 1 if drift and not args.fix else 0


# ---------- the usage limit (called by lua/watch.lua) ----------
#
# Every agent shares one subscription. An agent that reaches its usage limit stops
# with a line like "You've hit your 5-hour limit · resets 3pm": Claude Code reports
# that turn as `error` (or just goes idle), with the line as the record's msg or
# only on its screen. Nothing for the person to do: the task keeps its status,
# carries a `limit` and is continued after the reset. <state>/limit.json holds the
# limit for every repo while it lasts; `start` refuses meanwhile.

LIMIT_FILE = STATE / "limit.json"
LIMIT_RE = re.compile(r"you['’]ve hit your [\w' -]*limit|usage limit reached|limit will reset"
                      r"|out of usage credits", re.I)
LIMIT_STATES = ("error", "idle", "done")  # how a turn ends; a blocked agent waits on the person
LIMIT_GRACE = 120          # seconds after the reset before continuing: Claude Code may do it itself
LIMIT_FORGET = 5 * 3600    # seconds a limit with no known reset time holds `start` back
LIMIT_CONTINUE = "The usage limit has reset. Carry on where you stopped."
DAYS = ("mon", "tue", "wed", "thu", "fri", "sat", "sun")
MONTHS = ("jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec")
RESET_RE = re.compile(
    r"resets?(?:\s+at)?\s+"
    rf"(?:(?P<day>{'|'.join(DAYS)})[a-z]*,?\s+)?"
    rf"(?:(?P<month>{'|'.join(MONTHS)})[a-z]*\.?\s+(?P<date>\d{{1,2}})(?:,|\s+at)?\s+)?"
    r"(?P<h>\d{1,2})(?::(?P<m>\d{2}))?\s*(?P<ap>[ap])?(?(ap)\.?m\b\.?)"
    r"(?:\s*\((?P<tz>[A-Za-z_]+(?:/[A-Za-z0-9_+-]+)*)\))?", re.I)


def limit_reset(text, at=None):
    """When a limit message says the limit resets, as a local time, or None if it
    doesn't say so in a way we can read. Reads "resets 3pm", "resets 3:45pm",
    "resets Mon 12:00am", "resets Oct 9, 3pm", "reset at 3pm (America/New_York)"
    and "resets 15:00"; a time with no date is the next time the clock shows it."""
    at = at or dt.datetime.now().astimezone()
    m = re.search(r"limit reached\|(\d{10})\b", text)  # older Claude Code: a Unix time
    if m:
        return dt.datetime.fromtimestamp(int(m.group(1))).astimezone()
    m = RESET_RE.search(text)
    if not m or not (m["ap"] or m["m"]):
        return None
    hour, minute = int(m["h"]), int(m["m"] or 0)
    if m["ap"]:
        if not 1 <= hour <= 12:
            return None
        hour = hour % 12 + (12 if m["ap"].lower() == "p" else 0)
    if hour > 23 or minute > 59:
        return None
    here = at
    if m["tz"]:
        try:
            import zoneinfo
            here = at.astimezone(zoneinfo.ZoneInfo(m["tz"]))
        except Exception:
            return None
    when = here.replace(hour=hour, minute=minute, second=0, microsecond=0)
    slack = dt.timedelta(minutes=5)  # a time just gone is now, not tomorrow
    if m["month"]:
        try:
            when = when.replace(month=MONTHS.index(m["month"].lower()) + 1, day=int(m["date"]))
        except ValueError:
            return None
        if when < here - dt.timedelta(days=1):
            when = when.replace(year=when.year + 1)
    elif m["day"]:
        when += dt.timedelta(days=(DAYS.index(m["day"].lower()) - when.weekday()) % 7)
        if when <= here - slack:
            when += dt.timedelta(days=7)
    elif when <= here - slack:
        when += dt.timedelta(days=1)
    return when.astimezone()


def clock(when, at=None):
    """ "3:00 PM", with the day when it isn't today: "Mon 12:00 AM", "Oct 9 3:00 PM"."""
    at = (at or dt.datetime.now().astimezone()).astimezone()
    when = when.astimezone()
    text = f"{when.hour % 12 or 12}:{when.minute:02d} {'AM' if when.hour < 12 else 'PM'}"
    days = (when.date() - at.date()).days
    if days == 0:
        return text
    return (f"{when:%a} " if 0 < days < 7 else f"{when:%b} {when.day} ") + text


def limit_phrase(resets, at=None):
    """ "resets 3:00 PM" for a stored reset time, "reset time unknown" for none."""
    return f"resets {clock(dt.datetime.fromisoformat(resets), at)}" if resets else "reset time unknown"


def limit_active(at=None):
    """The usage limit in force ({"at", "resets", "text"}), or None. It holds until
    its reset time; one with no known reset time, for LIMIT_FORGET seconds."""
    lim = read_json(LIMIT_FILE)
    if not lim:
        return None
    if lim.get("resets"):
        return lim if seconds_since(lim["resets"], at) < 0 else None
    return lim if seconds_since(lim["at"], at) < LIMIT_FORGET else None


def limited(t, who):
    """Is this agent of the task (its "worker" or its "reviewer") stopped at the limit?"""
    return is_limited(t) and t["limit"]["who"] == who


def is_limited(t):
    """Is the agent the task is waiting on stopped at the usage limit?"""
    lim = t.get("limit")
    return bool(lim) and t["status"] == ("review" if lim["who"] == "reviewer" else "working")


def limit_why(lim, at=None):
    return f"Usage limit, {limit_phrase(lim.get('resets'), at)}"


def limit_text(session, block, state, msg, seen=None):
    """The limit message an agent's turn ended with, or None. It is the record's msg,
    or (for a turn that failed or went idle, not one that finished) among the last
    lines of its screen. A screen still showing the message we've already acted on
    (`seen`) says nothing new."""
    if state not in LIMIT_STATES:
        return None
    if LIMIT_RE.search(msg):
        return msg
    if state == "done":
        return None
    p = run([REX, "capture", *(["-s", session] if session else []), "-b", block], check=False)
    lines = [" ".join(l.split()) for l in p.stdout.splitlines() if l.strip()][-15:]
    for i in range(len(lines) - 1, -1, -1):
        if LIMIT_RE.search(lines[i]):
            text = clip(" ".join(lines[i:i + 2]), 200)  # with the next line: it may have wrapped
            return None if text == seen else text
    return None


def limit_hit(repo, t, who, args):
    """Did this event say the agent stopped at the usage limit? Then note it on the
    task and for `start`, with one line of news, and return True."""
    r = t.get("review") or {}
    session = r.get("session") if who == "reviewer" else t.get("session")
    text = limit_text(session, args.block, args.state, (args.msg or "").strip(), t.get("limit_seen"))
    if not text:
        return False
    text = clip(text, 200)
    before = limit_active()
    when = limit_reset(text)
    resets = when.isoformat(timespec="seconds") if when else (before or {}).get("resets")
    write_json(LIMIT_FILE, {"at": now(), "resets": resets, "text": text})
    name = f"{t['id']}'s reviewer" if who == "reviewer" else t["id"]
    waits = t["status"] in (("review", *WAITING) if who == "reviewer" else ("working", *WAITING, "stopped"))
    if waits:  # else the task doesn't wait on this agent: nothing to continue later
        lim = {"who": who, "at": now(), "resets": resets, "text": text}
        set_status(t, "review" if who == "reviewer" else "working", limit_why(lim))
        t["limit"] = lim
        t["last"] = f"{args.state}|{(args.msg or '').strip()}" if who == "worker" else t.get("last")
        repo.save_task(t)
    if resets:
        line = f"{name} stopped at the usage limit, {limit_phrase(resets)}: nothing to do" + \
            (", it is continued then" if waits else "")
    elif who == "reviewer":
        line = (f"{name} stopped at the usage limit, and its reset time couldn't be read, so it won't "
                f"be continued by itself: `bullpen go {t['id']} --review` shows it")
    else:
        line = (f"{name} stopped at the usage limit, and its reset time couldn't be read, so it won't "
                f"be continued by itself: `bullpen tell {t['id']} continue` once the limit has reset")
    if not before:
        line += ". `bullpen start` refuses meanwhile (`--force` overrides)"
    news(repo, t, "limit", line)
    return True


def limit_over(t):
    """The agent the task waits on is working again: forget its limit, and let
    `start` through. True if it was limited. limit.json stays, as a limit that has
    reset: other tasks may still wait to be continued (`cmd_limit` removes it)."""
    if not is_limited(t):
        return False
    t["limit_seen"] = t.pop("limit")["text"]
    lim = read_json(LIMIT_FILE)
    if lim:
        write_json(LIMIT_FILE, {**lim, "resets": now()})
    return True


def cmd_limit(args):
    """(watcher) once the limit has reset, continue the agents that stopped at it."""
    if not LIMIT_FILE.exists():
        return
    waiting = False
    for cfg in (STATE / "repos").glob("*/config.json"):
        root = read_json(cfg, {}).get("root")
        if not root or not Path(root).is_dir():
            continue
        repo = Repo(root)
        repo.dir = cfg.parent
        repo.cfg = load_config(cfg.parent)
        done = []
        for t in repo.tasks():
            resets = (t.get("limit") or {}).get("resets")
            if not is_limited(t) or not resets:
                continue  # no known reset time: the person (or coordinator) continues it
            if seconds_since(resets) < LIMIT_GRACE:
                waiting = True
                continue
            done.append((t, continue_limited(repo, t)))
        if done:
            news(repo, done[0][0], "limit-reset",
                 "the usage limit has reset: continued " + ", ".join(said for _, said in done), everyone=True)
    if not waiting and not limit_active():
        LIMIT_FILE.unlink(missing_ok=True)


def continue_limited(repo, t):
    """Tell the task's limited agent to carry on, in a new pane if its old one is
    gone. Returns what to call it in the news."""
    lim = t.pop("limit")
    t.update(limit_seen=lim["text"], why=None)
    r = t.get("review") or {}
    try:
        if lim["who"] == "reviewer":
            if rex_alive(r.get("session"), r.get("block")):
                rex_type(r["session"], r["block"], LIMIT_CONTINUE)
                repo.save_task(t)
                return f"{t['id']}'s reviewer"
            r["round"] = max(0, r.get("round", 1) - 1)  # the round it didn't finish
            start_review(repo, t, task_base_ref(repo, t))
            return f"{t['id']}'s reviewer (its pane was gone: started again)"
        alive = rex_alive(t.get("session"), t.get("block"))
        failed = hand_back(repo, t, LIMIT_CONTINUE)
        if failed:
            raise Fail(failed)
        return t["id"] if alive else f"{t['id']} (its pane was gone: resumed in a new one)"
    except Fail as e:
        stop(repo, t, lim["who"], "limit", f"Couldn't be continued after the usage limit: {clip(str(e), 120)}")
        who = "its coordinator" if t["status"] == "triage" else "the person"
        return f"{t['id']} (not continued, for {who} to see to: {clip(str(e), 120)})"


# ---------- following pull requests after ready (called by lua/watch.lua) ----------

FOLLOWED = ("review", "ready", *WAITING)  # statuses whose PR the watcher looks at
FAILED_CHECK = ("FAILURE", "ERROR", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE")
PR_FIELDS = "state,headRefOid,statusCheckRollup,reviews,comments"


def followed(t):
    """Does the watcher look at this task's PR? Only an open PR of a task that waits
    on review or the person: a working task is about to push anyway."""
    return bool(t.get("pr")) and not t.get("local") and t["status"] in FOLLOWED \
        and (t.get("gh") or {}).get("state", "open") == "open"


def mark_following():
    """<state>/prs exists while some task's PR may need a look: the watcher only
    calls `_prs` then."""
    STATE.mkdir(parents=True, exist_ok=True)
    (STATE / "prs").touch()


def checks(pr):
    """(summary, names of failed checks) from a PR's statusCheckRollup."""
    failed, running, total = [], False, 0
    for c in pr.get("statusCheckRollup") or []:
        total += 1
        name = c.get("name") or c.get("context") or "check"
        result = c.get("conclusion") or c.get("state") or ""
        if result in FAILED_CHECK:
            failed.append(name)
        elif (c.get("status") or "COMPLETED") != "COMPLETED" or result in ("PENDING", "EXPECTED"):
            running = True
    summary = "failing" if failed else "running" if running else "passing" if total else None
    return summary, failed


def is_bot(author):
    login = (author or {}).get("login") or ""
    return bool((author or {}).get("is_bot")) or login.endswith("[bot]")


def feedback(pr):
    """[(key, who, what)] for the people's comments and reviews on a PR. An approval
    is not feedback; a review with no text means comments on the code."""
    out = []
    for c in pr.get("comments") or []:
        if not is_bot(c.get("author")) and (c.get("body") or "").strip():
            out.append((c.get("id") or c.get("url") or c.get("createdAt"),
                        (c.get("author") or {}).get("login") or "someone", clip(c["body"])))
    for r in pr.get("reviews") or []:
        if is_bot(r.get("author")) or r.get("state") not in ("CHANGES_REQUESTED", "COMMENTED"):
            continue
        body = clip(r.get("body"))
        if r["state"] == "CHANGES_REQUESTED":
            what = "requested changes" + (f": {body}" if body else "")
        else:
            what = f"reviewed: {body}" if body else "left comments on the code"
        out.append((r.get("id") or r.get("submittedAt"), (r.get("author") or {}).get("login") or "someone", what))
    return out


def sent_back_by_github(t):
    """Is the worker fixing failed checks or comments from its PR? Set by `send_back`,
    cleared by the next `ready` (or a reviewer's changes)."""
    return bool((t.get("gh") or {}).get("sent"))


def pr_phrase(t):
    """The PR's latest state in a few words, for the board and `show`, or "".
    A working task only shows what GitHub sent it back for: anything else is
    about the old push."""
    g = t.get("gh") or {}
    if g.get("state") in ("merged", "closed"):
        return f"{g['state']} on GitHub"
    if t.get("status") == "working" and not sent_back_by_github(t):
        return ""
    parts = []
    if g.get("checks"):
        parts.append(f"checks {g['checks']}")
    if g.get("new"):
        parts.append(f"{g['new']} new comment{'s' if g['new'] != 1 else ''}")
    return ", ".join(parts)


def cmd_prs(args):
    """(watcher) look at the PRs of tasks past ready: failed checks, people's comments,
    a merge or close outside bullpen."""
    following = False
    for cfg in (STATE / "repos").glob("*/config.json"):
        root = read_json(cfg, {}).get("root")
        if not root or not Path(root).is_dir():
            continue
        repo = Repo(root)
        repo.dir = cfg.parent
        repo.cfg = load_config(cfg.parent)
        tasks = [t for t in repo.tasks() if followed(t)]
        if not tasks or not repo.has_origin():  # local mode: no PRs to follow
            continue
        for t in tasks:
            follow_pr(repo, t)
        following = following or any(followed(t) for t in repo.tasks())
    if not following:
        (STATE / "prs").unlink(missing_ok=True)


def follow_pr(repo, t):
    p = run([repo.gh(), "pr", "view", t["pr"], "--json", PR_FIELDS], cwd=repo.root, check=False)
    if p.returncode != 0:
        return  # offline or rate limited: next time
    try:
        pr = json.loads(p.stdout)
    except ValueError:
        return
    # gh took a while: accept, a verdict or an event may have changed the task meanwhile
    t = repo.task(t["id"])
    if not followed(t):
        return
    g = t.setdefault("gh", {})
    pr_name = pr_short(t["pr"])
    state = (pr.get("state") or "OPEN").lower()
    if state in ("merged", "closed"):
        g["state"] = state
        write_json(repo.task_path(t["id"]), t)
        verb = "accept" if state == "merged" else "drop"
        news(repo, t, f"pr-{state}", f"{t['id']}'s {pr_name} was {state} outside bullpen: "
             f"`bullpen {verb} {t['id']}` cleans up")
        return
    summary, failed = checks(pr)
    head = pr.get("headRefOid") or ""
    seen_failed = set(g.get("failed") or [])
    new_failed = [n for n in failed if f"{head}:{n}" not in seen_failed]
    g["failed"] = sorted(seen_failed | {f"{head}:{n}" for n in failed})
    g["checks"] = summary
    seen = set(g.get("seen") or [])
    new_notes = [(k, who, what) for k, who, what in feedback(pr) if k not in seen]
    g["seen"] = sorted(seen | {k for k, _, _ in new_notes})
    g["new"] = (g.get("new") or 0) + len(new_notes)
    if t.get("auto_wait") and (failed or new_notes):
        t.pop("auto_wait")  # the person (or the worker) has something to look at first
        why = "its PR's checks failed" if failed else "its PR has new comments"
        news(repo, t, "ready", f"{t['id']} not auto-accepted: {why}")
    elif t.get("auto_wait") and summary != "running" and t["status"] == "ready":
        write_json(repo.task_path(t["id"]), t)
        run_auto_accept(repo, t)
        return
    if not new_failed and not new_notes:
        write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
        return
    lines, asks = [], []
    if new_failed:
        lines.append(f"{t['id']}'s PR checks failed: {', '.join(new_failed)}")
        asks.append(f"These checks failed on your pull request ({t['pr']}): {', '.join(new_failed)}. "
                    f"Look at them (`gh pr checks {t['pr']}`) and fix them.")
    if new_notes:
        said = "; ".join(f"{who} {what}" for _, who, what in new_notes)
        n = len(new_notes)
        lines.append(f"{t['id']}'s PR has {n} new comment{'s' if n != 1 else ''}: {clip(said, 200)}")
        asks.append(f"New feedback on your pull request ({t['pr']}): {said}. Read it in full with "
                    f"`gh pr view {t['pr']} --comments` and address it; if you disagree, say why "
                    f"in your ready summary.")
    send_back(repo, t, lines, " ".join(asks))


def send_back(repo, t, lines, ask):
    """PR feedback goes to the task's worker, which fixes it and runs ready again.
    An unfinished review is moot once the work changes: close it, giving its round back."""
    r = t.get("review") or {}
    if r.get("block") and not r.get("verdict"):
        close_reviewer(t)
        r["round"] = max(0, r.get("round", 1) - 1)
    elif r.get("block"):
        close_reviewer(t)
    why = lines[0].removeprefix(f"{t['id']}'s ").rstrip(".")
    if not worker_idle(t):
        # typing now would answer its prompt: keep the feedback for later
        write_json(repo.task_path(t["id"]), t)
        at = "on a prompt" if t["status"] == "needs" else "stopped mid-turn"
        for line in lines:
            news(repo, t, "pr-feedback", f"{line.rstrip('.')}. Not sent to its worker, which "
                 f"is {at}: `bullpen tell {t['id']}` it when it's free")
        return
    if rex_alive(t.get("session"), t.get("block")):
        failed = hand_back(repo, t, clip(f"{ask} Then commit and run your ready command again.", 2000))
        if failed:  # its reviewer, if it had one, is closed: the coordinator passes this on
            stop(repo, t, "worker", "not_sent", why, [f"{line.rstrip('.')}. The message didn't reach its "
                 f"worker ({failed}): `bullpen tell {t['id']}` it" for line in lines], "pr-feedback")
            return
        t.setdefault("gh", {})["sent"] = True  # the board says so once its worker is on it
        repo.save_task(t)
        for line in lines:
            news(repo, t, "pr-feedback", f"{line.rstrip('.')}. Sent back to its worker.")
        return
    t.setdefault("gh", {})["sent"] = True
    stop(repo, t, "worker", "not_sent", why, [f"{line.rstrip('.')}. Its worker's pane is gone: "
         f"`bullpen resume {t['id']}` restarts it" for line in lines], "pr-feedback")


# ---------- core's commands, for bullpen's parser (cli.py) ----------

def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "go", cmd_go, "You", help="focus a task's pane (or the coordinator's, or the board's)")
    p.add_argument("id", nargs="?", help="the task (default: the coordinator)")
    p.add_argument("--next", action="store_true",
                   help="the next task that needs you; once none does, the board on one ready to accept")
    p.add_argument("--all-repos", action="store_true",
                   help="with --next: in any repository bullpen knows, not just this one")
    p.add_argument("--board", action="store_true",
                   help="the task board (with an ID: the task's board, with that task selected)")
    p.add_argument("--review", action="store_true", help="the task's reviewer, not its worker")
    p.add_argument("--from", dest="from_block", default=os.environ.get("REX_BLOCK"),
                   help="the pane you're in (default: this one)")
    p.add_argument("--session", help="the session you're in (key bindings pass it)")
    p.add_argument("--repo", help="the repository the task is in (default: the pane's, else this folder's)")
    p.add_argument("--json", action="store_true", help="print the target instead of focusing it")

    p = command(sub, "hide", cmd_hide, "You", help="put the worker pane you're in back into its capsule")
    p.add_argument("--from", dest="from_block", default=os.environ.get("REX_BLOCK"),
                   help="the pane to put away (default: this one)")
    p.add_argument("--session", help="the session you're in (key bindings pass it)")

    p = command(sub, "board", cmd_board, "You", help="a live list of tasks to switch between")
    p.add_argument("--coordinator", help="the coordinator it sits beside: its tasks are marked")

    p = command(sub, "ls", cmd_ls, "You", help="list tasks")
    p.add_argument("--all", action="store_true", help="closed tasks too (done and dropped)")
    p.add_argument("--all-repos", action="store_true",
                   help="every repository bullpen knows, by repository (those with nothing open left out)")
    p.add_argument("--json", action="store_true", help="the task records as JSON, with their raw status "
                   "(with --all-repos: a list of {repo, tasks})")

    p = command(sub, "show", cmd_show, "You", help="one task")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--json", action="store_true", help="its task record as JSON")

    p = command(sub, "diff", cmd_diff, "You",
                help="a task's changes against its base, in a pager (delta if installed)")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--stat", action="store_true", help="just the files changed, no pager")
    p.add_argument("--pane", action="store_true", help="show it in a floating pane (the board's v)")
    p.add_argument("--back", action="store_true", help=argparse.SUPPRESS)  # focus the board after
    p.add_argument("--session", help="the Rex session for its pane (default: the coordinator's)")

    p = command(sub, "answer", cmd_answer, "You", help="press an option of the prompt a task's agent is at")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("key", help="the option: its number, or y or n")
    p.add_argument("--prompt", metavar="ID", help="refuse unless it is still this prompt (its id from `prompt`)")

    p = command(sub, "reply", cmd_reply, "You",
                help="answer what the coordinator asked about a task: it goes to the coordinator")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("answer", nargs="+", help="the answer")

    p = command(sub, "accept", cmd_accept, "You",
                help="(the person's call) merge a ready task's PR, or its local branch without a remote (config --merge), and clean up; on a finished task (nothing to merge), close it")
    p.add_argument("id", help="the task, like BP-3")

    p = command(sub, "close", cmd_close, "You",
                help="(the person's call) close a finished task: remove its worktree, branch and pane (the same as accept on it)")
    p.add_argument("id", help="the task, like BP-3")

    p = command(sub, "drop", cmd_drop, "You", help="stop a task, close its PR, remove its worktree")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--force", action="store_true", help="drop it even with uncommitted changes in its worktree")

    p = command(sub, "add", cmd_add, "Coordinator", help="add a task; prints its ID")
    p.add_argument("title", help="a short name for the task")
    p.add_argument("--brief", help="what the worker should do, as text")
    p.add_argument("--brief-file", metavar="FILE", help="the same, read from a file (over --brief)")
    p.add_argument("--base", help="branch to start from and open the PR against")
    p.add_argument("--after", action="append", metavar="ID",
                   help="a task this one waits on (repeat, or comma-separated): it starts once they're accepted")
    p.add_argument("--model", help="claude's --model for this task's worker, over the config's, "
                   "whenever it starts (by itself too) or resumes")
    p.add_argument("--review", choices=REVIEW_MODES, dest="review_mode",
                   help="its first review: full, light, or auto (light for a small or docs-only change; the default)")
    p.add_argument("--auto-accept", dest="auto_accept", action="store_true", default=None,
                   help="accept it by itself once review approves it, whatever config --auto-accept says")
    p.add_argument("--no-auto-accept", dest="auto_accept", action="store_false",
                   help="keep it for you to accept even with config --auto-accept on")

    p = command(sub, "after", cmd_after, "Coordinator",
                help="set the tasks a task waits on (--none: nothing)")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("deps", nargs="*", metavar="ID", help="the tasks it waits on: it starts once they're accepted")
    p.add_argument("--none", action="store_true", help="it waits on nothing")

    p = command(sub, "model", cmd_model, "Coordinator",
                help="set the worker model a task starts and resumes with (--clear: the config's)")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("model", nargs="?", help="an alias like opus, or a model ID; default: let Claude Code pick")
    p.add_argument("--clear", action="store_true", help="use the config's worker model again")

    p = command(sub, "review-mode", cmd_review_mode, "Coordinator",
                help="set how a task's first review goes: full, light or auto")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("mode", choices=REVIEW_MODES, help="light: a quick look; auto: light for a small or docs-only change")

    p = command(sub, "start", cmd_start, "Coordinator", help="make a task's worktree and start its worker")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--model", help="claude's --model for this task's worker, over the config's")
    p.add_argument("--session", help="the Rex session for its pane (default: the coordinator's)")
    p.add_argument("--force", action="store_true",
                   help="start even while the usage limit is reached or what it waits on isn't done")
    p.add_argument("--auto-accept", dest="auto_accept", action="store_true", default=None,
                   help="accept it by itself once review approves it, whatever config --auto-accept says")
    p.add_argument("--no-auto-accept", dest="auto_accept", action="store_false",
                   help="keep it for you to accept even with config --auto-accept on")

    p = command(sub, "resume", cmd_resume, "Coordinator",
                help="restart a stopped worker, continuing its conversation")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("message", nargs="?", help="what to type to it first (default: carry on with your brief)")
    p.add_argument("--session", help="the Rex session for its pane (default: the coordinator's)")

    p = command(sub, "tell", cmd_tell, "Coordinator", help="type a message to a task's worker")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("message", nargs="+", help="the words to type")

    p = command(sub, "peek", cmd_peek, "Coordinator", help="print the end of a worker's screen")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--lines", type=int, default=40, help="how many lines (default 40)")
    p.add_argument("--vt", action="store_true", help="keep the screen's colours, as terminal escape codes")

    p = command(sub, "prompt", cmd_prompt, "Coordinator",
                help="the prompt a task's agent is at, and its screen, as JSON (for the board)")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--lines", type=int, default=40, help="how many lines of screen (default 40)")

    p = command(sub, "adopt", cmd_adopt, "Coordinator",
                help="(coordinator) make tasks yours: their news comes to this pane")
    p.add_argument("ids", nargs="+", metavar="ID", help="the tasks")

    p = command(sub, "ask", cmd_ask, "Coordinator",
                help="(coordinator) the task needs the person: the question shows on the board")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("question", nargs="+", help="what the person should decide")

    p = command(sub, "ok", cmd_ok, "Coordinator",
                help="(coordinator) a task waiting on you is seen to: back to work, nothing typed")
    p.add_argument("id", help="the task, like BP-3")

    p = command(sub, "review", cmd_review, "Coordinator",
                help="(coordinator) a new reviewer in place of one that stopped without a verdict")
    p.add_argument("id", help="the task, like BP-3")

    p = command(sub, "note", cmd_note, "Coordinator", help="an informational line on the board about a task")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("text", nargs="+", help="the line")

    p = command(sub, "check", cmd_check, "Coordinator",
                help="compare each task's status with what its agent is doing now")
    p.add_argument("ids", nargs="*", metavar="ID", help="only these tasks")
    p.add_argument("--fix", action="store_true",
                   help="apply the updates the missed events would have made (news included)")
    p.add_argument("--quiet", action="store_true", help="print nothing")
    p.add_argument("--all-repos", action="store_true", help="every repo bullpen knows (the watcher's)")

    p = command(sub, "news", cmd_news, "Coordinator",
                help="print news logged since the last time; --follow keeps printing")
    p.add_argument("--follow", action="store_true", help="keep running and print each line as it is logged")
    p.add_argument("--every", type=float, default=0.5, help=argparse.SUPPRESS)  # seconds between looks

    command(sub, "guide", cmd_guide, "Coordinator", help="print the coordinator's guide")

    p = command(sub, "ready", cmd_ready, "Worker and reviewer",
                help="(worker) push the branch, open the PR (none without a remote), tell the coordinator")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--summary", help="what you did, as text")
    p.add_argument("--summary-file", metavar="FILE", help="the same, read from a file")

    p = command(sub, "finish", cmd_finish, "Worker and reviewer",
                help="(worker) the task is done with nothing to merge: no PR, no review")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--summary", help="what you did, as text")
    p.add_argument("--summary-file", metavar="FILE", help="the same, read from a file")

    p = command(sub, "verdict", cmd_verdict, "Worker and reviewer",
                help="(reviewer) approve a task or send it back with findings")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("verdict", choices=["approve", "changes"], help="approve it, or send it back with changes to make")
    p.add_argument("--summary", help="what you did, as text")
    p.add_argument("--summary-file", metavar="FILE", help="the same, read from a file")

    p = command(sub, "up", cmd_up, "Setup",
                help="start (or register) this repo's coordinator and the watcher")
    p.add_argument("--here", action="store_true", help="make the agent in this pane the coordinator")
    p.add_argument("--session", help="Rex session (default: this pane's)")
    p.add_argument("--no-board", action="store_true", help="don't open the task board")
    p.add_argument("--verbose", action="store_true",
                   help="name each pane's block ID, and how news reaches the coordinator")

    p = command(sub, "config", cmd_config, "Setup",
                help="show or change this repo's settings (--global: every repo's)")
    p.add_argument("--global", dest="global_", action="store_true",
                   help="change the settings every repository gets unless it sets its own, in "
                        "$XDG_CONFIG_HOME/bullpen/config.json (default ~/.config/bullpen/config.json)")
    p.add_argument("--unset", action="append", metavar="KEY",
                   help="drop this repo's own value for a setting, like merge or worker_model, so the "
                        "global one (or the default) applies; with --global, drop the global one (repeatable)")
    p.add_argument("--json", action="store_true",
                   help="print the settings as JSON, with each one's source under \"sources\"")
    config_flags(p)
    p.add_argument("--model", help="claude's --model for workers and reviewers, follow-up ones too "
                   "(an alias like opus, or a model ID; \"default\": let Claude Code pick)")
    p.add_argument("--board-others", choices=["on", "off"],
                   help="with several coordinators: this coordinator's board shows the other ones' tasks too "
                        "(default off; the board's o key)")
    p.add_argument("--coordinator", metavar="ID",
                   help="the coordinator --board-others is for (default: the one whose pane or board runs it)")

    p = command(sub, "rules", cmd_rules, "Setup",
                help="print the rules every worker and reviewer gets, and where they're from")
    p.add_argument("--edit", action="store_true", help="edit your own rules for this repo in $EDITOR")

    # The internal commands below (the watcher's and bullpen's own) have no help and no
    p = command(sub, "_event", cmd_event)
    p.add_argument("block")
    p.add_argument("state")
    p.add_argument("kind", nargs="?")
    p.add_argument("msg", nargs="?")

    p = command(sub, "_stuck", cmd_stuck)
    p.add_argument("block")
    p.add_argument("minutes", type=int)

    command(sub, "_flush", cmd_flush)

    command(sub, "_prs", cmd_prs)

    command(sub, "_limit", cmd_limit)

    p = command(sub, "_gone", cmd_gone)
    p.add_argument("block")

    p = command(sub, "_coordinator_gone", cmd_coordinator_gone)
    p.add_argument("id")
    p.add_argument("--wait", type=float, default=GONE_WAIT)

    p = command(sub, "_auto_accept", cmd_auto_accept)
    p.add_argument("id", help="the task, like BP-3")
