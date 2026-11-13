"""close --keep-branch: end a task unmerged, its branch kept (and pushed first)."""

import json
import unittest
from pathlib import Path

import test_local_mode  # its setup, not its tests: a bare import keeps them out of this module
from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module


class KeepBranchLocalTest(unittest.TestCase):
    """Local mode: with no push remote the branch stays here; with one it goes there first."""

    setUp_with_origin = BullpenTest.setUp
    setUp = test_local_mode.LocalModeTest.setUp
    tearDown = test_local_mode.LocalModeTest.tearDown
    bp = BullpenTest.bp
    rex_calls = BullpenTest.rex_calls
    typed = BullpenTest.typed
    show = BullpenTest.show
    news = BullpenTest.news
    start_task = BullpenTest.start_task
    full_brief = BullpenTest.full_brief
    git = test_local_mode.LocalModeTest.git
    commit = test_local_mode.LocalModeTest.commit

    def experiment(self):
        tid = self.start_task("Try a port")
        t = self.show(tid)
        self.commit(t["worktree"], "port.txt", "ported\n", "Port it")
        return tid, t

    def backup(self):
        backup = self.tmp / "backup.git"
        sh("git", "init", "-q", "--bare", "-b", "main", str(backup))
        self.git("remote", "add", "backup", str(backup))
        self.git("push", "-q", "backup", "main")
        self.bp("config", "--push-remote", "backup")
        return backup

    def test_without_a_remote_the_local_branch_stays(self):
        tid, t = self.experiment()
        p = self.bp("close", tid, "--keep-branch")
        self.assertIn("BP-1 closed. Closed, branch kept here: bullpen/bp-1-try-a-port, not pushed.", p.stdout)
        self.assertIn("--push-remote", p.stdout)
        k = self.show(tid)
        self.assertEqual((k["status"], k["worktree"], k["block"]), ("done", None, None))
        self.assertEqual(k["kept"], {"branch": t["branch"], "remote": None})
        self.assertFalse(Path(t["worktree"]).exists())
        self.assertFalse((self.fake / "alive" / t["block"]).exists())
        self.assertEqual(self.git("show", f"{t['branch']}:port.txt"), "ported")
        self.assertNotIn("port.txt", self.git("ls-files"))  # nothing merged
        self.assertEqual(self.news()[-1], "bullpen · BP-1 closed without merging. Closed, branch kept here: "
                         "bullpen/bp-1-try-a-port, not pushed.")

    def test_as_renames_the_kept_branch(self):
        tid, t = self.experiment()
        self.bp("close", tid, "--keep-branch", "--as", "port")
        self.assertEqual(self.show(tid)["kept"], {"branch": "port", "remote": None})
        self.assertEqual(self.git("show", "port:port.txt"), "ported")
        self.assertNotIn(t["branch"], self.git("branch"))

    def test_with_a_push_remote_it_goes_there_first(self):
        backup = self.backup()
        tid, t = self.experiment()
        p = self.bp("close", tid, "--keep-branch", "--as", "port")
        self.assertIn("BP-1 closed. Closed, branch kept: port on backup.", p.stdout)
        self.assertEqual(self.show(tid)["kept"], {"branch": "port", "remote": "backup"})
        self.assertEqual(sh("git", "-C", str(backup), "show", "port:port.txt"), "ported\n")
        self.assertEqual(sh("git", "-C", str(backup), "rev-parse", "main").strip(), self.git("rev-parse", "main"))
        out = self.bp("ls", "--all").stdout
        self.assertIn("Closed, branch kept: port on backup.", out)
        self.assertNotIn("Dropped", out)
        self.assertIn("Closed, branch kept: port on backup.", self.bp("show", tid).stdout)

    def test_a_failed_push_refuses_and_changes_nothing(self):
        backup = self.backup()
        tid, t = self.experiment()
        backup.rename(self.tmp / "away.git")  # offline
        p = self.bp("close", tid, "--keep-branch", ok=False)
        self.assertIn("BP-1 isn't closed, so its work stays: bullpen/bp-1-try-a-port wasn't pushed to backup", p.stderr)
        k = self.show(tid)
        self.assertEqual((k["status"], k["worktree"]), ("working", t["worktree"]))
        self.assertNotIn("kept", k)
        self.assertTrue(Path(t["worktree"]).exists())
        self.assertTrue((self.fake / "alive" / t["block"]).exists())

    def test_a_branch_name_taken_on_the_remote_refuses(self):
        backup = self.backup()
        tid, t = self.experiment()
        self.commit(self.repo, "other.txt", "x\n", "Moved on")
        self.git("push", "-q", "backup", "main:port")  # someone else's port
        p = self.bp("close", tid, "--keep-branch", "--as", "port", ok=False)
        self.assertIn("backup has a branch port with commits this one doesn't", p.stderr)
        self.assertIn("pick another name with --as", p.stderr)
        self.assertEqual(self.show(tid)["status"], "working")

    def test_refusals(self):
        tid, t = self.experiment()
        (Path(t["worktree"]) / "dirty.txt").write_text("x\n")
        self.assertIn("uncommitted changes", self.bp("close", tid, "--keep-branch", ok=False).stderr)
        (Path(t["worktree"]) / "dirty.txt").unlink()
        self.assertIn("pass both", self.bp("close", tid, "--as", "x", ok=False).stderr)
        p = self.bp("close", tid, ok=False)
        self.assertIn(f"`close {tid} --keep-branch` ends it and keeps its branch unmerged", p.stderr)
        self.git("branch", "taken")
        p = self.bp("close", tid, "--keep-branch", "--as", "taken", ok=False)
        self.assertIn("there's a branch taken here already", p.stderr)
        empty = self.start_task("Nothing yet")
        self.assertIn("nothing to keep", self.bp("close", empty, "--keep-branch", ok=False).stderr)
        queued = self.bp("add", "Never started", "--brief", "x").stdout.strip()
        self.assertIn("has no branch to keep", self.bp("close", queued, "--keep-branch", ok=False).stderr)

    def test_finish_points_to_it(self):
        tid, t = self.experiment()
        f = self.tmp / "f.md"
        f.write_text("Ported.")
        p = self.bp("finish", tid, "--summary-file", str(f), cwd=t["worktree"], ok=False)
        self.assertIn(f"`bullpen close {tid} --keep-branch`", p.stderr)

    def test_what_waited_on_it_keeps_waiting(self):
        tid, t = self.experiment()
        self.bp("add", "Build on it", "--brief", "x", "--after", tid)
        self.bp("close", tid, "--keep-branch")
        self.assertEqual(self.show("BP-2")["status"], "todo")
        self.assertIn("BP-2 waits on BP-1, which was closed with its branch kept, not merged, so it stays "
                      "queued", self.news()[-1])
        self.assertIn("Waits on BP-1 (branch kept, not merged).", self.bp("ls").stdout)


