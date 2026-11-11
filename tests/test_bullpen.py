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
    if a[0] == "run":
        n = len(list(d.glob("block-*"))) + 1
        (d / f"block-{n}").write_text("")
        (alive / f"blk{n}").write_text("")
        print(json.dumps({"window_id": "w", "block_ids": [f"blk{n}"]}))
    elif a[:2] == ["block", "inspect"]:
        sys.exit(0 if (alive / a[2]).exists() else 1)
    elif a[:2] == ["block", "close"]:
        (alive / a[2]).unlink(missing_ok=True)
    elif a[0] == "capture":
        print("line one\\nline two\\n\\n")
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

    def typed(self):
        """Lines typed into panes with `rex send`, as (block, text)."""
        out = []
        for a in self.rex_calls():
            if a[0] == "send":
                out.append((a[a.index("-b") + 1], a[-1]))
        return out

    def show(self, tid):
        return json.loads(self.bp("show", tid, "--json").stdout)

    def start_task(self):
        self.bp("up")  # spawns the coordinator (blk1) and the watcher (blk2)
        tid = self.bp("add", "Fix the thing", "--brief", "Make it work.").stdout.strip()
        self.bp("start", tid)
        return tid

    def test_up_spawns_coordinator_and_one_watcher(self):
        self.bp("up")
        self.bp("up")
        runs = [a for a in self.rex_calls() if a[0] == "run"]
        labels = [a[a.index("--label") + 1] for a in runs]
        self.assertEqual(labels, ["bullpen: proj", "bullpen: watcher"])
        watcher = runs[1]
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
        self.assertEqual(t["block"], "blk3")
        run = [a for a in self.rex_calls() if a[0] == "run"][-1]
        self.assertEqual(run[run.index("-c") + 1], t["worktree"])
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
        self.assertIn('event="ready"', line)

    def test_ready_refuses_uncommitted_changes(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "dirty.txt").write_text("x\n")
        p = self.bp("ready", tid, cwd=wt, ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("uncommitted", p.stderr)

    def test_events_become_news_once(self):
        tid = self.start_task()
        self.bp("_event", "blk3", "working", "", "Reading files")
        self.bp("_event", "blk3", "blocked", "permission", "Allow rm?")
        self.bp("_event", "blk3", "blocked", "permission", "Allow rm?")  # repeat: no news
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual(len(news), 1)
        self.assertIn('event="needs-you"', news[0])
        self.assertIn("Allow rm?", news[0])
        self.assertEqual(self.show(tid)["status"], "needs")
        self.bp("_event", "blk3", "working", "", "")
        self.assertEqual(self.show(tid)["status"], "working")
        self.bp("_event", "blk3", "done", "", "")
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertIn('event="quiet"', news[-1])

    def test_done_after_ready_is_silent(self):
        tid = self.start_task()
        self.bp("ready", tid)  # no commits: ready with nothing to merge
        before = len(self.typed())
        self.bp("_event", "blk3", "done", "", "")
        self.assertEqual(len(self.typed()), before)

    def test_exit_marks_stopped_and_resume_relaunches(self):
        tid = self.start_task()
        (self.fake / "alive" / "blk3").unlink()
        self.bp("_gone", "blk3")
        t = self.show(tid)
        self.assertEqual(t["status"], "stopped")
        self.assertIn('event="stopped"', self.typed()[-1][1])
        self.bp("resume", tid)
        run = [a for a in self.rex_calls() if a[0] == "run"][-1]
        self.assertIn("--continue", run)
        self.assertEqual(self.show(tid)["status"], "working")

    def test_unknown_block_events_are_ignored(self):
        self.start_task()
        before = len(self.typed())
        self.bp("_event", "stranger", "blocked", "permission", "x")
        self.assertEqual(len(self.typed()), before)

    def test_accept_merges_and_cleans_up(self):
        tid = self.start_task()
        t = self.show(tid)
        wt = t["worktree"]
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
        self.assertFalse((self.fake / "alive" / "blk3").exists())
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


if __name__ == "__main__":
    unittest.main()
