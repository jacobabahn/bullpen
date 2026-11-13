"""Guided review tours, for the stepper (stepper.py): a tour's state and diff, and the
person's comments on its steps. The Rust bullpen writes tours (`_tour`, cli/src/tours/)
and owns `tour-comment`; this is the stepper's twin of what it needs, until T11b.
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

from .core import Fail, SELF, clip, git, head, is_status, merge_base, now, read_json, write_json
from .jobs import background
from .modes import task_base_ref
from .news import tell_coordinator
from .stops import stop
from .start import hand_back
from .review import close_reviewer, patch_id


# On request (`bullpen tour BP-3`, the board's g), a cheap headless agent writes a
# tour of a ready or in-review task's change: its steps in reading order, each a
# title, a note and the hunks it covers (file and line ranges). The agent only reads:
# it prints the tour as JSON and bullpen writes the file (briefs/<ID>.tour.json),
# adding a last step, "Other changes", for any hunk no step covers. Code is always
# shown from git, never from the agent's text. A tour is tied to the commit it was
# written on: it stays current while the patch is the same (as approvals do), and is
# stale otherwise. The person's comments on its steps are drafts
# (briefs/<ID>.tour-comments.json) until `tour --send` sends them to the worker as a
# findings-style file; its next ready gets a review of them that doesn't count toward
# review_rounds. t["tour"] says whether a tour is being written ("writing", with the
# pid writing it), was written, or failed (with why).

def tour_path(repo, t):
    return repo.dir / "briefs" / f"{t['id']}.tour.json"


def tour_log(repo, t):
    return repo.dir / "briefs" / f"{t['id']}.tour.log"


def comments_path(repo, t):
    return repo.dir / "briefs" / f"{t['id']}.tour-comments.json"


HUNK_RE = re.compile(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def git_path(line, prefix):
    """The path in a `--- a/x` or `+++ b/x` line, or None for /dev/null."""
    p = line[4:].rstrip("\t")
    return p[len(prefix):] if p.startswith(prefix) else None


def diff_files(wt, base, commit):
    """The diff from base to commit as a list of files, each {"file", "head": its header
    lines, "hunks": [{"old": (start, count), "new": (start, count), "text"}], "deleted"}.
    A binary file has a header and no hunks."""
    text = git(wt, "-c", "core.quotePath=false", "diff", "--no-color", "--no-ext-diff",
               "--src-prefix=a/", "--dst-prefix=b/", base, commit).stdout
    files = []
    for line in text.split("\n"):
        if line.startswith("diff --git "):
            s = line[len("diff --git a/"):]
            half = (len(s) - 3) // 2
            files.append({"file": s[:half] if s[half:half + 3] == " b/" else s, "head": [line],
                          "hunks": [], "deleted": False})
            continue
        if not files:
            continue
        f = files[-1]
        m = HUNK_RE.match(line)
        if m:
            o, oc, n, nc = m.groups()
            f["hunks"].append({"old": (int(o), int(1 if oc is None else oc)),
                               "new": (int(n), int(1 if nc is None else nc)), "text": [line]})
        elif f["hunks"]:
            f["hunks"][-1]["text"].append(line)
        else:
            f["head"].append(line)
            if line.startswith("+++ "):
                f["file"] = git_path(line, "b/") or f["file"]
            elif line.startswith("rename to "):
                f["file"] = line[len("rename to "):]
            elif line.startswith("deleted file"):
                f["deleted"] = True
    for f in files:
        for h in f["hunks"]:
            while h["text"] and h["text"][-1] == "":
                h["text"].pop()  # the diff's last newline
            h["text"] = "\n".join(h["text"])
    return files


def span(start_count):
    start, count = start_count
    start = max(start, 1)
    return start, start + max(count, 1) - 1


def covers(ref, f, h):
    """Does a tour's hunk reference {"file", "lines"} cover hunk h of file f?"""
    if ref["file"] != f["file"]:
        return False
    if not ref.get("lines"):
        return True  # the whole file
    a, b = ref["lines"]
    sides = [h["new"]] if not f["deleted"] else []
    if f["deleted"] or h["new"][1] == 0:
        sides.append(h["old"])  # lines it only removes are named by their old numbers
    return any(a <= hi and lo <= b for lo, hi in map(span, sides))


def tour_failed(repo, tid, why):
    """The tour wasn't written: a line on the board and in the log. No news, no triage."""
    t = repo.task(tid)
    t["tour"] = {"state": "failed", "at": now(), "why": why}
    t["note"] = f"tour failed: {why}"
    write_json(repo.task_path(tid), t)
    with tour_log(repo, t).open("a") as f:
        f.write(f"{now()} failed: {why}\n")


def pid_alive(pid):
    try:
        os.kill(pid, 0)
    except (OSError, TypeError):
        return False
    return True


