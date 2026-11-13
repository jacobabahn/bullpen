"""Accepting a ready task: merging it, then rebasing the other tasks onto the new base."""

import os
import re
import subprocess
from pathlib import Path

from .core import (
    CLOSED, Fail, HOME, Repo, command, git, is_status, pr_short, repo_root, run, set_status,
    task_base_ref, unindex_block, worker_idle,
)
from .rex import rex_alive, rex_close, rex_type
from .news import news, tell_coordinator
from .guide import tell_guide_changes
from .hooks import remove_worktree
from .start import hand_back, start_queued
from .review import close_reviewer, is_doc, keep_approval, rebase_check, shortstat, start_review
from .watcher import refresh_watcher
from .close import close_task
from .board_build import rebuild_board_if_stale
from .push_remote import push_base
from .keys import KEYS_CHANGED, keys_hash


def cmd_accept(args):
    repo = Repo(repo_root())
    accept(repo, repo.task(args.id))


def accept(repo, t, auto=False):
    """Merge a ready task (its PR, or its branch locally) and close it; a finished one
    (nothing to merge) just closes. auto: review approved it and auto-accept is on.
    Raises Fail, leaving the task as it was, when it can't merge."""
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
    t.update(block=None, worktree=None, auto_accepted=auto)
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
    if auto:
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


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "accept", cmd_accept, "You",
                help="(the person's call) merge a ready task's PR, or its local branch without a remote (config --merge), and clean up; on a finished task (nothing to merge), close it")
    p.add_argument("id", help="the task, like BP-3")
