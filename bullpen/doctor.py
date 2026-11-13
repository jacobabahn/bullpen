"""doctor: is everything bullpen needs in place?"""

import json
import os
import re
import shlex
import shutil
import tempfile
from pathlib import Path

from .core import (
    DEFAULT_CONFIG, Fail, INTERNAL, REPO_ONLY, REX, Repo, SELF, STATE, command, git,
    global_config_path, homes, read_global_file, read_json, repo_root, run, safe,
)
from .rex import rex_alive
from .coordinators import alive
from .watcher import watcher_stamp
from .board import BOARD, BOARD_LOG, BOARD_STAMP, old_board
from .board_build import board_building, board_dirty, board_source_hash
from .push_remote import check_push


# Each check returns (state, what it found): "ok", "fail" (with the fix in the text)
# or "skip" (it doesn't apply, or can't be told). `doctor` runs them all; `up` runs
# the cheap ones (no network, nothing `up` itself just fixed) and only counts problems.

REX_MIN = (0, 1, 0, 13)  # preview.13: minimized floating panes
KEY_ACTIONS = ("bullpen_next", "bullpen_board", "bullpen_coordinator", "bullpen_jump")


def rex_version_key(text):
    """(major, minor, patch, preview) from "0.1.0-preview.13"; a release sorts after its
    previews. None if it isn't a version."""
    m = re.match(r"v?(\d+)\.(\d+)\.(\d+)(?:-preview\.(\d+))?$", (text or "").strip())
    if not m:
        return None
    pre = m.group(4)
    return (*map(int, m.group(1, 2, 3)), int(pre) if pre else float("inf"))


def check_rex():
    exe = shutil.which(REX)
    if not exe:
        return "fail", f"{REX} isn't on PATH: install Rex (https://www.superlogical.com/rex)"
    p = run([REX, "server", "status", "--json"], check=False)
    if p.returncode != 0:
        return "fail", f"{exe}, but its server doesn't answer: start Rex, or run `rex server start`"
    try:
        version = json.loads(p.stdout).get("rex_version")
    except (ValueError, AttributeError):
        version = None
    key = rex_version_key(version)
    if key is None:
        return "skip", f"{exe}; can't tell the server's version ({version or 'none given'})"
    if key < REX_MIN:
        return "fail", (f"the server is {version}; bullpen needs 0.1.0-preview.13 or later "
                        f"(minimized floating panes): update Rex and restart its server")
    return "ok", f"server {version}, {exe}"


def check_gh(repo):
    if not repo or not repo.has_origin():
        return "skip", "local mode (no origin remote): gh isn't needed"
    gh = repo.gh()
    exe = shutil.which(gh)
    if not exe:
        return "fail", f"{gh} isn't on PATH, and this repo has a remote: install gh (https://cli.github.com)"
    p = run([exe, "auth", "status"], check=False)
    if p.returncode != 0:
        return "fail", f"{exe} isn't signed in: run `gh auth login`"
    return "ok", f"signed in, {exe}"


def check_agents(cfg):
    """The programs the coordinator, workers and reviewers run (claude, unless changed)."""
    progs = {}
    for key in ("coordinator_cmd", "worker", "reviewer"):
        if cfg.get(key):
            progs.setdefault(cfg[key][0], []).append(key)
    missing = [(p, keys) for p, keys in progs.items() if not shutil.which(p)]
    if missing:
        return "fail", "; ".join(
            (f"{p} isn't on PATH: install Claude Code (https://claude.com/claude-code)" if Path(p).name == "claude"
             else f"{p} isn't on PATH") + f" (config.json's {', '.join(keys)})" for p, keys in missing)
    return "ok", ", ".join(shutil.which(p) for p in progs)


def check_board():
    """Built from the board's source as it is now (see board_source_hash)."""
    if os.environ.get("BULLPEN_BOARD"):
        exe = Path(os.environ["BULLPEN_BOARD"])
        if os.access(exe, os.X_OK):
            return "ok", f"{exe} (BULLPEN_BOARD)"
        return "fail", f"BULLPEN_BOARD is {exe}, which isn't an executable file"
    build = f"run `{SELF} build-board`"
    cargo = "" if shutil.which("cargo") else " (it needs cargo: https://rustup.rs)"
    if board_building():
        return "ok", f"being rebuilt now (log: {BOARD_LOG}); running boards reload when it's done"
    if not os.access(BOARD, os.X_OK):
        return "fail", f"not built: {build}{cargo}"
    src, st = board_source_hash(), read_json(BOARD_STAMP, {})
    if st.get("source") == src:
        return "ok", f"{BOARD}, built from board/ as it is"
    if st.get("failed") == src:
        return "fail", f"older than board/: its build failed (log: {BOARD_LOG}). Fix that, then {build}{cargo}"
    if board_dirty():
        return "fail", (f"older than board/, which has uncommitted changes, so bullpen won't build it by "
                        f"itself: commit them, or {build} to build them as they are")
    return "fail", f"older than board/: `bullpen up` rebuilds it in the background, or {build}{cargo}"


