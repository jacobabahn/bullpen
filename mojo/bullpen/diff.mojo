"""`diff ID --stat` (bullpen/show.py cmd_diff, diff_stat). The diff itself goes through
a pager or a pane, so without --stat (or with --pane) it stays with Python."""

from std.os.path import isdir

from bullpen.args import Args
from bullpen.core import Repo, repo_root, git, task_base_ref, is_status
from bullpen.textutil import py_rstrip, py_splitlines

comptime UNCOMMITTED = "Uncommitted (not in any commit yet):"


def cmd_diff(args: Args) raises:
    if not args.flag("--stat") or args.flag("--pane"):
        raise Error("__python__")
    var repo = Repo(repo_root())
    var t = repo.task(args.pos[0])
    var wt = t.s("worktree")
    if not wt or not isdir(wt):
        raise Error(is_status(t) + ", with no worktree, so there's nothing to diff")
    print(diff_stat(wt, task_base_ref(repo, t), t.id()))


def diff_stat(wt: String, base_ref: String, tid: String) raises -> String:
    """The files the branch changed since its base, then any not committed yet."""
    var committed = py_rstrip(git(wt, ["diff", "--stat", base_ref + "...HEAD"]).out)
    var out = committed if committed else tid + " has no commits against " + base_ref + "."
    var loose = py_rstrip(git(wt, ["diff", "--stat", "HEAD"]).out)
    var untracked = py_splitlines(git(wt, ["ls-files", "--others", "--exclude-standard"]).out)
    if loose or len(untracked) > 0:
        out += "\n\n" + UNCOMMITTED
        if loose:
            out += "\n" + loose
        for f in untracked:
            out += "\n untracked: " + f
    return out^
