"""A task's setup and teardown hooks, as `show` needs them (bullpen/hooks.py)."""

from bullpen.core import Repo, Task


def hook_log(repo: Repo, t: Task, name: String) -> String:
    return repo.dir + "/briefs/" + t.id() + "." + name + ".log"
