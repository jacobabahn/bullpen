"""End-to-end tests for bin/bullpen against fake `rex` and `gh` and a local origin.

Run: python3 -m unittest discover -s tests
"""

import datetime as dt
import json
import os
import select
import shutil
import subprocess
import tempfile
import textwrap
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BULLPEN = ROOT / "bin" / "bullpen"

FAKE_REX = textwrap.dedent("""\
    #!/usr/bin/env python3
    import json, os, sys
    from pathlib import Path
    d = Path(os.environ["FAKE_DIR"])
    a = sys.argv[1:]
    with open(d / "rex.log", "a") as f:
        f.write(json.dumps(a) + "\\n")
    alive = d / "alive"
    alive.mkdir(exist_ok=True)
    reg = d / "reg.json"  # blocks: {id: {label, layer, window}}; layers: {id: state}; windows
    R = json.loads(reg.read_text()) if reg.exists() else {"blocks": {}, "layers": {}, "windows": []}

    def new_block(label, layer=None, window=None):
        n = len(R["blocks"]) + 1
        (alive / f"blk{n}").write_text("")
        R["blocks"][f"blk{n}"] = {"label": label, "layer": layer, "window": window}
        return f"blk{n}"

    def new_window():
        w = f"w{len(R['windows']) + 1}"
        R["windows"].append(w)
        return w

    if a[0] in ("send", "send-key") and (d / f"send_fails_{a[a.index('-b') + 1]}").exists():
        sys.exit("rex: couldn't send to the block")  # what the tests make a pane refuse
    if a[0] == "run":
        b = new_block(a[a.index("--label") + 1] if "--label" in a else None, window=new_window())
        print(json.dumps({"window_id": R["blocks"][b]["window"], "block_ids": [b]}))
    elif a[0] == "split":
        anchor = R["blocks"].get(a[a.index("-b") + 1]) if "-b" in a else None
        b = new_block(a[a.index("--label") + 1] if "--label" in a else None,
                      window=anchor["window"] if anchor else new_window())
        print(json.dumps({"block_ids": [b]}))
    elif a[:2] == ["block", "inspect"]:
        sys.exit(0 if (alive / a[2]).exists() else 1)
    elif a[:2] == ["block", "close"]:
        (alive / a[2]).unlink(missing_ok=True)
    elif a[0] == "capture":
        own = d / f"screen_{a[a.index('-b') + 1]}"  # one pane's screen, else every pane's
        screen = own if own.exists() else d / "screen"  # what the tests put on screen, else two lines
        print(screen.read_text() if screen.exists() else "line one\\nline two\\n\\n")
    elif a[:2] == ["block", "call"] and a[-1] == "process":
        pid = d / f"pid_{a[a.index('-b') + 1]}"  # a process the tests run on a terminal of their own
        if not pid.exists():
            sys.exit("rex: no such block")
        print(json.dumps({"child": {"pid": int(pid.read_text())}}))
    elif a[:2] == ["block", "call"] and a[-1] == "program_status":
        rec = d / f"status_{a[a.index('-b') + 1]}.json"  # the record the tests give a pane
        if not rec.exists():
            sys.exit("rex: no such method")  # an older Rex: bullpen goes by the screen
        print(json.dumps({"records": [json.loads(rec.read_text())]}))
    elif a[:2] == ["api", "call"]:
        rest = a[2:]
        sid = R.get("session", "session:1")
        if rest[0] == "-s":
            if rest[1] not in (sid, "main"):  # a session that no longer exists
                sys.exit(1)
            rest = rest[2:]
        method, args = rest[0], json.loads(rest[1]) if len(rest) > 1 else {}
        if method == "session.new_layer":
            lid = f"lyr{len(R['layers']) + 1}"
            R["layers"][lid] = "normal"
            b = new_block(args["layout"]["block"]["label"], lid,
                          args.get("window_id") or (R["windows"] or [new_window()])[0])
            print(json.dumps({"layer_id": lid, "block_ids": [b]}))
        elif method == "session.new_block":
            print(json.dumps({"block_id": new_block(args["label"], "detached")}))
        elif method == "session.move_block":
            blk = R["blocks"][args["block_id"]]
            blk.update(layer=None, window=R["blocks"][args["anchor_block_id"]]["window"])
            print("{}")
        elif method == "session.float_block":
            lid = f"lyr{len(R['layers']) + 1}"
            R["layers"][lid] = "normal"
            R["blocks"][args["block_id"]]["layer"] = lid
            print(json.dumps({"layer_id": lid}))
        elif method == "session.close_window":
            R["windows"].remove(args["window_id"])
            print("{}")
        elif method == "session.set_layer_state":
            R["layers"][args["layer_id"]] = args["state"]
            print("{}")
        elif method == "session.view":
            windows = []
            for w in R["windows"]:
                tiled = {"layer_id": f"tiled-{w}", "kind": "tiled", "blocks": []}
                layers = [tiled]
                for b, info in R["blocks"].items():
                    if not (alive / b).exists() or info["layer"] == "detached" or info["window"] != w:
                        continue
                    entry = {"block_id": b, "label": info["label"]}
                    if info["layer"]:
                        layers.append({"layer_id": info["layer"], "kind": "floating",
                                       "state": R["layers"][info["layer"]], "blocks": [entry]})
                    else:
                        tiled["blocks"].append(entry)
                windows.append({"window_id": w, "layers": layers})
            view = {"session_id": sid, "label": "main", "windows": windows}
            focused = d / "focused"  # the block the tests say has focus, in the active window
            if focused.exists():
                b = focused.read_text()
                view["active_window_id"] = R["blocks"][b]["window"]
                for w in windows:
                    if w["window_id"] == view["active_window_id"]:
                        w["focused_block_id"] = b
            print(json.dumps(view))
    reg.write_text(json.dumps(R))
""")

FAKE_GH = textwrap.dedent("""\
    #!/usr/bin/env python3
    import json, os, sys
    from pathlib import Path
    d = Path(os.environ["FAKE_DIR"])
    body = sys.stdin.read()  # like a gh that waits for input: it must get none
    with open(d / "gh.log", "a") as f:
        f.write(json.dumps({"argv": sys.argv[1:], "body": body}) + "\\n")
    if sys.argv[1:3] == ["pr", "create"]:
        print("https://github.com/me/repo/pull/7")
    elif sys.argv[1:3] == ["pr", "view"]:
        pr = d / "pr.json"  # what the tests say GitHub has on the PR
        print(pr.read_text() if pr.exists() else json.dumps({"state": "OPEN"}))
""")


def sh(*cmd, cwd=None):
    return subprocess.run(cmd, cwd=cwd, check=True, text=True, capture_output=True).stdout


def setUpModule():
    """Run the suite in a temp folder of its own, so a test that leaves files behind fails it."""
    global SUITE_TMP, OUTER_TMPDIR
    OUTER_TMPDIR = os.environ.get("TMPDIR")
    SUITE_TMP = tempfile.mkdtemp(prefix="bullpen-tests-")
    os.environ["TMPDIR"] = tempfile.tempdir = SUITE_TMP


def tearDownModule():
    left = sorted(os.listdir(SUITE_TMP))
    shutil.rmtree(SUITE_TMP, ignore_errors=True)
    tempfile.tempdir = None
    if OUTER_TMPDIR is None:
        os.environ.pop("TMPDIR", None)
    else:
        os.environ["TMPDIR"] = OUTER_TMPDIR
    if left:
        raise AssertionError(f"tests left {len(left)} files in TMPDIR: {', '.join(left[:5])}")


class BullpenTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        fake = self.tmp / "fake"
        fake.mkdir()
        for name, src in (("rex", FAKE_REX), ("gh", FAKE_GH)):
            p = fake / name
            p.write_text(src)
            p.chmod(0o755)
        self.fake = fake
        origin = self.tmp / "origin.git"
        sh("git", "init", "-q", "--bare", "-b", "main", str(origin))
        self.repo = self.tmp / "proj"
        sh("git", "init", "-q", "-b", "main", str(self.repo))
        sh("git", "-C", str(self.repo), "config", "user.email", "t@example.com")
        sh("git", "-C", str(self.repo), "config", "user.name", "T")
        (self.repo / "README").write_text("hi\n")
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "init")
        sh("git", "-C", str(self.repo), "remote", "add", "origin", str(origin))
        sh("git", "-C", str(self.repo), "push", "-q", "-u", "origin", "main")
        self.env = {**os.environ, "BULLPEN_STATE": str(self.tmp / "state"),
                    "BULLPEN_REX": str(fake / "rex"), "BULLPEN_GH": str(fake / "gh"),
                    "FAKE_DIR": str(fake), "REX_SESSION": "session:1", "REX_BLOCK": "me",
                    "GIT_AUTHOR_NAME": "T", "GIT_AUTHOR_EMAIL": "t@example.com",
                    "GIT_COMMITTER_NAME": "T", "GIT_COMMITTER_EMAIL": "t@example.com"}

    def bp(self, *args, cwd=None, ok=True):
        p = subprocess.run([str(BULLPEN), *args], cwd=cwd or self.repo, env=self.env,
                           text=True, capture_output=True)
        if ok and p.returncode != 0:
            self.fail(f"bullpen {' '.join(args)} failed: {p.stderr}")
        return p

    def rex_calls(self):
        log = self.fake / "rex.log"
        return [json.loads(l) for l in log.read_text().splitlines()] if log.exists() else []

    def api(self, method):
        """Arguments of each `rex api call` to method."""
        out = []
        for a in self.rex_calls():
            if a[:2] == ["api", "call"] and method in a:
                out.append(json.loads(a[a.index(method) + 1]))
        return out

    def layer_states(self):
        return json.loads((self.fake / "reg.json").read_text())["layers"]

    def restart_rex(self, new_session=False):
        """Rex restarts: every placed pane comes back with a new ID, detached ones don't.
        With new_session, the session gets a new ID too (its label stays)."""
        reg = self.fake / "reg.json"
        R = json.loads(reg.read_text())
        if new_session:
            R["session"] = "session:2"
            self.env["REX_SESSION"] = "session:2"
        for b, info in list(R["blocks"].items()):
            if (self.fake / "alive" / b).exists():
                (self.fake / "alive" / b).unlink()
                if info["layer"] != "detached":
                    n = f"blk{len(R['blocks']) + 1}"
                    R["blocks"][n] = dict(info)
                    (self.fake / "alive" / n).write_text("")
        reg.write_text(json.dumps(R))

    def typed(self):
        """Lines typed into panes with `rex send`, as (block, text)."""
        out = []
        for a in self.rex_calls():
            if a[0] == "send":
                out.append((a[a.index("-b") + 1], a[-1]))
        return out

    def show(self, tid):
        return json.loads(self.bp("show", tid, "--json").stdout)

    def start_task(self, title="Fix the thing"):
        self.bp("up")  # the coordinator (blk1), the board (blk2) and the detached watcher (blk3)
        tid = self.bp("add", title, "--brief", "Make it work.").stdout.strip()
        self.bp("start", tid)
        return tid

    def block(self, tid):
        return self.show(tid)["block"]

    def test_up_spawns_coordinator_and_one_watcher(self):
        self.bp("up")
        self.bp("up")
        runs = [a for a in self.rex_calls() if a[0] == "run"]
        labels = [a[a.index("--label") + 1] for a in runs]
        self.assertEqual(labels, ["bullpen: proj"])
        watchers = self.api("session.new_block")
        self.assertEqual(len(watchers), 1)  # detached: no tab
        self.assertEqual(watchers[0]["label"], "bullpen: watcher")
        splits = [a for a in self.rex_calls() if a[0] == "split"]
        self.assertEqual(len(splits), 1)  # one board, beside the coordinator
        self.assertEqual(splits[0][splits[0].index("-b") + 1], "blk1")
        self.assertEqual(splits[0][-3:], ["board", "--coordinator", "c1"])
        watcher = watchers[0]["options"]["command"]
        self.assertIn("--all", watcher)
        self.assertTrue(any(x.startswith("bin=") for x in watcher))

    def test_up_here_registers_this_pane(self):
        self.bp("up", "--here")
        self.assertFalse(any(a[0] == "run" and "bullpen: proj" in a for a in self.rex_calls()))
        cfg = json.loads(self.bp("config").stdout)
        self.assertEqual([(c["id"], c["block"], c["session"]) for c in cfg["coordinators"]],
                         [("c1", "me", "session:1")])

    def test_start_makes_worktree_branch_and_pane(self):
        tid = self.start_task()
        self.assertEqual(tid, "BP-1")
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        self.assertEqual(t["branch"], "bullpen/bp-1-fix-the-thing")
        self.assertTrue(Path(t["worktree"]).is_dir())
        self.assertEqual(t["block"], "blk4")
        layer = self.api("session.new_layer")[-1]
        self.assertEqual(layer["layout"]["block"]["options"]["cwd"], t["worktree"])
        self.assertEqual(layer["layout"]["block"]["label"], "bullpen: BP-1")
        brief = (self.tmp / "state").rglob("BP-1.full.md").__next__().read_text()
        self.assertIn("Make it work.", brief)
        self.assertIn("ready BP-1 --summary-file", brief)
        self.assertIn("Never add a Co-Authored-By", brief)

    def test_ready_pushes_and_opens_pr_then_news(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "fix.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        summary = self.tmp / "s.md"
        summary.write_text("Fixed the thing. Checked by hand.\n")
        self.bp("config", "--review", "off")
        self.bp("ready", tid, "--summary-file", str(summary), cwd=wt)
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertEqual(t["pr"], "https://github.com/me/repo/pull/7")
        sh("git", "--git-dir", str(self.tmp / "origin.git"), "rev-parse", "refs/heads/" + t["branch"])
        gh = [json.loads(l) for l in (self.fake / "gh.log").read_text().splitlines()]
        self.assertEqual(gh[0]["argv"][:2], ["pr", "create"])
        self.assertIn("Fix the thing (BP-1)", gh[0]["argv"])
        self.assertIn("Fixed the thing.", gh[0]["body"])
        self.assertNotIn("Claude", gh[0]["body"])
        block, line = self.typed()[-1]
        self.assertEqual(block, "blk1")
        self.assertEqual(line, "bullpen · BP-1 is ready (1 commit, PR #7): Fix the thing")

    def test_ready_refuses_uncommitted_changes(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "dirty.txt").write_text("x\n")
        p = self.bp("ready", tid, cwd=wt, ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("uncommitted", p.stderr)

    def test_events_become_news_once(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "working", "", "Reading files")
        self.bp("_event", self.block(tid), "blocked", "permission", "Allow rm?")
        self.bp("_event", self.block(tid), "blocked", "permission", "Allow rm?")  # repeat: no news
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual(len(news), 1)
        self.assertTrue(news[0].startswith("bullpen · BP-1 needs the person (permission)"))
        self.assertIn("Allow rm?", news[0])
        self.assertEqual(self.show(tid)["status"], "needs")
        self.bp("_event", self.block(tid), "working", "", "")
        self.assertEqual(self.show(tid)["status"], "working")
        self.bp("_event", self.block(tid), "done", "", "")
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertIn("ended its turn without `ready`", news[-1])

    def test_every_end_of_turn_is_news(self):
        # BP-12: it ended its turn, a background shell finished, it took another turn
        # and ended it the same way. Each end is news; a repeat within a turn isn't.
        tid = self.start_task()
        blk = self.block(tid)
        for turn in (1, 2):
            self.bp("_event", blk, "working", "", "Running the tests")
            self.assertEqual(self.show(tid)["status"], "working")
            self.bp("_event", blk, "idle", "", "Waiting for the tests")
            self.bp("_event", blk, "idle", "", "Waiting for the tests")  # repeat: no news
            t = self.show(tid)
            self.assertEqual((t["status"], t["why"]), ("triage", "Ended its turn without saying ready"))
            ended = [l for l in self.news() if "ended its turn" in l]
            self.assertEqual(len(ended), turn)

    def test_every_stop_of_a_reviewer_is_news(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        for turn in (1, 2):
            self.bp("_event", reviewer, "working", "", "Reading the diff")
            self.assertEqual(self.show(tid)["status"], "review")
            self.bp("_event", reviewer, "idle", "", "Waiting")
            self.bp("_event", reviewer, "idle", "", "Waiting")  # repeat: no news
            t = self.show(tid)
            self.assertEqual((t["status"], t["why"]), ("triage", "Reviewer stopped without a verdict"))
            stopped = [l for l in self.news() if "stopped without a verdict" in l]
            self.assertEqual(len(stopped), turn)

    def test_done_after_ready_is_silent(self):
        tid = self.start_task()
        self.bp("ready", tid)  # no commits: ready with nothing to merge
        before = len(self.typed())
        self.bp("_event", self.block(tid), "done", "", "")
        self.assertEqual(len(self.typed()), before)

    def test_exit_marks_stopped_and_resume_relaunches(self):
        tid = self.start_task()
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)
        t = self.show(tid)
        self.assertEqual(t["status"], "stopped")
        self.assertIn("stopped before saying ready", self.typed()[-1][1])
        self.bp("resume", tid)
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertIn("--continue", cmd)
        self.assertEqual(self.show(tid)["status"], "working")

    def test_unknown_block_events_are_ignored(self):
        self.start_task()
        before = len(self.typed())
        self.bp("_event", "stranger", "blocked", "permission", "x")
        self.assertEqual(len(self.typed()), before)

    def test_accept_merges_and_cleans_up(self):
        tid = self.start_task()
        t = self.show(tid)
        wt, blk = t["worktree"], t["block"]
        (Path(wt) / "fix.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        self.bp("config", "--review", "off")
        self.bp("ready", tid, "--summary", "done")
        self.bp("accept", tid)
        t = self.show(tid)
        self.assertEqual(t["status"], "done")
        self.assertFalse(Path(wt).exists())
        gh = [json.loads(l)["argv"] for l in (self.fake / "gh.log").read_text().splitlines()]
        self.assertEqual(gh[-1][:2], ["pr", "merge"])
        self.assertIn("--rebase", gh[-1])
        self.assertFalse((self.fake / "alive" / blk).exists())
        branches = sh("git", "-C", str(self.repo), "branch")
        self.assertNotIn("bullpen/", branches)

    def test_accept_refuses_unready(self):
        tid = self.start_task()
        p = self.bp("accept", tid, ok=False)
        self.assertNotEqual(p.returncode, 0)

    def test_drop_refuses_dirty_without_force(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "dirty.txt").write_text("x\n")
        self.assertNotEqual(self.bp("drop", tid, ok=False).returncode, 0)
        self.bp("drop", tid, "--force")
        self.assertEqual(self.show(tid)["status"], "dropped")
        self.assertFalse(Path(wt).exists())

    def ready_with_a_commit(self):
        self.bp("config", "--review", "off")
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "fix.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        self.bp("ready", tid, "--summary", "done")
        return tid

    def test_accept_from_elsewhere_is_news(self):
        tid = self.ready_with_a_commit()
        self.bp("accept", tid)  # REX_BLOCK is "me": the board, say, not the coordinator
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-1 accepted (merged PR #7, rebase)"))

    def test_accept_from_the_coordinators_pane_is_no_news(self):
        tid = self.ready_with_a_commit()
        before = self.typed()
        self.env["REX_BLOCK"] = "blk1"
        self.bp("accept", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(self.typed(), before)

    def test_accept_without_a_pane_of_its_own_is_news(self):
        tid = self.ready_with_a_commit()
        del self.env["REX_BLOCK"]  # can't tell where it ran: post
        self.bp("accept", tid)
        self.assertIn("BP-1 accepted", self.typed()[-1][1])

    def test_accept_of_a_pr_merged_outside_says_so(self):
        tid = self.ready_with_a_commit()
        (self.fake / "pr.json").write_text(json.dumps({"state": "MERGED"}))
        self.bp("_prs")
        self.bp("accept", tid)
        self.assertEqual(self.typed()[-1][1], "bullpen · BP-1 accepted (PR #7 was merged outside bullpen)")

    def test_drop_is_news_from_elsewhere_only(self):
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        self.bp("drop", "BP-1")
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-1 dropped"))
        before = self.typed()
        self.env["REX_BLOCK"] = "blk1"
        self.bp("drop", "BP-2")
        self.assertEqual(self.show("BP-2")["status"], "dropped")
        self.assertEqual(self.typed(), before)

    def watcher(self):
        return json.loads((self.tmp / "state" / "watcher.json").read_text())

    def make_watcher_stale(self, **change):
        w = self.watcher()
        w["stamp"] = {**w["stamp"], **change}
        (self.tmp / "state" / "watcher.json").write_text(json.dumps(w))

    def test_up_restarts_a_stale_watcher(self):
        self.bp("up")
        old = self.watcher()
        self.assertEqual(sorted(old["stamp"]), ["bullpen", "watch"])
        self.make_watcher_stale(watch="an older watch.lua")
        out = self.bp("up").stdout
        new = self.watcher()
        self.assertIn(f"watcher restarted (watch.lua or bullpen changed): {new['block']}", out)
        self.assertNotEqual(new["block"], old["block"])
        self.assertEqual(new["stamp"], old["stamp"])  # what runs now
        self.assertFalse((self.fake / "alive" / old["block"]).exists())  # the old one is closed
        self.assertTrue((self.fake / "alive" / new["block"]).exists())
        self.assertEqual(len(self.api("session.new_block")), 2)
        self.bp("up")  # current now: left alone
        self.assertEqual(self.watcher(), new)
        self.assertEqual(len(self.api("session.new_block")), 2)

    def test_a_watcher_from_before_stamps_is_restarted(self):
        self.bp("up")
        old = self.watcher()
        (self.tmp / "state" / "watcher.json").write_text(
            json.dumps({"block": old["block"], "session": old["session"]}))
        self.bp("up")
        self.assertNotEqual(self.watcher()["block"], old["block"])
        self.assertFalse((self.fake / "alive" / old["block"]).exists())

    def test_accept_restarts_a_stale_watcher_and_leaves_a_current_one(self):
        tid = self.ready_with_a_commit()
        old = self.watcher()
        self.bp("drop", tid)  # nothing merged: the watcher isn't looked at
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        self.bp("ready", "BP-2")
        self.bp("accept", "BP-2")
        self.assertEqual(self.watcher(), old)  # current: left alone
        self.assertTrue((self.fake / "alive" / old["block"]).exists())
        self.bp("add", "Third", "--brief", "x")
        self.bp("start", "BP-3")
        self.bp("ready", "BP-3")
        self.make_watcher_stale(bullpen="an older bullpen")
        self.bp("accept", "BP-3")
        new = self.watcher()
        self.assertNotEqual(new["block"], old["block"])
        self.assertFalse((self.fake / "alive" / old["block"]).exists())
        self.assertEqual(len(self.api("session.new_block")), 2)

    def commit_and_ready(self, tid, n=1):
        wt = self.show(tid)["worktree"]
        (Path(wt) / f"fix{n}.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", f"Fix {n}")
        self.bp("ready", tid, "--summary", "Did it.", cwd=wt)
        return self.show(tid)["review"]["block"]

    def verdict(self, tid, v, text="Looks right."):
        return self.bp("verdict", tid, v, "--summary", text)

    def test_ready_starts_a_reviewer(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        t = self.show(tid)
        self.assertEqual(t["status"], "review")
        self.assertEqual(t["pr"], "https://github.com/me/repo/pull/7")  # pushed and opened first
        layer = self.api("session.new_layer")[-1]
        self.assertEqual(layer["layout"]["block"]["label"], "bullpen: BP-1 review")
        self.assertEqual(self.layer_states()["lyr2"], "minimized")
        prompt = next((self.tmp / "state").rglob("BP-1.review-1.md")).read_text()
        self.assertIn("verdict BP-1 approve", prompt)
        self.assertIn("Did it.", prompt)
        self.assertFalse(any("is ready" in l for _, l in self.typed()))  # not ready yet
        self.assertTrue((self.fake / "alive" / reviewer).exists())

    def test_approval_makes_it_ready_and_closes_the_reviewer(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertEqual(t["review"]["verdict"], "approved")
        self.assertFalse((self.fake / "alive" / reviewer).exists())
        self.assertEqual(self.typed()[-1], ("blk1",
            "bullpen · BP-1 is ready (1 commit, PR #7), approved by light review (1 line in 1 file): Fix the thing"))

    def test_changes_go_back_to_the_worker_then_ready_after_the_last_round(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.commit_and_ready(tid)
        self.verdict(tid, "changes", "The fix misses the empty case.")
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        block, line = self.typed()[-1]
        self.assertEqual(block, worker)  # the findings go to the worker, not the coordinator
        self.assertIn("BP-1.review-1.findings.md", line)
        self.commit_and_ready(tid, 2)
        self.assertEqual(self.show(tid)["review"]["round"], 2)
        self.verdict(tid, "changes", "Still misses it.")
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertIn("after 2 rounds", t["why"])
        block, line = self.typed()[-1]
        self.assertEqual(block, "blk1")
        self.assertIn("still asks for changes", line)

    def test_changes_that_dont_reach_the_worker_go_to_the_coordinator(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.commit_and_ready(tid)
        (self.fake / f"send_fails_{worker}").write_text("")
        self.verdict(tid, "changes", "The fix misses the empty case.")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Review findings didn't reach its worker"))
        self.assertEqual(t["review"]["verdict"], "changes")
        line = self.news()[-1]
        self.assertIn("BP-1's reviewer asked for changes, but the message didn't reach its worker", line)
        self.assertIn("BP-1.review-1.findings.md", line)
        self.assertIn("`bullpen tell BP-1`", line)
        (self.fake / f"send_fails_{worker}").unlink()
        self.bp("tell", tid, "Read the findings.")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_an_empty_findings_file_is_refused(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        empty = self.tmp / "findings.md"
        empty.write_text("\n")
        p = self.bp("verdict", tid, "changes", "--summary-file", str(empty), ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("is empty", p.stderr)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["verdict"]), ("review", None))
        self.assertEqual(list((self.tmp / "state").rglob("BP-1.review-1.findings.md")), [])
        p = self.bp("verdict", tid, "changes", "--summary-file", str(self.tmp / "missing.md"), ok=False)
        self.assertIn("can't read", p.stderr)

    def test_an_empty_ready_summary_file_is_refused(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        empty = self.tmp / "summary.md"
        empty.write_text("")
        p = self.bp("ready", tid, "--summary-file", str(empty), cwd=wt, ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("is empty", p.stderr)
        self.assertEqual(self.show(tid)["status"], "working")

    def test_changes_resume_a_worker_that_exited(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        (self.fake / "alive" / self.block(tid)).unlink()
        self.verdict(tid, "changes", "Fix it.")
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertIn("--continue", cmd)
        self.assertTrue(any("findings" in x for x in cmd))

    def test_a_stuck_reviewer_needs_you(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp("_event", reviewer, "blocked", "permission", "Allow curl?")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Reviewer needs you (permission): Allow curl?"))
        self.assertIn("reviewer needs the person", self.typed()[-1][1])
        self.bp("_event", reviewer, "working", "", "")
        self.assertEqual(self.show(tid)["status"], "review")
        self.bp("_event", reviewer, "done", "", "")
        self.assertEqual(self.show(tid)["why"], "Reviewer stopped without a verdict")
        self.bp("_gone", reviewer)
        self.assertEqual(self.show(tid)["status"], "triage")

    def news(self):
        return [l for b, l in self.typed() if b == "blk1"]

    # ---------- status drift: check ----------

    def agent(self, blk, state, kind="", msg="", ago=600):
        """What Rex says the program in pane blk is doing, as of ago seconds back."""
        at = (dt.datetime.now().astimezone() - dt.timedelta(seconds=ago)).isoformat()
        rec = {"state": state, "app": "claude-code", "msg": msg, "updated_at": at}
        if kind:
            rec["kind"] = kind
        (self.fake / f"status_{blk}.json").write_text(json.dumps(rec))

    def settle(self):
        """Every task entered its status 3 minutes ago: past check's settle (and short
        of triage_minutes)."""
        at = (dt.datetime.now().astimezone() - dt.timedelta(minutes=3)).isoformat()
        for f in (self.tmp / "state").rglob("tasks/*.json"):
            t = json.loads(f.read_text())
            f.write_text(json.dumps({**t, "since": at}))

    def check(self, *args, settle=True):
        if settle:
            self.settle()
        p = self.bp("check", *args, ok=False)
        self.assertIn(p.returncode, (0, 1), p.stderr)
        return p.stdout.splitlines()

    def test_check_prints_nothing_when_all_agree(self):
        tid = self.start_task()
        self.agent(self.block(tid), "working", msg="Reading")
        self.settle()
        p = self.bp("check")
        self.assertEqual((p.returncode, p.stdout), (0, ""))

    def test_check_leaves_a_task_bullpen_just_changed(self):
        # `tell` (or start, resume, a verdict) marked it working a moment ago; its agent's
        # record still says idle from before, and its working event is on the way
        tid = self.start_task()
        self.agent(self.block(tid), "idle", ago=600)
        self.assertEqual(self.check(settle=False), [])
        self.bp("check", "--fix", "--quiet")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_check_skips_a_task_without_a_pane(self):
        self.bp("up")
        self.bp("add", "Later", "--brief", "x")  # todo: no pane
        tid = self.start_task("Now")
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)  # stopped, its pane forgotten
        self.assertEqual(self.show(tid)["status"], "stopped")
        self.settle()
        p = self.bp("check")
        self.assertEqual((p.returncode, p.stdout), (0, ""))

    def test_check_finds_a_missed_end_of_turn_and_fix_reports_it_once(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Running the tests")
        self.agent(blk, "idle", msg="Done")
        self.assertEqual(self.check(), [f"{tid} is marked working but its agent is idle at its prompt: "
                                        "`bullpen peek` it"])
        self.assertEqual(self.bp("check", ok=False).returncode, 1)
        self.assertEqual(self.show(tid)["status"], "working")  # no --fix: nothing changes
        before = len(self.news())
        self.assertEqual(self.bp("check", "--fix", "--quiet").stdout, "")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Ended its turn without saying ready"))
        news = self.news()[before:]
        self.assertEqual(len(news), 1)
        self.assertIn("ended its turn without `ready`", news[0])
        self.bp("check", "--fix", "--quiet")  # agrees now: nothing again
        self.assertEqual(self.check(), [])
        self.assertEqual(len(self.news()), before + 1)

    def test_check_leaves_a_state_that_just_changed(self):
        tid = self.start_task()
        self.agent(self.block(tid), "idle", ago=5)  # the watcher reports it after its own wait
        self.assertEqual(self.check(), [])

    def test_check_finds_a_missed_permission_prompt_and_its_end(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.agent(blk, "blocked", kind="permission", msg="Allow rm?")
        self.assertEqual(self.check(), [f"{tid} is marked working but its agent is waiting on you (permission)"])
        self.check("--fix")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Needs you (permission): Allow rm?"))
        self.assertIn("needs the person (permission)", self.news()[-1])
        self.agent(blk, "working", msg="Removing")
        self.assertEqual(self.check(), [f"{tid} is marked needs (permission) but its agent is working again"])
        self.assertEqual(self.check("--fix"), [f"{tid} is marked needs (permission) but its agent is "
                                               "working again (fixed)"])
        self.assertEqual(self.show(tid)["status"], "working")
        self.assertEqual(self.check(), [])

    def test_check_sees_a_busy_agent_working_again(self):
        # a working agent's record is new every few seconds (its message changes)
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        self.agent(blk, "working", msg="Running rm", ago=3)
        self.assertEqual(self.check(), [f"{tid} is marked needs (permission) but its agent is working again"])
        self.check("--fix", "--quiet")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_check_finds_a_gone_pane(self):
        tid = self.start_task()
        (self.fake / "alive" / self.block(tid)).unlink()
        self.assertEqual(self.check(), [f"{tid} is marked working but its pane is gone"])
        self.check("--fix", "--quiet")
        self.assertEqual(self.show(tid)["status"], "stopped")
        self.assertIn("stopped before saying ready", self.news()[-1])
        self.assertEqual(self.check(), [])

    def test_check_finds_a_reviewer_idle_without_a_verdict(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.agent(self.block(tid), "idle")  # the worker waits: that's fine in review
        self.agent(reviewer, "idle")
        self.assertEqual(self.check(), [f"{tid} is in review but its reviewer is idle without a verdict"])
        self.check("--fix", "--quiet")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Reviewer stopped without a verdict"))
        self.assertIn("stopped without a verdict", self.news()[-1])
        self.assertEqual(self.check(), [])
        self.agent(reviewer, "working")
        self.assertEqual(self.check(), [f"{tid} is marked triage (reviewer stopped without a verdict) but "
                                        "its reviewer is working again"])
        self.check("--fix")
        self.assertEqual(self.show(tid)["status"], "review")

    def test_check_counts_a_background_shell_as_busy(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Running the tests")
        self.agent(blk, "idle")
        (self.fake / f"screen_{blk}").write_text("❯ \n  ⏵⏵ auto mode on · 1 shell · ← 1 agent\n")
        self.assertEqual(self.check(), [])
        # and the watcher's end of turn is held back for it: it takes another turn
        self.bp("_event", blk, "idle", "", "Waiting for the tests")
        self.assertEqual(self.show(tid)["status"], "working")
        self.assertFalse([l for l in self.news() if "ended its turn" in l])
        (self.fake / f"screen_{blk}").unlink()  # the shell ended, and so did its next turn
        self.bp("_event", blk, "working", "", "Reading the results")
        self.bp("_event", blk, "idle", "", "Waiting for the tests")
        self.assertEqual(self.show(tid)["status"], "triage")

    def test_a_background_shell_left_running_is_drift_after_a_while(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Starting the server")
        (self.fake / f"screen_{blk}").write_text("❯ \n  ⏵⏵ auto mode on · 1 shell\n")
        self.agent(blk, "idle", ago=19 * 60)  # stuck_after is 20 min
        self.assertEqual(self.check(), [])
        self.agent(blk, "idle", ago=25 * 60)
        line = (f"{tid} is marked working but its agent has been idle at its prompt for 25 min, "
                "with a background shell still running: `bullpen peek` it")
        self.assertEqual(self.check(), [line])
        self.assertEqual(self.check("--fix"), [line + " (fixed)"])
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Ended its turn without saying ready"))
        self.assertIn("background shell of its own still runs", self.news()[-1])
        before = len(self.news())
        self.check("--fix")
        self.assertEqual((self.check(), len(self.news())), ([], before))

    def test_a_reviewers_background_shell_left_running_is_drift_after_a_while(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        (self.fake / f"screen_{reviewer}").write_text("❯ \n  ⏵⏵ auto mode on · 1 shell\n")
        self.agent(reviewer, "idle", ago=5 * 60)
        self.assertEqual(self.check(), [])
        self.agent(reviewer, "idle", ago=30 * 60)
        self.assertEqual(self.check(), [f"{tid} is in review but its reviewer is idle without a verdict"])

    def test_check_goes_by_the_screen_without_a_status_record(self):
        tid = self.start_task()
        blk = self.block(tid)
        (self.fake / f"screen_{blk}").write_text("✶ Misting… (28s)\n  ⏵⏵ auto mode on · esc to interrupt\n")
        self.assertEqual(self.check(), [])
        (self.fake / f"screen_{blk}").write_text("Done.\n\n❯ \n  ⏵⏵ auto mode on\n")
        self.assertEqual(self.check(), [f"{tid} is marked working but its agent is idle at its prompt: "
                                        "`bullpen peek` it"])
        (self.fake / f"screen_{blk}").write_text("something else\n")  # unsure: say nothing
        self.assertEqual(self.check(), [])

    def test_check_hints_at_finished_work_that_never_ran_ready(self):
        tid = self.start_task()
        blk = self.block(tid)
        wt = self.show(tid)["worktree"]
        (Path(wt) / "fix.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        self.bp("_event", blk, "idle", "", "")
        self.agent(blk, "idle")
        self.assertEqual(self.check(), [f"{tid} is idle with commits and a clean worktree but never ran "
                                        "ready: `bullpen peek` it, then `bullpen tell` it to run ready if "
                                        "it's done"])
        self.assertEqual(self.bp("check").returncode, 0)  # a hint, not drift
        (Path(wt) / "more.txt").write_text("x\n")  # still at it
        self.assertEqual(self.check(), [])

    def test_check_takes_ids_and_refuses_unknown_ones(self):
        tid = self.start_task()
        self.agent(self.block(tid), "idle")
        self.assertEqual(len(self.check(tid)), 1)
        self.assertNotEqual(self.bp("check", "BP-99", ok=False).returncode, 0)

    def test_check_all_repos_is_the_watchers(self):
        tid = self.start_task()
        self.agent(self.block(tid), "idle")
        self.settle()
        self.bp("check", "--fix", "--quiet", "--all-repos", cwd=self.tmp)
        self.assertEqual(self.show(tid)["status"], "triage")

    def test_a_quiet_worker_is_news_once_until_its_next_event(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "working", "", "Running the tests")
        self.bp("_stuck", worker, "20")
        self.bp("_stuck", worker, "21")  # the same quiet spell: no more news
        self.assertEqual(self.news(), ["bullpen · BP-1 has shown no progress for 20 min: `bullpen peek BP-1`"])
        t = self.show(tid)
        self.assertEqual((t["status"], t["stuck"]["who"]), ("working", "worker"))  # still working
        self.assertIn("no progress for 20 min", self.bp("ls").stdout)
        self.assertIn("no progress for 20 min", self.bp("show", tid).stdout)
        self.bp("_event", worker, "working", "", "Running the tests")  # a sign of life
        self.assertNotIn("stuck", self.show(tid))
        self.bp("_stuck", worker, "20")  # quiet again: a new spell
        self.assertEqual(len(self.news()), 2)

    def test_any_event_ends_a_quiet_spell(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "blocked", "permission", "Allow rm?")
        self.bp("_event", worker, "working", "", "")
        self.bp("_stuck", worker, "20")
        self.bp("_event", worker, "blocked", "permission", "Allow rm?")
        t = self.show(tid)
        self.assertEqual(t["status"], "needs")
        self.assertNotIn("stuck", t)

    def test_only_a_working_task_can_be_stuck(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "blocked", "permission", "Allow rm?")
        self.bp("_stuck", worker, "20")  # it waits on the person: that's known
        self.bp("_stuck", "blk1", "20")  # the coordinator
        self.bp("_stuck", "nope", "20")  # not bullpen's
        self.assertNotIn("stuck", self.show(tid))
        self.assertEqual(len(self.news()), 1)  # just the permission prompt

    # ----- the usage limit -----

    def limit_msg(self, minutes=60):
        """A limit message whose reset time is this many minutes from now."""
        import datetime as dt
        when = dt.datetime.now() + dt.timedelta(minutes=minutes)
        return f"You've hit your 5-hour limit · resets {when.hour % 12 or 12}:{when.minute:02d}{'am' if when.hour < 12 else 'pm'}"

    def limit_file(self):
        return self.tmp / "state" / "limit.json"

    def reset_passed(self, *tids):
        """Move the noted reset time (and each task's) ten minutes into the past."""
        import datetime as dt
        past = (dt.datetime.now().astimezone() - dt.timedelta(minutes=10)).isoformat(timespec="seconds")
        lim = json.loads(self.limit_file().read_text())
        self.limit_file().write_text(json.dumps({**lim, "resets": past}))
        for tid in tids:
            path = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
            t = json.loads(path.read_text())
            t["limit"]["resets"] = past
            path.write_text(json.dumps(t))

    def test_a_limit_message_marks_the_task_with_one_news_line(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "working", "", "")
        self.bp("_event", worker, "error", "", self.limit_msg())
        self.bp("_event", worker, "idle", "", "")  # what follows says nothing new
        t = self.show(tid)
        self.assertEqual(t["status"], "working")  # not "needs": nothing for the person to do
        self.assertEqual(t["limit"]["who"], "worker")
        self.assertRegex(t["why"], r"^Usage limit, resets (\w{3} )?\d{1,2}:\d\d [AP]M$")
        news = self.news()
        self.assertEqual(len(news), 1)
        self.assertRegex(news[0], r"^bullpen · BP-1 stopped at the usage limit, resets .*[AP]M: nothing to do, "
                                  r"it is continued then\. `bullpen start` refuses meanwhile \(`--force` overrides\)$")
        self.assertIn("Usage limit, resets", self.bp("ls").stdout)
        self.assertTrue(self.limit_file().exists())

    def test_a_limit_message_only_on_screen_is_found(self):
        tid = self.start_task()
        (self.fake / "screen").write_text(f"● Working on it\n  ⎿ {self.limit_msg()}\n\n❯ \n")
        self.bp("_event", self.block(tid), "error", "", "API error")
        t = self.show(tid)
        self.assertEqual((t["status"], t["limit"]["who"]), ("working", "worker"))
        self.assertIn("stopped at the usage limit", self.news()[-1])

    def test_a_finished_turn_is_not_a_limit_whatever_the_screen_says(self):
        tid = self.start_task()
        (self.fake / "screen").write_text(f"I added the text '{self.limit_msg()}' to the tests.\n")
        self.bp("_event", self.block(tid), "done", "", "")
        t = self.show(tid)
        self.assertEqual(t["status"], "triage")
        self.assertNotIn("limit", t)
        self.assertFalse(self.limit_file().exists())

    def test_start_is_refused_at_the_limit_unless_forced(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "error", "", self.limit_msg())
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        p = self.bp("start", tid2, ok=False)
        self.assertEqual(p.returncode, 1)
        self.assertRegex(p.stderr, r"usage limit is reached \(resets .*[AP]M\)")
        self.assertIn("--force", p.stderr)
        self.assertEqual(self.show(tid2)["status"], "todo")
        self.assertIsNone(self.show(tid2)["worktree"])
        self.bp("start", tid2, "--force")
        self.assertEqual(self.show(tid2)["status"], "working")

    def test_start_works_again_once_a_limited_worker_is_working(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "error", "", self.limit_msg())
        self.bp("_event", worker, "working", "", "")  # the person continued it, or Claude Code did
        t = self.show(tid)
        self.assertNotIn("limit", t)
        self.assertEqual((t["status"], t["why"]), ("working", None))
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        self.assertEqual(self.show(tid2)["status"], "working")

    def test_start_works_again_after_the_reset_time(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "error", "", self.limit_msg())
        self.reset_passed()
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        self.assertEqual(self.show(tid2)["status"], "working")

    def test_limited_tasks_are_continued_after_the_reset(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "error", "", self.limit_msg())
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", tid2, "--force")
        worker2 = self.block(tid2)
        self.bp("_event", worker2, "error", "", self.limit_msg())
        self.assertIn("BP-2 stopped at the usage limit", self.news()[-1])
        self.assertNotIn("bullpen start", self.news()[-1])  # said once, with the first
        (self.fake / "alive" / worker2).unlink()  # its pane goes meanwhile
        self.bp("_gone", worker2)
        self.assertEqual(self.show(tid2)["status"], "working")  # still waiting for the reset
        before = len(self.typed())
        self.bp("_limit")  # not yet
        self.assertEqual(len(self.typed()), before)
        self.reset_passed(tid, tid2)
        self.bp("_limit")
        self.assertIn((worker, "The usage limit has reset. Carry on where you stopped."), self.typed())
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertIn("--continue", cmd)  # BP-2, resumed in a new pane
        self.assertEqual(self.news()[-1], "bullpen · the usage limit has reset: continued BP-1, "
                                          "BP-2 (its pane was gone: resumed in a new one)")
        for t in (self.show(tid), self.show(tid2)):
            self.assertEqual((t["status"], t["why"]), ("working", None))
            self.assertNotIn("limit", t)
        self.assertFalse(self.limit_file().exists())
        n = len(self.typed())
        self.bp("_limit")  # nothing left to do
        self.assertEqual(len(self.typed()), n)

    def test_a_limited_worker_the_message_doesnt_reach_goes_to_its_coordinator(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "error", "", self.limit_msg())
        self.reset_passed(tid)
        (self.fake / f"send_fails_{worker}").write_text("")
        self.bp("_limit")
        t = self.show(tid)
        self.assertEqual((t["status"], t["stop"]), ("triage", {"by": "worker", "kind": "limit"}))
        self.assertEqual(t["why"], "Couldn't be continued after the usage limit: rex: couldn't send to the block")
        self.assertIn("BP-1 (not continued, for its coordinator to see to: rex: couldn't send", self.news()[-1])
        (self.fake / f"send_fails_{worker}").unlink()
        self.bp("tell", tid, "Carry on.")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_one_agent_working_again_leaves_the_others_to_be_continued(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "error", "", self.limit_msg())
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", tid2, "--force")
        worker2 = self.block(tid2)
        self.bp("_event", worker2, "error", "", self.limit_msg())
        self.bp("_event", worker, "working", "", "")  # before the reset: Claude Code's own continue, or a tell
        tid3 = self.bp("add", "A third", "--brief", "x").stdout.strip()
        self.bp("start", tid3)  # the limit no longer holds start back
        self.bp("_limit")  # BP-2's reset time hasn't come
        self.assertIn("limit", self.show(tid2))
        self.assertTrue(self.limit_file().exists())
        self.reset_passed(tid2)
        self.bp("_limit")
        self.assertIn((worker2, "The usage limit has reset. Carry on where you stopped."), self.typed())
        self.assertNotIn((worker, "The usage limit has reset. Carry on where you stopped."), self.typed())
        self.assertEqual(self.news()[-1], "bullpen · the usage limit has reset: continued BP-2")
        self.assertNotIn("limit", self.show(tid2))
        self.assertFalse(self.limit_file().exists())

    def test_a_blocked_agent_that_mentions_the_limit_needs_you(self):
        tid = self.start_task()
        worker = self.block(tid)
        (self.fake / "screen").write_text(f"  ⎿ {self.limit_msg()}\n")
        self.bp("_event", worker, "blocked", "permission", "Bash: grep -n 'usage limit reached' bin/bullpen")
        t = self.show(tid)
        self.assertEqual(t["status"], "needs")
        self.assertNotIn("limit", t)
        self.assertIn("BP-1 needs the person (permission)", self.news()[-1])
        self.assertFalse(self.limit_file().exists())
        reviewer_tid = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", reviewer_tid)
        reviewer = self.commit_and_ready(reviewer_tid)
        self.bp("_event", reviewer, "blocked", "permission", "Bash: grep 'usage limit reached' x")
        t = self.show(reviewer_tid)
        self.assertEqual((t["status"], t["why"][:18]), ("needs", "Reviewer needs you"))
        self.assertNotIn("limit", t)

    def test_an_unreadable_reset_time_is_not_continued(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "error", "", "Usage limit reached")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("working", "Usage limit, reset time unknown"))
        self.assertIsNone(t["limit"]["resets"])
        self.assertIn("its reset time couldn't be read, so it won't be continued by itself: "
                      "`bullpen tell BP-1 continue`", self.news()[-1])
        before = len(self.typed())
        self.bp("_limit")
        self.assertEqual(len(self.typed()), before)
        self.assertIn("limit", self.show(tid))
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.assertIn("reset time unknown", self.bp("start", tid2, ok=False).stderr)

    def test_a_limited_reviewer_is_continued_too(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp("_event", reviewer, "error", "", self.limit_msg())
        t = self.show(tid)
        self.assertEqual((t["status"], t["limit"]["who"]), ("review", "reviewer"))
        self.assertIn("BP-1's reviewer stopped at the usage limit", self.news()[-1])
        self.reset_passed(tid)
        self.bp("_limit")
        self.assertIn((reviewer, "The usage limit has reset. Carry on where you stopped."), self.typed())
        self.assertEqual(self.news()[-1], "bullpen · the usage limit has reset: continued BP-1's reviewer")
        self.assertEqual(self.show(tid)["status"], "review")

    def test_a_limit_stays_with_the_screen_that_showed_it(self):
        tid = self.start_task()
        worker = self.block(tid)
        (self.fake / "screen").write_text(f"  ⎿ {self.limit_msg()}\n")
        self.bp("_event", worker, "error", "", "")
        self.bp("_event", worker, "working", "", "")  # continued; the old line is still on screen
        self.bp("_event", worker, "idle", "", "")
        t = self.show(tid)
        self.assertEqual(t["status"], "triage")  # an ordinary end of turn
        self.assertNotIn("limit", t)

    def test_stuck_after_zero_turns_it_off(self):
        tid = self.start_task()
        self.assertEqual(json.loads(self.bp("config").stdout)["stuck_after"], 20)
        cfg = json.loads(self.bp("config", "--stuck-after", "0").stdout)
        self.assertEqual(cfg["stuck_after"], 0)
        self.bp("_stuck", self.block(tid), "20")
        self.assertNotIn("stuck", self.show(tid))
        self.assertEqual(self.news(), [])
        self.assertEqual(self.bp("config", "--stuck-after", "-1", ok=False).returncode, 1)

    def test_stuck_after_reaches_the_panes_the_watcher_reads(self):
        tid = self.start_task()
        entry = self.tmp / "state" / "blocks" / f"{self.block(tid)}.json"
        self.assertEqual(json.loads(entry.read_text())["stuck_after"], 20)
        self.bp("config", "--stuck-after", "45")
        self.assertEqual(json.loads(entry.read_text()),
                         {"repo": str(self.repo), "role": tid, "stuck_after": 45})
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        entry2 = self.tmp / "state" / "blocks" / f"{self.block(tid2)}.json"
        self.assertEqual(json.loads(entry2.read_text())["stuck_after"], 45)

    def test_a_quiet_reviewer_is_news_in_its_own_words(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp("_stuck", self.block(tid), "20")  # the worker is done: not its turn
        self.assertNotIn("stuck", self.show(tid))
        self.bp("_stuck", reviewer, "25")
        self.bp("_stuck", reviewer, "26")
        self.assertEqual(self.news(), ["bullpen · BP-1's reviewer has shown no progress for 25 min: "
                                       "`bullpen go BP-1 --review` shows it"])
        t = self.show(tid)
        self.assertEqual((t["status"], t["stuck"]["who"]), ("review", "reviewer"))
        self.bp("_event", reviewer, "working", "", "")
        self.assertNotIn("stuck", self.show(tid))
        self.bp("_stuck", reviewer, "20")
        self.verdict(tid, "approve")  # the status changes: the quiet spell is over
        self.assertNotIn("stuck", self.show(tid))

    def test_verdict_needs_a_review(self):
        tid = self.start_task()
        p = self.bp("verdict", tid, "approve", "--summary", "x", ok=False)
        self.assertIn("isn't in review", p.stderr)

    def test_review_off_goes_straight_to_ready(self):
        tid = self.start_task()
        self.bp("config", "--review", "off")
        wt = self.show(tid)["worktree"]
        (Path(wt) / "f.txt").write_text("x\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "x")
        self.bp("ready", tid, cwd=wt)
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertEqual(len(self.api("session.new_layer")), 1)  # no reviewer

    def test_go_review_brings_up_the_reviewer(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.bp("go", tid, "--review")
        self.assertEqual(self.layer_states(), {"lyr1": "minimized", "lyr2": "normal"})

    def test_note_shows_until_the_status_changes(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "done", "", "")
        self.bp("note", tid, "Waits", "on the CI image.")
        self.assertEqual(self.show(tid)["note"], "Waits on the CI image.")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.bp("_event", self.block(tid), "working", "", "")
        self.assertIsNone(self.show(tid)["note"])

    def windows(self):
        """{window: [labels]} from the fake's session view."""
        R = json.loads((self.fake / "reg.json").read_text())
        out = {w: [] for w in R["windows"]}
        for b, info in R["blocks"].items():
            if (self.fake / "alive" / b).exists() and info["window"] in out:
                out[info["window"]].append(info["label"])
        return {w: sorted(labels) for w, labels in out.items()}

    def test_a_restarted_coordinator_takes_the_workers_with_it(self):
        tid = self.start_task()
        self.commit_and_ready(tid)  # a reviewer capsule too
        both = sorted(["bullpen: proj", "bullpen: proj board", "bullpen: BP-1", "bullpen: BP-1 review"])
        self.assertEqual(self.windows(), {"w1": both})
        for b in ("blk1", "blk2"):  # the coordinator and board are closed
            (self.fake / "alive" / b).unlink()
        self.bp("up")  # a new coordinator, in a new tab
        self.assertEqual(self.windows(), {"w2": both})  # and the emptied tab is closed
        self.assertEqual(sorted(self.layer_states().items())[-2:],
                         [("lyr3", "minimized"), ("lyr4", "minimized")])

    def test_news_waits_while_the_person_is_typing_to_the_coordinator(self):
        tid = self.start_task()
        (self.fake / "screen").write_text("● Ready.\n────\n❯ drop BP-\n────\n  auto mode on\n")
        self.bp("_event", self.block(tid), "done", "", "")
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))  # not typed into the draft
        self.assertTrue((self.tmp / "state" / "outbox").exists())
        self.bp("_flush")  # still typing: still waiting
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))
        (self.fake / "screen").write_text("● Ready.\n────\n❯ \n────\n  auto mode on\n")
        self.bp("_flush")
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual(len(news), 1)
        self.assertIn("BP-1 ended its turn", news[0])
        self.assertFalse((self.tmp / "state" / "outbox").exists())

    def test_waiting_news_goes_out_in_order(self):
        tid = self.start_task()
        (self.fake / "screen").write_text("❯ hmm\n")
        self.bp("_event", self.block(tid), "blocked", "permission", "first")
        self.bp("_event", self.block(tid), "blocked", "permission", "second")
        (self.fake / "screen").write_text("❯ \n")
        self.bp("_event", self.block(tid), "blocked", "permission", "third")  # delivers all three
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual([n.rsplit(" ", 1)[-1] for n in news], ["first", "second", "third"])

    # How Claude Code draws its input box (`rex capture --format=vt`): a suggestion is
    # faint (SGR 2) after the ❯, text the person typed is not.
    RULE = "\x1b[0m\x1b[38;5;7m────────\r\n"
    SUGGESTION = RULE + "❯\xa0\x1b[0m\x1b[2mdrop BP-18 and BP-20\x1b[0m\r\n" + RULE
    TYPED = RULE + "❯\xa0\x1b[0mdrop BP-\x1b[7m \x1b[0m\r\n" + RULE

    def test_a_prompt_suggestion_doesnt_hold_typed_news(self):
        tid = self.start_task()
        (self.fake / "screen").write_text(self.SUGGESTION + "  \x1b[2m\x1b[38;5;7m⏵⏵ auto mode on\x1b[0m\r\n")
        self.bp("_event", self.block(tid), "done", "", "")
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual(len(news), 1)
        self.assertIn("BP-1 ended its turn", news[0])
        self.assertFalse((self.tmp / "state" / "outbox").exists())
        self.assertIn("--format=vt", [a for a in self.rex_calls() if a[0] == "capture"][-1])

    def test_typed_text_still_holds_typed_news(self):
        tid = self.start_task()
        (self.fake / "screen").write_text(self.TYPED)
        self.ask(tid, "one")
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))
        self.assertTrue((self.tmp / "state" / "outbox").exists())
        # typed text followed by a faint rest of the suggestion is still a draft
        (self.fake / "screen").write_text(self.RULE + "❯\xa0\x1b[0mdrop\x1b[2m BP-18\x1b[0m\r\n")
        self.bp("_flush")
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))
        # faint from the line before, then reset: the ❯ line is the person's
        (self.fake / "screen").write_text("\x1b[2mhint\r\n\x1b[0m❯ half\r\n")
        self.bp("_flush")
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))
        (self.fake / "screen").write_text("\x1b[2mhint\r\n❯ still faint\x1b[22m\r\n")
        self.bp("_flush")
        self.assertEqual(len([l for b, l in self.typed() if b == "blk1"]), 1)

    # ---------- news by monitor ----------

    def ask(self, tid, msg):
        """One news line: the worker asks something."""
        self.bp("_event", self.block(tid), "blocked", "question", msg)

    def follow(self, block="blk1"):
        """Start `bullpen news --follow` in a pane (the coordinator's); returns it and a
        function reading its next n lines."""
        p = subprocess.Popen([str(BULLPEN), "news", "--follow", "--every", "0.05"], cwd=self.repo,
                             env={**self.env, "REX_BLOCK": block}, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(lambda: (p.kill(), p.wait(), p.stdout.close(), p.stderr.close()))
        buf = b""

        def lines(n, wait=10):
            nonlocal buf
            end = time.time() + wait
            while buf.count(b"\n") < n:
                ready, _, _ = select.select([p.stdout], [], [], max(0, end - time.time()))
                if not ready:
                    self.fail(f"follow printed {buf!r}, wanted {n} lines")
                buf += os.read(p.stdout.fileno(), 4096)
            out, buf = buf.split(b"\n")[:n], b"\n".join(buf.split(b"\n")[n:])
            return [l.decode() for l in out]

        def quiet(wait=0.4):
            ready, _, _ = select.select([p.stdout], [], [], wait)
            return not buf and not ready

        return p, lines, quiet

    def stop(self, p):
        p.terminate()
        p.wait()

    def test_follow_prints_each_new_line_as_it_is_logged(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        p, lines, quiet = self.follow()
        self.assertTrue(quiet())  # nothing logged yet (and no news.log): nothing printed
        self.ask(tid, "first")
        self.assertEqual(lines(1), ["bullpen · BP-1 needs the person (question): first"])
        self.ask(tid, "second")
        self.ask(tid, "third")
        self.assertEqual([l.rsplit(" ", 1)[-1] for l in lines(2)], ["second", "third"])
        self.assertIsNone(p.poll())  # still running

    def test_a_restarted_follower_prints_what_it_missed_once(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        p, lines, _ = self.follow()
        self.ask(tid, "one")
        self.assertEqual(len(lines(1)), 1)
        self.stop(p)
        self.ask(tid, "two")  # logged while no follower runs
        self.ask(tid, "three")
        p, lines, quiet = self.follow()
        self.assertEqual([l.rsplit(" ", 1)[-1] for l in lines(2)], ["two", "three"])
        self.ask(tid, "four")
        self.assertEqual([l.rsplit(" ", 1)[-1] for l in lines(1)], ["four"])
        self.stop(p)
        p, lines, quiet = self.follow()
        self.assertTrue(quiet())  # nothing new: nothing again

    def test_a_second_follower_waits_and_takes_over(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        first, lines1, _ = self.follow()
        self.ask(tid, "one")
        self.assertEqual(len(lines1(1)), 1)
        second, lines2, quiet2 = self.follow()
        self.ask(tid, "two")
        self.assertEqual([l.rsplit(" ", 1)[-1] for l in lines1(1)], ["two"])
        self.assertTrue(quiet2())  # not printed twice
        p = self.bp_in("blk1", "news", ok=False)  # a one-off read doesn't wait
        self.assertIn("followed already", p.stderr)
        self.stop(first)
        self.ask(tid, "three")
        self.assertEqual([l.rsplit(" ", 1)[-1] for l in lines2(1)], ["three"])

    def test_news_prints_the_lines_since_last_time_and_exits(self):
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")  # no news.log yet
        tid = self.start_task()
        self.ask(tid, "one")
        self.ask(tid, "two")
        out = self.bp_in("blk1", "news").stdout.splitlines()
        self.assertEqual(out, ["bullpen · BP-1 needs the person (question): one",
                               "bullpen · BP-1 needs the person (question): two"])
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")
        self.ask(tid, "three")
        self.assertEqual(self.bp_in("blk1", "news").stdout, "bullpen · BP-1 needs the person (question): three\n")

    def test_news_never_replays_what_was_logged_before_it_first_ran(self):
        tid = self.start_task()
        self.ask(tid, "old")  # typed, in the default mode
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")
        self.ask(tid, "older still")
        self.bp_in("blk1", "news")
        self.ask(tid, "typed too")
        self.bp("config", "--news", "monitor")  # switching starts from here
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")
        self.ask(tid, "new")
        self.assertEqual(self.bp_in("blk1", "news").stdout, "bullpen · BP-1 needs the person (question): new\n")

    def test_monitor_mode_types_nothing_and_keeps_no_outbox(self):
        tid = self.start_task()
        p = self.bp("config", "--news", "monitor")
        self.assertEqual(json.loads(p.stdout)["news"], "monitor")
        self.assertIn("news --follow", p.stderr)
        self.ask(tid, "Which one?")
        self.bp("_event", self.block(tid), "working", "", "")
        self.bp("_event", self.block(tid), "done", "", "")
        self.bp("_flush")
        typed = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual(len(typed), 1)  # only the line that said news moved to the monitor
        self.assertIn("news now comes by monitor", typed[0])
        state = self.tmp / "state"
        self.assertFalse((state / "outbox").exists())
        self.assertEqual(list(state.rglob("outbox*.txt")), [])
        log = next(state.rglob("news.log")).read_text().splitlines()
        self.assertEqual(len(log), 2)
        self.assertIn("BP-1 needs-you: bullpen · BP-1 needs the person (question): Which one?", log[0])

    def test_type_is_the_default_and_can_be_set_back(self):
        tid = self.start_task()
        self.assertEqual(json.loads(self.bp("config").stdout)["news"], "type")
        self.bp("config", "--news", "monitor")
        self.bp("config", "--news", "type")
        self.ask(tid, "Which one?")
        self.assertEqual([l for b, l in self.typed() if b == "blk1"][1:],
                         ["bullpen · BP-1 needs the person (question): Which one?"])

    def test_switching_to_type_ends_a_running_follower_with_one_line(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        p, lines, quiet = self.follow()
        self.ask(tid, "one")
        self.assertEqual(len(lines(1)), 1)
        self.bp("config", "--news", "type")
        self.assertTrue(lines(1)[0].startswith("bullpen · news is typed into your pane now"))
        self.assertEqual(p.wait(timeout=10), 0)
        self.ask(tid, "two")  # typed, and not printed by a follower started again
        self.assertIn("two", self.typed()[-1][1])
        p, lines, quiet = self.follow()
        self.assertEqual(len(lines(1)), 1)  # the same one line, then it's gone
        self.assertEqual(p.wait(timeout=10), 0)
        self.assertEqual(p.stdout.read(), b"")
        self.bp("config", "--news", "monitor")  # switching again starts after "two": it was typed
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")

    def test_switching_to_monitor_tells_a_running_coordinator_by_one_last_typed_line(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        told = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual(len(told), 1)
        self.assertTrue(told[0].startswith("bullpen · news now comes by monitor"))
        self.assertIn(f"{BULLPEN} news --follow", told[0])
        self.bp("config", "--news", "monitor")  # no switch: not said again
        self.ask(tid, "one")
        self.assertEqual(len([l for b, l in self.typed() if b == "blk1"]), 1)

    def test_the_switch_line_waits_for_an_empty_input_like_other_typed_news(self):
        self.start_task()
        (self.fake / "screen").write_text("❯ half a message\n")
        self.bp("config", "--news", "monitor")
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))
        (self.fake / "screen").write_text("❯ \n")
        self.bp("_flush")
        self.assertEqual(len([l for b, l in self.typed() if b == "blk1"]), 1)

    def test_switching_to_monitor_types_nothing_from_the_coordinators_pane_or_without_one(self):
        self.bp("config", "--news", "monitor")  # no coordinator yet
        self.bp("config", "--news", "type")
        self.bp("up", "--here")  # this pane ("me") is the coordinator
        p = self.bp("config", "--news", "monitor")
        self.assertIn("news --follow", p.stderr)
        self.assertEqual(self.typed(), [])
        self.assertEqual(list((self.tmp / "state").rglob("outbox*.txt")), [])
        self.assertIn("News comes by monitor: start it first", self.bp("up", "--here").stdout)

    def test_up_says_how_news_arrives(self):
        out = self.bp("up").stdout.splitlines()
        self.assertTrue(out[-1].startswith("news: typed into the coordinator's pane"))
        self.bp("config", "--news", "monitor")
        for args in (["up"], ["up", "--here"]):
            last = self.bp(*args).stdout.splitlines()[-1]
            self.assertTrue(last.startswith("news: by monitor"))
            self.assertIn(f"{BULLPEN} news --follow", last)

    def test_the_guide_names_the_follow_command_in_full(self):
        guide = self.bp("guide").stdout
        self.assertIn(f"{BULLPEN} news --follow`", guide)
        self.assertNotIn("{{", guide)
        self.bp("config", "--news", "monitor")
        self.bp("up")
        run = next(a for a in self.rex_calls() if a[0] == "run")
        prompt = Path(run[run.index("--append-system-prompt-file") + 1]).read_text()
        self.assertEqual(prompt.strip(), guide.strip())
        self.assertIn("News comes by monitor", run[-1])

    def test_hide_puts_a_worker_away_and_goes_to_the_board(self):
        tid = self.start_task()
        self.bp("go", tid)
        self.assertEqual(self.layer_states(), {"lyr1": "normal"})
        self.bp("hide", "--from", self.block(tid))
        self.assertEqual(self.layer_states(), {"lyr1": "minimized"})
        self.assertEqual(self.focused()[-1], "blk2")  # the board

    def test_hide_lets_the_key_through_elsewhere(self):
        self.start_task()
        for pane in ("blk1", "blk2", "someone-elses"):  # coordinator, board, not ours
            p = self.bp("hide", "--from", pane, ok=False)
            self.assertEqual(p.returncode, 3)

    def test_board_pane_events_are_ignored(self):
        self.start_task()
        before = len(self.typed())
        self.bp("_event", "blk2", "done", "", "")
        self.bp("_gone", "blk2")
        self.assertEqual(len(self.typed()), before)

    def focused(self):
        return [a[-1] for a in self.rex_calls() if a[0] == "focus"]

    def test_go_focuses_coordinator_and_board(self):
        self.start_task()
        self.bp("go")
        self.bp("go", "--board")
        self.assertEqual(self.focused(), ["blk1", "blk2"])

    def test_workers_start_as_minimized_floating_panes(self):
        tid = self.start_task()
        self.assertEqual([a[0] for a in self.rex_calls()].count("run"), 1)  # only the coordinator
        layer = self.api("session.new_layer")[0]
        self.assertEqual(layer["window_id"], "w1")  # the coordinator's window
        self.assertEqual(self.layer_states(), {"lyr1": "minimized"})

    def test_go_brings_one_worker_up_at_a_time(self):
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        self.bp("go", "BP-1")
        self.assertEqual(self.layer_states(), {"lyr1": "normal", "lyr2": "minimized"})
        self.assertTrue(self.api("session.set_layer_state")[-1].get("focus"))
        self.bp("go", "BP-2")
        self.assertEqual(self.layer_states(), {"lyr1": "minimized", "lyr2": "normal"})
        self.bp("go")  # back to the coordinator: workers go back to capsules
        self.assertEqual(self.layer_states(), {"lyr1": "minimized", "lyr2": "minimized"})
        self.assertEqual(self.focused(), ["blk1"])

    def test_workers_tabs_setting(self):
        self.bp("up")
        self.bp("config", "--workers", "tabs")
        self.bp("add", "T", "--brief", "x")
        self.bp("start", "BP-1")
        run = [a for a in self.rex_calls() if a[0] == "run"][-1]
        self.assertIn("bullpen: BP-1", run)
        self.assertEqual(self.api("session.new_layer"), [])

    def test_up_finds_its_panes_again_after_a_rex_restart(self):
        a = self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        (self.fake / "alive" / self.block("BP-2")).unlink()  # closed while Rex was down
        self.restart_rex()
        runs = [x for x in self.rex_calls() if x[0] == "run"]
        self.bp("up")
        self.assertEqual([x for x in self.rex_calls() if x[0] == "run"], runs)  # no second coordinator
        cfg = json.loads(self.bp("config").stdout)
        self.assertEqual(cfg["coordinators"][0]["block"], "blk6")
        self.assertEqual(cfg["coordinators"][0]["board"]["block"], "blk7")
        self.assertEqual(self.block(a), "blk8")
        self.assertEqual(self.show("BP-2")["status"], "stopped")
        self.assertEqual(len(self.api("session.new_block")), 2)  # the watcher, again
        self.bp("_event", "blk8", "blocked", "permission", "ok?")  # events reach the task
        self.assertEqual(self.show(a)["status"], "needs")

    def test_up_after_a_restart_that_renamed_the_session(self):
        tid = self.start_task()
        self.restart_rex(new_session=True)
        self.bp("up")
        self.assertEqual([x[0] for x in self.rex_calls()].count("run"), 1)  # no second coordinator
        cfg = json.loads(self.bp("config").stdout)
        c = cfg["coordinators"][0]
        self.assertEqual((c["block"], c["session"]), ("blk5", "session:2"))
        self.assertEqual(self.show(tid)["block"], "blk7")
        self.assertEqual(self.show(tid)["session"], "session:2")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_go_after_a_restart_that_renamed_the_session(self):
        tid = self.start_task()
        self.restart_rex(new_session=True)
        env = {k: v for k, v in self.env.items() if k != "REX_SESSION"}  # as a key binding runs it
        p = subprocess.run([str(BULLPEN), "go", tid, "--session", "session:2", "--from", "x"],
                           cwd=self.tmp, env=env, text=True, capture_output=True)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(self.block(tid), "blk7")

    def test_go_finds_a_pane_again_after_a_rex_restart(self):
        tid = self.start_task()
        self.restart_rex()
        self.bp("go", tid)
        self.assertEqual(self.block(tid), "blk7")

    def test_news_is_one_line(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "blocked", "question", "Which one?\nA or B")
        line = self.typed()[-1][1]
        self.assertEqual(line, "bullpen · BP-1 needs the person (question): Which one? A or B")

    def test_go_refuses_a_task_without_a_pane(self):
        self.bp("up")
        tid = self.bp("add", "Later", "--brief", "x").stdout.strip()
        p = self.bp("go", tid, ok=False)
        self.assertIn("no pane", p.stderr)

    def test_go_next_cycles_through_tasks_needing_you(self):
        a = self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        b = "BP-2"
        self.bp("start", b)
        self.start_task("Third")  # working: never a target
        p = self.bp("go", "--next", "--from", "elsewhere")  # nothing needs you: the board
        self.assertIn("nothing needs you", p.stderr)
        self.assertEqual(self.focused()[-1], "blk2")
        self.bp("_event", self.block(b), "blocked", "permission", "Allow?")
        self.bp("_event", self.block(a), "blocked", "question", "Which?")
        nxt = lambda frm: json.loads(self.bp("go", "--next", "--json", "--from", frm).stdout)["block"]
        first = nxt("elsewhere")
        self.assertEqual(first, self.block(b))  # waiting longest comes first
        self.assertEqual(nxt(first), self.block(a))
        self.assertEqual(nxt(self.block(a)), self.block(b))  # wraps around

    def change_things(self, tid):
        """A committed new file, an uncommitted edit and an untracked file."""
        wt = self.show(tid)["worktree"]
        (Path(wt) / "fix.txt").write_text("one\ntwo\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        (Path(wt) / "README").write_text("changed\n")
        (Path(wt) / "scratch.txt").write_text("x\n")
        return wt

    def test_diff_stat(self):
        tid = self.start_task()
        self.assertEqual(self.bp("diff", tid, "--stat").stdout.strip(), "BP-1 has no commits against origin/main.")
        wt = self.change_things(tid)
        sh("git", "-C", str(self.repo), "commit", "-q", "--allow-empty", "-m", "Later on main")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")
        sh("git", "-C", wt, "fetch", "-q", "origin")  # the base moved on: only the task's changes show
        out = self.bp("diff", tid, "--stat").stdout
        committed, loose = out.split("\n\n")
        self.assertIn("fix.txt | 2 ++", committed)
        self.assertIn("1 file changed, 2 insertions(+)", committed)
        self.assertNotIn("README", committed)
        self.assertTrue(loose.startswith("Uncommitted"))
        self.assertIn("README | 2 +-", loose)
        self.assertIn("untracked: scratch.txt", loose)
        self.assertEqual(self.rex_calls()[-1][0], "api")  # no pane, no pager: nothing new in Rex

    def test_diff_prints_the_changes_when_not_on_a_terminal(self):
        tid = self.start_task()
        self.change_things(tid)
        out = self.bp("diff", tid).stdout
        self.assertIn("+++ b/fix.txt", out)
        self.assertIn("Uncommitted (not in any commit yet):", out)
        self.assertIn("+changed", out)
        self.assertIn("untracked: scratch.txt", out)
        self.assertNotIn("\x1b[", out)  # no colour codes into a file or pipe

    def test_diff_of_a_task_without_a_worktree(self):
        self.bp("up")
        tid = self.bp("add", "Later", "--brief", "x").stdout.strip()
        for args in ((), ("--stat",), ("--pane",)):
            p = self.bp("diff", tid, *args, ok=False)
            self.assertEqual(p.returncode, 1)
            self.assertIn("BP-1 has no worktree (it is todo)", p.stderr)
        self.assertEqual(self.api("session.new_layer"), [])

    def test_diff_pane_opens_over_the_board_and_goes_back_after(self):
        tid = self.start_task()  # its worker is the capsule lyr1
        wt = self.show(tid)["worktree"]
        self.bp("go", tid)  # the worker is up: the diff puts it away
        self.bp("diff", tid, "--pane")
        layer = self.api("session.new_layer")[-1]
        block = layer["layout"]["block"]
        self.assertEqual(block["label"], "bullpen: BP-1 diff")
        self.assertEqual(block["options"]["cwd"], wt)
        self.assertEqual(block["options"]["command"][-3:], ["diff", "BP-1", "--back"])
        self.assertEqual(layer["window_id"], "w1")
        self.assertEqual(self.layer_states(), {"lyr1": "minimized", "lyr2": "normal"})
        self.assertTrue(self.api("session.set_layer_state")[-1].get("focus"))
        self.bp("diff", tid, "--pane")  # again: the old diff pane closes first
        self.assertFalse((self.fake / "alive" / "blk5").exists())
        self.bp("diff", tid, "--back")  # what the pane runs: the board after the pager
        self.assertEqual(self.focused()[-1], "blk2")

    def test_ready_records_the_size_of_the_change(self):
        tid = self.start_task()
        self.change_things(tid)
        wt = self.show(tid)["worktree"]
        (Path(wt) / "README").write_text("hi\n")
        (Path(wt) / "scratch.txt").unlink()
        self.bp("ready", tid, cwd=wt)
        self.assertEqual(self.show(tid)["diffstat"], "1 file, +2 −0")
        self.assertIn("diffstat: 1 file, +2 −0", self.bp("show", tid).stdout)

    def test_board_runs_the_rust_board_on_this_repos_state(self):
        fake = self.fake / "board"
        fake.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\"\n")
        fake.chmod(0o755)
        self.env["BULLPEN_BOARD"] = str(fake)
        args = self.bp("board").stdout.splitlines()
        cfg = json.loads(self.bp("config").stdout)
        self.assertEqual(args[:4], ["--dir", cfg["state"], "--root", str(self.repo)])
        self.assertEqual(args[4], "--bullpen")

    def test_board_says_when_it_isnt_built(self):
        self.env["BULLPEN_BOARD"] = str(self.tmp / "missing")
        p = self.bp("board", ok=False)
        self.assertIn("isn't built", p.stderr)

    def test_board_layout_is_a_setting(self):
        self.bp("config", "--board-layout", "compact")
        self.assertEqual(json.loads(self.bp("config").stdout)["board_layout"], "compact")

    def test_board_bottom_gap_is_a_setting(self):
        self.assertEqual(json.loads(self.bp("config").stdout)["board_bottom_gap"], 1)
        self.bp("config", "--board-bottom-gap", "0")
        self.assertEqual(json.loads(self.bp("config").stdout)["board_bottom_gap"], 0)
        p = self.bp("config", "--board-bottom-gap", "-1", ok=False)
        self.assertIn("takes rows", p.stderr)

    def test_peek_vt_keeps_colours_and_drops_blank_coloured_lines(self):
        tid = self.start_task()
        (self.fake / "screen").write_text("\x1b[0m\x1b[31mred\x1b[0m\r\n\x1b[0mplain\n\x1b[0m\r\n\n")
        out = self.bp("peek", tid, "--vt").stdout
        self.assertEqual(out, "\x1b[0m\x1b[31mred\x1b[0m\n\x1b[0mplain\n")
        self.assertIn("--format=vt", [a for a in self.rex_calls() if a[0] == "capture"][-1])

    def test_go_finds_the_repo_from_any_directory(self):
        tid = self.start_task()
        out = self.bp("go", tid, "--json", "--from", "nowhere", cwd=self.tmp).stdout
        self.assertEqual(json.loads(out)["block"], self.block(tid))

    # ---- following the PR after ready ----

    def github(self, **pr):
        """What `gh pr view` says about the PR from now on."""
        (self.fake / "pr.json").write_text(json.dumps({"state": "OPEN", "headRefOid": "abc",
            "statusCheckRollup": [], "reviews": [], "comments": [], **pr}))

    def gh_views(self):
        log = self.fake / "gh.log"
        calls = [json.loads(l)["argv"] for l in log.read_text().splitlines()] if log.exists() else []
        return [a for a in calls if a[:2] == ["pr", "view"]]

    def sent_back_by(self, tid):
        """Who the board says sent a working task back (board/src/model.rs reads
        the same fields): "github", "review", or None."""
        t = self.show(tid)
        if (t.get("gh") or {}).get("sent"):
            return "github"
        return "review" if (t.get("review") or {}).get("verdict") == "changes" else None

    def news_lines(self):
        return [l for b, l in self.typed() if b == "blk1"]

    FAILING = [{"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "FAILURE"},
               {"__typename": "CheckRun", "name": "lint", "status": "COMPLETED", "conclusion": "SUCCESS"},
               {"__typename": "StatusContext", "context": "ci/build", "state": "ERROR"}]

    def test_a_failing_check_goes_back_to_the_worker_once(self):
        tid = self.start_task()
        worker = self.block(tid)
        reviewer = self.commit_and_ready(tid)
        self.assertTrue((self.tmp / "state" / "prs").exists())
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · BP-1's PR checks failed: test, ci/build. Sent back to its worker.")
        line = [l for b, l in self.typed() if b == worker][-1]
        self.assertIn("test, ci/build", line)
        self.assertIn("ready command again", line)
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        self.assertFalse((self.fake / "alive" / reviewer).exists())  # its review is moot
        self.assertEqual(t["review"]["round"], 0)  # and doesn't use up a round
        self.assertIn("checks failing", self.bp("show", tid).stdout)
        self.assertEqual((self.sent_back_by(tid), self.show(tid)["gh"]["checks"]), ("github", "failing"))
        # the worker runs ready again without fixing them: same commit, same failure
        self.bp("ready", tid, "--summary", "Did it.", cwd=t["worktree"])
        self.assertEqual(self.show(tid)["review"]["round"], 1)
        self.assertNotIn("sent", self.show(tid)["gh"])
        before = len(self.typed())
        self.bp("_prs")
        self.bp("_prs")
        self.assertEqual(len(self.typed()), before)
        self.assertEqual(self.show(tid)["status"], "review")
        # a new push that fails again is news again
        self.github(statusCheckRollup=self.FAILING, headRefOid="def")
        self.bp("_prs")
        self.assertIn("checks failed: test, ci/build", self.news_lines()[-1])

    def test_review_changes_after_the_watcher_looked_say_review(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.github(statusCheckRollup=[
            {"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "SUCCESS"}])
        self.bp("_prs")
        self.assertEqual(self.show(tid)["gh"]["checks"], "passing")
        self.verdict(tid, "changes", "The fix misses the empty case.")
        self.assertEqual(self.sent_back_by(tid), "review")
        self.assertNotIn("github:", self.bp("show", tid).stdout)

    def test_review_changes_after_a_github_send_back_say_review(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        self.assertEqual((self.sent_back_by(tid), self.show(tid)["gh"]["checks"]), ("github", "failing"))
        self.commit_and_ready(tid, 2)
        self.verdict(tid, "changes", "The fix misses the empty case.")
        self.assertEqual(self.sent_back_by(tid), "review")

    # ---- auto-accept ----

    RUNNING = [{"__typename": "CheckRun", "name": "test", "status": "IN_PROGRESS", "conclusion": ""}]
    PASSING = [{"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "SUCCESS"}]

    def approved_with_auto_accept(self, *flags):
        """A task started with auto-accept on (and flags on its start), approved by review."""
        self.bp("up")
        self.bp("config", "--auto-accept", "on")
        tid = self.bp("add", "Fix the thing", "--brief", "Make it work.").stdout.strip()
        self.bp("start", tid, *flags)
        wt = self.show(tid)["worktree"]
        self.commit_and_ready(tid)
        self.bp("verdict", tid, "approve", "--summary", "Looks right.", cwd=wt)  # from the reviewer's worktree
        return tid, wt

    def merges(self):
        return [a for a in (json.loads(l)["argv"] for l in (self.fake / "gh.log").read_text().splitlines())
                if a[:2] == ["pr", "merge"]]

    def test_auto_accept_merges_an_approved_pr(self):
        self.assertFalse(json.loads(self.bp("config").stdout)["auto_accept"])  # off by default
        tid, wt = self.approved_with_auto_accept()
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertFalse(Path(wt).exists())
        self.assertEqual(len(self.merges()), 1)
        self.assertIn("bullpen · BP-1 approved by review and auto-accepted (merged PR #7, rebase)",
                      self.news_lines())

    def test_auto_accept_notifies_with_notify_all(self):
        read = self.terminal("blk2")  # the board: the worker's pane closes on accept
        self.bp("config", "--notify", "all")
        tid, _ = self.approved_with_auto_accept()
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(read(), "\x1b]777;notify;bullpen · proj;BP-1 is ready: Fix the thing\x07"
                                 "\x1b]777;notify;bullpen · proj;BP-1 was auto-accepted: Fix the thing\x07")

    def test_auto_accept_waits_for_running_checks_then_merges(self):
        self.github(statusCheckRollup=self.RUNNING)
        tid, _ = self.approved_with_auto_accept()
        t = self.show(tid)
        self.assertEqual((t["status"], t["auto_wait"]), ("ready", True))
        self.assertIn("auto-accepts once its PR's checks pass", self.news_lines()[-1])
        self.bp("_prs")  # still running
        self.assertEqual(self.show(tid)["status"], "ready")
        self.github(statusCheckRollup=self.PASSING)
        self.bp("_prs")
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(len(self.merges()), 1)
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · BP-1 approved by review and auto-accepted (merged PR #7, rebase)")

    def test_auto_accept_waits_for_running_checks_that_then_fail(self):
        self.github(statusCheckRollup=self.RUNNING)
        tid, _ = self.approved_with_auto_accept()
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        t = self.show(tid)
        self.assertNotEqual(t["status"], "done")
        self.assertNotIn("auto_wait", t)
        self.assertIn("bullpen · BP-1 not auto-accepted: its PR's checks failed", self.news_lines())
        self.assertFalse(any(a[:2] == ["pr", "merge"] for a in self.merges()))

    def test_a_waiting_auto_accept_ends_when_the_task_leaves_ready(self):
        self.github(statusCheckRollup=self.RUNNING)
        tid, wt = self.approved_with_auto_accept()
        self.bp("tell", tid, "One more thing.")
        self.assertNotIn("auto_wait", self.show(tid))
        self.bp("config", "--review", "off")
        self.commit_and_ready(tid, 2)  # ready with no approval
        self.github(statusCheckRollup=self.PASSING)
        self.bp("_prs")
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertEqual(self.merges(), [])

    def test_auto_accept_skips_failing_checks(self):
        self.github(statusCheckRollup=self.FAILING)
        tid, _ = self.approved_with_auto_accept()
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertIn("not auto-accepted: its PR's checks are failing", self.news_lines()[-1])
        self.assertEqual(self.merges(), [])

    def test_auto_accept_refused_by_github_leaves_it_ready(self):
        gh = self.fake / "gh"
        gh.write_text(gh.read_text() + textwrap.dedent("""\
            if sys.argv[1:3] == ["pr", "merge"]:
                print("Pull request is not mergeable: the merge commit cannot be cleanly created", file=sys.stderr)
                sys.exit(1)
        """))
        tid, wt = self.approved_with_auto_accept()
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertTrue(Path(wt).exists())
        self.assertIn("auto-accept was refused: merge refused: Pull request is not mergeable",
                      self.news_lines()[-1])
        self.assertIn("Send it back to rebase", self.news_lines()[-1])

    def test_no_auto_accept_keeps_a_task_for_the_person(self):
        tid, _ = self.approved_with_auto_accept("--no-auto-accept")
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · BP-1 is ready (1 commit, PR #7), approved by light review (1 line in 1 file): Fix the thing")
        self.assertIn("kept for you", self.bp("show", tid).stdout)

    def test_auto_accept_on_a_task_wins_over_the_config(self):
        self.bp("up")
        tid = self.bp("add", "Fix the thing", "--brief", "x", "--auto-accept").stdout.strip()
        self.bp("start", tid)
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.assertEqual(self.show(tid)["status"], "done")

    def test_auto_accept_off_leaves_an_approved_task_ready(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertFalse((self.fake / "gh.log").exists() and self.merges())

    def test_a_new_comment_stops_a_waiting_auto_accept(self):
        self.github(statusCheckRollup=self.RUNNING)
        tid, _ = self.approved_with_auto_accept()
        self.github(statusCheckRollup=self.PASSING, comments=[
            {"id": "c1", "author": {"login": "pat"}, "body": "Hold on, rename it."}])
        self.bp("_prs")
        self.assertNotEqual(self.show(tid)["status"], "done")
        self.assertIn("bullpen · BP-1 not auto-accepted: its PR has new comments", self.news_lines())
        self.assertEqual(self.merges(), [])

    def test_a_new_comment_is_forwarded_once(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.github(comments=[
            {"id": "c1", "author": {"login": "ann"}, "body": "Please also\ncover the logout path."},
            {"id": "c2", "author": {"login": "codecov[bot]"}, "body": "Coverage 90%"}],
            reviews=[{"id": "r1", "author": {"login": "bo"}, "state": "APPROVED", "body": "LGTM"}])
        self.bp("_prs")
        self.assertEqual(self.news_lines()[-1], "bullpen · BP-1's PR has 1 new comment: "
                         "ann Please also cover the logout path. Sent back to its worker.")
        line = [l for b, l in self.typed() if b == worker][-1]
        self.assertIn("Please also cover the logout path.", line)
        self.assertNotIn("\n", line)
        self.assertNotIn("Coverage", line)
        self.assertNotIn("LGTM", line)
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        self.assertIn("1 new comment", self.bp("show", tid).stdout)
        self.commit_and_ready(tid, 2)
        before = len(self.typed())
        self.bp("_prs")
        self.assertEqual(len(self.typed()), before)  # already seen
        self.github(comments=[{"id": "c1", "author": {"login": "ann"}, "body": "Please also cover it."}],
                    reviews=[{"id": "r2", "author": {"login": "bo"}, "state": "CHANGES_REQUESTED", "body": ""}])
        self.bp("_prs")
        self.assertIn("bo requested changes", self.news_lines()[-1])

    def test_a_pr_merged_outside_bullpen_is_news_once(self):
        tid = self.start_task()
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        self.github(state="MERGED")
        self.bp("_prs")
        self.bp("_prs")
        merged = [l for l in self.news_lines() if "merged outside bullpen" in l]
        self.assertEqual(merged, ["bullpen · BP-1's PR #7 was merged outside bullpen: "
                                  "`bullpen accept BP-1` cleans up"])
        self.assertEqual(len(self.gh_views()), 1)  # a merged PR isn't looked at again
        self.assertFalse((self.tmp / "state" / "prs").exists())
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")  # nothing changes until the person says
        self.assertTrue(Path(t["worktree"]).exists())
        self.bp("accept", tid)
        gh = [json.loads(l)["argv"] for l in (self.fake / "gh.log").read_text().splitlines()]
        self.assertFalse(any(a[:2] == ["pr", "merge"] for a in gh))
        self.assertEqual(self.show(tid)["status"], "done")

    def test_a_pr_closed_outside_bullpen_points_at_drop(self):
        tid = self.start_task()
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        self.github(state="CLOSED")
        self.bp("_prs")
        self.assertEqual(self.news_lines()[-1], "bullpen · BP-1's PR #7 was closed outside bullpen: "
                                                "`bullpen drop BP-1` cleans up")

    def test_feedback_for_a_worker_that_is_gone_goes_to_the_coordinator(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        (self.fake / "alive" / worker).unlink()
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        self.assertFalse(any(b == worker for b, _ in self.typed()))
        t = self.show(tid)
        self.assertEqual(t["status"], "triage")
        self.assertEqual(t["why"], "PR checks failed: test, ci/build")
        self.assertIn("`bullpen resume BP-1` restarts it", self.news_lines()[-1])

    def test_feedback_that_doesnt_reach_the_worker_goes_to_the_coordinator(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        (self.fake / f"send_fails_{worker}").write_text("")
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "PR checks failed: test, ci/build"))
        self.assertNotIn("sent", t.get("gh") or {})
        line = self.news_lines()[-1]
        self.assertIn("The message didn't reach its worker (rex: couldn't send to the block)", line)
        self.assertNotIn("rex send", line)  # the reason, not the command
        self.assertIn("`bullpen tell BP-1` it", line)
        (self.fake / f"send_fails_{worker}").unlink()
        self.bp("tell", tid, "Fix the checks.")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_feedback_isnt_typed_into_a_prompt_the_person_was_asked_about(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        self.bp("_event", worker, "blocked", "permission", "Allow rm?")
        self.bp("ask", tid, "Should it delete the cache?")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"], t["stop"]),
                         ("needs", "Should it delete the cache?", {"by": "worker", "kind": "prompt"}))
        typed = len([1 for b, _ in self.typed() if b == worker])
        self.github(comments=[{"id": "c1", "author": {"login": "ann"}, "body": "Cover logout too."}])
        self.bp("_prs")
        self.assertEqual(len([1 for b, _ in self.typed() if b == worker]), typed)  # still at its prompt
        self.assertIn("Not sent to its worker", self.news_lines()[-1])
        self.assertEqual(self.show(tid)["why"], "Should it delete the cache?")

    def test_feedback_during_review_that_doesnt_reach_the_worker_goes_to_the_coordinator(self):
        tid = self.start_task()
        worker = self.block(tid)
        reviewer = self.commit_and_ready(tid)
        (self.fake / f"send_fails_{worker}").write_text("")
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "PR checks failed: test, ci/build"))
        self.assertFalse((self.fake / "alive" / reviewer).exists())  # moot once the work changes
        self.assertIn("The message didn't reach its worker", self.news_lines()[-1])
        (self.fake / f"send_fails_{worker}").unlink()
        self.bp("tell", tid, "Fix the checks.")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_feedback_is_not_typed_into_a_worker_blocked_on_a_prompt(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.commit_and_ready(tid)
        self.verdict(tid, "changes", "Fix the empty case.")  # back to the worker
        self.bp("_event", worker, "blocked", "permission", "Allow rm?")
        typed_to_worker = len([1 for b, _ in self.typed() if b == worker])
        self.github(comments=[{"id": "c1", "author": {"login": "ann"}, "body": "Cover logout too."}])
        self.bp("_prs")
        self.assertEqual(len([1 for b, _ in self.typed() if b == worker]), typed_to_worker)
        t = self.show(tid)
        self.assertEqual(t["status"], "needs")
        self.assertIn("Allow rm?", t["why"])
        self.assertIn("Not sent to its worker", self.news_lines()[-1])
        self.bp("_prs")  # recorded: not news again
        self.assertIn("Not sent to its worker", self.news_lines()[-1])
        self.assertEqual(len([l for l in self.news_lines() if "new comment" in l]), 1)

    def test_a_task_accepted_while_gh_answers_stays_accepted(self):
        tid = self.start_task()
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        self.github(statusCheckRollup=self.FAILING)
        # gh is slow: the person accepts the task while it answers
        slow = (self.fake / "gh").read_text().replace(
            'elif sys.argv[1:3] == ["pr", "view"]:',
            'elif sys.argv[1:3] == ["pr", "view"]:\n'
            '    import subprocess; subprocess.run(json.loads((d / "meanwhile").read_text()), check=True)')
        (self.fake / "gh").write_text(slow)
        accepted = json.loads(self.bp("show", tid, "--json").stdout)
        accepted.update(status="done", block=None, worktree=None)
        path = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
        tmp = self.tmp / "accepted.json"
        tmp.write_text(json.dumps(accepted))
        (self.fake / "meanwhile").write_text(json.dumps(["cp", str(tmp), str(path)]))
        self.bp("_prs")
        t = self.show(tid)
        self.assertEqual(t["status"], "done")
        self.assertFalse(any("checks failed" in l for l in self.news_lines()))

    def test_nothing_to_follow_calls_no_gh(self):
        tid = self.start_task()
        self.bp("_prs")  # working, no PR yet
        self.assertEqual(self.gh_views(), [])
        self.assertFalse((self.tmp / "state" / "prs").exists())

    def commit_and_ready_no_review(self, tid):
        wt = self.show(tid)["worktree"]
        (Path(wt) / "fix.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        self.bp("ready", tid, "--summary", "Did it.", cwd=wt)

    # ---------- rebase after accept ----------

    def two_tasks(self):
        """BP-1 and BP-2 started, review off, each with a commit; BP-1 ready."""
        self.bp("config", "--review", "off")
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        for tid, name in (("BP-1", "one.txt"), ("BP-2", "two.txt")):
            wt = self.show(tid)["worktree"]
            (Path(wt) / name).write_text(f"{tid}\n")
            sh("git", "-C", wt, "add", ".")
            sh("git", "-C", wt, "commit", "-qm", f"{tid} work")
        self.bp("ready", "BP-1", "--summary", "done")

    def merge_on_github(self, tid):
        """What `gh pr merge` would do: the task's branch lands on origin's main."""
        sh("git", "-C", str(self.repo), "push", "-q", "origin", f"{self.show(tid)['branch']}:main")

    def test_after_accept_a_ready_task_is_rebased_and_pushed(self):
        self.two_tasks()
        self.bp("ready", "BP-2", "--summary", "done")
        self.merge_on_github("BP-1")
        self.bp("accept", "BP-1")
        t = self.show("BP-2")
        self.assertEqual(t["status"], "ready")
        main = sh("git", "-C", str(self.repo), "rev-parse", "origin/main").strip()
        pushed = sh("git", "-C", str(self.tmp / "origin.git"), "rev-parse", t["branch"]).strip()
        self.assertEqual(sh("git", "-C", t["worktree"], "rev-parse", "HEAD").strip(), pushed)
        sh("git", "-C", t["worktree"], "merge-base", "--is-ancestor", main, pushed)
        self.assertEqual(t["rebased"], "main")  # the board says "Rebased onto main."
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 rebased.")

    def test_after_accept_a_working_task_is_told_not_rebased(self):
        self.two_tasks()
        wt = self.show("BP-2")["worktree"]
        head = sh("git", "-C", wt, "rev-parse", "HEAD")
        self.merge_on_github("BP-1")
        self.bp("accept", "BP-1")
        self.assertEqual(sh("git", "-C", wt, "rev-parse", "HEAD"), head)
        block, line = [x for x in self.typed() if x[0] == self.block("BP-2")][-1]
        self.assertIn("git fetch origin", line)
        self.assertIn("rebase your branch onto origin/main", line)
        self.assertEqual(self.show("BP-2")["status"], "working")
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 told to rebase.")

    # ---------- dependencies and the run queue ----------

    def set_config(self, **kv):
        path = next((self.tmp / "state" / "repos").iterdir()) / "config.json"
        path.write_text(json.dumps({**json.loads(path.read_text()), **kv}))

    def test_add_after_takes_repeats_and_commas_and_refuses_unknown_ids(self):
        for title in ("A", "B", "C"):
            self.bp("add", title, "--brief", "x")
        self.bp("add", "D", "--brief", "x", "--after", "BP-1", "--after", "2,bp-3")
        self.assertEqual(self.show("BP-4")["after"], ["BP-1", "BP-2", "BP-3"])
        p = self.bp("add", "E", "--brief", "x", "--after", "BP-9", ok=False)
        self.assertIn("no task BP-9", p.stderr)
        self.assertEqual(self.bp("add", "E", "--brief", "x").stdout.strip(), "BP-5")  # no ID was used up
        self.assertNotIn("after", self.show("BP-5"))

    def test_after_sets_changes_and_clears_and_refuses_cycles(self):
        for title in ("A", "B", "C"):
            self.bp("add", title, "--brief", "x")
        self.bp("after", "BP-2", "BP-1")
        self.bp("after", "BP-3", "BP-2")
        self.assertEqual(self.show("BP-3")["after"], ["BP-2"])
        p = self.bp("after", "BP-1", "BP-3", ok=False)
        self.assertIn("BP-3 already waits on BP-1 (BP-3 → BP-2 → BP-1): that would be a cycle", p.stderr)
        self.assertIn("can't wait on itself", self.bp("after", "BP-1", "BP-1", ok=False).stderr)
        self.assertIn("no task BP-7", self.bp("after", "BP-1", "BP-7", ok=False).stderr)
        self.assertNotEqual(self.bp("after", "BP-1", ok=False).returncode, 0)  # neither tasks nor --none
        self.bp("after", "BP-3", "BP-1,BP-2")
        self.assertEqual(self.show("BP-3")["after"], ["BP-1", "BP-2"])
        self.bp("after", "BP-3", "--none")
        self.assertNotIn("after", self.show("BP-3"))

    def test_start_refuses_a_task_whose_dependencies_arent_done_unless_forced(self):
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x", "--after", "BP-1")
        p = self.bp("start", "BP-2", ok=False)
        self.assertIn("BP-2 waits on BP-1 (working)", p.stderr)
        self.assertIn("--force", p.stderr)
        self.assertEqual(self.show("BP-2")["status"], "todo")
        self.bp("start", "BP-2", "--force")
        self.assertEqual(self.show("BP-2")["status"], "working")
        self.assertIn("### BP-1: First (working, not accepted yet)", self.full_brief("BP-2"))

    def test_ls_and_show_say_what_a_todo_task_waits_on(self):
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x", "--after", "BP-1")
        self.assertIn("(Waits on BP-1)", self.bp("ls").stdout)
        self.assertIn("waits: Waits on BP-1", self.bp("show", "BP-2").stdout)
        self.bp("drop", "BP-1", "--force")
        self.assertIn("(Waits on BP-1 (dropped))", self.bp("ls").stdout)

    def test_accept_starts_queued_tasks_in_id_order_within_max_running(self):
        tid = self.ready_with_a_commit()
        self.bp("add", "Plain", "--brief", "x")  # BP-2: no dependencies, waits for the coordinator
        for title in ("Next", "Then", "Later"):
            self.bp("add", title, "--brief", "x", "--after", tid)
        self.bp("add", "Other", "--brief", "x")
        self.bp("start", "BP-6")
        self.set_config(max_running=3)
        self.merge_on_github(tid)
        self.bp("accept", tid)
        status = {t: self.show(t)["status"] for t in ("BP-2", "BP-3", "BP-4", "BP-5")}
        self.assertEqual(status, {"BP-2": "todo", "BP-3": "working", "BP-4": "working", "BP-5": "todo"})
        news = self.news_lines()
        self.assertIn("bullpen · BP-3 started (BP-1 was accepted)", news)
        self.assertIn("bullpen · BP-4 started (BP-1 was accepted)", news)
        self.assertFalse(any("BP-5 started" in l or "BP-2 started" in l for l in news))
        # BP-6 is dropped: its slot goes to BP-5, which was queued; BP-2 still waits
        self.bp("drop", "BP-6", "--force")
        self.assertEqual(self.show("BP-5")["status"], "working")
        self.assertEqual(self.show("BP-2")["status"], "todo")
        self.assertIn("bullpen · BP-5 started (BP-6 was dropped, freeing a slot)", self.news_lines())

    def test_a_queued_task_starts_from_the_coordinators_pane_too(self):
        tid = self.ready_with_a_commit()
        self.bp("add", "Next", "--brief", "x", "--after", tid)
        self.env["REX_BLOCK"] = "blk1"
        self.merge_on_github(tid)
        self.bp("accept", tid)
        self.assertEqual(self.news_lines()[-1], "bullpen · BP-2 started (BP-1 was accepted)")

    def test_a_task_waits_for_all_its_dependencies(self):
        tid = self.ready_with_a_commit()
        self.bp("add", "Other", "--brief", "x")
        self.bp("add", "Both", "--brief", "x", "--after", f"{tid},BP-2")
        self.merge_on_github(tid)
        self.bp("accept", tid)
        self.assertEqual(self.show("BP-3")["status"], "todo")

    def test_a_dropped_dependency_starts_nothing_and_asks_once(self):
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x", "--after", "BP-1")
        self.bp("add", "Third", "--brief", "x", "--after", "BP-1")
        self.bp("drop", "BP-1", "--force")
        self.assertEqual((self.show("BP-2")["status"], self.show("BP-3")["status"]), ("todo", "todo"))
        asks = [l for l in self.news_lines() if "which was dropped" in l]
        self.assertEqual(len(asks), 1)
        self.assertIn("BP-2, BP-3 wait on BP-1, which was dropped, so they stay queued", asks[0])
        self.assertIn("bullpen after BP-2 --none", asks[0])
        self.assertIn("was dropped", self.bp("after", "BP-2", "BP-1", ok=False).stderr)

    def test_a_started_dependent_hears_what_landed(self):
        tid = self.ready_with_a_commit()
        briefs = next((self.tmp / "state" / "repos").iterdir()) / "briefs"
        (briefs / f"{tid}.review-1.findings.md").write_text("Looks fine.\n")
        self.bp("add", "Next", "--brief", "Build on it.", "--after", tid)
        self.merge_on_github(tid)
        self.bp("accept", tid)
        brief = self.full_brief("BP-2")
        self.assertIn("## What this builds on", brief)
        self.assertIn("### BP-1: Fix the thing (accepted, PR #7)", brief)
        self.assertIn("done", brief.split("### BP-1")[1])  # its summary
        self.assertIn(f"Review findings: {briefs / 'BP-1.review-1.findings.md'}", brief)
        self.assertLess(brief.index("Build on it."), brief.index("## What this builds on"))
        self.assertNotIn("## What this builds on", self.full_brief("BP-1"))  # no dependencies, no section

    def test_queued_tasks_wait_at_the_usage_limit(self):
        tid = self.ready_with_a_commit()
        self.bp("add", "Next", "--brief", "x", "--after", tid)
        (self.tmp / "state" / "limit.json").write_text(json.dumps(
            {"at": "2026-01-01T00:00:00+00:00", "resets": "2999-01-01T00:00:00+00:00", "text": "x"}))
        self.merge_on_github(tid)
        self.bp("accept", tid)
        self.assertEqual(self.show("BP-2")["status"], "todo")
        self.assertIn("BP-2 can start (BP-1 was accepted), but the usage limit is reached", self.news_lines()[-1])

    # ---------- rules ----------

    def commit_rules(self, text):
        (self.repo / ".bullpen").mkdir(exist_ok=True)
        (self.repo / ".bullpen" / "rules.md").write_text(text)
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "Rules")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")

    def personal_rules(self, text):
        d = next((self.tmp / "state" / "repos").iterdir())
        (d / "rules.md").write_text(text)

    def full_brief(self, tid="BP-1"):
        return next((self.tmp / "state").rglob(f"{tid}.full.md")).read_text()

    def test_rules_from_the_repo_go_in_the_brief(self):
        self.commit_rules("- Keep functions small.\n")
        self.start_task()
        brief = self.full_brief()
        self.assertIn("## Rules for this repository\n\n- Keep functions small.\n", brief)
        self.assertLess(brief.index("Make it work."), brief.index("Keep functions small."))
        self.assertLess(brief.index("Keep functions small."), brief.index("## How you work"))

    def test_personal_rules_go_in_the_brief(self):
        self.bp("up")
        self.personal_rules("- Say it plainly.\n")
        self.bp("add", "Fix", "--brief", "Make it work.")
        self.bp("start", "BP-1")
        self.assertIn("## Rules for this repository\n\n- Say it plainly.\n", self.full_brief())

    def test_both_rules_files_repo_first(self):
        self.commit_rules("- Repo rule.\n")
        self.bp("up")
        self.personal_rules("- My rule.\n")
        self.bp("add", "Fix", "--brief", "Make it work.")
        self.bp("start", "BP-1")
        brief = self.full_brief()
        self.assertEqual(brief.count("## Rules for this repository"), 1)
        self.assertLess(brief.index("Repo rule."), brief.index("My rule."))
        out = self.bp("rules").stdout
        self.assertIn("From .bullpen/rules.md on origin/main", out)
        self.assertIn("rules.md\n\n- My rule.", out)
        self.assertLess(out.index("Repo rule."), out.index("My rule."))

    def test_no_rules_no_heading(self):
        self.start_task()
        self.assertNotIn("Rules for this repository", self.full_brief())
        self.commit_and_ready("BP-1")
        prompt = next((self.tmp / "state").rglob("BP-1.review-1.md")).read_text()
        self.assertNotIn("Rules for this repository", prompt)
        self.assertIn("No rules.", self.bp("rules").stdout)

    def test_uncommitted_rules_in_the_main_checkout_are_not_used(self):
        self.commit_rules("- Committed rule.\n")
        (self.repo / ".bullpen" / "rules.md").write_text("- Half-written rule.\n")
        self.start_task()
        brief = self.full_brief()
        self.assertIn("Committed rule.", brief)
        self.assertNotIn("Half-written", brief)
        self.assertNotIn("Half-written", self.bp("rules").stdout)

    def test_reviewer_gets_the_workers_rules(self):
        self.commit_rules("- Repo rule.\n")
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / ".bullpen" / "rules.md").write_text("- Changed by the worker.\n")
        self.commit_and_ready(tid)
        prompt = next((self.tmp / "state").rglob("BP-1.review-1.md")).read_text()
        self.assertIn("## Rules for this repository", prompt)
        self.assertIn("Check that the work follows them.", prompt)
        self.assertIn("- Repo rule.", prompt)
        self.assertNotIn("Changed by the worker", prompt)

    def test_a_restarted_task_keeps_the_base_rules(self):
        self.commit_rules("- Repo rule.\n")
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / ".bullpen" / "rules.md").write_text("- Changed by the worker.\n")
        self.bp("_gone", self.block(tid))
        self.assertEqual(self.show(tid)["status"], "stopped")
        self.bp("start", tid)
        brief = self.full_brief()
        self.assertIn("- Repo rule.", brief)
        self.assertNotIn("Changed by the worker", brief)
        saved = next((self.tmp / "state").rglob("BP-1.rules.md")).read_text()
        self.assertEqual(saved, "- Repo rule.\n")

    def test_rules_edit_opens_the_personal_file(self):
        self.bp("up")
        editor = self.tmp / "ed"
        editor.write_text("#!/bin/sh\necho '- Edited.' > \"$1\"\n")
        editor.chmod(0o755)
        self.env = {**self.env, "EDITOR": str(editor), "VISUAL": ""}
        self.bp("rules", "--edit")
        out = self.bp("rules").stdout
        self.assertIn("- Edited.", out)
        self.assertIn(str(self.tmp / "state" / "repos"), out)


    # ---------- the agents' model ----------

    def last_command(self):
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        return cmd[cmd.index("--permission-mode"):]  # past env and the resolved program

    def set_commands(self, **cmds):
        """Edit the repo's config.json by hand, as a person would."""
        self.bp("config")
        path = next((self.tmp / "state").rglob("config.json"))
        cfg = json.loads(path.read_text())
        cfg.update(cmds)
        path.write_text(json.dumps(cfg))

    def models(self, cmd):
        return [cmd[i + 1] for i, a in enumerate(cmd) if a == "--model"]

    def test_no_model_set_leaves_the_command_alone(self):
        tid = self.start_task()
        self.assertNotIn("--model", self.last_command())
        self.assertIsNone(self.show(tid).get("model"))
        cfg = json.loads(self.bp("config").stdout)
        self.assertEqual(cfg["models"], {"worker": "default (Claude Code picks)",
                                         "reviewer": "default (Claude Code picks)"})

    def test_config_model_sets_both_and_the_per_role_flags_one_each(self):
        cfg = json.loads(self.bp("config", "--model", "opus").stdout)
        self.assertEqual((cfg["worker_model"], cfg["reviewer_model"]), ("opus", "opus"))
        self.assertEqual(cfg["models"], {"worker": "opus", "reviewer": "opus"})
        cfg = json.loads(self.bp("config", "--reviewer-model", "sonnet").stdout)
        self.assertEqual((cfg["worker_model"], cfg["reviewer_model"]), ("opus", "sonnet"))
        cfg = json.loads(self.bp("config", "--worker-model", "claude-opus-4-1").stdout)
        self.assertEqual((cfg["worker_model"], cfg["reviewer_model"]), ("claude-opus-4-1", "sonnet"))
        cfg = json.loads(self.bp("config", "--model", "default").stdout)
        self.assertEqual((cfg["worker_model"], cfg["reviewer_model"]), ("default", "default"))

    def test_model_is_passed_at_start_resume_and_review(self):
        self.bp("config", "--worker-model", "opus", "--reviewer-model", "sonnet")
        tid = self.start_task()
        self.assertEqual(self.models(self.last_command()), ["opus"])
        self.assertEqual(self.show(tid)["model"], "opus")
        self.assertIn("    model: opus", self.bp("show", tid).stdout)
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)
        self.bp("resume", tid)
        cmd = self.last_command()
        self.assertIn("--continue", cmd)
        self.assertEqual(self.models(cmd), ["opus"])
        self.commit_and_ready(tid)
        self.assertEqual(self.models(self.last_command()), ["sonnet"])

    def test_config_model_replaces_one_in_the_command(self):
        self.set_commands(worker=["claude", "--permission-mode", "auto", "--model", "opus"],
                          reviewer=["claude", "--model=haiku", "--permission-mode", "auto"])
        cfg = json.loads(self.bp("config").stdout)
        self.assertEqual(cfg["models"], {"worker": "opus", "reviewer": "haiku"})
        self.bp("config", "--model", "sonnet")
        tid = self.start_task()
        self.assertEqual(self.models(self.last_command()), ["sonnet"])
        self.assertEqual(self.show(tid)["model"], "sonnet")
        self.commit_and_ready(tid)
        cmd = self.last_command()
        self.assertEqual(self.models(cmd), ["sonnet"])
        self.assertFalse(any(a.startswith("--model=") for a in cmd))

    def test_model_default_drops_the_commands_own(self):
        self.set_commands(worker=["claude", "--permission-mode", "auto", "--model", "opus"],
                          reviewer=["claude", "--permission-mode", "auto", "--model=opus"])
        cfg = json.loads(self.bp("config", "--model", "default").stdout)
        self.assertEqual(cfg["models"], {"worker": "default (Claude Code picks)",
                                         "reviewer": "default (Claude Code picks)"})
        tid = self.start_task()
        self.assertEqual(self.models(self.last_command()), [])
        self.assertIsNone(self.show(tid).get("model"))
        self.commit_and_ready(tid)
        cmd = self.last_command()
        self.assertEqual(self.models(cmd), [])
        self.assertFalse(any(a.startswith("--model") for a in cmd))

    def test_start_model_is_for_that_task_and_stays_on_resume(self):
        self.bp("config", "--model", "opus")
        self.bp("up")
        tid = self.bp("add", "Fix it", "--brief", "x").stdout.strip()
        self.bp("start", tid, "--model", "haiku")
        self.assertEqual(self.models(self.last_command()), ["haiku"])
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)
        self.bp("resume", tid)
        self.assertEqual(self.models(self.last_command()), ["haiku"])
        tid2 = self.bp("add", "Other", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        self.assertEqual(self.models(self.last_command()), ["opus"])

    def test_add_model_carries_to_a_start_by_itself(self):
        self.bp("config", "--worker-model", "opus")
        tid = self.ready_with_a_commit()
        self.assertEqual(self.models(self.api("session.new_layer")[0]["layout"]["block"]["options"]["command"]), ["opus"])
        nxt = self.bp("add", "Next", "--brief", "x", "--after", tid, "--model", "haiku").stdout.strip()
        self.assertEqual(self.show(nxt)["worker_model"], "haiku")
        self.assertIn("[model haiku]", self.bp("ls").stdout)
        self.assertIn("    model: haiku (its own, over the config's opus)", self.bp("show", nxt).stdout)
        self.merge_on_github(tid)
        self.bp("accept", tid)
        self.assertEqual(self.show(nxt)["status"], "working")
        self.assertEqual(self.models(self.last_command()), ["haiku"])
        self.assertEqual(self.show(nxt)["model"], "haiku")

    def test_model_sets_and_clears_a_tasks_own(self):
        self.bp("config", "--worker-model", "opus")
        self.bp("up")
        tid = self.bp("add", "Fix it", "--brief", "x").stdout.strip()
        self.assertIn(f"{tid} uses sonnet", self.bp("model", tid, "sonnet").stdout)
        self.bp("start", tid)
        self.assertEqual(self.models(self.last_command()), ["sonnet"])
        out = self.bp("model", tid, "--clear").stdout
        self.assertIn(f"{tid} uses the config's worker model (opus)", out)
        self.assertIn("takes effect at its next start or resume", out)
        self.assertNotIn("worker_model", self.show(tid))
        self.assertNotIn("[model", self.bp("ls").stdout)
        self.assertEqual(len(self.api("session.new_layer")), 1)  # nothing restarted
        self.bp("model", tid, "haiku")
        self.assertIn("running sonnet until its next start or resume", self.bp("show", tid).stdout)
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)
        self.bp("resume", tid)
        self.assertEqual(self.models(self.last_command()), ["haiku"])
        self.assertNotEqual(self.bp("model", tid, ok=False).returncode, 0)

    def test_start_model_overrides_the_tasks_own(self):
        self.bp("up")
        tid = self.bp("add", "Fix it", "--brief", "x", "--model", "haiku").stdout.strip()
        self.bp("start", tid, "--model", "sonnet")
        self.assertEqual(self.models(self.last_command()), ["sonnet"])
        self.assertEqual(self.show(tid)["worker_model"], "sonnet")
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)
        self.bp("resume", tid)
        self.assertEqual(self.models(self.last_command()), ["sonnet"])

    def test_other_agents_commands_are_left_alone(self):
        self.set_commands(worker=["codex", "--permission-mode", "auto"])
        cfg = json.loads(self.bp("config", "--model", "opus").stdout)
        self.assertIn("not claude", cfg["models"]["worker"])
        self.assertEqual(cfg["models"]["reviewer"], "opus")
        tid = self.start_task()
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertNotIn("--model", cmd)
        self.assertIsNone(self.show(tid).get("model"))


    # ---------- several coordinators ----------

    def new_pane(self, label="claude"):
        """An agent's pane the person opened in Rex, in a tab of its own."""
        reg = self.fake / "reg.json"
        R = json.loads(reg.read_text()) if reg.exists() else {"blocks": {}, "layers": {}, "windows": []}
        b, w = f"pane{len(R['blocks']) + 1}", f"w{len(R['windows']) + 1}"
        R["windows"].append(w)
        R["blocks"][b] = {"label": label, "layer": None, "window": w}
        reg.write_text(json.dumps(R))
        (self.fake / "alive").mkdir(exist_ok=True)
        (self.fake / "alive" / b).write_text("")
        return b

    def bp_in(self, block, *args, ok=True):
        """bullpen run from another pane."""
        env, self.env = self.env, {**self.env, "REX_BLOCK": block}
        try:
            return self.bp(*args, ok=ok)
        finally:
            self.env = env

    def two_coordinators(self):
        """c1 started by `up` (blk1, board blk2), c2 an agent's pane made one with
        `up --here`. Returns c2's block."""
        self.bp("up")
        other = self.new_pane()
        self.bp_in(other, "up", "--here")
        return other

    def coordinators(self):
        return json.loads(self.bp("config").stdout)["coordinators"]

    def test_a_second_up_here_keeps_the_first_coordinator(self):
        other = self.two_coordinators()
        cs = self.coordinators()
        self.assertEqual([(c["id"], c["block"]) for c in cs], [("c1", "blk1"), ("c2", other)])
        splits = [a for a in self.rex_calls() if a[0] == "split"]
        self.assertEqual([a[a.index("-b") + 1] for a in splits], ["blk1", other])  # a board each
        self.assertEqual([a[-1] for a in splits], ["c1", "c2"])
        self.assertTrue(cs[0]["board"]["block"] and cs[1]["board"]["block"])
        out = self.bp_in(other, "up", "--here").stdout  # again: nothing changes
        self.assertIn("coordinator c2: this pane", out)
        self.assertEqual(len(self.coordinators()), 2)
        self.assertEqual(len([a for a in self.rex_calls() if a[0] == "split"]), 2)
        out = self.bp("up").stdout  # from elsewhere: as before, nothing new
        self.assertIn("coordinator already running: c1 (blk1), c2", out)
        self.assertEqual([a[0] for a in self.rex_calls()].count("run"), 1)

    def test_board_others_is_a_setting_per_board(self):
        other = self.two_coordinators()
        self.assertNotIn("board_others", self.coordinators()[0])  # off by default
        self.bp("config", "--board-others", "on", "--coordinator", "c1")  # as the board runs it
        self.assertEqual([c.get("board_others") for c in self.coordinators()], [True, None])
        self.bp_in(other, "config", "--board-others", "on")  # in c2's pane: c2's
        self.bp("config", "--board-others", "off", "--coordinator", "c1")
        self.assertEqual([c.get("board_others") for c in self.coordinators()], [False, True])
        board = self.coordinators()[1]["board"]["block"]
        self.bp_in(board, "config", "--board-others", "off")  # on c2's board: c2's
        self.assertEqual(self.coordinators()[1]["board_others"], False)
        p = self.bp("config", "--board-others", "on", ok=False)  # whose?
        self.assertIn("per board", p.stderr)
        p = self.bp("config", "--board-others", "on", "--coordinator", "c7", ok=False)
        self.assertIn("no coordinator c7", p.stderr)

    def test_go_next_skips_tasks_the_board_hides(self):
        other = self.two_coordinators()
        mine = self.bp("add", "Mine", "--brief", "x").stdout.strip()  # c1's
        theirs = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        for tid in (theirs, mine):
            self.bp("start", tid)
            self.bp("_event", self.block(tid), "blocked", "question", "Which?")
        nxt = lambda frm: json.loads(self.bp("go", "--next", "--json", "--from", frm).stdout)["block"]
        c1_board = self.coordinators()[0]["board"]["block"]
        self.assertEqual(nxt(c1_board), self.block(mine))
        self.assertEqual(nxt(self.block(mine)), self.block(mine))  # c2's isn't on c1's board
        self.assertEqual(nxt(other), self.block(theirs))  # nor c1's on c2's
        self.assertEqual(nxt("elsewhere"), self.block(theirs))  # not a coordinator's pane: every task
        self.bp("config", "--board-others", "on", "--coordinator", "c1")
        self.assertEqual(nxt(self.block(mine)), self.block(theirs))
        # c2's board is unchanged
        self.assertEqual(nxt(other), self.block(theirs))
        self.assertEqual(nxt(self.block(theirs)), self.block(theirs))

    def test_a_task_belongs_to_the_coordinator_that_added_it(self):
        other = self.two_coordinators()
        mine = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        primary = self.bp("add", "Elsewhere", "--brief", "x").stdout.strip()  # not a coordinator's pane
        self.assertEqual(self.show(mine)["coordinator"], "c2")
        self.assertEqual(self.show(primary)["coordinator"], "c1")
        self.assertIn("coord: c2", self.bp("show", mine).stdout)

    def test_news_goes_to_the_tasks_own_coordinator(self):
        other = self.two_coordinators()
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        b = self.bp("add", "Primary's", "--brief", "x").stdout.strip()
        self.bp("add", "Waits", "--brief", "x", "--after", a)
        self.bp("start", a)
        self.bp("start", b)
        self.ask(a, "for c2?")
        self.ask(b, "for c1?")
        self.assertEqual(self.typed(), [(other, "bullpen · BP-1 needs the person (question): for c2?"),
                                        ("blk1", "bullpen · BP-2 needs the person (question): for c1?")])
        self.bp("drop", a, "--force")
        typed = self.typed()[2:]
        self.assertEqual(typed[0], (other, "bullpen · BP-1 dropped"))  # its own coordinator alone
        # news about more than one task goes to every coordinator
        waits = [(blk, l) for blk, l in typed if "waits on BP-1, which was dropped" in l]
        self.assertEqual(sorted(blk for blk, _ in waits), sorted(["blk1", other]))

    def test_news_from_a_coordinators_pane_goes_to_the_owner_only(self):
        other = self.two_coordinators()
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", a)
        self.bp_in(other, "drop", a, "--force")  # c2 did it: no news to itself
        self.assertEqual(self.typed(), [])
        b = self.bp_in(other, "add", "Theirs too", "--brief", "x").stdout.strip()
        self.bp("start", b)
        self.bp_in("blk1", "drop", b, "--force")  # c1 dropped c2's task: c2 hears of it
        self.assertEqual(self.typed(), [(other, "bullpen · BP-2 dropped")])

    def test_each_coordinator_reads_its_own_news(self):
        other = self.two_coordinators()
        self.bp("config", "--news", "monitor")
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        b = self.bp("add", "Primary's", "--brief", "x").stdout.strip()
        self.bp("start", a)
        self.bp("start", b)
        self.ask(a, "one")
        self.ask(b, "two")
        self.ask(a, "three")
        self.assertEqual(self.bp_in(other, "news").stdout.splitlines(),
                         ["bullpen · BP-1 needs the person (question): one", "bullpen · BP-1 needs the person (question): three"])
        self.assertEqual(self.bp_in("blk1", "news").stdout, "bullpen · BP-2 needs the person (question): two\n")
        self.assertEqual(self.bp_in(other, "news").stdout, "")
        self.ask(b, "four")
        self.assertEqual(self.bp_in(other, "news").stdout, "")
        self.assertIn("isn't one of this repo's coordinators", self.bp("news", ok=False).stderr)  # another pane
        del self.env["REX_BLOCK"]  # outside Rex: the primary's
        self.assertEqual(self.bp("news").stdout, "bullpen · BP-2 needs the person (question): four\n")
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")  # one cursor for c1, wherever it's read
        # two followers at once, one per coordinator: neither waits for the other
        self.bp_in(other, "news")
        self.bp_in("blk1", "news")

    def test_a_monitor_in_another_pane_doesnt_take_the_coordinators_news(self):
        # What held BP-20's line for 15 minutes: an agent that used to be the
        # coordinator kept starting its monitor. Its follower held the lock, so the
        # coordinator's own waited, and the line was printed in the other pane.
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        stray, stray_lines, _ = self.follow("me")
        self.assertEqual(stray_lines(1), ["bullpen · this pane isn't one of this repo's coordinators, so it "
                                          "doesn't read their news: don't start the monitor again "
                                          "(`bullpen up --here` here makes it one)"])
        stray.wait(timeout=10)
        p, lines, _ = self.follow()  # the coordinator's own gets the lock at once
        self.ask(tid, "one")
        self.assertEqual(lines(1), ["bullpen · BP-1 needs the person (question): one"])
        self.assertIn("isn't one of", self.bp("news", ok=False).stderr)  # a one-off read neither

    def test_a_monitor_stops_when_its_pane_is_no_longer_a_coordinator(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        p, lines, _ = self.follow()
        self.ask(tid, "one")
        self.assertEqual(lines(1), ["bullpen · BP-1 needs the person (question): one"])
        path = next((self.tmp / "state" / "repos").iterdir()) / "config.json"
        cfg = json.loads(path.read_text())
        cfg["coordinators"][0]["block"] = "blk9"  # another pane took the role
        path.write_text(json.dumps(cfg))
        self.assertIn("isn't one of this repo's coordinators", lines(1)[0])
        p.wait(timeout=10)

    def test_an_old_config_with_one_coordinator_still_loads(self):
        self.bp("up")  # blk1, board blk2
        path = next((self.tmp / "state" / "repos").iterdir()) / "config.json"
        cfg = json.loads(path.read_text())
        for k in ("coordinators", "next_coordinator"):
            cfg.pop(k)
        cfg["coordinator"] = {"block": "blk1", "session": "session:1"}
        cfg["board"] = {"block": "blk2", "session": "session:1"}
        path.write_text(json.dumps(cfg))
        (path.parent / "news.log").write_text("")
        (path.parent / "news.cursor").write_text("0\n")  # its one cursor
        cs = self.coordinators()
        self.assertEqual(cs, [{"id": "c1", "block": "blk1", "session": "session:1",
                               "board": {"block": "blk2", "session": "session:1"}}])
        self.assertNotIn("coordinator", json.loads(self.bp("config").stdout))
        self.assertIn("coordinator already running: c1 (blk1)", self.bp("up").stdout)
        self.assertEqual([a[0] for a in self.rex_calls()].count("split"), 1)  # its board stays
        self.bp("config", "--news", "monitor")
        tid = self.bp("add", "T", "--brief", "x").stdout.strip()
        self.assertEqual(self.show(tid)["coordinator"], "c1")
        other = self.new_pane()
        self.bp_in(other, "up", "--here")
        self.assertEqual([c["id"] for c in self.coordinators()], ["c1", "c2"])

    def test_an_old_cursor_carries_on_for_the_first_coordinator(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        d = next((self.tmp / "state" / "repos").iterdir())
        self.ask(tid, "one")
        for p in d.glob("news-*.cursor"):  # as an older bullpen left it
            (d / "news.cursor").write_text(p.read_text())
            p.unlink()
        self.assertEqual(self.bp_in("blk1", "news").stdout, "bullpen · BP-1 needs the person (question): one\n")
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")

    def test_a_gone_coordinators_tasks_fall_back_to_the_others(self):
        other = self.two_coordinators()
        third = self.new_pane()
        self.bp_in(third, "up", "--here")
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", a)
        board2 = self.coordinators()[1]["board"]["block"]
        (self.fake / "alive" / other).unlink()  # c2's agent is closed
        self.ask(a, "anyone?")
        self.assertEqual(sorted(self.typed()), sorted([("blk1", "bullpen · BP-1 needs the person (question): anyone?"),
                                                       (third, "bullpen · BP-1 needs the person (question): anyone?")]))
        self.bp("up")  # drops c2, and closes its board
        self.assertEqual([c["id"] for c in self.coordinators()], ["c1", "c3"])
        self.assertIn(["block", "close", board2, "-s", "session:1"], self.rex_calls())
        self.assertIn("coord: c2, gone: its news goes to every coordinator", self.bp("show", a).stdout)
        self.bp_in(third, "adopt", a)
        self.assertEqual(self.show(a)["coordinator"], "c3")
        self.ask(a, "c3?")
        self.assertEqual(self.typed()[-1], (third, "bullpen · BP-1 needs the person (question): c3?"))
        self.assertIn("coordinator's own pane", self.bp("adopt", a, ok=False).stderr)

    def test_waiting_news_for_a_gone_coordinator_goes_to_the_others(self):
        other = self.two_coordinators()
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", a)
        (self.fake / "screen").write_text("❯ half a message\n")  # the person is typing: news waits
        self.ask(a, "waiting")
        self.assertEqual(self.typed(), [])
        (self.fake / "alive" / other).unlink()
        (self.fake / "screen").write_text("❯ \n")
        self.bp("_flush")
        self.assertEqual(self.typed(), [("blk1", "bullpen · BP-1 needs the person (question): waiting")])
        self.assertEqual(list((self.tmp / "state").rglob("outbox*.txt")), [])

    def test_news_while_no_coordinator_is_alive_is_typed_once(self):
        other = self.two_coordinators()
        third = self.new_pane()
        self.bp_in(third, "up", "--here")
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", a)
        for b in ("blk1", other, third):
            (self.fake / "alive" / b).unlink()  # every coordinator's agent is closed
        self.ask(a, "nobody home")
        self.assertEqual(self.typed(), [])
        (self.fake / "alive" / "blk1").write_text("")  # one comes back
        self.bp("_flush")
        self.assertEqual(self.typed(), [("blk1", "bullpen · BP-1 needs the person (question): nobody home")])
        self.assertEqual(list((self.tmp / "state").rglob("outbox*.txt")), [])
        self.assertFalse((self.tmp / "state" / "outbox").exists())

    def test_go_and_hide_use_the_tasks_own_coordinator(self):
        other = self.two_coordinators()
        board2 = self.coordinators()[1]["board"]["block"]
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", a)
        self.assertEqual(self.api("session.new_layer")[-1]["window_id"], "w2")  # c2's tab
        self.bp_in(board2, "go")  # the board's own coordinator
        self.assertEqual(self.focused()[-1], other)
        self.bp_in(board2, "go", "--board")
        self.assertEqual(self.focused()[-1], board2)
        self.bp("go", a)
        self.bp("hide", "--from", self.block(a))
        self.assertEqual(self.focused()[-1], board2)
        self.bp("go", "--from", self.block(a))
        self.assertEqual(self.focused()[-1], other)
        self.bp("go")  # from elsewhere: the primary one
        self.assertEqual(self.focused()[-1], "blk1")


    # ---------- triage: the coordinator's, then the person's ----------

    def ago(self, tid, minutes):
        """The task entered its status this many minutes ago."""
        f = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
        t = json.loads(f.read_text())
        at = dt.datetime.now().astimezone() - dt.timedelta(minutes=minutes)
        f.write_text(json.dumps({**t, "since": at.isoformat()}))

    def test_worker_stops_go_to_the_coordinator_and_prompts_to_the_person(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "error", "", "API error")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Failed: API error"))
        self.assertTrue(self.news()[-1].startswith("bullpen · For the coordinator: BP-1's turn failed: API error"))
        self.assertIn("goes to the person", self.news()[-1])
        self.bp("_event", blk, "working", "", "")  # clears by itself
        self.assertEqual(self.show(tid)["status"], "working")
        self.bp("_event", blk, "idle", "", "")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Ended its turn without saying ready"))
        self.assertTrue(self.news()[-1].startswith("bullpen · For the coordinator: BP-1 ended its turn without `ready`"))
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Needs you (permission): Allow rm?"))
        self.assertEqual(self.news()[-1], "bullpen · BP-1 needs the person (permission): Allow rm?")

    def test_reviewer_stops_go_to_the_coordinator_and_prompts_to_the_person(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp("_event", reviewer, "blocked", "permission", "Allow curl?")
        self.assertEqual(self.show(tid)["status"], "needs")
        self.assertEqual(self.news()[-1], "bullpen · BP-1's reviewer needs the person (permission): Allow curl?")
        self.bp("_event", reviewer, "working", "", "")
        self.assertEqual(self.show(tid)["status"], "review")
        self.bp("_event", reviewer, "idle", "", "")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Reviewer stopped without a verdict"))
        self.assertIn("For the coordinator: BP-1's reviewer stopped without a verdict", self.news()[-1])
        self.assertIn("`bullpen review BP-1`", self.news()[-1])
        self.bp("_gone", reviewer)
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Reviewer exited without a verdict"))
        self.assertIn("For the coordinator: BP-1's reviewer exited without a verdict", self.news()[-1])

    def test_ask_puts_the_question_to_the_person(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        p = self.bp("ask", tid, "Use", "SQLite or Postgres?")
        self.assertIn("needs the person", p.stdout)
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Use SQLite or Postgres?"))
        self.assertEqual(self.news()[-1], "bullpen · BP-1 needs the person: Use SQLite or Postgres?")
        self.assertEqual(json.loads(self.bp("go", "--next", "--json").stdout)["block"], blk)
        # its agent's own state doesn't undo it: the question is the person's
        self.agent(blk, "working", msg="Reading")
        self.assertEqual(self.check(), [])
        # the coordinator asking from its own pane hears nothing of it
        before = len(self.news())
        self.bp_in("blk1", "ask", tid, "Still SQLite?")
        self.assertEqual(len(self.news()), before)
        self.assertEqual(self.show(tid)["why"], "Still SQLite?")
        # the person answers in its pane: it works again
        self.bp("_event", blk, "working", "", "")
        t = self.show(tid)
        self.assertEqual((t["status"], t.get("ask")), ("working", None))
        self.assertNotEqual(self.bp("ask", tid, ok=False).returncode, 0)  # no question

    def test_ok_clears_triage_without_typing(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.assertNotEqual(self.bp("ok", tid, ok=False).returncode, 0)  # working: nothing to clear
        self.bp("_event", blk, "idle", "", "")
        typed = len(self.typed())
        self.bp("ok", tid)
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"], t["ok"]), ("working", None, True))
        self.assertEqual(len(self.typed()), typed)  # nothing typed anywhere
        self.agent(blk, "idle")
        self.assertEqual(self.check(), [])  # the coordinator said it's fine
        self.agent(blk, "blocked", "permission", "Allow?")
        self.assertEqual(len(self.check()), 1)  # a prompt still counts
        self.bp("_event", blk, "working", "", "")
        self.assertNotIn("ok", self.show(tid))  # its next turn is reported as usual
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        (self.fake / "alive" / blk).unlink()
        p = self.bp("ok", tid, ok=False)
        self.assertIn("`bullpen resume BP-1`", p.stderr)

    def test_ok_puts_a_task_whose_reviewer_stopped_back_in_review(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp("_event", reviewer, "idle", "", "")
        self.bp("ok", tid)
        self.assertEqual(self.show(tid)["status"], "review")
        self.agent(reviewer, "idle")
        self.assertEqual(self.check(), [])
        self.bp("_event", reviewer, "working", "", "")
        self.bp("_event", reviewer, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        (self.fake / "alive" / reviewer).unlink()
        p = self.bp("ok", tid, ok=False)
        self.assertIn("`bullpen review BP-1`", p.stderr)

    def test_triage_goes_to_the_person_after_triage_minutes(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "error", "", "boom")
        self.ago(tid, 5)
        self.assertEqual(self.bp("check").stdout, "")
        self.bp("check", "--fix", "--quiet")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.ago(tid, 11)
        out = self.bp("check", ok=False)
        self.assertEqual(out.returncode, 1)
        self.assertEqual(out.stdout.strip(), "BP-1 waits on its coordinator, but its coordinator didn't "
                                             "resolve it in 10 min: it needs the person")
        self.assertEqual(self.show(tid)["status"], "triage")  # no --fix: nothing changes
        before = len(self.news())
        self.bp("check", "--fix", "--quiet")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Failed: boom (coordinator didn't resolve it)"))
        self.assertEqual(self.news()[before:], ["bullpen · BP-1 needs the person: Failed: boom "
                                                "(coordinator didn't resolve it)"])
        self.bp("check", "--fix", "--quiet")
        self.assertEqual(len(self.news()), before + 1)  # once
        # the repo's own setting
        tid2 = self.bp("add", "Two", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        self.bp("config", "--triage-minutes", "30")
        self.bp("_event", self.block(tid2), "idle", "", "")
        self.ago(tid2, 20)
        self.bp("check", "--fix", "--quiet", tid2)
        self.assertEqual(self.show(tid2)["status"], "triage")
        self.assertIn("in 30 min", [l for l in self.news() if "BP-2" in l][-1])

    def test_triage_needs_the_person_when_no_coordinator_runs(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "error", "", "boom")
        self.assertEqual(self.show(tid)["status"], "triage")
        (self.fake / "alive" / "blk1").unlink()  # the coordinator's pane is gone
        self.bp("check", "--fix", "--quiet")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Failed: boom (no coordinator is running)"))
        self.bp("_event", blk, "working", "", "")
        self.bp("_event", blk, "idle", "", "")  # straight to the person
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Ended its turn without saying ready"))
        line = [l for l in self.outbox_lines() if "ended its turn" in l][-1]
        self.assertIn("No coordinator is running to see to it, so it needs the person", line)
        self.assertNotIn("For the coordinator", line)

    def outbox_lines(self):
        return [l for f in (self.tmp / "state").rglob("outbox*.txt") for l in f.read_text().splitlines()]

    def test_triage_follows_its_own_coordinator(self):
        other = self.two_coordinators()
        tid = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", tid)
        (self.fake / "alive" / other).unlink()  # its owner is gone; c1 still runs
        self.bp("_event", self.block(tid), "error", "", "boom")
        self.assertEqual(self.show(tid)["status"], "needs")

    def test_review_starts_a_new_reviewer_within_the_rounds(self):
        tid = self.start_task()
        self.assertIn("no review", self.bp("review", tid, ok=False).stderr)
        reviewer = self.commit_and_ready(tid)
        self.assertEqual(self.show(tid)["review"]["round"], 1)
        self.bp("_event", reviewer, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.assertIn("round 2 of 2", self.bp("review", tid).stdout)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["round"], t["review"]["verdict"]), ("review", 2, None))
        self.assertNotEqual(t["review"]["block"], reviewer)
        self.assertFalse((self.fake / "alive" / reviewer).exists())  # the old one is closed
        self.bp("_gone", t["review"]["block"])
        self.assertEqual(self.show(tid)["status"], "triage")
        p = self.bp("review", tid, ok=False)
        self.assertIn("used its 2 review rounds: `bullpen ask BP-1`", p.stderr)
        self.assertEqual(self.show(tid)["status"], "triage")

    def test_review_is_refused_while_the_worker_is_on_it(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.verdict(tid, "changes", "Fix it.")
        self.assertEqual(self.show(tid)["status"], "working")
        self.assertIn("isn't waiting on a reviewer", self.bp("review", tid, ok=False).stderr)

    def test_review_is_refused_for_a_worker_stopped_after_ready_was_undone(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.bp("_event", blk, "idle", "", "")
        self.bp("_event", blk, "working", "", "More")  # back to working, its approval on record
        self.assertEqual(self.show(tid)["review"]["verdict"], "approved")
        self.bp("_event", blk, "error", "", "boom")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.assertIn("isn't waiting on a reviewer", self.bp("review", tid, ok=False).stderr)
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        self.assertIn("isn't waiting on a reviewer", self.bp("review", tid, ok=False).stderr)
        self.assertEqual(self.show(tid)["status"], "needs")

    def test_a_ready_task_whose_worker_works_again_is_working_again(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Fixing")
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.assertEqual(self.show(tid)["status"], "ready")
        self.bp("_event", blk, "working", "", "Still in the turn that ran ready")
        self.assertEqual(self.show(tid)["status"], "ready")  # the same turn: nothing new
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "ready")
        self.bp("_event", blk, "working", "", "The person asked for more")
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["verdict"]), ("working", "approved"))  # kept on record
        self.assertIn("BP-1 was ready, but its worker started working again", self.news()[-1])
        self.assertIn("run `ready` again, and its approval holds only if the code doesn't change", self.news()[-1])
        self.assertNotEqual(self.bp("accept", tid, ok=False).returncode, 0)

    def test_old_needs_tasks_become_triage_once(self):
        tid = self.start_task()
        d = next((self.tmp / "state").rglob("config.json")).parent
        cfg = json.loads((d / "config.json").read_text())
        cfg.pop("triage_migrated", None)
        (d / "config.json").write_text(json.dumps(cfg))
        f = d / "tasks" / f"{tid}.json"
        base = json.loads(f.read_text())
        whys = {"BP-1": "Ended its turn without saying ready", "BP-2": "Needs you (permission): Allow?",
                "BP-3": "Reviewer stopped without a verdict", "BP-4": "Reviewer needs you (input)",
                "BP-5": "PR checks failed: test", "BP-6": "Failed: boom"}
        for i, why in whys.items():
            (d / "tasks" / f"{i}.json").write_text(json.dumps({**base, "id": i, "status": "needs", "why": why,
                                                               "block": None}))
        self.bp("ls")
        got = {i: self.show(i)["status"] for i in whys}
        self.assertEqual(got, {"BP-1": "triage", "BP-2": "needs", "BP-3": "triage", "BP-4": "needs",
                               "BP-5": "triage", "BP-6": "triage"})
        # once: a task needing the person for such a reason later stays so
        t = json.loads((d / "tasks" / "BP-1.json").read_text())
        (d / "tasks" / "BP-1.json").write_text(json.dumps({**t, "status": "needs"}))
        self.bp("ls")
        self.assertEqual(self.show("BP-1")["status"], "needs")

    def test_old_stopped_tasks_get_their_stop_once(self):
        tid = self.start_task()
        d = next((self.tmp / "state").rglob("config.json")).parent
        cfg = json.loads((d / "config.json").read_text())
        cfg.pop("stops_migrated", None)
        (d / "config.json").write_text(json.dumps(cfg))
        base = json.loads((d / "tasks" / f"{tid}.json").read_text())
        old = {"BP-1": ("triage", "Reviewer stopped without a verdict", {}),
               "BP-2": ("needs", "Should it go?", {"ask": True, "ask_of": "Needs you (permission): Allow?"}),
               "BP-3": ("stopped", "Agent exited before saying ready", {}),
               "BP-4": ("triage", "PR checks failed: test", {}),
               "BP-5": ("working", None, {})}
        for i, (status, why, more) in old.items():
            (d / "tasks" / f"{i}.json").write_text(json.dumps({**base, "id": i, "status": status, "why": why,
                                                               "block": None, **more}))
        self.bp("ls")
        got = {i: self.show(i).get("stop") for i in old}
        self.assertEqual(got, {"BP-1": {"by": "reviewer", "kind": "ended"},
                               "BP-2": {"by": "worker", "kind": "prompt"},
                               "BP-3": {"by": "worker", "kind": "exited"},
                               "BP-4": {"by": "worker", "kind": "not_sent"}, "BP-5": None})
        self.assertNotIn("ask_of", self.show("BP-2"))
        # once: a stop recorded later is left as it is
        t = json.loads((d / "tasks" / "BP-1.json").read_text())
        (d / "tasks" / "BP-1.json").write_text(json.dumps({**t, "stop": {"by": "worker", "kind": "failed"}}))
        self.bp("ls")
        self.assertEqual(self.show("BP-1")["stop"], {"by": "worker", "kind": "failed"})

    def test_go_next_skips_triage(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        p = self.bp("go", "--next", "--json", ok=False)
        self.assertIn("nothing needs you", p.stderr)

    # ---------- cheaper review: kept approvals, follow-up rounds, light reviews ----------

    def commit_file(self, tid, name, text, msg=None):
        wt = self.show(tid)["worktree"]
        (Path(wt) / name).parent.mkdir(parents=True, exist_ok=True)
        (Path(wt) / name).write_text(text)
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", msg or f"Change {name}")
        return wt

    def ready(self, tid):
        return self.bp("ready", tid, "--summary", "Did it.", cwd=self.show(tid)["worktree"])

    def review_prompt(self, tid, n):
        return next((self.tmp / "state").rglob(f"{tid}.review-{n}.md")).read_text()

    def approved_task(self):
        """A started task with one small commit, reviewed and approved, its worker
        having said it works first (so a later working event is a new turn)."""
        tid = self.start_task()
        self.bp("_event", self.block(tid), "working", "", "Fixing")
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        return tid

    def test_an_approval_is_kept_when_ready_runs_again_on_the_same_commit(self):
        tid = self.approved_task()
        blk = self.block(tid)
        self.bp("_event", blk, "idle", "", "")
        self.bp("_event", blk, "working", "", "A stray message")
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["verdict"]), ("working", "approved"))
        self.assertIn("its approval holds only if the code doesn't change", self.news()[-1])
        out = self.ready(tid).stdout
        self.assertIn("approval carries over (the same commit)", out)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["round"], t["review"]["verdict"]), ("ready", 1, "approved"))
        self.assertEqual(list((self.tmp / "state").rglob(f"{tid}.review-2.md")), [])  # no new round
        self.assertEqual(self.news()[-1], "bullpen · BP-1 is ready (1 commit, PR #7), its approval carried "
                                          "over (the same commit): Fix the thing")
        self.assertIn("kept 1 time as the code didn't change", self.bp("show", tid).stdout)

    def test_a_turn_that_changes_no_code_leaves_a_ready_task_ready(self):
        tid = self.approved_task()
        blk = self.block(tid)
        self.bp("_event", blk, "idle", "", "")
        self.bp("_event", blk, "working", "", "Answering a question")
        self.bp("_event", blk, "idle", "", "")
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["verdict"]), ("ready", "approved"))
        self.assertIn("BP-1 is ready again: its worker's turn left the code as approved (the same commit)",
                      self.news()[-1])
        # a turn that does change it ends as usual, and its next ready is reviewed again
        self.bp("_event", blk, "working", "", "Doing more")
        self.commit_file(tid, "more.txt", "more\n")
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.ready(tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["kind"], r["verdict"]), (2, "followup", None))

    def test_an_approval_is_kept_after_a_clean_rebase_with_the_same_patch(self):
        tid = self.approved_task()
        wt = self.show(tid)["worktree"]
        (self.repo / "other.txt").write_text("other\n")
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "Other work")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")
        before = sh("git", "-C", wt, "rev-parse", "HEAD")
        sh("git", "-C", wt, "fetch", "-q", "origin")
        sh("git", "-C", wt, "rebase", "-q", "origin/main")
        self.assertNotEqual(sh("git", "-C", wt, "rev-parse", "HEAD"), before)
        self.bp("tell", tid, "Rebase onto main.")
        self.assertIn("same patch after a clean rebase onto origin/main", self.ready(tid).stdout)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["round"]), ("ready", 1))
        self.assertEqual(t["review"]["commit"], sh("git", "-C", wt, "rev-parse", "HEAD").strip())

    def test_a_real_code_change_is_reviewed_again(self):
        tid = self.approved_task()
        self.bp("tell", tid, "Handle the empty case too.")
        self.commit_file(tid, "fix1.txt", "fixed better\n")
        self.assertIn("A reviewer checks it now (follow-up review)", self.ready(tid).stdout)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["round"], t["review"]["kind"]), ("review", 2, "followup"))
        prompt = self.review_prompt(tid, 2)
        self.assertIn("Round 1 approved the change. Its notes", prompt)
        self.assertIn("Looks right.", prompt)
        self.assertIn("+fixed better", prompt)

    def test_round_two_follows_up_on_the_findings_with_the_diff_since(self):
        self.bp("config", "--reviewer-model", "opus")
        tid = self.start_task()
        self.bp("review-mode", tid, "full")
        self.commit_file(tid, "big.py", "print('one')\n")
        self.ready(tid)
        self.assertEqual(self.models(self.last_command()), ["opus"])  # the first round: the reviewer model
        first = self.show(tid)["review"]["commit"]
        self.verdict(tid, "changes", "big.py misses the empty case.")
        self.commit_file(tid, "big.py", "print('one')\nprint('empty')\n", "Handle the empty case")
        self.ready(tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["kind"], r["model"]), (2, "followup", "sonnet"))
        self.assertEqual(self.models(self.last_command()), ["sonnet"])
        prompt = self.review_prompt(tid, 2)
        self.assertIn("round 2 of 2 (follow-up review)", prompt)
        self.assertIn("Round 1 asked for changes. Its findings", prompt)
        self.assertIn("big.py misses the empty case.", prompt)
        self.assertIn(f"diff {first[:12]}..HEAD", prompt)
        self.assertIn("+print('empty')", prompt)
        self.assertNotIn("+print('one')", prompt)  # round 1 saw that: not in the diff since
        self.assertIn("confirm that each finding is fixed", prompt)
        self.bp("config", "--reviewer-followup-model", "haiku")
        self.verdict(tid, "approve")
        self.assertIn("approved by follow-up review", self.news()[-1])

    def test_a_small_or_docs_only_change_gets_a_light_review(self):
        tid = self.start_task()
        self.commit_file(tid, "fix.py", "x = 1\n")
        self.ready(tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["kind"], r["why"], r["model"]), ("light", "1 line in 1 file", "sonnet"))
        self.assertIn("This is a light review", self.review_prompt(tid, 1))
        self.assertIn("review:", self.bp("show", tid).stdout)
        self.assertIn("(light review (1 line in 1 file) on sonnet)", self.bp("show", tid).stdout)
        self.verdict(tid, "approve")
        self.assertIn("approved by light review (1 line in 1 file): Fix the thing", self.news()[-1])
        self.bp("add", "Docs", "--brief", "x")
        self.bp("start", "BP-2")
        self.commit_file("BP-2", "docs/guide.md", "line\n" * 300)
        self.commit_file("BP-2", "prompts/x.txt", "line\n" * 300)
        self.ready("BP-2")
        self.assertEqual(self.show("BP-2")["review"]["kind"], "light")
        self.assertEqual(self.show("BP-2")["review"]["why"], "docs only")

    def test_a_bigger_change_gets_a_full_review(self):
        self.bp("config", "--light-review-lines", "40")
        tid = self.start_task()
        self.commit_file(tid, "big.py", "x = 1\n" * 41)
        self.ready(tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["kind"], r["model"]), ("full", None))
        self.assertNotIn("--model", self.last_command())
        self.assertIn("What to check", self.review_prompt(tid, 1))
        self.assertNotIn("light review", self.review_prompt(tid, 1))
        # small, but in three files: full too
        self.bp("add", "Three", "--brief", "x")
        self.bp("start", "BP-2")
        for name in ("a.py", "b.py", "c.py"):
            self.commit_file("BP-2", name, "x\n")
        self.ready("BP-2")
        self.assertEqual(self.show("BP-2")["review"]["kind"], "full")

    def test_the_review_mode_overrides_auto_both_ways(self):
        self.bp("up")
        tid = self.bp("add", "Small", "--brief", "x", "--review", "full").stdout.strip()
        self.bp("start", tid)
        self.commit_file(tid, "fix.py", "x = 1\n")
        self.ready(tid)
        self.assertEqual(self.show(tid)["review"]["kind"], "full")
        self.assertIn("mode: full first review", self.bp("show", tid).stdout)
        self.bp("add", "Big", "--brief", "x")
        self.bp("review-mode", "BP-2", "light")
        self.bp("start", "BP-2")
        self.commit_file("BP-2", "big.py", "x = 1\n" * 200)
        self.ready("BP-2")
        r = self.show("BP-2")["review"]
        self.assertEqual((r["kind"], r["why"]), ("light", "set for BP-2"))
        self.bp("review-mode", "BP-2", "auto")
        self.assertNotIn("review_mode", self.show("BP-2"))
        self.assertNotEqual(self.bp("review-mode", "BP-2", "medium", ok=False).returncode, 0)

    def test_a_light_review_that_asks_for_changes_is_followed_up(self):
        tid = self.start_task()
        self.commit_file(tid, "fix.py", "x = 1\n")
        self.ready(tid)
        self.verdict(tid, "changes", "x should be 2.")
        self.commit_file(tid, "fix.py", "x = 2\n")
        self.ready(tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["kind"]), (2, "followup"))
        self.assertIn("x should be 2.", self.review_prompt(tid, 2))

    def test_a_replaced_reviewer_keeps_its_rounds_kind(self):
        self.set_commands(review_rounds=3)
        tid = self.start_task()
        self.commit_file(tid, "fix.py", "x = 1\n")
        self.ready(tid)
        self.verdict(tid, "changes", "x should be 2.")
        self.commit_file(tid, "fix.py", "x = 2\n")
        self.ready(tid)
        reviewer = self.show(tid)["review"]["block"]
        self.bp("_event", reviewer, "idle", "", "")
        self.bp("review", tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["kind"], r["prev"]["round"]), (3, "followup", 1))
        self.assertIn("x should be 2.", self.review_prompt(tid, 3))

    def test_ready_again_during_review_of_the_same_commit_leaves_the_reviewer(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        out = self.ready(tid).stdout
        self.assertIn("A reviewer is already checking this commit", out)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["block"]), (1, reviewer))
        self.assertTrue((self.fake / "alive" / reviewer).exists())

    def test_a_reviewer_with_a_background_shell_stays_in_review(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        (self.fake / f"screen_{reviewer}").write_text("❯ \n  ⏵⏵ auto mode on · 1 shell\n")
        self.bp("_event", reviewer, "idle", "", "Waiting for the tests")
        self.assertEqual(self.show(tid)["status"], "review")
        self.assertFalse([l for l in self.news() if "without a verdict" in l])
        self.agent(reviewer, "idle", ago=5 * 60)
        self.assertEqual(self.check(), [])
        self.agent(reviewer, "idle", ago=30 * 60)  # past stuck_after: not the tests any more
        self.check("--fix", "--quiet")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Reviewer stopped without a verdict"))
        self.assertIn("background shell of its own still runs", self.news()[-1])
        # its shell ended, and so did its turn: that's a stop
        (self.fake / f"screen_{reviewer}").unlink()
        self.bp("_event", reviewer, "working", "", "")
        self.bp("_event", reviewer, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")

    def test_after_accept_an_approval_survives_a_clean_rebase(self):
        tid = self.approved_task()
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        self.commit_file("BP-2", "two.txt", "two\n")
        self.ready("BP-2")
        self.verdict("BP-2", "approve")
        self.merge_on_github(tid)
        self.bp("accept", tid)
        t = self.show("BP-2")
        self.assertEqual((t["status"], t["review"]["round"]), ("ready", 1))
        self.assertIn("BP-2 rebased, its approval kept (the same patch after a clean rebase onto origin/main)",
                      self.news()[-1])

    def test_after_accept_a_rebase_that_changes_the_patch_is_reviewed_again(self):
        (self.repo / "list.txt").write_text("a\nb\nc\nd\ne\nf\n")
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "A list")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")
        tid = self.start_task()
        self.commit_file(tid, "list.txt", "A\nb\nc\nd\ne\nf\n")
        self.ready(tid)
        self.verdict(tid, "approve")
        # two lines down: it rebases cleanly, but BP-1's line is in its patch's context
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        self.commit_file("BP-2", "list.txt", "a\nb\nc\nD\ne\nf\n")
        self.ready("BP-2")
        self.verdict("BP-2", "approve")
        self.merge_on_github(tid)
        self.bp("accept", tid)
        t = self.show("BP-2")
        self.assertEqual((t["status"], t["review"]["round"], t["review"]["kind"]), ("review", 2, "followup"))
        self.assertIn("BP-2 rebased, but its patch changed: a reviewer checks it again", self.news()[-1])
        self.assertIn("range-diff", self.review_prompt("BP-2", 2))

    def test_after_accept_a_conflict_resolved_by_hand_is_reviewed_again(self):
        tid = self.approved_task()
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        wt = self.show("BP-2")["worktree"]
        self.bp("_event", self.block("BP-2"), "working", "", "")
        self.commit_file("BP-2", "fix1.txt", "mine\n")
        self.ready("BP-2")
        self.verdict("BP-2", "approve")
        self.bp("_event", self.block("BP-2"), "idle", "", "")
        self.merge_on_github(tid)
        self.bp("accept", tid)
        t = self.show("BP-2")
        self.assertEqual((t["status"], t["review"]["verdict"], t["review"]["conflict"]), ("working", "approved", True))
        sh("git", "-C", wt, "fetch", "-q", "origin")
        p = subprocess.run(["git", "-C", wt, "rebase", "origin/main"], capture_output=True, text=True)
        self.assertNotEqual(p.returncode, 0)
        (Path(wt) / "fix1.txt").write_text("mine\n")
        sh("git", "-C", wt, "add", ".")
        subprocess.run(["git", "-C", wt, "-c", "core.editor=true", "rebase", "--continue"], check=True,
                       capture_output=True, env=self.env)
        self.ready("BP-2")
        r = self.show("BP-2")["review"]
        self.assertEqual((r["round"], r["kind"], r["verdict"]), (2, "followup", None))

    def test_an_approval_carried_over_auto_accepts(self):
        self.bp("config", "--auto-accept", "on")
        tid = self.start_task()
        self.bp("_event", self.block(tid), "working", "", "Fixing")
        (self.fake / "pr.json").write_text(json.dumps({"state": "OPEN", "statusCheckRollup": [
            {"name": "test", "status": "COMPLETED", "conclusion": "FAILURE"}]}))
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.assertEqual(self.show(tid)["status"], "ready")  # its checks failed
        (self.fake / "pr.json").write_text(json.dumps({"state": "OPEN"}))
        self.bp("_event", self.block(tid), "idle", "", "")
        self.bp("tell", tid, "Run ready again.")
        self.ready(tid)
        for _ in range(100):
            if self.show(tid)["status"] == "done":
                break
            time.sleep(0.1)
        self.assertEqual(self.show(tid)["status"], "done")


    # ---------- finish and close: nothing to merge ----------

    def finish(self, tid, text="Recorded the demo video\n\nSaved to ~/Movies/demo.mov.", ok=True):
        f = self.tmp / "finish.md"
        f.write_text(text)
        wt = self.show(tid)["worktree"]
        return self.bp("finish", tid, "--summary-file", str(f), cwd=wt, ok=ok)

    def test_finish_refuses_a_branch_with_a_net_diff(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "notes.txt").write_text("notes\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Notes")
        p = self.finish(tid, ok=False)
        self.assertIn("run `ready` instead", p.stderr)
        self.assertIn("1 file, +1 −0", p.stderr)
        self.assertEqual(self.show(tid)["status"], "working")
        (Path(wt) / "dirty.txt").write_text("x\n")
        self.assertIn("uncommitted", self.finish(tid, ok=False).stderr)

    def test_finish_with_commits_that_cancel_out(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "scratch.txt").write_text("tmp\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Scratch")
        sh("git", "-C", wt, "rm", "-q", "scratch.txt")
        sh("git", "-C", wt, "commit", "-qm", "Remove scratch")
        self.bp("_event", self.block(tid), "working", "", "Recording")
        p = self.finish(tid)
        self.assertIn("The person closes it", p.stdout)
        t = self.show(tid)
        self.assertEqual(t["status"], "finished")
        self.assertIn("Saved to ~/Movies/demo.mov.", t["summary"])
        self.assertIsNone(t.get("review"))  # no review
        self.assertTrue((self.fake / "alive" / t["block"]).exists())  # the person closes it
        self.assertFalse((self.fake / "gh.log").exists())  # no push, no PR
        self.assertEqual(self.news()[-1], "bullpen · BP-1 finished (nothing to merge): Recorded the demo video")
        # its turn ends cleanly: no triage, no news
        before = len(self.typed())
        self.bp("_event", self.block(tid), "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "finished")
        self.assertEqual(len(self.typed()), before)
        self.assertEqual(self.check(), [])
        self.assertIn("finished Fix the thing  (nothing to merge: Recorded the demo video)", self.bp("ls").stdout)

    def test_finish_needs_a_summary(self):
        tid = self.start_task()
        self.assertIn("--summary-file", self.bp("finish", tid, ok=False).stderr)
        self.assertIn("is empty", self.finish(tid, text="", ok=False).stderr)

    def test_close_removes_the_worktree_branch_and_pane(self):
        tid = self.start_task()
        t = self.show(tid)
        wt, blk = t["worktree"], t["block"]
        self.assertIn("not finished", self.bp("close", tid, ok=False).stderr)
        self.finish(tid)
        (Path(wt) / "late.txt").write_text("x\n")  # left over: no warning, it goes
        self.bp("close", tid)
        t = self.show(tid)
        self.assertEqual((t["status"], t["closed"], t["worktree"], t["block"]), ("done", True, None, None))
        self.assertFalse(Path(wt).exists())
        self.assertFalse((self.fake / "alive" / blk).exists())
        self.assertNotIn("bullpen/", sh("git", "-C", str(self.repo), "branch"))
        self.assertEqual(self.news()[-1], "bullpen · BP-1 closed (finished, nothing to merge)")
        self.assertIn("not finished", self.bp("close", tid, ok=False).stderr)

    def test_accept_closes_a_finished_task(self):
        tid = self.start_task()
        t = self.show(tid)
        wt, blk = t["worktree"], t["block"]
        self.assertIn("not ready or finished", self.bp("accept", tid, ok=False).stderr)
        self.finish(tid)
        p = self.bp("accept", tid)
        self.assertIn(f"{tid} closed", p.stdout)
        t = self.show(tid)
        self.assertEqual((t["status"], t["closed"], t["worktree"], t["block"]), ("done", True, None, None))
        self.assertFalse(Path(wt).exists())
        self.assertFalse((self.fake / "alive" / blk).exists())
        self.assertNotIn("bullpen/", sh("git", "-C", str(self.repo), "branch"))
        self.assertEqual(self.news()[-1], "bullpen · BP-1 closed (finished, nothing to merge)")
        self.assertFalse((self.fake / "gh.log").exists())  # no PR to merge
        self.assertIn("not ready or finished", self.bp("accept", tid, ok=False).stderr)

    def test_close_starts_what_waited_on_it(self):
        tid = self.start_task("First")
        self.bp("add", "Second", "--brief", "x", "--after", tid)
        self.finish(tid)
        self.bp("close", tid)
        self.assertEqual(self.show("BP-2")["status"], "working")
        self.assertIn("bullpen · BP-2 started (BP-1 was closed)", self.news())
        self.assertIn("### BP-1: First (finished with nothing to merge, closed)", self.full_brief("BP-2"))

    def test_auto_accept_closes_a_finished_task(self):
        self.bp("up")
        self.bp("config", "--auto-accept", "on")
        tid = self.bp("add", "Record the demo", "--brief", "x").stdout.strip()
        self.bp("start", tid)
        t = self.show(tid)
        p = self.finish(tid)
        self.assertIn("auto-accept closes it now", p.stdout)
        self.assertEqual((self.show(tid)["status"], self.show(tid)["closed"]), ("done", True))
        self.assertFalse(Path(t["worktree"]).exists())
        self.assertFalse((self.fake / "alive" / t["block"]).exists())
        self.assertEqual(self.news()[-1], "bullpen · BP-1 finished (nothing to merge) and was closed "
                         "by auto-accept: Recorded the demo video")

    def test_a_tasks_own_auto_accept_closes_it_and_no_auto_accept_keeps_it(self):
        self.bp("up")
        self.bp("add", "One", "--brief", "x", "--auto-accept")
        self.bp("start", "BP-1")
        self.finish("BP-1")
        self.assertEqual(self.show("BP-1")["status"], "done")
        self.bp("config", "--auto-accept", "on")
        self.bp("add", "Two", "--brief", "x", "--no-auto-accept")
        self.bp("start", "BP-2")
        self.finish("BP-2")
        self.assertEqual(self.show("BP-2")["status"], "finished")

    def test_a_finished_task_whose_worker_works_again_is_working_again(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Writing notes")
        self.finish(tid)
        self.bp("_event", blk, "working", "", "Still in the turn that ran finish")
        self.assertEqual(self.show(tid)["status"], "finished")
        self.bp("_event", blk, "idle", "", "")
        self.bp("_event", blk, "working", "", "The person asked for more")
        self.assertEqual(self.show(tid)["status"], "working")
        self.assertIn("BP-1 was finished, but its worker started working again", self.news()[-1])
        self.assertIn("run `finish` (or `ready`) again", self.news()[-1])
        self.assertIn("not finished", self.bp("close", tid, ok=False).stderr)

    COMPACTING = "● Done.\n\n✽ Compacting conversation… (12s · esc to interrupt)\n\n❯ \n  ⏵⏵ auto mode on\n"

    def compact(self, blk):
        """Claude Code auto-compacts in pane blk: it says it's working, and its spinner
        says what at."""
        (self.fake / f"screen_{blk}").write_text(self.COMPACTING)
        self.bp("_event", blk, "working", "", "")

    def compacted(self, blk, state="idle"):
        (self.fake / f"screen_{blk}").write_text("✻ Conversation compacted\n\n❯ \n  ⏵⏵ auto mode on\n")
        self.bp("_event", blk, state, "", "")

    def test_compaction_leaves_a_ready_task_ready_with_its_approval(self):
        tid = self.approved_task()
        blk = self.block(tid)
        self.bp("_event", blk, "idle", "", "")
        before = len(self.news())
        self.compact(blk)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["verdict"]), ("ready", "approved"))
        self.assertIn('"compacting": true', (self.tmp / "state" / "blocks" / f"{blk}.json").read_text())
        self.compacted(blk)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["verdict"]), ("ready", "approved"))
        self.assertNotIn("compacting", t)
        self.assertNotIn("compacting", (self.tmp / "state" / "blocks" / f"{blk}.json").read_text())
        self.assertEqual(len(self.news()), before)  # no "working again", no "ended its turn"
        self.bp("accept", tid)  # its approval still counts

    def test_compaction_leaves_a_finished_task_finished(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Writing notes")
        self.finish(tid)
        self.bp("_event", blk, "done", "", "")
        before = len(self.news())
        self.compact(blk)
        self.assertEqual(self.show(tid)["status"], "finished")
        self.compacted(blk, "done")
        t = self.show(tid)
        self.assertEqual(t["status"], "finished")
        self.assertIn("Saved to ~/Movies/demo.mov.", t["summary"])
        self.assertEqual(len(self.news()), before)

    def test_compaction_leaves_a_triage_task_in_triage(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Fixing")
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        before = len(self.news())
        self.compact(blk)
        self.compacted(blk)
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Ended its turn without saying ready"))
        self.assertEqual(len(self.news()), before)

    def test_a_real_turn_after_compaction_still_counts(self):
        tid = self.approved_task()
        blk = self.block(tid)
        self.bp("_event", blk, "idle", "", "")
        self.compact(blk)
        (self.fake / f"screen_{blk}").write_text("✶ Misting… (3s · esc to interrupt)\n")
        self.bp("_event", blk, "working", "", "The person asked for more")
        self.assertEqual(self.show(tid)["status"], "working")
        self.assertIn("BP-1 was ready, but its worker started working again", self.news()[-1])
        self.commit_file(tid, "more.txt", "more\n")
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.assertIn("ended its turn without `ready`", self.news()[-1])

    def test_compaction_then_a_turn_from_tell_still_counts(self):
        tid = self.approved_task()
        blk = self.block(tid)
        self.bp("_event", blk, "idle", "", "")
        self.compact(blk)
        self.bp("tell", tid, "Handle the empty case too.")
        self.assertEqual(self.show(tid)["status"], "working")
        self.compacted(blk)  # the turn tell started ends: not a compaction's end
        self.assertEqual(self.show(tid)["status"], "triage")  # as after any told turn: its next ready decides
        self.assertIn("ended its turn without `ready`", self.news()[-1])

    def test_check_leaves_a_compacting_agent_alone(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Fixing")
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.agent(blk, "working")
        (self.fake / f"screen_{blk}").write_text(self.COMPACTING)
        self.assertEqual(self.check(), [])
        self.assertEqual(self.check("--fix"), [])
        self.assertEqual(self.show(tid)["status"], "triage")
        (self.fake / f"screen_{blk}").write_text("✶ Misting… (3s · esc to interrupt)\n")
        self.assertEqual(self.check(), [f"{tid} is marked triage (ended its turn without saying ready) "
                                        "but its agent is working again"])

    def test_a_compacting_reviewer_stays_where_it_is(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        reviewer = self.show(tid)["review"]["block"]
        self.bp("_event", reviewer, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.compact(reviewer)
        self.assertEqual(self.show(tid)["status"], "triage")
        entry = self.tmp / "state" / "blocks" / f"{reviewer}.json"
        self.assertIn('"compacting": true', entry.read_text())  # the watcher passes on its next turn
        self.agent(reviewer, "working")
        self.assertEqual(self.check(), [])
        self.compacted(reviewer)
        t = self.show(tid)
        self.assertEqual(t["status"], "triage")
        self.assertNotIn("compacting", t["review"])
        self.assertNotIn("compacting", entry.read_text())

    def test_a_reviewers_real_turn_after_compaction_still_counts(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        reviewer = self.show(tid)["review"]["block"]
        self.bp("_event", reviewer, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.compact(reviewer)
        (self.fake / f"screen_{reviewer}").write_text("✶ Misting… (3s · esc to interrupt)\n")
        self.bp("_event", reviewer, "working", "", "Reading the diff")
        t = self.show(tid)
        self.assertEqual(t["status"], "review")
        self.assertNotIn("compacting", t["review"])
        self.assertNotIn("compacting", (self.tmp / "state" / "blocks" / f"{reviewer}.json").read_text())

    def test_the_workers_instructions_say_when_to_finish(self):
        tid = self.start_task()
        brief = self.full_brief(tid)
        self.assertIn(f"finish {tid} --summary-file", brief)
        self.assertIn(f"ready {tid} --summary-file", brief)

    # ----- help and ls -----

    def test_help_leaves_out_the_internal_commands(self):
        out = self.bp("--help").stdout
        for name in ("_event", "_stuck", "_flush", "_prs", "_limit", "_gone", "_auto_accept"):
            self.assertNotIn(name, out)
        self.assertIn("{up,go,hide,", out)
        self.assertIn("accept", out)
        self.bp("_flush")  # still there for the watcher
        self.bp("_limit")
        self.bp("_prs")

    def test_ls_uses_the_boards_words(self):
        tid = self.start_task()
        self.bp("add", "Later", "--brief", "Then this.")
        out = self.bp("ls").stdout
        self.assertRegex(out, rf"(?m)^{tid} +working +Fix the thing")
        self.assertRegex(out, r"(?m)^BP-2 +queued \(todo\) +Later")
        self.bp("_event", self.block(tid), "blocked", "permission", "Allow rm?")
        self.assertRegex(self.bp("ls").stdout, rf"(?m)^{tid} +needs you +Fix the thing")
        self.assertEqual(self.show(tid)["status"], "needs")
        self.assertEqual({t["status"] for t in json.loads(self.bp("ls", "--json").stdout)}, {"needs", "todo"})

    # ---------- notifications for the person ----------

    def terminal(self, blk):
        """Give pane blk a real terminal, with a process on it; returns a function that
        reads what was written to it since."""
        import pty
        pid, fd = pty.fork()
        if pid == 0:
            os.execvp("sleep", ["sleep", "60"])
        def done():
            os.kill(pid, 9)
            os.waitpid(pid, 0)
            os.close(fd)
        self.addCleanup(done)
        (self.fake / f"pid_{blk}").write_text(str(pid))

        def read():
            out = b""
            while select.select([fd], [], [], 0.2)[0]:
                try:
                    out += os.read(fd, 4096)
                except OSError:
                    break
            return out.decode()
        return read

    def notified(self):
        log = next((self.tmp / "state").rglob("notify.log"), None)
        return [l.split(" ", 1)[1] for l in log.read_text().splitlines()] if log else []

    def test_a_task_that_needs_you_sends_a_notification_from_its_pane(self):
        self.assertEqual(json.loads(self.bp("config").stdout)["notify"], "needs")  # the default
        tid = self.start_task()
        blk = self.block(tid)
        read = self.terminal(blk)
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        self.assertEqual(read(), "\x1b]777;notify;bullpen · proj;BP-1 needs you (permission): Allow rm?\x07")
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")  # the same prompt: once
        self.assertEqual(read(), "")
        self.bp("_event", blk, "working", "", "")
        self.bp("_event", blk, "idle", "", "")  # triage: the coordinator's, not the person's
        self.assertEqual(read(), "")
        self.bp("ask", tid, "Use SQLite?")
        self.assertEqual(read(), "\x1b]777;notify;bullpen · proj;BP-1 needs you: Use SQLite?\x07")
        self.assertEqual(self.notified(), [f"BP-1 sent to {blk}: BP-1 needs you (permission): Allow rm?",
                                           f"BP-1 sent to {blk}: BP-1 needs you: Use SQLite?"])

    def test_notify_needs_leaves_ready_out_and_all_sends_it(self):
        self.bp("config", "--review", "off")
        tid = self.start_task()
        read = self.terminal(self.block(tid))
        self.commit_and_ready_no_review(tid)
        self.assertEqual((self.show(tid)["status"], read()), ("ready", ""))
        tid2 = self.bp("add", "Second thing", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        read2 = self.terminal(self.block(tid2))
        self.bp("config", "--notify", "all")
        wt = self.show(tid2)["worktree"]
        (Path(wt) / "two.txt").write_text("two\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Two")
        self.bp("ready", tid2, "--summary", "Did it.", cwd=wt)
        self.assertEqual(read2(), "\x1b]777;notify;bullpen · proj;BP-2 is ready: Second thing\x07")

    def test_notify_off_sends_nothing(self):
        tid = self.start_task()
        read = self.terminal(self.block(tid))
        self.bp("config", "--notify", "off")
        self.bp("_event", self.block(tid), "blocked", "permission", "Allow rm?")
        self.assertEqual((read(), self.notified()), ("", []))
        self.assertNotEqual(self.bp("config", "--notify", "loud", ok=False).returncode, 0)

    def test_no_notification_while_the_person_looks_at_the_task_or_its_board(self):
        tid = self.start_task()
        blk = self.block(tid)
        read = self.terminal(blk)
        (self.fake / "focused").write_text("blk2")  # the board
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        self.assertEqual(read(), "")
        self.assertEqual(self.notified(), ["BP-1 not sent: the person is looking: "
                                           "BP-1 needs you (permission): Allow rm?"])
        self.bp("_event", blk, "working", "", "")
        (self.fake / "focused").write_text("blk1")  # the coordinator: it doesn't show the task
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        self.assertIn("needs you (permission)", read())

    def test_a_task_without_a_terminal_sends_from_its_board(self):
        tid = self.start_task()
        read = self.terminal("blk2")  # the worker's pane has none Rex can tell
        self.bp("_event", self.block(tid), "blocked", "permission", "Allow rm?")
        self.assertIn("BP-1 needs you (permission)", read())


class LocalModeTest(unittest.TestCase):
    """A repository without an origin remote: no push, no gh; accept merges locally."""

    setUp_with_origin = BullpenTest.setUp
    bp = BullpenTest.bp
    rex_calls = BullpenTest.rex_calls
    api = BullpenTest.api
    typed = BullpenTest.typed
    show = BullpenTest.show
    start_task = BullpenTest.start_task

    def setUp(self):
        self.setUp_with_origin()
        sh("git", "-C", str(self.repo), "remote", "remove", "origin")

    def tearDown(self):
        self.assertFalse((self.fake / "gh.log").exists(), "gh was called in local mode")

    def git(self, *args, cwd=None):
        return sh("git", "-C", str(cwd or self.repo), *args).strip()

    def commit(self, where, name, text, msg):
        (Path(where) / name).write_text(text)
        self.git("add", ".", cwd=where)
        self.git("commit", "-qm", msg, cwd=where)

    def ready_task(self, review=False):
        """A started task with one commit (fix.txt), and an unrelated commit on main
        made since it started, then run ready."""
        if not review:
            self.bp("config", "--review", "off")
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        self.commit(wt, "fix.txt", "fixed\n", "Fix it")
        self.commit(self.repo, "other.txt", "other\n", "Other work on main")
        self.bp("ready", tid, "--summary", "Fixed the thing.", cwd=wt)
        return tid, wt

    def test_finish_and_close_without_a_remote(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        self.commit(self.repo, "other.txt", "other\n", "Other work on main")
        f = self.tmp / "f.md"
        f.write_text("Answered the question.")
        self.bp("finish", tid, "--summary-file", str(f), cwd=wt)
        self.assertEqual(self.show(tid)["status"], "finished")
        self.bp("close", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertFalse(Path(wt).exists())
        self.assertEqual(self.git("log", "-1", "--format=%s", "main"), "Other work on main")  # nothing merged

    def test_accept_closes_a_finished_task_without_a_remote(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        f = self.tmp / "f.md"
        f.write_text("Answered the question.")
        self.bp("finish", tid, "--summary-file", str(f), cwd=wt)
        main = self.git("rev-parse", "main")
        self.bp("accept", tid)
        t = self.show(tid)
        self.assertEqual((t["status"], t["closed"]), ("done", True))
        self.assertFalse(Path(wt).exists())
        self.assertEqual(self.git("rev-parse", "main"), main)  # nothing merged

    def test_start_uses_the_local_base(self):
        tid = self.start_task()
        t = self.show(tid)
        self.assertEqual(t["base"], "main")
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=t["worktree"]), self.git("rev-parse", "main"))

    def test_ready_marks_the_local_branch_ready_without_push_or_gh(self):
        tid, wt = self.ready_task()
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertIsNone(t["pr"])
        self.assertTrue(t["local"])
        self.assertEqual(t["commits"], 1)
        self.bp("_prs")  # no PR to follow: no gh call (tearDown checks)
        self.assertFalse((self.tmp / "state" / "prs").exists())
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-1 is ready (1 commit, local branch): Fix the thing"))
        ls = self.bp("ls").stdout
        self.assertIn("local branch bullpen/bp-1-fix-the-thing", ls)
        self.assertNotIn("None", ls)
        show = self.bp("show", tid).stdout
        self.assertIn("local branch", show)
        self.assertNotIn("None", show)

    def test_ready_still_refuses_uncommitted_changes_and_handles_no_commits(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "dirty.txt").write_text("x\n")
        self.assertIn("uncommitted", self.bp("ready", tid, cwd=wt, ok=False).stderr)
        (Path(wt) / "dirty.txt").unlink()
        self.bp("ready", tid, cwd=wt)
        self.assertEqual(self.show(tid)["why"], "Nothing to merge (no commits)")

    def test_rules_from_the_local_base(self):
        (self.repo / ".bullpen").mkdir()
        self.commit(self.repo, ".bullpen/rules.md", "- Local rule.\n", "Rules")
        self.start_task()
        self.assertIn("- Local rule.", next((self.tmp / "state").rglob("BP-1.full.md")).read_text())
        self.assertIn("From .bullpen/rules.md on main", self.bp("rules").stdout)

    def test_ready_starts_the_reviewer(self):
        tid, wt = self.ready_task(review=True)
        t = self.show(tid)
        self.assertEqual(t["status"], "review")
        layer = self.api("session.new_layer")[-1]
        self.assertEqual(layer["layout"]["block"]["label"], "bullpen: BP-1 review")
        prompt = next((self.tmp / "state").rglob("BP-1.review-1.md")).read_text()
        self.assertIn(f"diff main...HEAD", prompt)
        self.bp("verdict", tid, "approve", "--summary", "Fine.")
        self.assertEqual(self.typed()[-1][1], "bullpen · BP-1 is ready (1 commit, local branch), approved by "
                                              "light review (1 line in 1 file): Fix the thing")

    def accepted(self, tid, wt):
        t = self.show(tid)
        self.assertEqual(t["status"], "done")
        self.assertFalse(Path(wt).exists())
        self.assertNotIn("bullpen/", self.git("branch"))
        self.assertEqual(self.git("show", "main:fix.txt"), "fixed")
        self.assertEqual(self.git("show", "main:other.txt"), "other")
        self.assertEqual(self.git("status", "--porcelain"), "")  # the checkout moved along

    def test_accept_rebase(self):
        tid, wt = self.ready_task()
        self.bp("accept", tid)
        self.accepted(tid, wt)
        self.assertEqual(self.git("log", "--format=%s", "main"), "Fix it\nOther work on main\ninit")
        self.assertEqual(self.git("rev-list", "--merges", "main"), "")

    def test_accept_starts_a_queued_task_on_the_new_local_base(self):
        tid, wt = self.ready_task()
        self.bp("add", "Next", "--brief", "x", "--after", tid)
        self.bp("accept", tid)
        t = self.show("BP-2")
        self.assertEqual(t["status"], "working")
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=t["worktree"]), self.git("rev-parse", "main"))
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-2 started (BP-1 was accepted)"))
        brief = next((self.tmp / "state").rglob("BP-2.full.md")).read_text()
        self.assertIn("### BP-1: Fix the thing (accepted, merged into main)", brief)
        self.assertIn("Fixed the thing.", brief)

    def test_accept_squash(self):
        self.bp("config", "--merge", "squash")
        tid, wt = self.ready_task()
        self.commit(wt, "more.txt", "more\n", "More")  # two commits become one
        self.bp("ready", tid, "--summary", "Fixed the thing.", cwd=wt)
        self.bp("accept", tid)
        self.accepted(tid, wt)
        self.assertEqual(self.git("log", "--format=%s", "main"), "Fix the thing (BP-1)\nOther work on main\ninit")
        self.assertEqual(self.git("log", "-1", "--format=%b", "main"), "Fixed the thing.")
        self.assertEqual(self.git("show", "main:more.txt"), "more")

    def test_accept_merge(self):
        self.bp("config", "--merge", "merge")
        tid, wt = self.ready_task()
        self.bp("accept", tid)
        self.accepted(tid, wt)
        parents = self.git("log", "-1", "--format=%P", "main").split()
        self.assertEqual(len(parents), 2)
        self.assertIn("BP-1", self.git("log", "-1", "--format=%s", "main"))

    def test_accept_merges_locally_even_if_an_origin_was_added_after_ready(self):
        tid, wt = self.ready_task()
        origin = self.tmp / "later.git"
        sh("git", "init", "-q", "--bare", "-b", "main", str(origin))
        self.git("remote", "add", "origin", str(origin))
        self.bp("accept", tid)
        self.accepted(tid, wt)

    def test_accept_keeps_unrelated_uncommitted_changes_in_the_main_checkout(self):
        tid, wt = self.ready_task()
        (self.repo / "README").write_text("edited, not committed\n")
        self.bp("accept", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual((self.repo / "README").read_text(), "edited, not committed\n")
        self.assertEqual(self.git("show", "main:fix.txt"), "fixed")
        self.assertEqual(self.git("status", "--porcelain"), "M README")

    def assert_untouched(self, tid, wt, main):
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertTrue(Path(wt).is_dir())
        self.assertEqual(t["worktree"], wt)
        self.assertIn("bullpen/bp-1-fix-the-thing", self.git("branch"))
        self.assertEqual(self.git("rev-parse", "main"), main)
        self.assertEqual(self.git("status", "--porcelain", cwd=wt), "")
        rebase = self.git("rev-parse", "--path-format=absolute", "--git-path", "rebase-merge", cwd=wt)
        self.assertFalse(Path(rebase).exists())  # no rebase left half done

    # ---- auto-accept ----

    def news_tail(self):
        return [l for b, l in self.typed() if b == "blk1"][-1]

    def test_auto_accept_merges_an_approved_branch(self):
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.ready_task(review=True)
        self.bp("verdict", tid, "approve", "--summary", "Fine.", cwd=wt)
        self.accepted(tid, wt)
        self.assertEqual(self.news_tail(),
                         "bullpen · BP-1 approved by review and auto-accepted (merged into main, rebase)")

    def test_auto_accept_off_leaves_it_ready(self):
        tid, wt = self.ready_task(review=True)
        self.bp("verdict", tid, "approve", "--summary", "Fine.", cwd=wt)
        self.assertEqual(self.show(tid)["status"], "ready")

    def test_no_auto_accept_holds_a_task(self):
        self.bp("config", "--auto-accept", "on")
        self.bp("up")
        tid = self.bp("add", "Fix the thing", "--brief", "x", "--no-auto-accept").stdout.strip()
        self.bp("start", tid)
        wt = self.show(tid)["worktree"]
        self.commit(wt, "fix.txt", "fixed\n", "Fix it")
        self.bp("ready", tid, "--summary", "Fixed.", cwd=wt)
        self.bp("verdict", tid, "approve", "--summary", "Fine.", cwd=wt)
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertIs(self.show(tid)["auto_accept"], False)

    def test_auto_accept_never_takes_open_findings_after_the_last_round(self):
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.ready_task(review=True)
        self.bp("verdict", tid, "changes", "--summary", "Missing a case.", cwd=wt)
        self.commit(wt, "fix2.txt", "more\n", "Fix more")
        self.bp("ready", tid, "--summary", "Fixed.", cwd=wt)
        self.bp("verdict", tid, "changes", "--summary", "Still missing.", cwd=wt)
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertIn("after 2 rounds", t["why"])
        self.assertTrue(Path(wt).exists())

    def test_auto_accept_never_takes_a_task_with_review_off(self):
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.ready_task()
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertTrue(Path(wt).exists())

    def test_a_refused_auto_accept_leaves_it_ready_with_news(self):
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.ready_task(review=True)
        self.commit(self.repo, "fix.txt", "fixed differently\n", "Clash")
        main = self.git("rev-parse", "main")
        self.bp("verdict", tid, "approve", "--summary", "Fine.", cwd=wt)
        self.assert_untouched(tid, wt, main)
        self.assertIn("approved by review, but the auto-accept was refused: merge refused", self.news_tail())
        self.assertIn("Send it back to rebase", self.news_tail())

    def test_accept_refuses_a_conflict_and_leaves_the_task(self):
        for mode in ("rebase", "squash", "merge"):
            with self.subTest(mode=mode):
                self.setUp()
                self.bp("config", "--merge", mode)
                tid, wt = self.ready_task()
                self.commit(self.repo, "fix.txt", "fixed differently\n", "Clash")
                main = self.git("rev-parse", "main")
                p = self.bp("accept", tid, ok=False)
                self.assertNotEqual(p.returncode, 0)
                self.assertIn("merge refused", p.stderr)
                self.assert_untouched(tid, wt, main)

    def test_accept_refuses_when_a_dirty_file_in_the_main_checkout_would_be_overwritten(self):
        tid, wt = self.ready_task()
        (self.repo / "fix.txt").write_text("mine\n")  # untracked, in the way
        main = self.git("rev-parse", "main")
        p = self.bp("accept", tid, ok=False)
        self.assertIn("can't fast-forward main", p.stderr)
        self.assert_untouched(tid, wt, main)
        self.assertEqual((self.repo / "fix.txt").read_text(), "mine\n")

    def test_accept_when_the_base_is_not_checked_out(self):
        tid, wt = self.ready_task()
        self.git("switch", "-q", "-c", "elsewhere")
        self.bp("accept", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(self.git("show", "main:fix.txt"), "fixed")
        self.assertEqual(self.git("branch", "--show-current"), "elsewhere")

    def test_accept_with_nothing_to_merge(self):
        tid = self.start_task()
        self.bp("ready", tid)
        main = self.git("rev-parse", "main")
        self.bp("accept", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(self.git("rev-parse", "main"), main)

    def test_diff_against_the_local_base(self):
        tid, wt = self.ready_task()  # main moved on since: only the task's own change shows
        out = self.bp("diff", tid, "--stat").stdout
        self.assertIn("fix.txt | 1 +", out)
        self.assertNotIn("other.txt", out)
        self.assertNotIn("Uncommitted", out)
        self.assertEqual(self.show(tid)["diffstat"], "1 file, +1 −0")

    def test_drop_closes_no_pr(self):
        tid, wt = self.ready_task()
        self.bp("drop", tid)
        self.assertEqual(self.show(tid)["status"], "dropped")
        self.assertFalse(Path(wt).exists())
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-1 dropped"))

    def test_accept_is_news_naming_the_base_and_the_merge(self):
        self.bp("config", "--merge", "squash")
        tid, wt = self.ready_task()
        self.bp("accept", tid)
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-1 accepted (merged into main, squash)"))

    def test_accept_from_the_coordinators_pane_is_no_news(self):
        tid, wt = self.ready_task()
        before = self.typed()
        self.env["REX_BLOCK"] = "blk1"
        self.bp("accept", tid)
        self.accepted(tid, wt)
        self.assertEqual(self.typed(), before)

    def test_accept_with_nothing_to_merge_says_so(self):
        tid = self.start_task()
        self.bp("ready", tid)
        self.bp("accept", tid)
        self.assertEqual(self.typed()[-1][1], "bullpen · BP-1 accepted (nothing to merge)")


    def second_ready(self, name, text):
        """BP-2, started from main before BP-1's accept, with one commit, ready."""
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        wt = self.show("BP-2")["worktree"]
        self.commit(wt, name, text, "Second")
        self.bp("ready", "BP-2", "--summary", "done", cwd=wt)
        return wt

    def news_lines(self):
        return [l for b, l in self.typed() if b == "blk1"]

    def test_after_accept_a_ready_task_is_rebased(self):
        tid, _ = self.ready_task()
        wt = self.second_ready("two.txt", "two\n")
        self.bp("accept", tid)
        t = self.show("BP-2")
        self.assertEqual(t["status"], "ready")
        self.git("merge-base", "--is-ancestor", "main", "HEAD", cwd=wt)
        self.assertEqual(self.git("log", "--format=%s", "-1", cwd=wt), "Second")
        self.assertEqual(t["rebased"], "main")  # the board says "Rebased onto main."
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 rebased.")
        self.bp("ready", "BP-2", "--summary", "again", cwd=wt)
        self.assertNotIn("rebased", self.show("BP-2"))  # said once, until its next ready

    def test_after_accept_a_conflict_goes_back_with_the_branch_untouched(self):
        tid, _ = self.ready_task()
        wt = self.second_ready("fix.txt", "fixed differently\n")
        head = self.git("rev-parse", "HEAD", cwd=wt)
        self.bp("accept", tid)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)
        self.assertEqual(self.git("status", "--porcelain", cwd=wt), "")
        rebase = self.git("rev-parse", "--path-format=absolute", "--git-path", "rebase-merge", cwd=wt)
        self.assertFalse(Path(rebase).exists())
        t = self.show("BP-2")
        self.assertEqual(t["status"], "working")
        block, line = self.typed()[-2]
        self.assertEqual(block, t["block"])
        self.assertIn("conflicts in fix.txt", line)
        self.assertIn("rebase your branch onto main", line)
        self.assertNotIn("fetch", line)
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · After BP-1: BP-2 conflicts in fix.txt, sent back.")

    def test_after_accept_a_conflict_that_cant_be_sent_leaves_the_task(self):
        tid, _ = self.ready_task()
        self.second_ready("fix.txt", "fixed differently\n")
        (self.fake / f"send_fails_{self.show('BP-2')['block']}").write_text("")
        self.bp("accept", tid)
        self.assertEqual(self.show("BP-2")["status"], "ready")
        line = self.news_lines()[-1]
        self.assertIn("BP-2 conflicts in fix.txt, but the message didn't reach its worker", line)
        self.assertIn("`bullpen tell BP-2` it to rebase onto main and fix them", line)

    def test_after_accept_a_conflict_resumes_a_worker_that_exited(self):
        tid, _ = self.ready_task()
        self.second_ready("fix.txt", "fixed differently\n")
        (self.fake / "alive" / self.show("BP-2")["block"]).unlink()
        self.bp("accept", tid)
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertIn("--continue", cmd)
        self.assertTrue(any("conflicts in fix.txt" in x for x in cmd))
        self.assertEqual(self.show("BP-2")["status"], "working")

    def test_after_accept_a_dirty_worktree_is_skipped(self):
        tid, _ = self.ready_task()
        wt = self.second_ready("two.txt", "two\n")
        head = self.git("rev-parse", "HEAD", cwd=wt)
        (Path(wt) / "scratch.txt").write_text("x\n")
        self.bp("accept", tid)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)
        self.assertEqual(self.show("BP-2")["status"], "ready")
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · After BP-1: BP-2 has uncommitted changes, not rebased.")

    def test_after_accept_several_tasks_in_one_line(self):
        tid, _ = self.ready_task()
        self.second_ready("fix.txt", "fixed differently\n")
        self.bp("add", "Third", "--brief", "x")
        self.bp("start", "BP-3")  # working, its pane up
        self.bp("accept", tid)
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 conflicts in fix.txt, "
                         "sent back; BP-3 told to rebase.")

    def test_after_accept_a_worker_waiting_on_a_prompt_is_not_rebased(self):
        tid, _ = self.ready_task()
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        t = self.show("BP-2")
        self.commit(t["worktree"], "two.txt", "two\n", "Second")
        head = self.git("rev-parse", "HEAD", cwd=t["worktree"])
        self.bp("_event", t["block"], "blocked", "permission", "Allow rm?")
        self.bp("accept", tid)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=t["worktree"]), head)
        self.assertEqual(self.show("BP-2")["status"], "needs")
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · After BP-1: BP-2 waiting on the person, not rebased.")

    def test_after_accept_a_task_whose_reviewer_is_stuck_is_left_alone(self):
        tid, _ = self.ready_task()
        self.bp("config", "--review", "on")
        wt = self.second_ready("fix.txt", "fixed differently\n")  # would conflict
        head = self.git("rev-parse", "HEAD", cwd=wt)
        reviewer = self.show("BP-2")["review"]["block"]
        self.bp("_event", reviewer, "blocked", "permission", "Allow rm?")
        self.assertEqual(self.show("BP-2")["status"], "needs")
        self.bp("accept", tid)
        t = self.show("BP-2")
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)
        self.assertEqual(t["status"], "needs")
        self.assertEqual(t["review"]["block"], reviewer)
        self.assertTrue((self.fake / "alive" / reviewer).exists())
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 left in review.")

    def test_rebase_others_off(self):
        tid, _ = self.ready_task()
        wt = self.second_ready("two.txt", "two\n")
        head = self.git("rev-parse", "HEAD", cwd=wt)
        cfg = next((self.tmp / "state").rglob("config.json"))
        cfg.write_text(json.dumps({**json.loads(cfg.read_text()), "rebase_others": False}))
        self.bp("accept", tid)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)
        self.assertEqual(self.news_lines()[-1], "bullpen · BP-1 accepted (merged into main, rebase)")


class HelpersTest(unittest.TestCase):
    @classmethod
    def cli(cls):
        """bin/bullpen as a module, to call its functions directly."""
        import importlib.util
        from importlib.machinery import SourceFileLoader
        loader = SourceFileLoader("bullpen_cli", str(BULLPEN))
        spec = importlib.util.spec_from_loader("bullpen_cli", loader)
        mod = importlib.util.module_from_spec(spec)
        loader.exec_module(mod)
        return mod

    @classmethod
    def setUpClass(cls):
        cls.bp = cls.cli()

    # ---------- help and ls ----------

    def test_every_command_and_option_has_help_but_the_internal_ones(self):
        sub = next(a for a in self.bp.parser()._actions if a.choices)
        listed = {a.dest for a in sub._choices_actions}
        for name, p in sub.choices.items():
            self.assertEqual(name in listed, not name.startswith("_"), name)
            if name.startswith("_"):
                continue
            for a in p._actions:
                if a.dest != "help" and a.help != self.bp.argparse.SUPPRESS:
                    self.assertTrue(a.help, f"{name} {a.option_strings or a.dest}")

    def test_status_words_are_the_boards(self):
        w = self.bp.status_words
        self.assertEqual(w("needs"), "needs you")
        self.assertEqual(w("stopped"), "needs you (stopped)")
        self.assertEqual(w("triage"), "coordinator on it (triage)")
        self.assertEqual(w("review"), "in review")
        self.assertEqual(w("ready"), "ready to accept")
        self.assertEqual(w("todo"), "queued (todo)")
        self.assertEqual(w("dropped"), "closed (dropped)")
        self.assertEqual(set(self.bp.BOARD_WORDS), {"todo", "working", "review", "ready", "finished",
                                                   "triage", "needs", "stopped", "done", "dropped"})

    # ---------- the task lifecycle ----------

    def stopped(self, status, by, kind, **more):
        return {"status": status, "stop": {"by": by, "kind": kind}, **more}

    def test_whether_a_worker_is_between_turns_goes_by_its_stop(self):
        idle = self.bp.worker_idle
        self.assertTrue(idle({"status": "working"}))
        self.assertTrue(idle(self.stopped("triage", "worker", "ended")))
        self.assertTrue(idle(self.stopped("triage", "worker", "not_sent")))
        self.assertTrue(idle(self.stopped("triage", "reviewer", "ended")))
        self.assertTrue(idle(self.stopped("needs", "reviewer", "prompt")))
        self.assertFalse(idle(self.stopped("needs", "worker", "prompt")))
        self.assertFalse(idle(self.stopped("triage", "worker", "failed")))
        self.assertFalse(idle(self.stopped("triage", "reviewer", "limit")))
        # the person asked about it: the question is its why, the stop still counts
        self.assertFalse(idle(self.stopped("needs", "worker", "prompt", why="Delete it?", ask=True)))
        self.assertTrue(idle({"status": "needs", "why": "Delete it?", "ask": True}))
        self.assertTrue(idle(self.stopped("stopped", "worker", "exited")))

    def test_what_a_status_change_tells_the_person(self):
        n = self.bp.notice
        t = lambda status, **more: {"id": "BP-3", "title": "Fix it", "status": status, **more}
        self.assertEqual(n(t("needs", why="Needs you (permission): Allow rm?"), t("working")),
                         ("needs", "BP-3 needs you (permission): Allow rm?"))
        self.assertEqual(n(t("needs", why="Reviewer needs you (input)"), t("review")),
                         ("needs", "BP-3's reviewer needs you (input)"))
        self.assertEqual(n(t("needs", why="Failed: boom (coordinator didn't resolve it)"), t("triage")),
                         ("needs", "BP-3 needs you: Failed: boom (coordinator didn't resolve it)"))
        prompt = t("needs", why="Needs you (permission): Allow rm?")
        self.assertIsNone(n(prompt, prompt))  # no change
        asked = t("needs", why="Use SQLite?", ask=True)
        self.assertEqual(n(asked, prompt), ("needs", "BP-3 needs you: Use SQLite?"))  # asked at a prompt
        self.assertIsNone(n(asked, asked))
        self.assertEqual(n(t("needs", why="Or Postgres?", ask=True), asked), ("needs", "BP-3 needs you: Or Postgres?"))
        self.assertIsNone(n(t("triage", why="Ended its turn without saying ready"), t("working")))
        self.assertEqual(n(t("ready"), t("review")), ("news", "BP-3 is ready: Fix it"))
        self.assertIsNone(n(t("ready"), t("ready")))
        self.assertEqual(n(t("finished"), t("working")), ("news", "BP-3 finished: Fix it"))
        self.assertEqual(n(t("done", auto_accepted=True), t("ready")), ("news", "BP-3 was auto-accepted: Fix it"))
        self.assertIsNone(n(t("done", auto_accepted=False), t("ready")))  # the person accepted it
        self.assertIsNone(n(t("working"), {}))

    def test_a_task_is_on_its_reviewer_by_its_stop(self):
        on = self.bp.on_reviewer
        self.assertTrue(on(self.stopped("needs", "reviewer", "prompt")))
        self.assertTrue(on(self.stopped("triage", "reviewer", "exited")))
        self.assertTrue(on(self.stopped("needs", "reviewer", "ended", why="Delete it?", ask=True)))
        self.assertFalse(on(self.stopped("triage", "reviewer", "limit")))  # seen to as its worker's
        self.assertFalse(on(self.stopped("triage", "worker", "ended")))
        self.assertFalse(on({"status": "review"}))
        t = self.stopped("needs", "reviewer", "ended", review={"block": "blk2", "session": "s"}, block="blk1")
        self.assertEqual(self.bp.waited_on(t), ("reviewer", "s", "blk2"))
        t["stop"] = {"by": "worker", "kind": "failed"}
        self.assertEqual(self.bp.waited_on(t), ("worker", None, "blk1"))

    def test_a_new_status_ends_ask_ok_and_the_stop(self):
        t = self.stopped("triage", "worker", "ended", why="Ended its turn without saying ready", ok=True)
        self.bp.ask(t, "Delete it?")
        self.assertEqual((t["status"], t["why"], t["ask"], t["stop"]),
                         ("needs", "Delete it?", True, {"by": "worker", "kind": "ended"}))
        self.assertNotIn("ok", t)
        self.bp.ask(t, "Or keep it?")  # asked again: still asked
        self.assertTrue(t["ask"])
        self.bp.set_status(t, "working")
        self.assertEqual((t["status"], t["why"]), ("working", None))
        self.assertFalse({"ask", "ok", "stop"} & set(t))
        t = self.stopped("triage", "reviewer", "ended")
        self.bp.ok(t)
        self.assertEqual((t["status"], t["ok"]), ("review", True))
        self.assertNotIn("stop", t)
        t = self.stopped("triage", "worker", "failed")
        self.bp.ok(t)
        self.assertEqual(t["status"], "working")

    def test_old_whys_say_the_stop(self):
        f = self.bp.stop_from_why
        self.assertEqual(f("Needs you (permission): Allow?"), {"by": "worker", "kind": "prompt"})
        self.assertEqual(f("Reviewer needs you (input)"), {"by": "reviewer", "kind": "prompt"})
        self.assertEqual(f("Failed (coordinator didn't resolve it)"), {"by": "worker", "kind": "failed"})
        self.assertEqual(f("Reviewer exited without a verdict"), {"by": "reviewer", "kind": "exited"})
        self.assertEqual(f("Its pane was gone"), {"by": "worker", "kind": "pane_gone"})
        self.assertEqual(f("Couldn't be continued after the usage limit: x"), {"by": "worker", "kind": "limit"})
        self.assertEqual(f("Something else"), {"by": "worker", "kind": "other"})
        self.assertIsNone(f(None))

    def test_a_local_task_is_on_a_local_branch(self):
        self.assertEqual(self.bp.where({"status": "ready", "pr": None, "local": True}), "local branch")

    def test_a_working_task_only_shows_what_github_sent_it_back_for(self):
        gh = {"state": "open", "checks": "passing", "new": 0}
        self.assertEqual(self.bp.pr_phrase({"status": "working", "gh": gh}), "")
        self.assertEqual(self.bp.pr_phrase({"status": "working", "gh": {**gh, "sent": True}}), "checks passing")
        self.assertEqual(self.bp.pr_phrase({"status": "ready", "gh": gh}), "checks passing")

    def test_only_the_agent_a_task_waits_on_is_stuck_or_limited(self):
        import datetime as dt
        ago = (dt.datetime.now().astimezone() - dt.timedelta(minutes=25)).isoformat()
        t = {"status": "working", "stuck": {"who": "worker", "since": ago}}
        self.assertTrue(self.bp.is_stuck(t))
        self.assertEqual(self.bp.stuck_phrase(t), "no progress for 25 min")
        self.assertFalse(self.bp.is_stuck({**t, "status": "review"}))
        self.assertFalse(self.bp.is_stuck({"status": "working"}))
        at = dt.datetime(2026, 11, 11, 13, 0).astimezone()
        lim = {"who": "worker", "resets": at.replace(hour=15).isoformat(), "text": "x"}
        self.assertTrue(self.bp.is_limited({"status": "working", "limit": lim}))
        self.assertFalse(self.bp.is_limited({"status": "ready", "limit": lim}))
        self.assertEqual(self.bp.limit_why(lim, at), "Usage limit, resets 3:00 PM")

    def test_reset_times_are_read(self):
        import datetime as dt
        at = dt.datetime(2026, 11, 11, 13, 0).astimezone()  # a Wednesday, 1 PM
        when = lambda text: self.bp.limit_reset(text, at)
        day = lambda d, h, m=0: dt.datetime(2026, 11, d, h, m).astimezone()
        self.assertEqual(when("You've hit your 5-hour limit · resets 3pm"), day(11, 15))
        self.assertEqual(when("You've hit your session limit · resets 3:45pm"), day(11, 15, 45))
        self.assertEqual(when("Resets at 3:10 PM"), day(11, 15, 10))
        self.assertEqual(when("resets 15:00"), day(11, 15))
        self.assertEqual(when("You've hit your Opus limit · resets 9am"), day(12, 9))  # tomorrow
        self.assertEqual(when("resets 12:58pm"), day(11, 12, 58))  # just gone: now, not tomorrow
        self.assertEqual(when("You've hit your weekly limit · resets Mon 12:00am"), day(16, 0))
        self.assertEqual(when("weekly limit · resets Nov 14, 3pm · progress saved"), day(14, 15))
        self.assertEqual(when("Claude AI usage limit reached|1794430800"),
                         dt.datetime.fromtimestamp(1794430800).astimezone())
        ny = when("Claude usage limit reached. Your limit will reset at 3pm (America/New_York).")
        self.assertEqual((ny.utcoffset(), ny.astimezone(dt.timezone.utc).hour),
                         (at.utcoffset(), 20))  # 3 PM in New York, told in local time
        for text in ("Usage limit reached", "spend limit reached · resets Aug 8", "resets in 8 minutes",
                     "resets 3", "resets 13pm", "resets 3pm (Nowhere/Land)"):
            self.assertIsNone(when(text), text)
        self.assertEqual(self.bp.clock(day(11, 15), at), "3:00 PM")
        self.assertEqual(self.bp.clock(day(16, 0), at), "Mon 12:00 AM")
        self.assertEqual(self.bp.clock(day(30, 9, 5), at), "Nov 30 9:05 AM")



# Drives lua/stuck.lua with made-up times: T counts minutes.
STUCK_LUA = """
local q = dofile(rex.args.lib).new()
local limits = { a = 20, b = 20, off = 0 }
local function limit(block) return limits[block] end
local function due(minute)
  local out = {}
  for _, p in ipairs(q.due(minute * 60, limit)) do out[#out + 1] = p.block .. "=" .. p.minutes end
  table.sort(out)
  return table.concat(out, ",")
end
local got = {}
local function check(name, value, want)
  if value ~= want then got[#got + 1] = name .. ": " .. tostring(value) .. ", not " .. tostring(want) end
end
check("first event", q.event("a", true, 0), false)
q.event("b", false, 0)       -- not working: never due
q.event("off", true, 0)      -- its repo turned this off
check("too early", due(19), "")
check("due at the limit", due(20), "a=20")
check("once per spell", due(45), "")
check("the next event ends the spell", q.event("a", true, 46 * 60), true)
check("and only that one", q.event("a", true, 47 * 60), false)
check("counted from the last event", due(66), "")
check("a new spell", due(68), "a=21")
q.event("b", true, 60 * 60)
q.forget("b")
check("a pane that's gone", due(200), "")
limits.off = 30
check("turned on later", due(201), "off=201")
return #got == 0 and "ok" or table.concat(got, "; ")
"""

# Runs lua/watch.lua against a stand-in for Rex's `rex` table and a clock the
# script sets, and returns the bullpen commands it called.
WATCH_LUA = """
local args = rex.args
local clock, handlers, timers, calls = 0, {}, {}, {}
os.time = function() return clock end
rex = {
  args = { bin = "bullpen", state = args.state },
  on = function(name, fn) handlers[name] = fn end,
  every = function(seconds, fn) timers[seconds] = fn end,
  after = function(seconds, fn) fn() end,
  exec = function(argv)
    calls[#calls + 1] = table.concat(argv, " ", 4)
    return { ok = true }
  end,
}
print = function() end
dofile(args.watch)
local function status(block, state, msg, minute)
  clock = minute * 60
  handlers["terminal.program_status_changed"]({ block_id = block }, { id = "", record = { state = state, msg = msg } })
end
local function tick(minute)
  clock = minute * 60
  timers[60]()
end
status("blk1", "working", "Reading", 0)
status("blk1", "working", "Testing", 5)   -- dropped, but a sign of life
status("other", "working", "", 0)         -- not bullpen's pane
status("blk2", "blocked", "Allow?", 0)    -- waiting on the person, not stuck
tick(24)
tick(25)
tick(40)
status("blk1", "working", "Testing", 41)  -- repeats are dropped, but not this one
status("blk1", "working", "Testing", 42)
tick(61)
tick(62)
handlers["block_closed"]({}, { block_id = "blk1" })
tick(200)
timers[30]()                              -- no limit noted: nothing to call
local f = io.open(args.state .. "/limit.json", "w") f:write("{}") f:close()
timers[30]()
timers[180]()                             -- status drift, every 3 minutes
return table.concat(calls, "|")
"""


@unittest.skipUnless(shutil.which("rex"), "needs Rex, to run Lua")
class WatcherTest(unittest.TestCase):
    def lua(self, script, **args):
        if subprocess.run(["rex", "do", "-e", "return 1"], capture_output=True).returncode != 0:
            self.skipTest("rex do doesn't run here")
        tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, tmp, ignore_errors=True)
        path = Path(tmp) / "test.lua"
        path.write_text(script)
        p = subprocess.run(["rex", "do", str(path), *(f"{k}={v}" for k, v in args.items())],
                           text=True, capture_output=True)
        self.assertEqual(p.returncode, 0, p.stderr)
        return json.loads(p.stdout)

    def test_a_quiet_spell_is_due_once(self):
        self.assertEqual(self.lua(STUCK_LUA, lib=ROOT / "lua" / "stuck.lua"), "ok")

    def test_the_watcher_passes_working_events_on_while_compacting(self):
        state = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, state, ignore_errors=True)
        (state / "blocks").mkdir()
        entry = state / "blocks" / "blk1.json"
        entry.write_text(json.dumps({"repo": "/r", "role": "BP-1", "stuck_after": 20}, indent=2))
        script = WATCH_LUA.split("status(\"blk1\", \"working\", \"Reading\", 0)")[0] + """
status("blk1", "working", "", 0)
status("blk1", "working", "Dropped", 1)
local f = io.open(args.state .. "/blocks/blk1.json", "w")
f:write('{"repo": "/r", "role": "BP-1", "compacting": true}') f:close()
status("blk1", "working", "Passed on", 2)
return table.concat(calls, "|")
"""
        calls = self.lua(script, watch=ROOT / "lua" / "watch.lua", state=state).split("|")
        self.assertEqual(calls, ["_event blk1 working  ", "_event blk1 working  Passed on"])

    def test_the_watcher_reports_and_clears_a_quiet_pane(self):
        state = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, state, ignore_errors=True)
        (state / "blocks").mkdir()
        for block, minutes in (("blk1", 20), ("blk2", 20)):
            (state / "blocks" / f"{block}.json").write_text(
                json.dumps({"repo": "/r", "role": "BP-1", "stuck_after": minutes}, indent=2))
        calls = self.lua(WATCH_LUA, watch=ROOT / "lua" / "watch.lua", state=state).split("|")
        self.assertEqual(calls, [
            "_event blk1 working  Reading",
            "_event blk2 blocked  Allow?",
            "_stuck blk1 20",              # at minute 25: 20 after its last event
            "_event blk1 working  Testing",  # the event that ends the spell
            "_stuck blk1 20",              # quiet again from minute 42
            "_gone blk1",
            "_limit",                      # only while <state>/limit.json exists
            "check --fix --quiet --all-repos",
        ])


if __name__ == "__main__":
    unittest.main()
