"""Two copies of bullpen at work in one repository: the repository's own copy (the one
its coordinator was started from, test_board_build's) and another checkout of bullpen
with older source. A command run by the other copy rebuilds the checkout it merged
into, leaves the watcher to the coordinator's copy, and doctor says which copy ran."""

import json
import shutil
import subprocess
import unittest
from pathlib import Path

import test_board_build  # its setup, not its tests: a bare import keeps them out of this module
from support import setUpModule, tearDownModule, ROOT, sh, BullpenTest  # a temp folder for each module


class CopiesTest(unittest.TestCase):
    B = test_board_build.BoardBuildTest
    setUp_with_origin, setUp_home, bp, settle, builds, board = (
        B.setUp_with_origin, B.setUp, B.bp, B.settle, B.builds, B.board)
    show, start_task, news_log, rex_calls = B.show, B.start_task, B.news_log, B.rex_calls

    def setUp(self):
        self.setUp_home()
        self.bp("build")
        self.bp("config", "--review", "off")
        # the other copy: a checkout of the same source, with a build of its own
        self.other = self.tmp / "other-copy"
        shutil.copytree(self.home, self.other, symlinks=True, ignore=shutil.ignore_patterns("target"))
        (self.other / "target").mkdir()
        shutil.copy2(ROOT / "target" / "bullpen", self.other / "target" / "bullpen")
        self.other_bin = self.other / "bin" / "bullpen"
        self.bp_other("build", cwd=self.other)

    def bp_other(self, *args, cwd=None, ok=True):
        """A command in the repository, run by the other copy's bullpen."""
        p = subprocess.run([str(self.other_bin), *args], cwd=cwd or self.repo, env=self.env,
                           text=True, capture_output=True)
        if ok and p.returncode != 0:
            self.fail(f"the other copy's bullpen {' '.join(args)} failed: {p.stderr}")
        return p

    def commit(self, where, rel, text, msg="Change"):
        (Path(where) / rel).write_text(text)
        sh("git", "-C", str(where), "add", ".")
        sh("git", "-C", str(where), "-c", "user.name=T", "-c", "user.email=t@example.com", "commit", "-qm", msg)

    def ready_task(self, rel="cli/src/main.rs", text="fn main() { accepted() }\n"):
        tid = self.start_task()
        wt = Path(self.show(tid)["worktree"])
        self.commit(wt, rel, text)
        self.bp("ready", tid, "--summary", "Changed it.", cwd=wt)
        return tid

    def stamp(self, home):
        return json.loads((home / "target" / "bullpen.json").read_text())

    def board_news(self):
        return [l for l in self.news_log().splitlines() if " board board: " in l or " bullpen build: " in l]

    def config(self):
        return next((self.tmp / "state" / "repos").glob("*/config.json"))

    def set_coordinator(self, **keys):
        """Change the coordinator's record (a key set to None goes)."""
        cfg = json.loads(self.config().read_text())
        for k, v in keys.items():
            cfg["coordinators"][0].pop(k, None) if v is None else cfg["coordinators"][0].update({k: v})
        self.config().write_text(json.dumps(cfg))

    def doctor(self, bp=None):
        """doctor's lines, by check: (mark, text)."""
        lines = {}
        for line in (bp or self.bp)("doctor", ok=False).stdout.splitlines()[:-1]:
            mark, rest = line.split(None, 1)
            name, text = rest.split(": ", 1)
            lines[name] = (mark, text)
        return lines

    def watcher(self):
        return json.loads((self.tmp / "state" / "watcher.json").read_text())

    def watcher_commands(self):
        """The command line of each watcher started, oldest first."""
        return [b["options"]["command"] for b in BullpenTest.api(self, "session.new_block")]

    def test_an_accept_by_another_copy_rebuilds_the_checkout_it_merged_into(self):
        tid = self.ready_task()
        other_before = (self.other / "target" / "bullpen").read_text(), self.stamp(self.other)
        out = self.bp_other("accept", tid).stdout
        self.assertIn("rebuilding bullpen's command line in the background (its source changed): "
                      f"{self.home}, this repository's own copy of bullpen", out)
        self.settle()
        self.assertIn("accepted()", self.board("bullpen"))  # built from its own source, by its own bullpen
        self.assertNotIn("accepted()", self.board())  # the board's source didn't change
        self.assertEqual(self.stamp(self.home).keys(), {"source"})
        self.assertEqual(self.bp("doctor", ok=False).stdout.count("FAIL  bullpen:"), 0)
        # the copy that ran the accept isn't rebuilt for it
        self.assertEqual(((self.other / "target" / "bullpen").read_text(), self.stamp(self.other)), other_before)
        self.assertNotIn("accepted()", (self.other / "target" / "build.log").read_text())
        self.assertEqual(self.board_news(), [])  # a build that works isn't news

    def test_an_accept_by_another_copy_leaves_the_watcher_to_the_coordinators_copy(self):
        self.bp("up")
        mine = self.watcher()
        self.assertIn(f"bin={self.bin}", self.watcher_commands()[-1])
        # the other copy's source is older than the repository's: its own build, its own stamp
        self.commit(self.home, "cli/src/main.rs", "fn main() { newer() }\n")
        self.bp("build")
        current = self.watcher()
        self.assertNotEqual(current["block"], mine["block"])  # its own build replaces it
        tid = self.ready_task("fix.txt", "fixed\n")
        out = self.bp_other("accept", tid).stdout
        self.assertNotIn("watcher", out)
        self.assertNotIn("guide", out)  # its coordinator has its own copy's guide
        self.assertEqual(self.watcher(), current)
        self.assertNotIn(f"bin={self.other_bin}", " ".join(map(str, self.watcher_commands())))

    def test_uncommitted_source_in_the_checkout_merged_into_is_never_built(self):
        tid = self.ready_task()
        (self.home / "board" / "src" / "main.rs").write_text("fn main() { half_done() }\n")
        out = self.bp_other("accept", tid).stdout
        self.assertIn("rebuilding bullpen's command line in the background", out)
        self.settle()
        self.assertIn("accepted()", self.board("bullpen"))
        self.assertNotIn("half_done()", self.board())
        self.assertEqual(len(self.board_news()), 1)
        self.assertIn(f"The board wasn't rebuilt: {self.home} has uncommitted changes to core, board",
                      self.board_news()[0])
        self.assertEqual(self.stamp(self.home).keys(), {"source"})
        self.assertIn("skipped", json.loads((self.target / "bullpen-board.json").read_text()))

    def test_a_failed_build_of_the_checkout_merged_into_is_news(self):
        tid = self.ready_task()
        (self.fake / "cargo_fails").write_text("")
        self.bp_other("accept", tid)
        self.settle()
        self.assertNotIn("accepted()", self.board("bullpen"))
        self.assertEqual(len(self.board_news()), 1)
        self.assertIn("bullpen's command line wasn't rebuilt: cargo failed: error[E0425]", self.board_news()[0])
        self.assertIn(f"Log: {self.target / 'build.log'}", self.board_news()[0])
        self.assertIn("failed", self.stamp(self.home))
        self.assertEqual(self.doctor()["bullpen"][0], "FAIL")

    def test_a_build_already_running_in_the_checkout_merged_into_is_left_to_it(self):
        import fcntl
        tid = self.ready_task()
        with (self.target / "build.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            out = self.bp_other("accept", tid).stdout
        self.assertNotIn("rebuilding", out)
        self.assertNotIn("accepted()", self.board("bullpen"))

    def test_a_checkout_of_bullpens_source_that_was_never_built_isnt_built(self):
        # a clone someone works on with an installed bullpen: nothing runs from it
        clone = self.tmp / "clone"
        shutil.copytree(self.home, clone, symlinks=True, ignore=shutil.ignore_patterns("target"))
        self.repo = clone
        self.bp("config", "--review", "off")
        before = self.builds()
        tid = self.ready_task()
        out = self.bp("accept", tid).stdout
        self.assertNotIn("rebuilding", out)
        self.assertEqual(self.builds(), before)
        self.assertFalse((clone / "target").exists())

    def test_up_records_the_copy_a_coordinator_was_started_from(self):
        self.bp("up")
        cfg = json.loads(self.config().read_text())
        self.assertEqual(cfg["coordinators"][0]["bullpen"], str(self.home))
        self.assertEqual(self.watcher()["home"], str(self.home))
        self.assertEqual(self.doctor()["copy"], ("ok", f"this repo's coordinator (c1) uses this copy, {self.home}"))
        # up from another copy leaves the coordinator, and so the watcher, with the first
        before = self.watcher()
        self.commit(self.other, "cli/src/main.rs", "fn main() { other() }\n")
        self.bp_other("build", cwd=self.other)
        self.assertNotIn("Watcher", self.bp_other("up").stdout)
        self.assertEqual(json.loads(self.config().read_text())["coordinators"][0]["bullpen"], str(self.home))
        self.assertEqual(self.watcher(), before)

    def test_another_copy_starts_the_watcher_from_the_coordinators_copy(self):
        self.bp("up")
        (self.fake / "reg.json").unlink()  # Rex restarted: the watcher's pane is gone
        self.bp_other("up", "--session", "session:1")
        self.assertIn(f"bin={self.bin}", self.watcher_commands()[-1])
        self.assertIn(str(self.home / "lua" / "watch.lua"), self.watcher_commands()[-1])
        self.assertEqual(self.watcher()["home"], str(self.home))

    def test_a_coordinator_and_watcher_from_before_copies_were_recorded(self):
        self.bp("up")
        record = self.watcher()
        self.set_coordinator(bullpen=None)
        (self.tmp / "state" / "watcher.json").write_text(
            json.dumps({k: v for k, v in record.items() if k != "home"}))
        mark, text = self.doctor()["copy"]
        self.assertEqual(mark, "--")
        self.assertIn("c1 was started before bullpen recorded its copy", text)
        self.assertNotIn("Watcher", self.bp("up").stdout)  # the same watcher, now recorded as this copy's
        self.assertEqual(self.watcher(), record)
        self.assertEqual(self.doctor()["copy"][0], "ok")

    def test_doctor_says_a_command_was_run_by_an_older_copy(self):
        self.bp("up")
        self.commit(self.home, "cli/src/main.rs", "fn main() { newer() }\n")
        self.bp("build")
        tid = self.ready_task("fix.txt", "fixed\n")
        self.assertEqual(self.doctor()["copy"][0], "ok")
        self.bp_other("accept", tid)  # the repository is now its commit and the merge ahead
        mark, text = self.doctor()["copy"]
        self.assertEqual(mark, "FAIL")
        self.assertIn(f"`accept` was run here by another copy of bullpen ({self.other}, ", text)
        self.assertIn("whose source is older than this one's (2 commits behind), the one this repo's "
                      f"coordinator (c1) uses. Run this repo's commands with {self.bin}", text)
        # run by the other copy, doctor says it of itself
        mark, text = self.doctor(self.bp_other)["copy"]
        self.assertEqual(mark, "FAIL")
        self.assertEqual(text, f"this is {self.other}'s bullpen, but this repo's coordinator (c1) uses "
                               f"{self.home}'s, and this one's source is older than that one's "
                               f"(2 commits behind): run {self.bin} here")
        self.assertIn("1 problem: run `bullpen doctor`", self.bp_other("up").stdout)
        # up from the coordinator's copy sees to it
        self.bp("up")
        self.assertEqual(self.doctor()["copy"][0], "ok")

    def test_doctor_says_which_is_older_of_a_worktree_on_a_branch_that_parted(self):
        # as it happened: the other copy is a worktree of the repository itself, on its own branch
        self.bp("up")
        side = self.tmp / "side"
        sh("git", "-C", str(self.home), "worktree", "add", "-q", "-b", "side", str(side))
        (side / "target").mkdir()
        shutil.copy2(ROOT / "target" / "bullpen", side / "target" / "bullpen")
        (side / "cli" / "src" / "main.rs").write_text("fn main() { side() }\n")
        subprocess.run(["git", "-C", str(side), "commit", "-qam", "Its own work"], check=True,
                       env={**self.env, "GIT_COMMITTER_DATE": "2020-01-01T00:00:00Z"})
        self.commit(self.home, "cli/src/main.rs", "fn main() { newer() }\n")
        self.other_bin = side / "bin" / "bullpen"
        mark, text = self.doctor(self.bp_other)["copy"]
        self.assertEqual(mark, "FAIL")
        self.assertIn("this one's source is older than that one's (it lacks 1 of the other's commits, and has 1 "
                      "of its own)", text)

    def test_a_second_coordinator_started_from_another_copy_runs_its_own(self):
        self.bp("up")
        self.commit(self.home, "cli/src/main.rs", "fn main() { newer() }\n")
        self.bp("build")
        before = self.watcher()
        tid = self.ready_task("fix.txt", "fixed\n")
        pane = BullpenTest.new_pane(self)
        env = self.env
        self.env = {**env, "REX_BLOCK": pane}
        self.bp_other("up", "--here")  # c2: an agent that was given the other copy's bullpen
        copies = [c["bullpen"] for c in json.loads(self.config().read_text())["coordinators"]]
        self.assertEqual(copies, [str(self.home), str(self.other)])
        self.bp_other("accept", tid)  # c2's own command: not another copy's
        self.env = env
        self.assertEqual(self.watcher(), before)  # the watcher follows the first coordinator's copy
        self.assertFalse((self.config().parent / "other-copy.json").exists())
        self.assertEqual(self.doctor()["copy"], ("ok", f"this repo's first coordinator (c1) uses this copy, "
                                                       f"{self.home}; c2 uses {self.other}"))
        self.assertEqual(self.doctor(self.bp_other)["copy"],
                         ("ok", f"this repo's first coordinator (c1) uses {self.home}, and the watcher runs "
                                f"from that copy; c2 uses {self.other}"))

    def test_doctor_says_nothing_of_a_copy_with_the_same_source(self):
        self.bp("up")
        tid = self.ready_task("fix.txt", "fixed\n")
        self.bp_other("accept", tid)
        self.assertEqual(self.doctor()["copy"][0], "ok")
        self.assertEqual(self.doctor(self.bp_other)["copy"],
                         ("ok", f"this repo's coordinator (c1) uses {self.home}; this copy ({self.other}) "
                                "has the same source"))

    def test_a_coordinators_copy_that_is_gone_is_doctors_to_say(self):
        self.bp("up")
        before = self.watcher()
        gone = self.tmp / "gone"
        self.set_coordinator(bullpen=str(gone))
        tid = self.ready_task("fix.txt", "fixed\n")  # its `up` starts no watcher from a guess
        self.bp("accept", tid)
        self.assertEqual(self.watcher(), before)
        mark, text = self.doctor()["copy"]
        self.assertEqual(mark, "FAIL")
        self.assertIn(f"this repo's coordinator (c1) was started from {gone}, which is gone", text)
        self.assertIn(f"Run `{self.bin} up --here` in its pane to have it use this copy", text)
        env = self.env
        self.env = {**env, "REX_BLOCK": json.loads(self.config().read_text())["coordinators"][0]["block"]}
        self.bp("up", "--here")
        self.env = env
        self.assertEqual(self.doctor()["copy"][0], "ok")


if __name__ == "__main__":
    unittest.main()