def tour_state(repo, t):
    """(state, tour, why): "writing", "failed" (why), "current", "stale" (why) or "none".
    A tour being written whose writer is gone has failed."""
    run_ = t.get("tour") or {}
    if run_.get("state") == "writing":
        if pid_alive(run_.get("pid")):
            return "writing", None, None
        tour_failed(repo, t["id"], "its writer stopped before the tour was written")
        t.update(repo.task(t["id"]))
        run_ = t["tour"]
    if run_.get("state") == "failed":
        return "failed", read_json(tour_path(repo, t)), run_.get("why")
    tour = read_json(tour_path(repo, t))
    if not tour:
        return "none", None, None
    stale = tour_stale(repo, t, tour)
    return ("stale", tour, stale) if stale else ("current", tour, None)


def tour_stale(repo, t, tour):
    """Why the tour no longer shows the task's code, or None while it does: the same
    commit, or the same patch (after a clean rebase, as approvals carry over)."""
    wt = t.get("worktree")
    if not wt or not Path(wt).is_dir():
        return None  # nothing to compare with: the tour is all there is
    if head(wt) == tour["commit"] or patch_id(wt, task_base_ref(repo, t)) == tour.get("patch"):
        return None
    return f"the code changed since the tour was written (on {tour['commit'][:12]})"


def has_commit(wt, c):
    return bool(c) and git(wt, "cat-file", "-e", f"{c}^{{commit}}", check=False).returncode == 0


def tour_diff(repo, t, tour):
    """The diff the tour was written on, as diff_files: from its own commits, else (they're
    gone after a rebase) from the task's code now."""
    wt = t["worktree"]
    if has_commit(wt, tour["commit"]) and has_commit(wt, tour.get("base")):
        return diff_files(wt, tour["base"], tour["commit"])
    base_ref = task_base_ref(repo, t)
    return diff_files(wt, merge_base(wt, base_ref), head(wt))


def step_diff(files, step):
    """A step's hunks from git, as a patch: each file the step names, in the step's order,
    with its header and the hunks the step covers."""
    out = []
    names = list(dict.fromkeys(r["file"] for r in step["hunks"]))
    for name in names:
        for f in files:
            if f["file"] != name:
                continue
            refs = [r for r in step["hunks"] if r["file"] == name]
            hunks = [h["text"] for h in f["hunks"] if any(covers(r, f, h) for r in refs)]
            if hunks or not f["hunks"]:
                out.append("\n".join(f["head"] + hunks))
    return "\n".join(out) + ("\n" if out else "")


def step_files(step):
    """ "bin/bullpen 120-140, README.md": a step's files and lines, for people."""
    return ", ".join(r["file"] + (" {}-{}".format(*r["lines"]) if r.get("lines") else "")
                     for r in step["hunks"]) or "(no code)"


def tour_status(t):
    if t["status"] not in ("ready", "review"):
        raise Fail(f"{is_status(t)}: a tour is only for a task that is "
                   f"ready or in review, whose code isn't changing")


def start_tour(repo, t, wait, opener=None):
    """Write a tour of the task (`_tour`): now (wait), or by a process of its own in the
    background. t["tour"] says it's being written, with that process's pid, so the board
    can show it. With opener ({"session", "from"}), the stepper opens there once it's written."""
    if not t.get("worktree") or not Path(t["worktree"]).is_dir():
        raise Fail(f"{t['id']} has no worktree, so there's nothing to tour")
    t["tour"] = {"state": "writing", "at": now(), "pid": os.getpid(), **({"open": opener} if opener else {})}
    if (t.get("note") or "").startswith("tour failed"):
        t["note"] = None
    write_json(repo.task_path(t["id"]), t)
    if wait:
        subprocess.run([SELF, "_tour", t["id"]], cwd=repo.root, stdin=subprocess.DEVNULL)
        run_ = repo.task(t["id"]).get("tour") or {}
        if run_.get("state") == "written":
            return tour_path(repo, t)
        why = run_.get("why")
        if run_.get("state") != "failed":
            why = "its writer stopped before the tour was written"
            tour_failed(repo, t["id"], why)
        raise Fail(f"{t['id']}'s tour failed: {why} (log: {tour_log(repo, t)})")
    t["tour"]["pid"] = background([SELF, "_tour", t["id"]], repo.root, repo.dir / "jobs.log",
                                  f"Writing {t['id']}'s tour", t["id"])
    write_json(repo.task_path(t["id"]), t)
    return None


