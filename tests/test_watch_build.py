"""The watcher's periodic check (`check --fix --all-repos`) builds its own copy of bullpen
when its source is newer than its build: after a `git pull`, or a merge by a copy of
bullpen that didn't rebuild the checkout it merged into."""

import json
import unittest

import test_board_build  # its setup, not its tests: a bare import keeps them out of this module
from support import setUpModule, tearDownModule, sh  # a temp folder for each module


class WatchBuildTest(unittest.TestCase):
    """test_board_build's copy of bullpen, built, with a coordinator and a watcher."""

    B = test_board_build.BoardBuildTest
    setUp_with_origin, setUp_home, bp, settle, builds, board = (
        B.setUp_with_origin, B.setUp, B.bp, B.settle, B.builds, B.board)
    news_log, rex_calls = B.news_log, B.rex_calls

    def setUp(self):
        self.setUp_home()
        self.bp("build")
        self.bp("up")
        self.watcher = self.watcher_block()

    def git(self, *args):
        return sh("git", "-C", str(self.home), "-c", "user.name=T", "-c", "user.email=t@example.com", *args)

    def pull(self, text="fn main() { pulled() }\n", crate="cli"):
        """A commit to the source that no accept of this copy made: a `git pull`."""
        (self.home / crate / "src" / "main.rs").write_text(text)
        self.git("commit", "-qam", "Pulled")

    def check(self):
        """The watcher's periodic check, and the build it started, done."""
        self.bp("check", "--fix", "--quiet", "--all-repos")
        self.settle()

    def watcher_block(self):
        return json.loads((self.tmp / "state" / "watcher.json").read_text())["block"]

    def build_lines(self):
        return [l for l in self.news_log().splitlines() if " bullpen build: " in l or " board board: " in l]

    def test_a_source_newer_than_its_build_is_built_once_and_the_watcher_restarted(self):
        self.pull()
        self.check()
        self.assertEqual(self.builds(), 2)
        self.assertIn("pulled()", (self.target / "bullpen").read_text())
        self.assertNotEqual(self.watcher_block(), self.watcher)
        after = self.watcher_block()
        self.check()
        self.assertEqual(self.builds(), 2)
        self.assertEqual(self.watcher_block(), after)
        self.assertEqual(self.build_lines(), [])  # silent
        line = next(l for l in self.bp("doctor", ok=False).stdout.splitlines() if " bullpen: " in l)
        self.assertTrue(line.startswith("ok"), line)

    def test_nothing_happens_when_the_build_is_current(self):
        self.check()
        self.check()
        self.assertEqual(self.builds(), 1)
        self.assertEqual(self.watcher_block(), self.watcher)
        self.assertEqual(self.build_lines(), [])
        self.assertFalse((self.target / "watch-build.json").exists())

    def test_a_check_that_isnt_the_watchers_builds_nothing(self):
        self.pull()
        self.bp("check", "--fix")
        self.bp("check", "--all-repos", ok=False)
        self.settle()
        self.assertEqual(self.builds(), 1)

    def test_uncommitted_source_is_skipped_with_one_news_line(self):
        self.pull()
        (self.home / "lua" / "watch.lua").write_text("-- half done\n")  # not a program's source: the watcher's
        self.check()
        self.check()
        self.assertEqual(self.builds(), 1)
        lines = self.build_lines()
        self.assertEqual(len(lines), 1, lines)
        self.assertIn("uncommitted changes under core/, cli/, board/, lua/", lines[0])
        self.git("checkout", "-q", "--", "lua")
        self.check()  # dropped: built
        self.assertEqual(self.builds(), 2)
        self.assertEqual(len(self.build_lines()), 1)

    def test_a_merge_in_progress_is_skipped_with_one_news_line(self):
        self.git("checkout", "-q", "-b", "theirs")
        self.pull("fn main() { theirs() }\n", crate="board")
        self.git("checkout", "-q", "main")
        self.pull("fn main() { ours() }\n", crate="board")
        p = self.B.bp(self, "build")  # main as it is: current
        self.assertEqual(self.builds(), 2, p.stdout)
        self.pull()
        import subprocess
        merge = subprocess.run(["git", "-C", str(self.home), "merge", "-q", "theirs"], capture_output=True, text=True)
        self.assertNotEqual(merge.returncode, 0)  # a conflict: the merge is left open
        self.check()
        self.check()
        self.assertEqual(self.builds(), 2)
        lines = self.build_lines()
        self.assertEqual(len(lines), 1, lines)
        self.assertIn("a merge is in progress", lines[0])
        self.git("merge", "--abort")
        self.check()
        self.assertEqual(self.builds(), 3)
        self.assertEqual(len(self.build_lines()), 1)

    def test_a_running_build_is_left_alone_and_said_once_when_it_is_still_running_a_check_later(self):
        (self.fake / "cargo_slow").write_text("4")
        self.pull()
        self.assertIn("rebuilding", self.bp("up").stdout)  # the build that's running
        check = lambda: self.bp("check", "--fix", "--quiet", "--all-repos")
        check()
        self.assertEqual(self.build_lines(), [])  # an accept's build is usually done by the next check
        check()
        check()
        lines = self.build_lines()
        self.assertEqual(len(lines), 1, lines)
        self.assertRegex(lines[0], r"a build \(pid \d+\) has been running .* stop it \(`kill \d+`\)")
        self.settle()
        self.check()
        self.assertEqual(self.builds(), 2)
        self.assertEqual(len(self.build_lines()), 1)
        self.assertFalse((self.target / "watch-build.json").exists())

    def test_a_build_that_failed_isnt_tried_again_for_the_same_source(self):
        (self.fake / "cargo_fails").write_text("")
        self.pull()
        self.check()
        self.check()
        self.check()
        self.assertEqual(self.builds(), 2)
        lines = self.build_lines()
        self.assertEqual(len(lines), 1, lines)
        self.assertIn("bullpen's command line wasn't rebuilt: ", lines[0])
        self.assertIn("error[E0425]", lines[0])
        (self.fake / "cargo_fails").unlink()
        self.pull("fn main() { fixed() }\n")  # a new commit is tried
        self.check()
        self.assertEqual(self.builds(), 3)
        self.assertEqual(len(self.build_lines()), 1)

    def test_a_guide_the_pull_changed_is_news_after_the_build(self):
        guide = self.home / "prompts" / "coordinator.md"
        guide.write_text(guide.read_text() + "\nA new rule: say hello.\n")
        self.pull()
        self.check()
        self.assertNotIn(" guide: ", self.news_log())
        self.check()  # the first check with the new build
        lines = [l for l in self.news_log().splitlines() if " guide: " in l]
        self.assertEqual(len(lines), 1, lines)
        self.assertIn("Your coordinator guide ", lines[0])
        self.check()
        self.assertEqual(len([l for l in self.news_log().splitlines() if " guide: " in l]), 1)


if __name__ == "__main__":
    unittest.main()
