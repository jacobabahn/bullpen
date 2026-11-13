#!/usr/bin/env python3
"""A bullpen state to check the Rust port (as BP-92 did the Mojo one) against the Python, with a task in every
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
import hashlib
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
    cfg = {"prefix": "BP", "next": 42, "base": None, "news": "monitor", "coordinators": coords,
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
    task(28, "Kept for you", "todo", 21, auto_accept=False, review_mode="full", placement="tab")
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
    task(40, "Triage, worker alive", "triage", 3, why="Ended its turn without saying ready",
         stop={"by": "worker", "kind": "ended"}, block="blk15", session="session:1", ok=None)
    task(41, "Triage on the reviewer", "triage", 3, why="Reviewer stopped without a verdict",
         stop={"by": "reviewer", "kind": "ended"}, pr="https://github.com/me/proj/pull/41",
         review={"round": 1, "kind": "full", "block": "blk16"})

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
    for blk in ("blk1", "blk2", "blk3", "blk4", "blk5", "blk8", "blk11", "blk12", "blk13", "blk14", "blk15", "blk16"):
        (fake / "alive" / blk).write_text("")
    (fake / "screen_blk4").write_text("$ pytest\n\x1b[32m3 passed\x1b[0m\x1b[3Cok\n" + "".join(
        f"line {i}\n" for i in range(30)) + "❯ \n\n\n")
    (fake / "screen_blk11").write_text("Should it keep the old flag?\n❯ \n")
    (fake / "screen_blk5").write_text("Thinking…\n(esc to interrupt)\n")
    upd = dt.datetime.fromtimestamp(now - 600).astimezone().isoformat()
    (fake / "status_blk12.json").write_text(json.dumps({"state": "idle", "kind": "", "msg": "", "updated_at": upd}))
    (fake / "status_blk8.json").write_text(json.dumps({"state": "working", "kind": "", "msg": "Reading", "updated_at": upd}))

    # extras a case asks for with env:FIXTURE=a,b (compare.sh); none by default
    extras = set(filter(None, os.environ.get("FIXTURE", "").split(",")))

    def say(blk, state, ago, **kw):  # a status record for pane blk, updated ago seconds back
        rec = {"state": state, "kind": "", "msg": "", **kw}
        if ago is not None:
            rec["updated_at"] = dt.datetime.fromtimestamp(now - ago).astimezone().isoformat()
        (fake / f"status_{blk}.json").write_text(json.dumps(rec))

    def live(blk, screen=None):
        (fake / "alive" / blk).write_text("")
        if screen is not None:
            (fake / f"screen_{blk}").write_text(screen)

    if "check" in extras:  # a task for each thing `check` says (or doesn't)
        def wtask(n, title, status, blk, mins=20, **kw):
            task(n, title, status, mins, block=blk, session="session:1", **kw)
            live(blk)

        def rtask(n, title, status, blk, mins=20, **kw):
            task(n, title, status, mins, review={"round": 1, "kind": "full", "block": blk}, **kw)
            live(blk)
        wtask(40, "Blocked worker", "working", "blk40")
        say("blk40", "blocked", 600, kind="permission")
        wtask(41, "Failed turn", "working", "blk41")
        say("blk41", "error", 600)
        wtask(42, "Long shell", "working", "blk42")
        say("blk42", "idle", 1200)
        live("blk42", "❯ \n  2 background shells · ↓ to manage\n")
        wtask(43, "Short shell", "working", "blk43")
        say("blk43", "idle", 600)
        live("blk43", "❯ \n  1 shell still running\n")
        wtask(44, "Needs, working", "needs", "blk44", why="Needs you (permission): run x",
              stop={"by": "worker", "kind": "failed"})
        say("blk44", "working", 600, msg="Reading")
        wtask(45, "Triage, blocked", "triage", "blk45", mins=30,
              why="Failed: The tool crashed with a message that goes on past sixty characters for sure",
              stop={"by": "worker", "kind": "failed"})
        say("blk45", "blocked", 600)
        wtask(46, "At its prompt", "needs", "blk46", why="Needs you: Do you want to?",
              stop={"by": "worker", "kind": "prompt"})
        say("blk46", "blocked", 600)
        wtask(47, "Said ok", "working", "blk47", ok=True)
        say("blk47", "idle", 600)
        wtask(48, "Compacting, by its record", "working", "blk48")
        say("blk48", "working", 600, msg="Compacting conversation…")
        wtask(49, "Compacting, by its screen", "working", "blk49")
        live("blk49", "✽ Compacting conversation… (esc to interrupt)\n")
        # these two are "just now" for as long as the check runs: an hour after NOW
        # (compare.sh's NOW is minutes old by its last case)
        just = dt.datetime.fromtimestamp(now + 3600).astimezone().isoformat()
        wtask(50, "Just went idle", "working", "blk50")
        say("blk50", "idle", None, updated_at=just)
        wtask(51, "Just started", "working", "blk51", since=just)
        say("blk51", "idle", 600)
        rtask(52, "Reviewer idle", "review", "blk52")
        say("blk52", "idle", 600)
        rtask(53, "Reviewer blocked", "review", "blk53")
        say("blk53", "blocked", 600, kind="plan")
        rtask(54, "Reviewer working again", "needs", "blk54", why="Reviewer needs you (plan): ok?",
              stop={"by": "reviewer", "kind": "prompt"})
        say("blk54", "working", 600)
        rtask(55, "Reviewer shell", "review", "blk55")
        say("blk55", "idle", 1200)
        live("blk55", "3 Background Shells\n")
        rtask(56, "Reviewer short shell", "review", "blk56")
        say("blk56", "idle", 600)
        live("blk56", "3 shells\n")
        wt57 = worktree("BP-57", "bullpen/bp-57-done", {"done.py": "done = True\n"})
        wtask(57, "Done, never said", "working", "blk57", branch="bullpen/bp-57-done", worktree=wt57)
        say("blk57", "idle", 600)
        task(58, "Reviewer gone", "needs", 20, why="Reviewer ended without a verdict",
             stop={"by": "reviewer", "kind": "ended"}, review={"round": 1, "block": "blk58"})
        wtask(59, "Prompt on screen", "working", "blk59")
        live("blk59", "Do you want to proceed?\n❯ 1. Yes\n")
        wtask(60, "Empty record", "working", "blk60")
        (fake / "status_blk60.json").write_text("{}")
        wtask(61, "Record without a stamp", "working", "blk61")
        say("blk61", "", None)
        wtask(62, "lowercase needs", "needs", "blk62", why="needs you (lower): hm",
              stop={"by": "worker", "kind": "failed"})
        say("blk62", "working", 600)
        wtask(63, "Stopped", "stopped", "blk63")
        say("blk63", "idle", 600)
    if "deadcoords" in extras:  # no coordinator is running
        for blk in ("blk1", "blk3"):
            (fake / "alive" / blk).unlink()
    if "nocoord" in extras:  # the repo has no coordinators
        cfg.update(coordinators=[], next_coordinator=1)
        (sdir / "config.json").write_text(json.dumps(cfg, indent=2) + "\n")
    if "typed" in extras:  # news is typed into the coordinators' panes (config --news type)
        cfg.update(news="type")
        (sdir / "config.json").write_text(json.dumps(cfg, indent=2) + "\n")
    if "outbox" in extras:  # typed news waiting: for everyone, for c2, and for a gone coordinator
        (sdir / "outbox.txt").write_text("bullpen · shared news\n")
        (sdir / "outbox-c2.txt").write_text("bullpen · for c2\x1f\tdocs.md | 1 +\n")
        (sdir / "outbox-c9.txt").write_text("bullpen · for a gone one\n")
        (state / "outbox").write_text("")
    if "held" in extras:  # notifications held from a burst, quiet for a while now
        (sdir / "notify.json").write_text(json.dumps(
            {"at": now - 60, "ids": ["BP-12", "BP-13"], "held": [["BP-13", "BP-13 needs you: x"]],
             "last": now - 30}, indent=2) + "\n")
        (state / "outbox").write_text("")
    if "oldcursor" in extras:  # the one cursor from before several coordinators
        (sdir / "news-c1.cursor").unlink()
        (sdir / "news.cursor").write_text(f"{first}\n")
    if "badcursor" in extras:
        (sdir / "news-c1.cursor").write_text("junk\n")
    if "pastend" in extras:  # the log was cut short
        (sdir / "news-c1.cursor").write_text("99999\n")
    if "nolog" in extras:
        (sdir / "news.log").unlink()
    if "midrecord" in extras:  # a cursor on a diff's line, and a line still being written
        (sdir / "news-c1.cursor").write_text(f"{len(('\n'.join(news[:3]) + '\n').encode())}\n")
        with (sdir / "news.log").open("a") as f:
            f.write(f"{stamp(1)} [c1] BP-3 note: bullpen · half wri")
    if "badbytes" in extras:  # a line that isn't UTF-8
        with (sdir / "news.log").open("ab") as f:
            f.write(stamp(1).encode() + b" BP-3 note: bullpen \xc2\xb7 caf\xe9 \xff\xfe ok\n")
    if "locked" in extras:  # a follower holds c1's lock for a while
        import fcntl
        lk = open(sdir / "news-c1.lock", "w")
        fcntl.flock(lk, fcntl.LOCK_EX)
        subprocess.Popen(["sleep", "5"], pass_fds=(lk.fileno(),), stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL, start_new_session=True)

    if "panes" in extras:  # Rex's windows: c1's tab with the board and capsules, a task's own tab, c2's tab
        # with a capsule of c1's task that strayed there, background panes, and BP-6's pane under a new ID
        def pane(n, **kw):
            path = tasks / f"BP-{n}.json"
            path.write_text(json.dumps({**json.loads(path.read_text()), **kw}, indent=2) + "\n")
        pane(3, pane="float")
        pane(12, pane="background")
        pane(27, pane="tabs")
        pane(10, review={**json.loads((tasks / "BP-10.json").read_text())["review"], "pane": "background"})
        hint = " · ctrl+esc hides"
        R = {"blocks": {f"blk{i}": {"label": None, "layer": None, "window": None} for i in range(1, 17)},
             "layers": {"lyr1": "minimized", "lyr2": "normal", "lyr3": "minimized", "lyr4": "minimized",
                        "lyr5": "minimized"},
             "windows": ["w1", "w2", "w3"]}
        for blk, label, layer, window in (
                ("blk1", "bullpen: proj", None, "w1"), ("blk2", "bullpen: proj board", None, "w1"),
                ("blk4", "bullpen: BP-3" + hint, "lyr1", "w1"), ("blk5", "bullpen: BP-4" + hint, "lyr2", "w1"),
                ("blk15", "bullpen: BP-40" + hint, "lyr4", "w1"), ("blk12", "bullpen: BP-27", None, "w2"),
                ("blk3", "bullpen: proj", None, "w3"), ("blk13", "bullpen: BP-31" + hint, "lyr3", "w3"),
                ("blk11", "bullpen: BP-12" + hint, "detached", None),
                ("blk8", "bullpen: BP-10 review" + hint, "detached", None),
                ("blk14", "bullpen: BP-32 review" + hint, "detached", None),
                ("blk16", "bullpen: BP-41 review" + hint, "detached", None),
                ("blk20", "bullpen: BP-6" + hint, "lyr5", "w1")):
            R["blocks"][blk] = {"label": label, "layer": layer, "window": window}
        (fake / "reg.json").write_text(json.dumps(R))
        live("blk20")
        for blk, role in (("blk2", "board"), ("blk4", "BP-3"), ("blk5", "BP-4"), ("blk11", "BP-12"),
                          ("blk12", "BP-27"), ("blk13", "BP-31"), ("blk15", "BP-40"), ("blk8", "BP-10 review")):
            (blocks / f"{blk}.json").write_text(json.dumps({"repo": str(root), "role": role, "stuck_after": 15}))

    def config(**kw):
        path = sdir / "config.json"
        path.write_text(json.dumps({**json.loads(path.read_text()), **kw}, indent=2) + "\n")

    def commit(name, text):  # a file committed on main
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_text(text)
        g("add", "-A")
        g("commit", "-qm", f"add {name}")

    if "queued" in extras:  # tasks waiting on done ones, and room for two more to work (12 do)
        for n, after in ((1, ["BP-23"]), (28, ["BP-25"]), (39, ["BP-23", "BP-24"])):
            path = tasks / f"BP-{n}.json"
            path.write_text(json.dumps({**json.loads(path.read_text()), "after": after}, indent=2) + "\n")
        config(max_running=14)
    if "full" in extras:  # as queued, with no room
        config(max_running=3)
    if "rules" in extras:  # the repo's rules, committed, and the person's own
        commit(".bullpen/rules.md", "# Rules\n\nKeep functions small.\n\n")
        (sdir / "rules.md").write_text("\nMy own rule.\n")
    if "setup" in extras:  # a committed setup script that works
        commit(".bullpen/setup.sh", "#!/bin/sh\necho \"setting up $BULLPEN_TASK on $BULLPEN_BRANCH\"\n")
        (root / ".bullpen" / "setup.sh").chmod(0o755)
    if "badsetup" in extras:  # a setup in the config that fails, over a committed one
        commit(".bullpen/setup.sh", "echo committed\n")
        config(setup="echo nope >&2; exit 3")
    if "briefs" in extras:  # briefs for the tasks a case starts
        for n in (1, 2, 13, 16, 28, 39):
            (sdir / "briefs" / f"BP-{n}.md").write_text(f"Do task {n}.\n")
        (sdir / "briefs" / "BP-23.review-1.findings.md").write_text("one\n")
        (sdir / "briefs" / "BP-23.review-2.findings.md").write_text("two\n")
        path = tasks / "BP-23.json"
        path.write_text(json.dumps({**json.loads(path.read_text()), "summary": "  Merged the parser.\n"},
                                   indent=2) + "\n")

    def retask(n, **kw):  # change task n's fields
        path = tasks / f"BP-{n}.json"
        path.write_text(json.dumps({**json.loads(path.read_text()), **kw}, indent=2) + "\n")

    def out(*a, cwd):
        return subprocess.run(["git", *a], cwd=cwd, check=True, capture_output=True, text=True, env=git_env).stdout

    def covers(wt, base="main"):  # what a review of wt's commit recorded: (commit, base, patch, own)
        diff = out("diff", "--binary", f"{base}...HEAD", cwd=wt)
        patch = subprocess.run(["git", "-C", wt, "patch-id", "--stable"], input=diff, text=True,
                               capture_output=True).stdout.split()[0]
        own, h = {}, None
        for line in out("diff", "-U0", "--binary", "--no-color", "--no-ext-diff", "--no-renames",
                      f"{base}...HEAD", cwd=wt).split("\n"):
            if line.startswith("diff --git "):
                pair = line[len("diff --git "):]
                h = own[pair[2:2 + (len(pair) - 5) // 2]] = hashlib.sha1()
            elif h and not line.startswith(("index ", "@@")):
                h.update(line.encode() + b"\n")
        return (out("rev-parse", "HEAD", cwd=wt).strip(), out("merge-base", base, "HEAD", cwd=wt).strip(), patch,
                {k: v.hexdigest() for k, v in own.items()})

    # BP-17's approval: on its commit (approved), its patch in another commit (reworded), its own lines
    # with the base moved under it (moved), or with its own lines changed where a rebase conflicted (conflicted)
    for kind in ("approved", "reworded", "moved", "conflicted"):
        if kind not in extras:
            continue
        commit, base, patch, own = covers(wt17)
        r = dict(review_full, block=None, commit=commit, base=base, patch=patch, own=own)
        if kind == "reworded":
            r.update(commit="0" * 40)
        if kind in ("moved", "conflicted"):
            r.update(commit="0" * 40, base="1" * 40, patch="2" * 40)
        if kind == "conflicted":
            r.update(own={"docs.md": "3" * 40}, conflict=["docs.md"], conflict_diff=str(sdir / "briefs" / "c.diff"))
            (sdir / "briefs" / "c.diff").write_text("<<<<<<< ours\n=======\n>>>>>>> theirs\n")
        retask(17, review=r, status="working", block="blk4", session="session:1")
    if "inreview" in extras:  # BP-17 in review, its reviewer gone without a verdict
        retask(17, status="review", review={"round": 1, "kind": "light", "why": "docs only", "block": "blk19",
                                            "session": "session:1", "verdict": None, "prev": None})
    if "toursent" in extras:  # the person's comments on BP-17's tour, sent back
        (sdir / "briefs" / "BP-17.tour-comments.md").write_text("Rename it.\n")
        commit, base, _, _ = covers(wt17)
        retask(17, tour_sent={"n": 1, "count": 1, "file": str(sdir / "briefs" / "BP-17.tour-comments.md"),
                              "commit": commit, "base": base})
    if "verdicts" in extras:  # reviewers in panes for each kind of review
        retask(36, review={"round": 1, "kind": "rebase", "block": "blk17", "session": "session:1",
                           "verdict": None}, commits=2)
        retask(37, review={"round": 2, "kind": "tour", "tour": 1, "block": "blk18", "session": "session:1",
                           "verdict": None}, tour_sent={"n": 1, "count": 2, "file": "x"})
        retask(11, review={"round": 3, "kind": "followup", "block": "blk10", "verdict": None},
               pr="https://github.com/me/proj/pull/11", gh={"sent": True})
        for blk in ("blk17", "blk18", "blk10"):
            (fake / "alive" / blk).write_text("")
    if "noreview" in extras:
        config(review=False)

    env = {"HOME": home, "BULLPEN_STATE": state, "BULLPEN_REX": fake / "rex", "FAKE_DIR": fake,
           "XDG_CONFIG_HOME": d / "config", "FIXTURE_ROOT": root,
           "BULLPEN_NO_BUILD": 1}  # never build, or take the build lock in, the checkout it runs from
    for k, v in env.items():
        print(f"export {k}='{v}'")
    print("unset REX_BLOCK REX_SESSION NO_COLOR")


if __name__ == "__main__":
    main(sys.argv[1], float(sys.argv[2]))
