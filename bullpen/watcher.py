"""Replacing the Rex watcher (lua/watch.lua) after the catch-up's build (board_build.py):
cli/src/watcher.rs's twin, which writes the same stamp."""

import hashlib
import shutil
import sys
from pathlib import Path

from .core import Fail, REX, SELF, STATE, TERMINAL, WATCH_LUA, file_hash, read_json, write_json
from .rex import rex_alive, rex_api, rex_close, with_state


def code_hash():
    """A hash of bullpen's code: the entry scripts, the package's modules, and the Rust
    build (target/bullpen) when there is one."""
    h = hashlib.sha256(Path(SELF).read_bytes())
    py = Path(SELF).with_name("bullpen-py")
    h.update(py.read_bytes())
    for f in sorted(Path(__file__).parent.glob("*.py")):
        h.update(f.name.encode() + b"\0" + f.read_bytes())
    built = Path(SELF).parent.parent / "target" / "bullpen"
    if built.is_file():
        h.update(b"target/bullpen\0" + built.read_bytes())
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


def restart_watcher():
    """After a build: a running watcher started from the old one is replaced (none is
    started). Never fails the build."""
    try:
        w = read_json(STATE / "watcher.json", {})
        if rex_alive(w.get("session"), w.get("block")):
            ensure_watcher(w["session"])
    except Fail as e:
        print(f"bullpen: {e}", file=sys.stderr)