def check_keys():
    """Rex lists the actions its config defines: bullpen's keys define four."""
    if not shutil.which(REX):
        return "skip", "can't tell without rex"
    p = run([REX, "actions", "--json"], check=False)
    try:
        actions = json.loads(p.stdout)["actions"] if p.returncode == 0 else None
    except (ValueError, KeyError, TypeError):
        actions = None
    if actions is None:
        return "skip", "can't tell: `rex actions --json` didn't list the actions"
    have = {a.get("name"): a for a in actions}
    missing = [a for a in KEY_ACTIONS if a not in have]
    if missing and len(missing) < len(KEY_ACTIONS):
        # the line is in the config, but Rex loaded keys.lua before it had these
        return "fail", (f"{', '.join(missing)} missing: Rex loaded an older keys.lua. "
                        f"Run `rex config reload`")
    if missing:
        return "fail", "not in your Rex config: run `bullpen keys --install`"
    src = (have[KEY_ACTIONS[0]].get("source") or "").rsplit(":", 1)[0]
    return "ok", f"set up, from {src}" if src else "set up"


def check_watcher():
    w = read_json(STATE / "watcher.json", {})
    if not w.get("block"):
        return "fail", "never started: run `bullpen up`"
    if not rex_alive(w.get("session"), w["block"]):
        return "fail", f"not running (its pane {w['block']} is gone): run `bullpen up`"
    if w.get("stamp") != watcher_stamp():
        return "fail", (f"running ({w['block']}), but from an older watch.lua or bullpen: "
                        f"run `bullpen up` to restart it")
    return "ok", f"running and current, {w['block']}"


def check_state():
    try:
        STATE.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=STATE, prefix=".doctor-"):
            pass
    except OSError as e:
        return "fail", f"can't write to {STATE}: {e.strerror or e}. Fix its permissions, or set BULLPEN_STATE"
    return "ok", f"{STATE} is writable"


def check_config(d):
    path = d / "config.json"
    if not path.exists():
        return "skip", "none yet: `bullpen up` here makes one"
    try:
        json.loads(path.read_text())
    except ValueError as e:
        return "fail", f"{path} doesn't parse ({e}): fix it, or move it away and run `bullpen up`"
    return "ok", f"{path} parses"


def check_global():
    """The global config.json, if there is one, parses and holds no repository's own keys."""
    path = global_config_path()
    if not path.exists():
        return "skip", f"none ({path}; `bullpen config --global` makes one)"
    try:
        g = read_global_file()
    except Fail as e:
        return "fail", str(e)
    bad = [k for k in g if k in REPO_ONLY or k in INTERNAL]
    if bad:
        return "fail", (f"{path} sets {', '.join(bad)}, which only a repository can have: "
                        "remove it from the file (each repo's `bullpen config` sets it)")
    return "ok", f"{path} parses"


def global_parses():
    try:
        read_global_file()
        return True
    except Fail:
        return False


def shell_word_found(word, cwd):
    """Is word something sh can run here: a program on PATH, a path, or a builtin?"""
    return run(["sh", "-c", 'command -v "$1" >/dev/null', "sh", word], cwd=cwd, check=False).returncode == 0


def shebang_program(argv):
    """The program a #! line runs: its first word, or for env, the first word after
    env's options (-S splits its string) and NAME=value settings."""
    if Path(argv[0]).name != "env":
        return argv[0]
    words, skip = argv[1:], False
    for w in words:
        if skip:
            skip = False
        elif w in ("-u", "-C", "--unset", "--chdir"):
            skip = True  # their argument follows
        elif w.startswith("-S") and len(w) > 2:
            return w[2:]  # -Sbash
        elif not w.startswith("-") and "=" not in w:
            return w
    return None


def check_hook(repo, name, why_not=None):
    """The setup (or teardown) a new task would get: config.json's command, else
    .bullpen/<name>.sh as committed on the base. It must parse and find what it runs."""
    if not repo:
        return "skip", f"can't tell: {why_not or 'not in a git repository'}"
    cmd = repo.cfg.get(name)
    path = f".bullpen/{name}.sh"
    if cmd:
        source = f"config.json's {name}"
        if run(["sh", "-n", "-c", cmd], check=False).returncode != 0:
            return "fail", f"{source} isn't valid sh: fix it with `bullpen config --{name} \"<cmd>\"`"
        try:
            word = shlex.split(cmd)[0]
        except (ValueError, IndexError):
            word = None
        if word and "=" not in word and not shell_word_found(word, repo.root):
            return "fail", f"{source} runs {word}, which isn't on PATH: install it, or fix the command"
        return "ok", f"{source}: {cmd}"
    base = repo.base()
    ref = f"origin/{base}" if repo.has_origin() and git(
        repo.root, "rev-parse", "-q", "--verify", f"origin/{base}", check=False).returncode == 0 else base
    p = git(repo.root, "show", f"{ref}:{path}", check=False)
    text = p.stdout if p.returncode == 0 and p.stdout.strip() else None
    if text is None:
        if (repo.root / path).exists():
            return "fail", f"{path} isn't committed on {ref}, so it won't run: commit it (and push it)"
        return "skip", f"none ({path} or `bullpen config --{name}`)"
    first = text.splitlines()[0]
    if first.startswith("#!"):
        argv = first[2:].split()
        if not argv:
            return "fail", f"{path} on {ref} has an empty #! line: name its interpreter, or remove the line"
        prog = shebang_program(argv)
        if not prog:
            return "fail", f"{path} on {ref} has a #! line that names no program: fix it"
        if not (os.access(prog, os.X_OK) if "/" in prog else shutil.which(prog)):
            return "fail", f"{path} on {ref} runs under {prog}, which isn't here: install it, or fix its #! line"
        shell = Path(prog).name
    else:
        shell = "sh"
    if shell in ("sh", "bash", "dash", "zsh") and run([shell, "-n"], input=text, check=False).returncode != 0:
        return "fail", f"{path} on {ref} isn't valid {shell}: `{shell} -n {path}` says where"
    return "ok", f"{path} on {ref}" + ("" if first.startswith("#!") else ", run under sh")


