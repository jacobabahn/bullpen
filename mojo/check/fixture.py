#!/usr/bin/env python3
"""A bullpen state to check the Mojo port against the Python, with a task in every
status and most of what the board says about one.

    fixture.py DIR NOW

makes DIR afresh: DIR/home (a $HOME, so paths print with ~), the project at
DIR/home/dev/proj (a git repo, local mode, with real worktrees for two tasks), its
state in DIR/state, a global config in DIR/config, and the tests' fake rex in
DIR/fake (some panes alive, with screens and status records). Every stamp is NOW
(seconds since the epoch) less some minutes and 30 s, so `ago` reads the same for a
while. Print the environment to run bullpen in it: `eval "$(fixture.py DIR NOW)"`.
"""

import datetime as dt
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tests.support import FAKE_REX  # noqa: E402


def sh(*cmd, cwd=None):
    subprocess.run(cmd, cwd=cwd, check=True, capture_output=True, text=True)


def main(d, now):
    d = Path(d).resolve()
    shutil.rmtree(d, ignore_errors=True)
    home, state, fake = d / "home", d / "state", d / "fake"
    root = home / "dev" / "proj"
    for p in (root, state, fake, d / "config" / "bullpen"):
        p.mkdir(parents=True)

    def stamp(minutes, micro=False):
        t = dt.datetime.fromtimestamp(now - minutes * 60 - 30).astimezone()
        return t.isoformat(timespec="microseconds" if micro else "seconds")

    git_env = {**os.environ, "HOME": str(home), "GIT_AUTHOR_DATE": "2026-01-01T00:00:00",
               "GIT_COMMITTER_DATE": "2026-01-01T00:00:00"}

    def g(*a, cwd=root):
        subprocess.run(["git", *a], cwd=cwd, check=True, capture_output=True, env=git_env)

    g("init", "-q", "-b", "main")
    g("config", "user.email", "fixture@example.com")
    g("config", "user.name", "Fixture")
    (root / "README.md").write_text("proj\n")
    (root / "app.py").write_text("print('hi')\n" * 20)
    g("add", "-A")
    g("commit", "-qm", "start")

    wts = home / "dev" / "proj-bullpen"

    def worktree(tid, branch, changes, loose=None, untracked=None):
        wt = wts / tid
        g("worktree", "add", "-q", "-b", branch, str(wt), "main")
        for name, text in changes.items():
            (wt / name).write_text(text)
        g("add", "-A", cwd=wt)
        g("commit", "-qm", f"{tid} work", cwd=wt)
        for name, text in (loose or {}).items():
            (wt / name).write_text(text)
        for name in untracked or []:
            (wt / name).parent.mkdir(parents=True, exist_ok=True)
            (wt / name).write_text("new\n")
        return str(wt)

    wt3 = worktree("BP-3", "bullpen/bp-3-parser", {"parser.py": "x = 1\n" * 30, "app.py": "print('hi')\n" * 18},
                   loose={"README.md": "proj, changed\n"}, untracked=["notes.txt", "scratch/a.txt"])
    wt17 = worktree("BP-17", "bullpen/bp-17-docs", {"docs.md": "# Docs\n"})
    g("worktree", "add", "-q", "-b", "bullpen/bp-6-nothing", str(wts / "BP-6"), "main")
    wt6 = str(wts / "BP-6")

    sdir = state / "repos" / str(root).strip("/").replace("/", "-")
    tasks = sdir / "tasks"
    tasks.mkdir(parents=True)
    (sdir / "briefs").mkdir()
    (sdir / "briefs" / "BP-13.setup.log").write_text("setup ran\n")
    (sdir / "briefs" / "BP-21.teardown.log").write_text("teardown ran\n")

    coords = [
        {"id": "c1", "block": "blk1", "session": "session:1", "board": {"block": "blk2", "session": "session:1"}},
        {"id": "c2", "block": "blk3", "session": "session:1", "board": None},
    ]
    cfg = {"prefix": "BP", "next": 40, "base": None, "news": "monitor", "coordinators": coords,
           "next_coordinator": 3, "auto_accept": False, "worker_model": "sonnet",
           "root": str(root), "triage_migrated": True, "stops_migrated": True, "layered": True}
    (sdir / "config.json").write_text(json.dumps(cfg, indent=2) + "\n")
    (d / "config" / "bullpen" / "config.json").write_text(json.dumps({"review_rounds": 3, "stuck_after": 15}, indent=2) + "\n")

    def task(n, title, status, mins=5, **kw):
        t = {"id": f"BP-{n}", "title": title, "status": status, "created": stamp(mins + 60),
             "base": "main", "branch": None, "worktree": None, "block": None, "session": None,
             "pr": None, "why": None, "coordinator": "c1", "updated": stamp(mins),
             "note": None, "since": stamp(mins, micro=True)}
        t.update(kw)
        (tasks / f"BP-{n}.json").write_text(json.dumps(t, indent=2) + "\n")

    review_full = {"round": 1, "kind": "full", "verdict": "approved", "model": "opus", "block": "blk9",
                   "session": "session:1"}
    task(1, "Queued thing", "todo", 3)
    task(2, "Waits on others", "todo", 4, after=["BP-1", "BP-26", "BP-99", "BP-23"])
    task(3, "Parser rewrite", "working", 12, branch="bullpen/bp-3-parser", worktree=wt3, block="blk4",
         session="session:1", model="sonnet")
    task(4, "Quiet worker", "working", 40, block="blk5", session="session:1",
         stuck={"who": "worker", "since": stamp(31)})
    task(5, "Hit the limit", "working", 50, block="blk6", session="session:1",
         limit={"who": "worker", "resets": stamp(-90), "at": stamp(5)})
    task(6, "Limit, no reset", "working", 50, block="blk7", session="session:1",
         limit={"who": "worker", "at": stamp(5)})
    task(7, "With a note", "working", 8, note="Resumed after the rebase.")
    task(8, "GitHub sent it back", "working", 9, gh={"sent": True, "checks": "failing", "new": 2},
         pr="https://github.com/me/proj/pull/8")
    task(9, "Fixing review findings", "working", 15, review={"round": 2, "kind": "full", "verdict": "changes",
                                                             "findings": "Two bugs in parse()."})
    task(10, "Under review", "review", 6, review={"round": 1, "kind": "light", "why": "docs only",
                                                  "block": "blk8", "session": "session:1"})
    task(11, "Reviewer quiet", "review", 200, review={"round": 2, "kind": "followup", "block": "blk10"},
         stuck={"who": "reviewer", "since": stamp(150)})
    task(12, "Asked a question", "needs", 2, ask=True, why="Should it keep the old flag?",
         stop={"by": "worker", "kind": "ended"}, block="blk11", session="session:1")
    task(13, "At a prompt", "needs", 9, why="Needs you: Do you want to run rm -rf build?",
         stop={"by": "worker", "kind": "prompt"}, needs_setup=True)
    task(14, "Ended its turn", "needs", 1, why="Ended its turn without saying ready",
         stop={"by": "worker", "kind": "ended"})
    task(15, "Coordinator triage", "triage", 3, why="Failed: the tool crashed",
         stop={"by": "worker", "kind": "failed"}, coordinator="c2")
    task(16, "Agent exited", "stopped", 25, why="Agent exited", stop={"by": "worker", "kind": "exited"})
    task(17, "Docs page", "ready", 30, branch="bullpen/bp-17-docs", worktree=wt17, local=True,
         review=dict(review_full, kept=2), rebased="BP-23", diffstat="docs.md | 1 +",
         summary="# Wrote the docs page\n\nAnd checked the links.")
    task(18, "Changes asked", "ready", 45, pr="https://github.com/me/proj/pull/18",
         review={"round": 3, "kind": "full", "verdict": "changes", "findings": "Still a bug."},
         gh={"checks": "passing", "new": 1})
    task(19, "Nothing to merge", "ready", 11, why="Nothing to merge: the branch is main", auto_accept=True)
    task(20, "Waiting on checks", "ready", 7, auto_wait=True, pr="https://github.com/me/proj/pull/20",
         review=review_full)
    task(21, "Research notes", "finished", 22, summary="Recorded the demo\nwith more lines")
    task(22, "No summary", "finished", 22)
    task(23, "Accepted locally", "done", 3000, local=True, merged="abc123", base="main")
    task(24, "Closed one", "done", 3000, closed=True)
    task(25, "Merged PR", "done", 3000, pr="https://github.com/me/proj/pull/25", gh={"state": "merged"})
    task(26, "Dropped one", "dropped", 4000, coordinator="c9")
    task(27, "Own model", "working", 20, worker_model="opus", block="blk12", session="session:1", model="opus")
    task(28, "Kept for you", "todo", 21, auto_accept=False, review_mode="full")
    task(29, "Needs you, stale", "needs", 9, why="Reviewer needs you: approve the plan?",
         stop={"by": "reviewer", "kind": "prompt"})
    task(30, "幅の広い題名のタスク — wide title with ünïcödé and a long tail that goes on and on for the cut",
         "working", 2, coordinator="c2")
    task(31, "Doing something", "working", 4, block="blk13", session="session:1")
    task(32, "Reviewing now", "review", 4, review={"round": 1, "kind": "full", "block": "blk14"})
    task(33, "Branch, nothing committed", "working", 4, branch="bullpen/bp-6-nothing", worktree=wt6)
    task(34, "Auto-accepts", "working", 4, auto_accept=True, coordinator=None)
    task(35, "PR closed", "ready", 4, pr="https://github.com/me/proj/pull/35", gh={"state": "closed"})
    task(36, "Rebase check", "review", 4, review={"round": 1, "kind": "rebase"})
    task(37, "Tour review", "review", 4, review={"round": 2, "kind": "tour"})
    task(38, "Asked while ready", "needs", 4, ask=True, why="Merge now?",
         asked_from={"status": "ready", "why": None})
    task(39, "Old stamp only", "todo", 0, since=None)

    # what agents are doing now (the board's doing/), and the panes' index
    doing, blocks = state / "doing", state / "blocks"
    doing.mkdir()
    blocks.mkdir()
    at = now - 60
    for blk, text, st in (("blk13", "Running   the tests\nagain", "working"), ("blk14", "Reading diff", "working"),
                          ("blk4", "Old news", "idle")):
        (doing / f"{blk}.json").write_text(json.dumps({"state": st, "text": text, "at": at}))
    for blk, role in (("blk1", "coordinator"), ("blk3", "coordinator"), ("blk4", "worker")):
        (blocks / f"{blk}.json").write_text(json.dumps({"repo": str(root), "role": role, "stuck_after": 15}))

    # news: for c1, c2, both, everyone; a diff's lines after one
    news = [f"{stamp(30)} [c1] BP-3 started: bullpen · BP-3 started",
            f"{stamp(29)} [c2] BP-15 triage: bullpen · BP-15 failed: the tool crashed",
            f"{stamp(28)} [c1,c2] BP-17 ready: bullpen · BP-17 is ready: Docs page (1 commit)",
            "\tdocs.md | 1 +",
            "\t 1 file changed",
            f"{stamp(27)} BP-1 added: bullpen · BP-1 added",
            f"{stamp(26)} [c7] BP-2 added: bullpen · for a gone coordinator: everyone's",
            f"{stamp(25)} [c2] BP-30 note: bullpen · c2's only"]
    (sdir / "news.log").write_text("\n".join(news) + "\n")
    first = len((news[0] + "\n").encode())
    (sdir / "news-c1.cursor").write_text(f"{first}\n")

    # the fake rex: these panes are alive; some have screens and status records
    (fake / "rex").write_text(FAKE_REX)
    (fake / "rex").chmod(0o755)
    (fake / "alive").mkdir()
    for blk in ("blk1", "blk2", "blk3", "blk4", "blk5", "blk8", "blk11", "blk12", "blk13", "blk14"):
        (fake / "alive" / blk).write_text("")
    (fake / "screen_blk4").write_text("$ pytest\n\x1b[32m3 passed\x1b[0m\x1b[3Cok\n" + "".join(
        f"line {i}\n" for i in range(30)) + "❯ \n\n\n")
    (fake / "screen_blk11").write_text("Should it keep the old flag?\n❯ \n")
    (fake / "screen_blk5").write_text("Thinking…\n(esc to interrupt)\n")
    upd = dt.datetime.fromtimestamp(now - 600).astimezone().isoformat()
    (fake / "status_blk12.json").write_text(json.dumps({"state": "idle", "kind": "", "msg": "", "updated_at": upd}))
    (fake / "status_blk8.json").write_text(json.dumps({"state": "working", "kind": "", "msg": "Reading", "updated_at": upd}))

    env = {"HOME": home, "BULLPEN_STATE": state, "BULLPEN_REX": fake / "rex", "FAKE_DIR": fake,
           "XDG_CONFIG_HOME": d / "config", "FIXTURE_ROOT": root}
    for k, v in env.items():
        print(f"export {k}='{v}'")
    print("unset REX_BLOCK NO_COLOR")


if __name__ == "__main__":
    main(sys.argv[1], float(sys.argv[2]))
