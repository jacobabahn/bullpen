"""bullpen's key bindings in the Rex config."""

import os
import re
import shutil
from pathlib import Path

from .core import Fail, HOME, REX, SELF, STATE, command, file_hash, run


def keys_line():
    opts = f'bin = "{SELF}"'
    if os.environ.get("BULLPEN_STATE"):
        opts += f', state = "{STATE}"'
    return f'dofile("{KEYS_LUA}").setup{{ {opts} }}'


KEYS_LUA = HOME / "lua" / "keys.lua"


KEYS_CHANGED = "Keys changed: run `rex config reload`"


def keys_hash():
    """keys.lua as it is, which Rex reads only when its config loads: an accept that
    changes it says to reload."""
    try:
        return file_hash(KEYS_LUA)
    except OSError:
        return None


def rex_config_path():
    """The init.lua Rex reads: REX_CONFIG, else rex/init.lua in the config directory."""
    if os.environ.get("REX_CONFIG"):
        return Path(os.environ["REX_CONFIG"]).expanduser()
    base = os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config"
    return Path(base) / "rex" / "init.lua"


KEYS_IN_CONFIG = re.compile(r"""dofile\s*\(\s*["']([^"']*keys\.lua)["']\s*\)\s*\.\s*setup""")


def cmd_keys(args):
    if args.install:
        return install_keys()
    print(f"-- Add to your Rex config (rex config shows where), then run `rex config reload`\n"
          f"-- (`bullpen keys --install` does both):\n"
          f"{keys_line()}\n"
          f"-- Defaults: cmd+;>n next task needing you, cmd+;>b board, cmd+;>c coordinator,\n"
          f"-- cmd+;>j jump to any open task (type part of it, in any repository),\n"
          f"-- and ctrl+escape puts the worker pane you're in back into its capsule.\n"
          f'-- Change them with prefix = "...", or keys = {{ next = ..., board = ..., coordinator = ..., jump = ..., hide = ... }}.')


def install_keys():
    """Add bullpen's line to the end of the Rex config if it isn't there, then reload.
    It only ever appends: what's above it (a hook that must run first, say) stays put."""
    path = rex_config_path()
    try:
        text = path.read_text()
    except FileNotFoundError:
        text = ""
    found = [m.group(1) for m in KEYS_IN_CONFIG.finditer(text)]
    if any(Path(f).expanduser().resolve() == KEYS_LUA.resolve() for f in found):
        print(f"already in {path}")
    elif found:
        raise Fail(f"{path} already loads another keys.lua ({found[0]}): change that line to\n{keys_line()}")
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        gap = "" if not text else "\n" if text.endswith("\n") else "\n\n"  # a blank line after what's there
        with open(path, "a") as f:
            f.write(f"{gap}-- bullpen's keys (`bullpen keys` says what they are)\n{keys_line()}\n")
        print(f"added to the end of {path}")
    if not shutil.which(REX):
        raise Fail("rex isn't on PATH: run `rex config reload` once it is")
    p = run([REX, "config", "reload"], check=False)
    if p.returncode != 0:
        raise Fail(f"`rex config reload` failed: {(p.stderr or p.stdout).strip() or 'it said nothing'}")
    print("Rex config reloaded: the cmd+; keys work now")


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "keys", cmd_keys, "Setup", help="print the Rex config line for bullpen's key bindings")
    p.add_argument("--install", action="store_true",
                   help="add the line to the end of your Rex config if it's missing, then `rex config reload`")