def check_coordinators(repo, why_not=None):
    """The coordinators config.json lists still have their panes (and boards)."""
    if not repo and why_not:
        return "skip", f"can't tell: {why_not}"
    if not repo or not repo.cfg["coordinators"]:
        return "skip", "none yet: run `bullpen up`"
    gone, old, ok_ = [], [], []
    for c in repo.cfg["coordinators"]:
        if not alive(c):
            gone.append(f"{c['id']}'s pane ({c.get('block')}) is gone")
            continue
        b = c.get("board") or {}
        if b.get("block") and not rex_alive(b.get("session"), b["block"]):
            gone.append(f"{c['id']}'s board ({b['block']}) is gone")
        elif b.get("block") and old_board(b):
            old.append(f"{c['id']}'s board is from an older build")
        else:
            ok_.append(f"{c['id']} ({c['block']})")
    if gone or old:
        return "fail", "; ".join(
            ([("; ".join(gone) + ": run `bullpen up`, which drops gone coordinators and reopens boards")]
             if gone else [])
            + ([", ".join(old) + ": `bullpen up` restarts " + ("it" if len(old) == 1 else "them")]
               if old else []))
    return "ok", ", ".join(ok_)


def doctor_repo():
    """This repo's Repo and its state folder, or (None, folder or None): outside a git
    repository, or when its config.json doesn't parse."""
    try:
        root = repo_root()
    except Fail:
        return None, None
    d = STATE / "repos" / safe(str(root).strip("/").replace("/", "-"))
    try:
        return Repo(root), d
    except (ValueError, Fail):  # its config.json, or the global one, doesn't parse
        return None, d


def doctor_checks(cheap=False):
    """[(name, state, text)]. cheap leaves out what needs the network or what `up` sees to."""
    repo, d = doctor_repo()
    cfg = repo.cfg if repo else DEFAULT_CONFIG
    why_not = None if repo or not d else \
        "config.json doesn't parse" if global_parses() else "the global config.json doesn't parse"
    out = [("rex", check_rex)]
    if not cheap:
        out.append(("gh", lambda: check_gh(repo)))
    out += [("agents", lambda: check_agents(cfg)), ("board", check_board), ("keys", check_keys)]
    if not cheap:
        out.append(("watcher", check_watcher))
    out.append(("state", check_state))
    out.append(("global", check_global))
    if d:
        out.append(("config", lambda: check_config(d)))
    out += [("setup", lambda: check_hook(repo, "setup", why_not)),
            ("teardown", lambda: check_hook(repo, "teardown", why_not))]
    if not cheap:
        out.append(("coordinators", lambda: check_coordinators(repo, why_not)))
        out.append(("push", lambda: check_push(repo, why_not)))
    res = []
    for name, fn in out:
        try:
            res.append((name, *fn()))
        except (Fail, OSError) as e:
            res.append((name, "skip", f"can't tell: {e}"))
    return res


def problems_phrase(n):
    return f"{n} problem{'s' if n != 1 else ''}"


def cmd_doctor(args):
    res = doctor_checks()
    marks = {"ok": "ok", "fail": "FAIL", "skip": "--"}
    for name, state, text in res:
        print(f"{marks[state]:<4}  {name}: {homes(text)}")
    failed = [name for name, state, _ in res if state == "fail"]
    n = len(failed)
    print(f"{problems_phrase(n)}: {', '.join(failed)}." if n else "All good.")
    return 1 if n else 0


def up_hint():
    """`up`'s one line about problems doctor would find, or None."""
    try:
        n = sum(state == "fail" for _, state, _ in doctor_checks(cheap=True))
    except Exception:  # a hint must never break up
        return None
    return f"{problems_phrase(n)}: run `bullpen doctor`" if n else None


def commands(sub):
    """This module's commands, for bullpen's parser."""
    command(sub, "doctor", cmd_doctor, "Setup",
            help="check bullpen's setup: one line per check, with the fix for what's wrong")
