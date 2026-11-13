"""Rex: running its commands, and finding, typing into and closing panes."""

import json
import os
import re
import tty

from .core import REX, clip, run


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


def coordinator_label(repo):
    return f"bullpen: {repo.root.name}"


def board_label(repo):
    return f"bullpen: {repo.root.name} board"


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


def review_label(t):
    return f"bullpen: {t['id']} review"


def screen(session, block):
    """The text on a pane's screen, or "" if Rex can't say."""
    p = run([REX, "capture", *(["-s", session] if session else []), "-b", block], check=False)
    return p.stdout if p.returncode == 0 else ""


def program_status(session, block):
    """The program's own (root) status record in the pane, as Rex keeps it, or None."""
    cmd = [REX, "block", "call", *(["-s", session] if session else []), "-b", block, "program_status"]
    p = run(cmd, check=False)
    try:
        records = json.loads(p.stdout).get("records") if p.returncode == 0 else None
    except ValueError:
        return None
    return next((r for r in records or [] if not r.get("id")), None)
