"""End-to-end tests for bin/bullpen against fake `rex` and `gh` and a local origin.

Run: python3 -m unittest discover -s tests
"""

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
        screen = d / "screen"  # what the tests put on screen, else two lines
        print(screen.read_text() if screen.exists() else "line one\\nline two\\n\\n")
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
            print(json.dumps({"session_id": sid, "label": "main", "windows": windows}))
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


class BullpenTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
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
        self.assertEqual(splits[0][-1], "board")
        watcher = watchers[0]["options"]["command"]
        self.assertIn("--all", watcher)
        self.assertTrue(any(x.startswith("bin=") for x in watcher))

    def test_up_here_registers_this_pane(self):
        self.bp("up", "--here")
        self.assertFalse(any(a[0] == "run" and "bullpen: proj" in a for a in self.rex_calls()))
        cfg = json.loads(self.bp("config").stdout)
        self.assertEqual(cfg["coordinator"], {"block": "me", "session": "session:1"})

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
        self.assertTrue(news[0].startswith("bullpen · BP-1 needs you (permission)"))
        self.assertIn("Allow rm?", news[0])
        self.assertEqual(self.show(tid)["status"], "needs")
        self.bp("_event", self.block(tid), "working", "", "")
        self.assertEqual(self.show(tid)["status"], "working")
        self.bp("_event", self.block(tid), "done", "", "")
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertIn("ended its turn without `ready`", news[-1])

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
            "bullpen · BP-1 is ready (1 commit, PR #7), approved by review: Fix the thing"))

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
        self.assertIn("reviewer needs you", self.typed()[-1][1])
        self.bp("_event", reviewer, "working", "", "")
        self.assertEqual(self.show(tid)["status"], "review")
        self.bp("_event", reviewer, "done", "", "")
        self.assertEqual(self.show(tid)["why"], "Reviewer stopped without a verdict")
        self.bp("_gone", reviewer)
        self.assertEqual(self.show(tid)["status"], "needs")

    def news(self):
        return [l for b, l in self.typed() if b == "blk1"]

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
        self.assertEqual(t["status"], "needs")
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
        self.assertIn("BP-1 needs you (permission)", self.news()[-1])
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
        self.assertEqual(t["status"], "needs")  # an ordinary end of turn
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
        self.bp("note", tid, "Asks:", "python3 or python?")
        self.assertEqual(self.show(tid)["note"], "Asks: python3 or python?")
        self.assertEqual(self.show(tid)["status"], "needs")
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

    # ---------- news by monitor ----------

    def ask(self, tid, msg):
        """One news line: the worker asks something."""
        self.bp("_event", self.block(tid), "blocked", "question", msg)

    def follow(self):
        """Start `bullpen news --follow`; returns it and a function reading its next n lines."""
        p = subprocess.Popen([str(BULLPEN), "news", "--follow", "--every", "0.05"], cwd=self.repo,
                             env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
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
        self.assertEqual(lines(1), ["bullpen · BP-1 needs you (question): first"])
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
        p = self.bp("news", ok=False)  # a one-off read doesn't wait
        self.assertIn("followed already", p.stderr)
        self.stop(first)
        self.ask(tid, "three")
        self.assertEqual([l.rsplit(" ", 1)[-1] for l in lines2(1)], ["three"])

    def test_news_prints_the_lines_since_last_time_and_exits(self):
        self.assertEqual(self.bp("news").stdout, "")  # no news.log yet
        tid = self.start_task()
        self.ask(tid, "one")
        self.ask(tid, "two")
        out = self.bp("news").stdout.splitlines()
        self.assertEqual(out, ["bullpen · BP-1 needs you (question): one",
                               "bullpen · BP-1 needs you (question): two"])
        self.assertEqual(self.bp("news").stdout, "")
        self.ask(tid, "three")
        self.assertEqual(self.bp("news").stdout, "bullpen · BP-1 needs you (question): three\n")

    def test_news_never_replays_what_was_logged_before_it_first_ran(self):
        tid = self.start_task()
        self.ask(tid, "old")  # typed, in the default mode
        self.assertEqual(self.bp("news").stdout, "")
        self.ask(tid, "older still")
        self.bp("news")
        self.ask(tid, "typed too")
        self.bp("config", "--news", "monitor")  # switching starts from here
        self.assertEqual(self.bp("news").stdout, "")
        self.ask(tid, "new")
        self.assertEqual(self.bp("news").stdout, "bullpen · BP-1 needs you (question): new\n")

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
        self.assertEqual(list(state.rglob("outbox.txt")), [])
        log = next(state.rglob("news.log")).read_text().splitlines()
        self.assertEqual(len(log), 2)
        self.assertIn("BP-1 needs-you: bullpen · BP-1 needs you (question): Which one?", log[0])

    def test_type_is_the_default_and_can_be_set_back(self):
        tid = self.start_task()
        self.assertEqual(json.loads(self.bp("config").stdout)["news"], "type")
        self.bp("config", "--news", "monitor")
        self.bp("config", "--news", "type")
        self.ask(tid, "Which one?")
        self.assertEqual([l for b, l in self.typed() if b == "blk1"][1:],
                         ["bullpen · BP-1 needs you (question): Which one?"])

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
        self.assertEqual(self.bp("news").stdout, "")

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
        self.assertEqual(list((self.tmp / "state").rglob("outbox.txt")), [])
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
        self.assertEqual(cfg["coordinator"]["block"], "blk6")
        self.assertEqual(cfg["board"]["block"], "blk7")
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
        self.assertEqual(cfg["coordinator"], {"block": "blk5", "session": "session:2"})
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
        self.assertEqual(line, "bullpen · BP-1 needs you (question): Which one? A or B")

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

    def detail(self, tid):
        """The board's sentence for the task."""
        return BoardTest.cli().task_detail(self.show(tid))

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
        self.assertEqual(self.detail(tid), "Fixing what GitHub said. Checks failing.")
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
        self.assertEqual(self.detail(tid), "Fixing what review round 1 found.")
        self.assertNotIn("github:", self.bp("show", tid).stdout)

    def test_review_changes_after_a_github_send_back_say_review(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        self.assertEqual(self.detail(tid), "Fixing what GitHub said. Checks failing.")
        self.commit_and_ready(tid, 2)
        self.verdict(tid, "changes", "The fix misses the empty case.")
        self.assertEqual(self.detail(tid), "Fixing what review round 1 found.")

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

    def test_feedback_for_a_worker_that_is_gone_needs_you(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        (self.fake / "alive" / worker).unlink()
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        self.assertFalse(any(b == worker for b, _ in self.typed()))
        t = self.show(tid)
        self.assertEqual(t["status"], "needs")
        self.assertEqual(t["why"], "PR checks failed: test, ci/build")
        self.assertIn("`bullpen resume BP-1` restarts it", self.news_lines()[-1])

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

    def test_other_agents_commands_are_left_alone(self):
        self.set_commands(worker=["codex", "--permission-mode", "auto"])
        cfg = json.loads(self.bp("config", "--model", "opus").stdout)
        self.assertIn("not claude", cfg["models"]["worker"])
        self.assertEqual(cfg["models"]["reviewer"], "opus")
        tid = self.start_task()
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertNotIn("--model", cmd)
        self.assertIsNone(self.show(tid).get("model"))


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
        self.assertIn(f"log --oneline main..HEAD", prompt)
        self.bp("verdict", tid, "approve", "--summary", "Fine.")
        self.assertEqual(self.typed()[-1][1],
                         "bullpen · BP-1 is ready (1 commit, local branch), approved by review: Fix the thing")

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


class BoardTest(unittest.TestCase):
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

    def test_groups_follow_what_you_do_next(self):
        tasks = [{"id": f"BP-{i}", "status": st} for i, st in enumerate(
            ["todo", "working", "ready", "needs", "stopped", "review", "done"], 1)]
        groups = [(h, [t["id"] for t in ts]) for h, ts in self.bp.board_groups(tasks)]
        self.assertEqual(groups, [("Needs you", ["BP-4", "BP-5"]), ("Ready to accept", ["BP-3"]),
                                  ("In review", ["BP-6"]), ("Working", ["BP-2"]), ("Queued", ["BP-1"])])
        self.assertEqual(self.bp.board_groups(tasks, finished=True)[-1][0], "Finished")

    def test_detail_says_what_a_task_needs(self):
        d = self.bp.task_detail
        self.assertEqual(d({"status": "needs", "why": "Ended its turn without saying ready",
                            "note": "Asks: python3 or python?"}), "Asks: python3 or python?")
        self.assertIn("probably with a question", d({"status": "needs", "why": "Ended its turn without saying ready"}))
        self.assertEqual(d({"status": "review", "review": {"round": 2}}), "Reviewer on round 2 of 2.")
        self.assertEqual(d({"status": "ready", "pr": "https://x/pull/4", "review": {"verdict": "approved"}}),
                         "PR #4, approved by review.")
        self.assertEqual(d({"status": "working", "review": {"verdict": "changes", "round": 1}}),
                         "Fixing what review round 1 found.")
        self.assertEqual(d({"status": "ready", "pr": "https://x/pull/4", "gh": {"checks": "failing", "new": 1}}),
                         "PR #4. Checks failing, 1 new comment.")
        self.assertEqual(d({"status": "working", "gh": {"checks": "failing", "sent": True}}),
                         "Fixing what GitHub said. Checks failing.")
        self.assertEqual(d({"status": "ready", "pr": "https://x/pull/4", "gh": {"state": "merged"}}),
                         "PR #4. Merged on GitHub.")

    def test_a_working_task_says_who_sent_it_back(self):
        d = self.bp.task_detail
        gh = {"state": "open", "checks": "passing", "new": 0}
        # a reviewer's changes, after the watcher recorded the old push's checks
        self.assertEqual(d({"status": "working", "pr": "https://x/pull/4",
                            "review": {"round": 1, "verdict": "changes"}, "gh": gh}),
                         "Fixing what review round 1 found.")
        # GitHub's, even when an earlier review round asked for changes
        self.assertEqual(d({"status": "working", "pr": "https://x/pull/4",
                            "review": {"round": 1, "verdict": "changes"},
                            "gh": {"state": "open", "checks": "failing", "new": 1, "sent": True}}),
                         "Fixing what GitHub said. Checks failing, 1 new comment.")
        # resumed or told by the person: nothing about the old push
        self.assertEqual(d({"status": "working", "pr": "https://x/pull/4", "gh": gh}), "")
        self.assertEqual(self.bp.pr_phrase({"status": "working", "gh": gh}), "")
        self.assertEqual(self.bp.pr_phrase({"status": "ready", "gh": gh}), "checks passing")

    def test_local_tasks_show_their_branch(self):
        d = self.bp.task_detail
        t = {"status": "ready", "pr": None, "local": True, "branch": "bullpen/bp-2-x", "base": "main",
             "review": {"verdict": "approved"}}
        self.assertEqual(d(t), "Local branch bullpen/bp-2-x, approved by review.")
        self.assertEqual(d({**t, "review": None}), "Local branch bullpen/bp-2-x.")
        self.assertEqual(d({**t, "status": "done", "merged": "abc"}), "Accepted. Merged into main.")
        self.assertEqual(self.bp.confirm_text("accept", t, "squash"), "Accept and squash-merge into main?")
        self.assertEqual(self.bp.confirm_text("drop", t), "Drop it?")
        self.assertEqual(self.bp.where(t), "local branch")

    def test_keys_fit_the_task(self):
        self.assertTrue(self.bp.task_keys({"status": "ready"}).startswith("a accept"))
        self.assertTrue(self.bp.task_keys({"status": "needs"}).startswith("⏎ answer"))
        self.assertEqual(self.bp.task_keys(None), "")
        self.assertTrue(self.bp.task_keys({"status": "working", "worktree": "/w"}).endswith("v diff"))
        self.assertNotIn("v diff", self.bp.task_keys({"status": "todo"}))
        self.assertNotIn("v diff", self.bp.task_keys({"status": "done", "worktree": "/w"}))

    def test_stat_shows_for_review_and_ready(self):
        t = {"status": "ready", "diffstat": "3 files, +120 −14"}
        self.assertEqual(self.bp.task_stat(t), "3 files, +120 −14")
        self.assertEqual(self.bp.task_stat({**t, "status": "review"}), "3 files, +120 −14")
        self.assertEqual(self.bp.task_stat({**t, "status": "working"}), "")
        self.assertEqual(self.bp.task_stat({"status": "ready"}), "")

    def test_a_stuck_task_says_so(self):
        import datetime as dt
        ago = lambda m: (dt.datetime.now().astimezone() - dt.timedelta(minutes=m)).isoformat()
        d = self.bp.task_detail
        t = {"status": "working", "stuck": {"who": "worker", "since": ago(25)}}
        self.assertTrue(self.bp.is_stuck(t))
        self.assertEqual(d(t), "No progress for 25 min. p shows its screen.")
        self.assertEqual(d({**t, "note": "Hung on npm install"}), "Hung on npm install")
        self.assertEqual(d({"status": "review", "review": {"round": 1},
                            "stuck": {"who": "reviewer", "since": ago(190)}}),
                         "Reviewer has shown no progress for 3 h. ⏎ opens it.")
        # a worker's quiet spell says nothing about a task that has moved on
        moved = {"status": "review", "review": {"round": 1}, "stuck": {"who": "worker", "since": ago(25)}}
        self.assertFalse(self.bp.is_stuck(moved))
        self.assertEqual(d(moved), "Reviewer on round 1 of 2.")
        self.assertFalse(self.bp.is_stuck({"status": "working"}))

    def test_a_limited_task_says_when_it_resets(self):
        import datetime as dt
        at = dt.datetime(2026, 11, 11, 13, 0).astimezone()
        d = lambda t: self.bp.limit_detail(t, at)
        lim = {"who": "worker", "resets": at.replace(hour=15).isoformat(), "text": "x"}
        t = {"status": "working", "limit": lim}
        self.assertTrue(self.bp.is_limited(t))
        self.assertEqual(d(t), "Usage limit, resets 3:00 PM. Continues by itself then.")
        self.assertEqual(self.bp.limit_why(lim, at), "Usage limit, resets 3:00 PM")
        self.assertEqual(d({"status": "working", "limit": {**lim, "resets": None}}),
                         "Usage limit, reset time unknown. t tells it to continue.")
        self.assertEqual(d({"status": "review", "limit": {**lim, "who": "reviewer"}}),
                         "Reviewer stopped at the usage limit, resets 3:00 PM. Continues by itself then.")
        self.assertIn("Usage limit, resets", self.bp.task_detail(t))
        # a worker's limit says nothing about a task that has moved on
        self.assertFalse(self.bp.is_limited({"status": "ready", "limit": lim}))

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

    def test_fit_and_age(self):
        import datetime as dt
        self.assertEqual(self.bp.fit("Add a done command", 10), "Add a don…")
        at = dt.datetime(2026, 1, 1, 12, 0, tzinfo=dt.timezone.utc)
        self.assertEqual(self.bp.ago("2026-01-01T11:55:00+00:00", at), "5m")
        self.assertEqual(self.bp.ago("2026-01-01T09:55:00+00:00", at), "2h")


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
return table.concat(calls, "|")
"""


@unittest.skipUnless(shutil.which("rex"), "needs Rex, to run Lua")
class WatcherTest(unittest.TestCase):
    def lua(self, script, **args):
        if subprocess.run(["rex", "do", "-e", "return 1"], capture_output=True).returncode != 0:
            self.skipTest("rex do doesn't run here")
        path = Path(tempfile.mkdtemp()) / "test.lua"
        path.write_text(script)
        p = subprocess.run(["rex", "do", str(path), *(f"{k}={v}" for k, v in args.items())],
                           text=True, capture_output=True)
        self.assertEqual(p.returncode, 0, p.stderr)
        return json.loads(p.stdout)

    def test_a_quiet_spell_is_due_once(self):
        self.assertEqual(self.lua(STUCK_LUA, lib=ROOT / "lua" / "stuck.lua"), "ok")

    def test_the_watcher_reports_and_clears_a_quiet_pane(self):
        state = Path(tempfile.mkdtemp())
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
        ])


if __name__ == "__main__":
    unittest.main()
