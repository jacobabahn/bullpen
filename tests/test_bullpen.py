"""End-to-end tests for bin/bullpen against fake `rex` and `gh` and a local origin.

Run: python3 -m unittest discover -s tests
"""

import json
import os
import subprocess
import tempfile
import textwrap
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
    reg = d / "reg.json"  # blocks: {id: {label, layer}}; layers: {id: state}
    R = json.loads(reg.read_text()) if reg.exists() else {"blocks": {}, "layers": {}}

    def new_block(label, layer=None):
        n = len(R["blocks"]) + 1
        (alive / f"blk{n}").write_text("")
        R["blocks"][f"blk{n}"] = {"label": label, "layer": layer}
        return f"blk{n}"

    if a[0] in ("run", "split"):
        b = new_block(a[a.index("--label") + 1] if "--label" in a else None)
        print(json.dumps({"window_id": "w", "block_ids": [b]}))
    elif a[:2] == ["block", "inspect"]:
        sys.exit(0 if (alive / a[2]).exists() else 1)
    elif a[:2] == ["block", "close"]:
        (alive / a[2]).unlink(missing_ok=True)
    elif a[0] == "capture":
        print("line one\\nline two\\n\\n")
    elif a[:2] == ["api", "call"]:
        rest = a[2:]
        if rest[0] == "-s":
            rest = rest[2:]
        method, args = rest[0], json.loads(rest[1]) if len(rest) > 1 else {}
        if method == "session.new_layer":
            lid = f"lyr{len(R['layers']) + 1}"
            R["layers"][lid] = "normal"
            b = new_block(args["layout"]["block"]["label"], lid)
            print(json.dumps({"layer_id": lid, "block_ids": [b]}))
        elif method == "session.new_block":
            print(json.dumps({"block_id": new_block(args["label"], "detached")}))
        elif method == "session.set_layer_state":
            R["layers"][args["layer_id"]] = args["state"]
            print("{}")
        elif method == "session.view":
            tiled = {"layer_id": "tiled", "kind": "tiled", "blocks": []}
            layers = [tiled]
            for b, info in R["blocks"].items():
                if not (alive / b).exists() or info["layer"] == "detached":
                    continue
                entry = {"block_id": b, "label": info["label"]}
                if info["layer"]:
                    layers.append({"layer_id": info["layer"], "kind": "floating",
                                   "state": R["layers"][info["layer"]], "blocks": [entry]})
                else:
                    tiled["blocks"].append(entry)
            print(json.dumps({"session_id": "session:1", "label": "sess1",
                              "windows": [{"window_id": "w1", "layers": layers}]}))
    reg.write_text(json.dumps(R))
""")

FAKE_GH = textwrap.dedent("""\
    #!/usr/bin/env python3
    import json, os, sys
    from pathlib import Path
    d = Path(os.environ["FAKE_DIR"])
    body = sys.stdin.read() if "--body-file" in sys.argv else ""
    with open(d / "gh.log", "a") as f:
        f.write(json.dumps({"argv": sys.argv[1:], "body": body}) + "\\n")
    if sys.argv[1:3] == ["pr", "create"]:
        print("https://github.com/me/repo/pull/7")
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
                    "FAKE_DIR": str(fake), "REX_SESSION": "sess1", "REX_BLOCK": "me",
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

    def restart_rex(self):
        """Rex restarts: every placed pane comes back with a new ID, detached ones don't."""
        reg = self.fake / "reg.json"
        R = json.loads(reg.read_text())
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
        self.assertEqual(cfg["coordinator"], {"block": "me", "session": "sess1"})

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
        self.bp("ready", tid, "--summary", "done")
        self.bp("accept", tid)
        t = self.show(tid)
        self.assertEqual(t["status"], "done")
        self.assertFalse(Path(wt).exists())
        gh = [json.loads(l)["argv"] for l in (self.fake / "gh.log").read_text().splitlines()]
        self.assertEqual(gh[-1][:2], ["pr", "merge"])
        self.assertIn("--squash", gh[-1])
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
        p = self.bp("go", "--next", "--from", "elsewhere", ok=False)
        self.assertIn("nothing needs you", p.stderr)
        self.bp("_event", self.block(b), "blocked", "permission", "Allow?")
        self.bp("_event", self.block(a), "blocked", "question", "Which?")
        nxt = lambda frm: json.loads(self.bp("go", "--next", "--json", "--from", frm).stdout)["block"]
        first = nxt("elsewhere")
        self.assertEqual(first, self.block(b))  # waiting longest comes first
        self.assertEqual(nxt(first), self.block(a))
        self.assertEqual(nxt(self.block(a)), self.block(b))  # wraps around

    def test_go_finds_the_repo_from_any_directory(self):
        tid = self.start_task()
        out = self.bp("go", tid, "--json", "--from", "nowhere", cwd=self.tmp).stdout
        self.assertEqual(json.loads(out)["block"], self.block(tid))


class BoardTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import importlib.util
        from importlib.machinery import SourceFileLoader
        loader = SourceFileLoader("bullpen_cli", str(BULLPEN))
        spec = importlib.util.spec_from_loader("bullpen_cli", loader)
        cls.bp = importlib.util.module_from_spec(spec)
        loader.exec_module(cls.bp)

    def test_order_puts_what_needs_you_first(self):
        tasks = [{"id": f"BP-{i}", "status": s} for i, s in
                 enumerate(["todo", "working", "ready", "needs", "stopped", "needs"], 1)]
        order = [t["id"] for t in self.bp.board_order(tasks)]
        self.assertEqual(order, ["BP-4", "BP-6", "BP-5", "BP-3", "BP-2", "BP-1"])

    def test_row_shows_time_in_status_and_why(self):
        import datetime as dt
        at = dt.datetime(2026, 1, 1, 12, 0, tzinfo=dt.timezone.utc)
        t = {"id": "BP-3", "status": "needs", "title": "Fix login",
             "since": "2026-01-01T11:55:00+00:00", "why": "Needs you (permission)"}
        row = self.bp.board_row(t, 200, at)
        self.assertEqual(row, "BP-3    needs      5m  Fix login  · Needs you (permission)")
        t.update(status="ready", why=None, pr="https://github.com/me/repo/pull/12")
        self.assertTrue(self.bp.board_row(t, 200, at).endswith("· PR #12"))
        self.assertEqual(len(self.bp.board_row(t, 20, at)), 19)


if __name__ == "__main__":
    unittest.main()
