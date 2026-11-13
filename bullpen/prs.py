"""Following a task's pull request after ready (called by lua/watch.lua): its checks and the feedback on it."""

import json
from pathlib import Path

from .core import (
    Repo, STATE, WAITING, clip, command, load_config, on_save, pr_short, read_json, run,
    worker_idle, write_json,
)
from .rex import rex_alive
from .news import news
from .stops import stop
from .start import hand_back
from .review import close_reviewer
from .auto_accept import PR_FIELDS, checks, run_auto_accept


FOLLOWED = ("review", "ready", *WAITING)  # statuses whose PR the watcher looks at


def followed(t):
    """Does the watcher look at this task's PR? Only an open PR of a task that waits
    on review or the person: a working task is about to push anyway."""
    return bool(t.get("pr")) and not t.get("local") and t["status"] in FOLLOWED \
        and (t.get("gh") or {}).get("state", "open") == "open"


def mark_following():
    """<state>/prs exists while some task's PR may need a look: the watcher only
    calls `_prs` then."""
    STATE.mkdir(parents=True, exist_ok=True)
    (STATE / "prs").touch()


@on_save
def follow_saved(repo, t, old):
    """A saved task whose PR the watcher looks at marks it to follow."""
    if followed(t):
        mark_following()


def is_bot(author):
    login = (author or {}).get("login") or ""
    return bool((author or {}).get("is_bot")) or login.endswith("[bot]")


def feedback(pr):
    """[(key, who, what)] for the people's comments and reviews on a PR. An approval
    is not feedback; a review with no text means comments on the code."""
    out = []
    for c in pr.get("comments") or []:
        if not is_bot(c.get("author")) and (c.get("body") or "").strip():
            out.append((c.get("id") or c.get("url") or c.get("createdAt"),
                        (c.get("author") or {}).get("login") or "someone", clip(c["body"])))
    for r in pr.get("reviews") or []:
        if is_bot(r.get("author")) or r.get("state") not in ("CHANGES_REQUESTED", "COMMENTED"):
            continue
        body = clip(r.get("body"))
        if r["state"] == "CHANGES_REQUESTED":
            what = "requested changes" + (f": {body}" if body else "")
        else:
            what = f"reviewed: {body}" if body else "left comments on the code"
        out.append((r.get("id") or r.get("submittedAt"), (r.get("author") or {}).get("login") or "someone", what))
    return out


def cmd_prs(args):
    """(watcher) look at the PRs of tasks past ready: failed checks, people's comments,
    a merge or close outside bullpen."""
    following = False
    for cfg in (STATE / "repos").glob("*/config.json"):
        root = read_json(cfg, {}).get("root")
        if not root or not Path(root).is_dir():
            continue
        repo = Repo(root)
        repo.dir = cfg.parent
        repo.cfg = load_config(cfg.parent)
        tasks = [t for t in repo.tasks() if followed(t)]
        if not tasks or not repo.has_origin():  # local mode: no PRs to follow
            continue
        for t in tasks:
            follow_pr(repo, t)
        following = following or any(followed(t) for t in repo.tasks())
    if not following:
        (STATE / "prs").unlink(missing_ok=True)