def show_tour(repo, t, args):
    """The tour as it is, for the board: its state and steps (--json), or one step's
    hunks from git (--step N), as a patch for delta or a pager."""
    state, tour, why = tour_state(repo, t)
    if args.step is None:
        drafts = read_comments(repo, t)["comments"]
        out = {"id": t["id"], "state": state, "why": why, "path": str(tour_path(repo, t)) if tour else None,
               "commit": tour and tour["commit"], "written": tour and tour.get("written"),
               "comments": len(drafts),
               "steps": [{"n": i, "title": s["title"], "note": s["note"], "hunks": s["hunks"],
                          "files": step_files(s), "comments": sum(c["step"] == i for c in drafts)}
                         for i, s in enumerate(tour["steps"], 1)] if tour else []}
        if args.json:
            print(json.dumps(out, indent=2))
            return
        print(f"{t['id']}'s tour: {state}" + (f" ({why})" if why else ""))
        for s in out["steps"]:
            print(f"{s['n']:>3}. {s['title']}  [{s['files']}]" +
                  (f"  ({s['comments']} comment{'s' if s['comments'] != 1 else ''})" if s["comments"] else ""))
        return
    if not tour:
        raise Fail(f"{t['id']} has no tour: `bullpen tour {t['id']}` writes one")
    if not 1 <= args.step <= len(tour["steps"]):
        raise Fail(f"{t['id']}'s tour has steps 1 to {len(tour['steps'])}")
    step = tour["steps"][args.step - 1]
    diff = step_diff(tour_diff(repo, t, tour), step) if t.get("worktree") and Path(t["worktree"]).is_dir() else ""
    if args.json:
        print(json.dumps({"n": args.step, "of": len(tour["steps"]), "title": step["title"],
                          "note": step["note"], "hunks": step["hunks"], "files": step_files(step),
                          "diff": diff}, indent=2))
        return
    sys.stdout.write(diff)


def read_comments(repo, t):
    return read_json(comments_path(repo, t)) or {"next": 1, "comments": []}


def add_comment(repo, t, step, text):
    """Add a draft comment on step N of the task's tour; returns its number."""
    tour = read_json(tour_path(repo, t))
    if not tour:
        raise Fail(f"{t['id']} has no tour to comment on: `bullpen tour {t['id']}` writes one")
    if not 1 <= step <= len(tour["steps"]):
        raise Fail(f"{t['id']}'s tour has steps 1 to {len(tour['steps'])}")
    c = read_comments(repo, t)
    s = tour["steps"][step - 1]
    # the step as it was: a new tour may number its steps differently
    c["comments"].append({"id": c["next"], "step": step, "title": s["title"], "hunks": s["hunks"],
                          "commit": tour["commit"], "text": text, "at": now()})
    c["next"] += 1
    write_json(comments_path(repo, t), c)
    return c["next"] - 1


def comments_text(t, comments, k):
    """The findings-style file of the person's comments: each one with its step, files and lines."""
    parts = [f"# The person's comments on {t['id']}, after a guided tour of the change\n\n"
             f"Each comment is on one step of the tour: the step's title, the files and lines it covers "
             f"(in the code the tour was written on), then the comment. A line the person points at "
             f"is in the comment's text."]
    for i, c in enumerate(comments, 1):
        parts.append(f"## Comment {i}: step {c['step']}, {c['title']}\n\n"
                     f"Files: {step_files(c)} (commit {c['commit'][:12]})\n\n{c['text']}")
    return "\n\n".join(parts) + "\n"


def send_comments(repo, t):
    """Send the person's draft comments to the worker, as a findings-style file: the task
    goes back to working, and its next ready gets a review of them."""
    tour_status(t)
    drafts = read_comments(repo, t)["comments"]
    if not drafts:
        raise Fail(f"{t['id']} has no draft comments to send: `bullpen tour-comment {t['id']} STEP TEXT` adds one")
    k = (t.get("tours_sent") or 0) + 1
    path = repo.dir / "briefs" / f"{t['id']}.tour-{k}.comments.md"
    path.write_text(comments_text(t, drafts, k))
    wt = t["worktree"]
    base_ref = task_base_ref(repo, t)
    n = len(drafts)
    said = f"{n} comment{'s' if n != 1 else ''}"
    t["tours_sent"] = k
    t["tour_sent"] = {"n": k, "file": str(path), "count": n, "at": now(),
                      "commit": head(wt), "base": merge_base(wt, base_ref)}
    if t["status"] == "review":
        close_reviewer(t)  # it reviews code the person has just sent back
    msg = (f"The person went through your change with a guided tour and sent back {said}. Read {path}, "
           f"address each comment, commit, and run your ready command again. If you disagree with a "
           f"comment, say why in your ready summary.")
    try:
        failed = hand_back(repo, t, msg)
    except Fail as e:
        failed = clip(str(e), 160)
    comments_path(repo, t).unlink(missing_ok=True)  # sent: in the file now, no longer drafts
    if failed:
        stop(repo, t, "worker", "not_sent", "The person's tour comments didn't reach its worker",
             f"{t['id']} was sent back by the person after the tour ({said}), but the message didn't "
             f"reach its worker ({failed}). The comments are in {path}: `bullpen tell {t['id']}` it "
             f"to read them", "not-sent")
        print(f"{said} written to {path}, but they didn't reach {t['id']}'s worker: its coordinator is told")
        return
    tell_coordinator(repo, t, "tour-sent", f"{t['id']} sent back by the person after the tour: {said}")
    print(f"sent {said} to {t['id']}'s worker ({path})")
