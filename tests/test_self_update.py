"""An accept in bullpen's own repository that changes its command line (cli/ changes):
the rebuild and the push happen whichever way the accept ran, an internal call the
running build doesn't have yet (a job's, the watcher's) waits for the new build or
builds it, and a job that fails is news."""

import fcntl
import json
import os
import re
import subprocess
import time
import unittest
from pathlib import Path

import test_board_build  # its setup, not its tests: a bare import keeps them out of this module
from support import setUpModule, tearDownModule, sh  # a temp folder for each module


class SelfUpdateTest(unittest.TestCase):
    """bullpen's own checkout (test_board_build's copy, local mode, a fake cargo) with a
    backup remote to push to."""

    B = test_board_build.BoardBuildTest
    setUp_with_origin, setUp_home, bp, settle, builds, board = (
        B.setUp_with_origin, B.setUp, B.bp, B.settle, B.builds, B.board)
    show, start_task, news_log, rex_calls, install = B.show, B.start_task, B.news_log, B.rex_calls, B.install

    def setUp(self):
        self.setUp_home()
        self.backup = self.tmp / "backup.git"
        sh("git", "init", "-q", "--bare", "-b", "main", str(self.backup))
        self.git("remote", "add", "backup", str(self.backup))
        self.git("push", "-q", "backup", "main")
        self.bp("build")
        self.bp("config", "--push-remote", "backup")

    def git(self, *args, cwd=None):
        return sh("git", "-C", str(cwd or self.home), *args).strip()

    def move_a_command(self, where):
        """A commit in where that adds a command to the command line (cli/ changes): `_t11b`,
        which only the new build has."""
        where = Path(where)
        (where / "cli" / "src" / "main.rs").write_text("// moved: _t11b\nfn main() { _t11b() }\n")
        sh("git", "-C", str(where), "commit", "-qam", "_t11b is new")

    def task_moving_a_command(self):
        tid = self.start_task()
        wt = Path(self.show(tid)["worktree"])
        self.move_a_command(wt)
        self.bp("ready", tid, "--summary", "Moved _t11b.", cwd=wt)
        return tid, wt

    def wait_for(self, what, cond, wait=15):
        deadline = time.time() + wait
        while not cond():
            if time.time() > deadline:
                self.fail(f"{what}; news: {self.news_log()}")
            time.sleep(0.05)

    def assert_rebuilt_and_pushed(self, builds=2):
        self.wait_for("no push", lambda: self.git("rev-parse", "main", cwd=self.backup) == self.git("rev-parse", "main"))
        self.wait_for("no rebuild", lambda: "_t11b()" in (self.board("bullpen") or ""))
        self.settle()
        self.assertEqual(self.builds(), builds)
        self.assertEqual(self.bp("_t11b").stdout, "_t11b runs in the new build\n")

    def test_a_plain_accept_rebuilds_and_pushes(self):
        self.bp("config", "--review", "off")
        tid, wt = self.task_moving_a_command()
        out = self.bp("accept", tid).stdout
        self.assertIn("pushing main to backup in the background", out)
        self.assertIn("rebuilding bullpen's command line in the background", out)
        self.assert_rebuilt_and_pushed()

    def test_an_auto_accept_from_a_reviewer_whose_pane_closes_rebuilds_and_pushes(self):
        # BP-118: the verdict closes the reviewer's pane last, and Claude Code kills the
        # verdict's tree as it ends: the push and the build were still its children
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.task_moving_a_command()
        reviewer = self.show(tid)["review"]["block"]
        (self.fake / f"kills_tree_{reviewer}").write_text("")
        (self.fake / "cargo_slow").write_text("1")  # as for real, both take a while
        hook = self.backup / "hooks" / "pre-receive"
        hook.write_text("#!/bin/sh\nsleep 1\n")
        hook.chmod(0o755)
        self.bp("verdict", tid, "approve", "--summary", "Fine.", cwd=wt, ok=False)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assert_rebuilt_and_pushed()
        self.assertNotIn(" job: ", self.news_log())

    def old_build(self, *missing):
        """The build in place doesn't have these internal commands yet (the merge that adds
        them isn't built): it says the name is unknown to it, so the call catches up. (The
        name goes on as `_missing:NAME`, which every build of the fake cargo turns back.)"""
        build = self.target / "bullpen"
        real = self.target / "release" / "bullpen-real"
        line = f'case $1 in {"|".join(missing)}) n=$1; shift; exec "{real}" "_missing:$n" "$@" ;; esac  # old\n'
        build.write_text(build.read_text().replace("#!/bin/sh\n", "#!/bin/sh\n" + line, 1))
    def test_a_tour_through_the_old_build_catches_up_and_is_written(self):
        # BP-119: tour starts _tour in the background through a build that doesn't have it
        # yet: the job catches up
        import test_tour
        agent = self.fake / "tour-agent"
        agent.write_text(test_tour.FAKE_TOUR)
        agent.chmod(0o755)
        (self.fake / "tour_out").write_text('{"steps": [{"title": "The fix", "note": "", "hunks": []}]}')
        (self.tmp / "xdg" / "bullpen").mkdir(parents=True, exist_ok=True)
        (self.tmp / "xdg" / "bullpen" / "config.json").write_text(json.dumps({"tour": [str(agent)]}))
        tid = self.start_task()
        wt = Path(self.show(tid)["worktree"])
        (wt / "fix.txt").write_text("fixed\n")
        sh("git", "-C", str(wt), "add", ".")
        sh("git", "-C", str(wt), "commit", "-qm", "Fix")
        self.bp("config", "--review", "off")
        self.bp("ready", tid, "--summary", "Fixed.", cwd=wt)
        # a merge that adds _tour, not yet built: cli/ changes
        # (the new build answers _t11a, a name only it has, itself)
        (self.home / "cli" / "src" / "main.rs").write_text("// moved: _t11a\nfn main() { _t11a() }\n")
        self.git("commit", "-qam", "_tour is new")
        self.old_build("_tour")
        self.assertIn("writing a tour of", self.bp("tour", tid).stdout)
        self.wait_for("no tour", lambda: (self.show(tid).get("tour") or {}).get("state") == "written", wait=30)
        self.assertNotIn("# old", self.board("bullpen"))  # the new build is in place
        self.assertEqual(self.bp("_t11a").stdout, "_t11a runs in the new build\n")
        self.settle()
        self.assertNotIn(" job: ", self.news_log())

    def test_an_accept_through_the_old_build_pushes_with_the_new_build(self):
        # BP-119: the build that runs the accept doesn't have _push yet; the push catches up
        # (waits for accept's rebuild, or builds both programs itself, so accept's rebuild
        # skips), then the new build pushes
        self.bp("config", "--review", "off")
        tid, wt = self.task_moving_a_command()
        self.old_build("_push")
        self.bp("accept", tid)
        self.assert_rebuilt_and_pushed()
        self.assertNotIn("# old", self.board("bullpen"))  # the new build is in place
        state = next((self.tmp / "state" / "repos").iterdir())
        self.assertRegex((state / "jobs.log").read_text(),  # it caught up, either way
                         r"`_missing:_push` is (newer than this build: building it first|in the build that's running)")
        self.assertNotIn(" job: ", self.news_log())

    def test_a_gone_coordinators_look_through_the_old_build_catches_up(self):
        # BP-119: _gone starts the _coordinator_gone job through a build that doesn't have
        # it yet: the job builds first
        import json
        self.bp("up")
        cfg = next((self.tmp / "state" / "repos").glob("*/config.json"))
        block = json.loads(cfg.read_text())["coordinators"][0]["block"]
        (self.home / "cli" / "src" / "main.rs").write_text("// moved: _t11b\nfn main() { _t11b() }\n")
        self.git("commit", "-qam", "_coordinator_gone is new")
        self.old_build("_coordinator_gone")
        (self.fake / "alive" / block).unlink()  # its pane closed
        self.env["BULLPEN_GONE_WAIT"] = "0"  # look now, and wait for the look
        self.bp("_gone", block)
        self.assertEqual(json.loads(cfg.read_text())["coordinators"], [])  # dropped
        self.assertNotIn("# old", self.board("bullpen"))  # the new build is in place
        self.assertEqual(self.bp("_t11b").stdout, "_t11b runs in the new build\n")
        self.settle()
        self.assertNotIn(" job: ", self.news_log())

    def test_an_auto_accept_through_the_old_build_catches_up_and_merges_a_move(self):
        # BP-119: the build in place doesn't have _auto_accept (and _prs) yet (a merge that
        # adds them, not yet built). The verdict's job catches up, then the new build accepts
        # a task that changes the command line again: it merges, rebuilds again and pushes
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.task_moving_a_command()
        (self.home / "core" / "src" / "lib.rs").write_text("// _auto_accept's merge\n")
        self.git("commit", "-qam", "_auto_accept is new")
        self.old_build("_auto_accept", "_prs")
        self.bp("verdict", tid, "approve", "--summary", "Fine.", cwd=wt)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assert_rebuilt_and_pushed(builds=3)  # setUp's, the catch-up's, and the accept's
        self.assertNotIn("# old", self.board("bullpen"))  # the new build is in place
        state = next((self.tmp / "state" / "repos").iterdir())
        self.assertRegex((state / "jobs.log").read_text(),
                         r"`_missing:_auto_accept` is (newer than this build: building it first|in the build that's running)")
        self.assertNotIn(" job: ", self.news_log())

    def test_an_accept_finishes_on_its_own_build_after_the_new_one_is_in_place(self):
        # hard spot 9: the accept's rebuild puts a new command line in place while the
        # accept still runs (its run queue starts a task whose setup waits for that
        # build); the accept goes on to the end in the build it started in, and the next
        # call runs the new one
        self.bp("config", "--review", "off")
        tid, wt = self.task_moving_a_command()
        built = self.target / "bullpen"
        self.bp("config", "--setup", f"i=0; until grep -q '_t11b()' {built}; do sleep 0.05; i=$((i+1)); "
                f"[ $i -gt 300 ] && exit 1; done; echo the new build is in")
        nxt = self.bp("add", "Next", "--brief", "x", "--after", tid).stdout.strip()
        p = self.bp("accept", tid)
        self.assertIn(f"{tid} done", p.stdout)
        self.assertIn("rebuilding bullpen's command line in the background", p.stdout)
        self.assertEqual(p.stderr, "")
        self.assertEqual(self.show(nxt)["status"], "working")  # its setup saw the new build first
        state = next((self.tmp / "state" / "repos").iterdir())
        self.assertIn("the new build is in", (state / "briefs" / f"{nxt}.setup.log").read_text())
        self.assertIn("_t11b()", built.read_text())  # in place before the accept was done
        self.settle()
        self.assertEqual(self.bp("_t11b").stdout, "_t11b runs in the new build\n")
        self.assertNotIn(" job: ", self.news_log())

    def with_a_stuck_build(self, *args):
        """bullpen args while a build that never ends holds the lock (this test's pid in it)."""
        lock = (self.target / "build.lock").open("a")
        fcntl.flock(lock, fcntl.LOCK_EX)
        lock.truncate(0)
        lock.write(f"{os.getpid()}\n")
        lock.flush()
        env = {**self.env, "BULLPEN_BUILD_WAIT": "0.5"}
        try:
            return subprocess.run([str(self.bin), *args], cwd=self.home, env=env, text=True,
                                  capture_output=True, timeout=15)
        finally:
            lock.close()

    def assert_stuck(self, p):
        self.assertEqual(p.returncode, 1)
        self.assertIn(f"the build that's running (pid {os.getpid()}) still isn't done after 0.5s: "
                      f"if it's stuck, stop it (`kill {os.getpid()}`)", p.stderr)

    def test_a_moved_command_stops_waiting_for_a_build_that_never_ends(self):
        self.move_a_command(self.home)
        p = self.with_a_stuck_build("_t11b")
        self.assertIn("`_t11b` is in the build that's running: waiting for it", p.stderr)
        self.assert_stuck(p)
        self.assertEqual(p.stdout, "")

    def test_build_stops_waiting_for_a_build_that_never_ends(self):
        p = self.with_a_stuck_build("build")
        self.assertEqual(p.stdout, "another build of bullpen is running: waiting for it\n")
        self.assert_stuck(p)

    def failed_old_build(self):
        """A merge whose build failed: the old build in place, its source's build marked failed."""
        self.move_a_command(self.home)
        (self.fake / "cargo_fails").write_text("")
        self.bp("_t11b", ok=False)
        (self.fake / "cargo_fails").unlink()
        self.assertIn("its last build failed", self.bp("_t11b", ok=False).stderr)

    def test_build_rebuilds_a_failed_build_with_the_old_build(self):
        self.failed_old_build()
        self.assertIn("built bullpen's command line", self.bp("build").stdout)
        self.assertEqual(self.bp("_t11b").stdout, "_t11b runs in the new build\n")

    def test_install_rebuilds_a_failed_build_with_the_old_build(self):
        self.failed_old_build()
        p = self.install()
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(self.bp("_t11b").stdout, "_t11b runs in the new build\n")
        self.assertIn("_t11b()", self.board("bullpen"))

    def test_a_moved_command_before_its_build_builds_it_first(self):
        self.move_a_command(self.home)  # merged, not yet built: the build in place has no `_t11b`
        p = self.bp("_t11b")
        self.assertEqual(p.stdout, "_t11b runs in the new build\n")
        self.assertIn("`_t11b` is newer than this build: building it first", p.stderr)
        self.assertEqual(self.builds(), 2)
        self.assertEqual(self.bp("_t11b").stderr, "")  # the next call runs the new build at once

    def test_a_moved_command_in_a_fixture_never_touches_the_build(self):
        # BP-121: a test's call (BULLPEN_NO_BUILD) to a command the build doesn't have
        self.move_a_command(self.home)
        (self.target / "build.lock").unlink(missing_ok=True)
        before = {f.name: f.read_bytes() for f in self.target.iterdir() if f.is_file()}
        self.env["BULLPEN_NO_BUILD"] = "1"
        p = self.bp("_t11b", ok=False)
        self.assertEqual((p.returncode, p.stderr),
                         (2, "bullpen: no command `_t11b`. `bullpen --help` lists them.\n"))
        self.assertEqual(self.builds(), 1)  # setUp's: no cargo
        self.assertEqual({f.name: f.read_bytes() for f in self.target.iterdir() if f.is_file()}, before)  # no lock, no stamp
        self.assertIn("builds are off here", self.bp("build", ok=False).stderr)
        self.assertNotIn("rebuilding", self.bp("up").stdout)
        self.assertEqual(self.builds(), 1)
        self.assertFalse((self.target / "build.lock").exists())

    def test_a_job_of_a_call_that_caught_up_catches_up_too(self):
        self.move_a_command(self.home)
        self.env["BULLPEN_CAUGHT_UP"] = "1"  # its parent's: an auto-accept that caught up, say
        self.assertEqual(self.bp("_t11b").stdout, "_t11b runs in the new build\n")

    def test_a_moved_command_waits_for_the_build_that_is_running(self):
        self.move_a_command(self.home)
        lock = (self.target / "build.lock").open("a")
        fcntl.flock(lock, fcntl.LOCK_EX)  # a build is running
        call = subprocess.Popen([str(self.bin), "_t11b"], cwd=self.home, env=self.env, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        time.sleep(0.5)
        self.assertIsNone(call.poll())  # waiting on it
        lock.close()  # it's done (it didn't build: the call builds it)
        out, err = call.communicate(timeout=15)
        self.assertIn("`_t11b` is in the build that's running: waiting for it", err)
        self.assertEqual(out, "_t11b runs in the new build\n")

    def test_a_moved_command_with_a_failed_build_says_so(self):
        self.move_a_command(self.home)
        (self.fake / "cargo_fails").write_text("")
        p = self.bp("_t11b", ok=False)
        self.assertEqual(p.returncode, 2)
        self.assertIn("`_t11b` isn't in this build of bullpen: rebuild it with `bullpen build`", p.stderr)
        self.assertIn("its last build failed", p.stderr)
        p = self.bp("_t11b", ok=False)  # a source whose build failed isn't tried again on every call
        self.assertNotIn("building it first", p.stderr)
        self.assertEqual(self.builds(), 2)

    def test_a_job_that_fails_without_saying_so_is_news(self):
        self.bp("config", "--review", "off")
        tid = self.start_task()
        wt = Path(self.show(tid)["worktree"])
        (wt / "fix.txt").write_text("fixed\n")
        sh("git", "-C", str(wt), "add", ".")
        sh("git", "-C", str(wt), "commit", "-qm", "Fix")
        self.bp("ready", tid, "--summary", "Fixed.", cwd=wt)
        state = next((self.tmp / "state" / "repos").iterdir())
        (state / "push.lock").mkdir()  # the push dies on it, saying only "bullpen: ..." on stderr
        self.bp("accept", tid)
        self.wait_for("no news of the push", lambda: " job: " in self.news_log())
        line = next(l for l in self.news_log().splitlines() if " job: " in l)
        self.assertIn(f"{tid} job: bullpen · Pushing main to backup failed: bullpen: can't take the push lock "
                      f"{state / 'push.lock'}: Is a directory", line)
        self.assertIn(f"Log: {state / 'jobs.log'}", line)
        self.assertIn("Is a directory", (state / "jobs.log").read_text())

    def test_a_build_restarts_a_running_watcher(self):
        self.bp("up")
        before = self.watcher_block()
        self.move_a_command(self.home)
        self.bp("build")
        self.assertNotEqual(self.watcher_block(), before)

    def watcher_block(self):
        import json
        return json.loads((self.tmp / "state" / "watcher.json").read_text())["block"]


if __name__ == "__main__":
    unittest.main()
