"""Reviews: what a review covers, starting a reviewer, and what ready does with review on."""

import re
import subprocess
from pathlib import Path

from .core import (
    PROMPTS, SELF, auto_accepts, commits, git, head, index_block, merge_base, resolve, run,
    set_status, unindex_block, where, with_model,
)
from .rex import HIDE_HINT, review_label, rex_close, rex_run
from .agents import pane, running
from .coordinators import session_for
from .news import ready_news
from .rules import task_rules
from .start import float_pane


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
            and running(t, "reviewer")):
        repo.save_task(t)
        return ("A reviewer is already checking this commit: end your turn, and you'll hear back "
                "if it asks for changes.")
    start_review(repo, t, base_ref, rebase=check[1] if check else None)
    return (f"A reviewer checks it now ({review_kind(t['review'])}): end your turn, and you'll "
            f"hear back if it asks for changes.")


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
    import hashlib  # here, not at the top: it loads OpenSSL
    files, h = {}, None
    for line in out.split("\n"):
        if line.startswith("diff --git "):
            pair = line[len("diff --git "):]  # "a/<path> b/<path>", the same path twice
            h = files[pair[2:2 + (len(pair) - 5) // 2]] = hashlib.sha1()
        elif h and not line.startswith(("index ", "@@")):
            h.update(line.encode() + b"\n")
    return {path: h.hexdigest() for path, h in files.items()}


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
        rex_close(*pane(t, "reviewer"))
        r["block"] = None