class KeepBranchRemoteTest(BullpenTest):
    """Remote mode: the branch is pushed to origin and its PR closed."""

    def test_it_goes_to_origin_and_the_pr_closes(self):
        tid = self.start_task("Try a port")
        self.commit_and_ready(tid)
        branch = self.show(tid)["branch"]
        self.bp("close", tid, "--keep-branch", "--as", "port")
        k = self.show(tid)
        self.assertEqual((k["status"], k["kept"]), ("done", {"branch": "port", "remote": "origin"}))
        origin = self.tmp / "origin.git"
        self.assertEqual(sh("git", "-C", str(origin), "show", "port:fix1.txt"), "fixed\n")
        heads = sh("git", "-C", str(origin), "for-each-ref", "--format=%(refname)").split()
        self.assertNotIn(f"refs/heads/{branch}", heads)  # the PR's branch went, the kept one stays
        self.assertIn('["pr", "close"', (self.fake / "gh.log").read_text())

    def test_kept_under_its_own_name_its_branch_on_origin_stays(self):
        tid = self.start_task("Try a port")
        self.commit_and_ready(tid)
        branch = self.show(tid)["branch"]
        self.bp("close", tid, "--keep-branch")
        heads = sh("git", "-C", str(self.tmp / "origin.git"), "for-each-ref", "--format=%(refname)").split()
        self.assertIn(f"refs/heads/{branch}", heads)
        self.assertIn("Closed, branch kept: " + branch + " on origin.", self.bp("ls", "--all").stdout)
