"""Local mode with a backup remote: accept pushes the base branch there."""

import json
import shutil
import subprocess
import time
import unittest
from pathlib import Path

import support
import test_local_mode  # its setup, not its tests: a bare import keeps them out of this module
from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module


class PushRemoteTest(unittest.TestCase):
    """Local mode with a backup remote ("backup", a local bare repo): accept pushes the
    base branch there."""

    setUp_with_origin = BullpenTest.setUp
    setUp_local = test_local_mode.LocalModeTest.setUp
    tearDown = test_local_mode.LocalModeTest.tearDown
    bp = BullpenTest.bp
    rex_calls = BullpenTest.rex_calls
    typed = BullpenTest.typed
    show = BullpenTest.show
    start_task = BullpenTest.start_task
    git = test_local_mode.LocalModeTest.git
    commit = test_local_mode.LocalModeTest.commit
    ready_task = test_local_mode.LocalModeTest.ready_task
    doctor = BullpenTest.doctor

    def setUp(self):
        self.setUp_local()
        self.env["BULLPEN_PUSH_WAIT"] = "1"  # accept waits for its background push
        self.backup = self.tmp / "backup.git"
        sh("git", "init", "-q", "--bare", "-b", "main", str(self.backup))
        self.git("remote", "add", "backup", str(self.backup))
        self.git("push", "-q", "backup", "main")
        self.bp("config", "--push-remote", "backup")

    def backup_main(self):
        return sh("git", "-C", str(self.backup), "rev-parse", "main").strip()

    def push_news(self):
        log = next((self.tmp / "state").rglob("news.log"), None)
        return [l for l in log.read_text().splitlines() if " push: " in l] if log else []

    def test_accept_pushes_the_base_branch_only(self):
        tid, wt = self.ready_task()
        self.assertIn("pushing main to backup in the background", self.bp("accept", tid).stdout)
        self.assertEqual(self.backup_main(), self.git("rev-parse", "main"))
        heads = sh("git", "-C", str(self.backup), "for-each-ref", "--format=%(refname)").split()
        self.assertEqual(heads, ["refs/heads/main"])  # never a task branch
        self.assertEqual(self.push_news(), [])

    def test_two_accepts_in_a_row_both_land(self):
        del self.env["BULLPEN_PUSH_WAIT"]  # as for real: the pushes run in the background
        tid, wt = self.ready_task()
        self.bp("accept", tid)
        tid2 = self.bp("add", "Second", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        wt2 = self.show(tid2)["worktree"]
        self.commit(wt2, "second.txt", "2\n", "Second")
        self.bp("ready", tid2, "--summary", "Second.", cwd=wt2)
        self.bp("accept", tid2)
        want = self.git("rev-parse", "main")
        for _ in range(100):
            if self.backup_main() == want:
                break
            time.sleep(0.1)
        self.assertEqual(self.backup_main(), want)
        lock = next((self.tmp / "state").rglob("push.lock"))
        import fcntl
        with open(lock) as f:  # the last push is done before the test's folder goes
            fcntl.flock(f, fcntl.LOCK_EX)

    def test_a_rejected_push_gives_one_news_line_and_the_accept_counts(self):
        other = self.tmp / "other"
        sh("git", "clone", "-q", str(self.backup), str(other))
        self.commit(other, "theirs.txt", "theirs\n", "Moved on elsewhere")
        sh("git", "-C", str(other), "push", "-q", "origin", "main")
        moved = self.backup_main()
        tid, wt = self.ready_task()
        self.bp("accept", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(self.git("show", "main:fix.txt"), "fixed")
        self.assertEqual(self.backup_main(), moved)  # never forced
        lines = self.push_news()
        self.assertEqual(len(lines), 1)
        self.assertIn("BP-1 push: bullpen · After BP-1, main wasn't pushed to backup: rejected, backup has commits main "
                      "doesn't", lines[0])
        self.assertIn("The accept still counts", lines[0])
        self.assertIn("main wasn't pushed to backup", self.typed()[-1][1])
        _, lines = self.doctor()
        self.assertEqual(lines["push"][0], "FAIL")
        self.assertIn("backup's main has commits this one doesn't", lines["push"][1])

    def test_an_unreachable_remote_gives_news_and_doctor_counts_how_far_behind(self):
        tid, wt = self.ready_task()
        self.backup.rename(self.tmp / "away.git")  # offline
        self.bp("accept", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(len(self.push_news()), 1)
        (self.tmp / "away.git").rename(self.backup)
        _, lines = self.doctor()
        self.assertEqual(lines["push"], ("FAIL", "backup is 2 commits behind main: `git push backup main`"))

    def test_up_catches_up_a_base_ahead_of_the_push_remote(self):
        self.commit(self.repo, "by-hand.txt", "x\n", "Merged by hand")
        self.assertNotEqual(self.backup_main(), self.git("rev-parse", "main"))
        self.assertIn("pushing main to backup in the background", self.bp("up").stdout)
        self.assertEqual(self.backup_main(), self.git("rev-parse", "main"))
        self.assertNotIn("pushing", self.bp("up").stdout)  # caught up: nothing to push
        _, lines = self.doctor()
        self.assertEqual(lines["push"], ("ok", "backup has main as it is here"))

    def test_close_pushes_nothing(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        self.commit(self.repo, "by-hand.txt", "x\n", "Merged by hand")
        before = self.backup_main()
        f = self.tmp / "f.md"
        f.write_text("Answered.")
        self.bp("finish", tid, "--summary-file", str(f), cwd=wt)
        self.assertNotIn("pushing", self.bp("close", tid).stdout)
        self.assertEqual(self.backup_main(), before)

    def test_the_setting_off(self):
        self.bp("config", "--push-remote", "")
        tid, wt = self.ready_task()
        before = self.backup_main()
        self.assertNotIn("pushing", self.bp("accept", tid).stdout)
        self.assertEqual(self.backup_main(), before)
        self.bp("config", "--push-remote", "backup")
        cfg = json.loads(self.bp("config", "--json").stdout)
        self.assertEqual((cfg["push_remote"], cfg["sources"]["push_remote"]), ("backup", "repo"))
        cfg = json.loads(self.bp("config", "--unset", "push_remote", "--json").stdout)
        self.assertIsNone(cfg["push_remote"])
        _, lines = self.doctor()
        self.assertEqual(lines["push"][0], "--")

    def test_global_refuses_it(self):
        for v in ("backup", ""):
            p = self.bp("config", "--global", "--push-remote", v, ok=False)
            self.assertIn("--push-remote is per repository", p.stderr)
        p = self.bp("config", "--global", "--unset", "push_remote", ok=False)
        self.assertIn("push_remote is this repository's own", p.stderr)

    def test_doctor_writes_home_as_a_tilde_on_the_push_line(self):
        self.env["HOME"] = str(self.tmp)
        self.backup.rename(self.tmp / "moved.git")  # the remote's path, gone
        _, lines = self.doctor()
        self.assertEqual(lines["push"][0], "FAIL")
        self.assertIn("~/backup.git", lines["push"][1])
        self.assertNotIn(str(self.tmp), lines["push"][1])
        self.assertRegex(self.bp("config").stdout, r"Tasks\n(  .*\n)*  push_remote +backup +repo\n")

    def test_a_missing_remote_in_doctor(self):
        self.bp("config", "--push-remote", "nope")
        _, lines = self.doctor()
        self.assertEqual(lines["push"][0], "FAIL")
        self.assertIn("there's no remote nope", lines["push"][1])

    def test_pr_mode_ignores_it(self):
        origin = self.tmp / "origin.git"
        self.git("remote", "add", "origin", str(origin))
        self.git("push", "-q", "origin", "main")
        self.commit(self.repo, "by-hand.txt", "x\n", "Merged by hand")
        before = self.backup_main()
        self.assertNotIn("pushing", self.bp("up").stdout)
        self.assertEqual(self.backup_main(), before)
        _, lines = self.doctor()
        self.assertEqual(lines["push"], ("--", "backup is set, but this repo has an origin remote (remote mode), "
                                                "so nothing is pushed to it"))
        (self.fake / "gh.log").unlink(missing_ok=True)  # doctor's gh check, in remote mode: fine here

    def test_an_accept_that_changes_bullpens_own_code_still_does_what_follows(self):
        # The project is bullpen itself, run from its main checkout: the merge replaces
        # the code of the accept that's running. The task makes notifications.py (loaded
        # by the first save, after the merge) need what only the new agents.py has.
        for d in ("bin", "bullpen", "prompts", "lua"):
            shutil.copytree(support.ROOT / d, self.repo / d, ignore=shutil.ignore_patterns("__pycache__"))
        (self.repo / ".gitignore").write_text("__pycache__/\n/target/\n")
        # its build, as every checkout has one (the run queue is Rust's `_queue`)
        (self.repo / "target").mkdir()
        shutil.copy2(support.ROOT / "target" / "bullpen", self.repo / "target" / "bullpen")
        self.git("add", ".")
        self.git("commit", "-qm", "bullpen")
        self.bp("config", "--review", "off")
        tid = self.start_task()
        wt = Path(self.show(tid)["worktree"])
        with (wt / "bullpen" / "agents.py").open("a") as f:
            f.write("\n\ndef brand_new():\n    return 1\n")
        n = wt / "bullpen" / "notifications.py"
        n.write_text(n.read_text().replace("from .agents import ", "from .agents import brand_new, ", 1))
        self.git("commit", "-qam", "Needs the new agents.py", cwd=wt)
        self.bp("ready", tid, "--summary", "Done.", cwd=wt)
        nxt = self.bp("add", "Next", "--brief", "x", "--after", tid).stdout.strip()
        own = self.repo / "bin" / "bullpen"  # the accept runs from the code it merges into
        p = subprocess.run([str(own), "accept", tid], cwd=self.repo, env=self.env, text=True, capture_output=True)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(p.stderr, "")
        self.assertIn("brand_new", (self.repo / "bullpen" / "agents.py").read_text())  # merged
        log = next((self.tmp / "state").rglob("news.log")).read_text()
        self.assertIn(f"{tid} accepted (merged into main", log)
        self.assertEqual(self.backup_main(), self.git("rev-parse", "main"))
        self.assertEqual(self.show(nxt)["status"], "working")  # the run queue moved