def follow_pr(repo, t):
    p = run([repo.gh(), "pr", "view", t["pr"], "--json", PR_FIELDS], cwd=repo.root, check=False)
    if p.returncode != 0:
        return  # offline or rate limited: next time
    try:
        pr = json.loads(p.stdout)
    except ValueError:
        return
    # gh took a while: accept, a verdict or an event may have changed the task meanwhile
    t = repo.task(t["id"])
    if not followed(t):
        return
    g = t.setdefault("gh", {})
    pr_name = pr_short(t["pr"])
    state = (pr.get("state") or "OPEN").lower()
    if state in ("merged", "closed"):
        g["state"] = state
        write_json(repo.task_path(t["id"]), t)
        verb = "accept" if state == "merged" else "drop"
        news(repo, t, f"pr-{state}", f"{t['id']}'s {pr_name} was {state} outside bullpen: "
             f"`bullpen {verb} {t['id']}` cleans up")
        return
    summary, failed = checks(pr)
    head = pr.get("headRefOid") or ""
    seen_failed = set(g.get("failed") or [])
    new_failed = [n for n in failed if f"{head}:{n}" not in seen_failed]
    g["failed"] = sorted(seen_failed | {f"{head}:{n}" for n in failed})
    g["checks"] = summary
    seen = set(g.get("seen") or [])
    new_notes = [(k, who, what) for k, who, what in feedback(pr) if k not in seen]
    g["seen"] = sorted(seen | {k for k, _, _ in new_notes})
    g["new"] = (g.get("new") or 0) + len(new_notes)
    if t.get("auto_wait") and (failed or new_notes):
        t.pop("auto_wait")  # the person (or the worker) has something to look at first
        why = "its PR's checks failed" if failed else "its PR has new comments"
        news(repo, t, "ready", f"{t['id']} not auto-accepted: {why}")
    elif t.get("auto_wait") and summary != "running" and t["status"] == "ready":
        write_json(repo.task_path(t["id"]), t)
        run_auto_accept(repo, t)
        return
    if not new_failed and not new_notes:
        write_json(repo.task_path(t["id"]), t)  # not save_task: the status didn't change
        return
    lines, asks = [], []
    if new_failed:
        lines.append(f"{t['id']}'s PR checks failed: {', '.join(new_failed)}")
        asks.append(f"These checks failed on your pull request ({t['pr']}): {', '.join(new_failed)}. "
                    f"Look at them (`gh pr checks {t['pr']}`) and fix them.")
    if new_notes:
        said = "; ".join(f"{who} {what}" for _, who, what in new_notes)
        n = len(new_notes)
        lines.append(f"{t['id']}'s PR has {n} new comment{'s' if n != 1 else ''}: {clip(said, 200)}")
        asks.append(f"New feedback on your pull request ({t['pr']}): {said}. Read it in full with "
                    f"`gh pr view {t['pr']} --comments` and address it; if you disagree, say why "
                    f"in your ready summary.")
    send_back(repo, t, lines, " ".join(asks))


def send_back(repo, t, lines, ask):
    """PR feedback goes to the task's worker, which fixes it and runs ready again.
    An unfinished review is moot once the work changes: close it, giving its round back."""
    r = t.get("review") or {}
    if r.get("block") and not r.get("verdict"):
        close_reviewer(t)
        r["round"] = max(0, r.get("round", 1) - 1)
    elif r.get("block"):
        close_reviewer(t)
    why = lines[0].removeprefix(f"{t['id']}'s ").rstrip(".")
    if not worker_idle(t):
        # typing now would answer its prompt: keep the feedback for later
        write_json(repo.task_path(t["id"]), t)
        at = "on a prompt" if t["status"] == "needs" else "stopped mid-turn"
        for line in lines:
            news(repo, t, "pr-feedback", f"{line.rstrip('.')}. Not sent to its worker, which "
                 f"is {at}: `bullpen tell {t['id']}` it when it's free")
        return
    if rex_alive(t.get("session"), t.get("block")):
        failed = hand_back(repo, t, clip(f"{ask} Then commit and run your ready command again.", 2000))
        if failed:  # its reviewer, if it had one, is closed: the coordinator passes this on
            stop(repo, t, "worker", "not_sent", why, [f"{line.rstrip('.')}. The message didn't reach its "
                 f"worker ({failed}): `bullpen tell {t['id']}` it" for line in lines], "pr-feedback")
            return
        t.setdefault("gh", {})["sent"] = True  # the board says so once its worker is on it
        repo.save_task(t)
        for line in lines:
            news(repo, t, "pr-feedback", f"{line.rstrip('.')}. Sent back to its worker.")
        return
    t.setdefault("gh", {})["sent"] = True
    stop(repo, t, "worker", "not_sent", why, [f"{line.rstrip('.')}. Its worker's pane is gone: "
         f"`bullpen resume {t['id']}` restarts it" for line in lines], "pr-feedback")


def commands(sub):
    """This module's commands, for bullpen's parser."""
    command(sub, "_prs", cmd_prs)
