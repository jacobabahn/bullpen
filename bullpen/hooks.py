"""A worktree's setup and teardown hooks, and removing a worktree."""

import os
import subprocess
import sys
from pathlib import Path

from .core import Fail, git, now
from .modes import task_base_ref
from .stops import stop


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


def remove_worktree(repo, t, force=False):
    # what a teardown leaves behind mustn't keep the worktree (drop checked it was clean before)
    force = tear_down(repo, t) or force
    if t.get("worktree") and Path(t["worktree"]).exists():
        git(repo.root, "worktree", "remove", *(["--force"] if force else []), t["worktree"])
    if t.get("branch"):
        git(repo.root, "branch", "-D", t["branch"], check=False)
