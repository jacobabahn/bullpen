"""Worktree setup and teardown."""

import json
import os
from pathlib import Path

from support import setUpModule, tearDownModule, BullpenTest  # a temp folder for each module


class WorktreeTest(BullpenTest):
    def test_setup_runs_in_the_new_worktree_before_the_worker(self):
        (self.repo / ".env").write_text("SECRET=1\n")  # the main checkout's, not committed
        self.commit_hook("setup", "#!/bin/sh\necho setting up\ncp \"$BULLPEN_MAIN/.env\" .env\n"
                         "echo \"$BULLPEN_TASK $BULLPEN_BRANCH $BULLPEN_WORKTREE $(pwd)\" > ../setup-ran\n")
        tid = self.start_task()
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        self.assertNotIn("needs_setup", t)
        wt = t["worktree"]
        self.assertEqual((Path(wt) / ".env").read_text(), "SECRET=1\n")
        ran = (Path(wt).parent / "setup-ran").read_text().split()
        self.assertEqual(ran[:3], [tid, t["branch"], wt])
        self.assertEqual(os.path.realpath(ran[3]), os.path.realpath(wt))
        log = next((self.tmp / "state").rglob(f"{tid}.setup.log"))
        self.assertIn("setting up", log.read_text())
        self.assertIn(".bullpen/setup.sh on origin/main", log.read_text())
        self.assertIn(f"setup: {log}", self.bp("show", tid).stdout)

    def test_a_failed_setup_puts_the_task_in_triage_without_a_worker(self):
        self.commit_hook("setup", "echo no npm here\nexit 3\n")
        self.bp("up")
        tid = self.bp("add", "Fix the thing", "--brief", "Make it work.").stdout.strip()
        p = self.bp("start", tid, ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("setup failed", p.stderr)
        t = self.show(tid)
        log = next((self.tmp / "state").rglob(f"{tid}.setup.log"))
        self.assertEqual(t["status"], "triage")
        self.assertEqual(t["stop"], {"by": "worker", "kind": "setup"})
        self.assertIn(str(log), t["why"])
        self.assertIsNone(t["block"])
        self.assertTrue(t["needs_setup"])
        self.assertEqual(len(self.api("session.new_layer")), 0)  # no worker pane
        self.assertIn("no npm here", log.read_text())
        self.assertIn(f"For the coordinator: {tid}'s setup failed (exit 3), so its worker didn't start: "
                      f"see {log}", self.news_log())
        self.assertIn("(failed: the worker didn't start)", self.bp("show", tid).stdout)
        # fixed in the person's config: start sets it up again and starts the worker
        self.bp("config", "--setup", "echo fixed")
        self.bp("start", tid)
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        self.assertNotIn("needs_setup", t)
        self.assertIn("fixed", log.read_text())
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertNotIn("--continue", cmd)

    def test_start_after_bullpen_died_in_setup_sets_it_up_again(self):
        # the first setup kills bullpen itself (its parent), as Ctrl-C would
        self.bp("config", "--setup", "if [ -e ../died ]; then echo ok; else touch ../died; kill -9 $PPID; fi")
        self.bp("up")
        tid = self.bp("add", "Fix the thing", "--brief", "Make it work.").stdout.strip()
        self.assertNotEqual(self.bp("start", tid, ok=False).returncode, 0)
        t = self.show(tid)
        self.assertEqual(t["status"], "todo")
        self.assertTrue(t["worktree"] and t["needs_setup"])  # it knows its worktree
        self.bp("start", tid)
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        self.assertNotIn("needs_setup", t)

    def test_resume_after_a_failed_setup_sets_it_up_and_starts_afresh(self):
        self.bp("config", "--setup", "exit 1")
        self.bp("up")
        tid = self.bp("add", "Fix the thing", "--brief", "Make it work.").stdout.strip()
        self.bp("start", tid, ok=False)
        self.assertEqual(self.show(tid)["status"], "triage")
        self.bp("config", "--setup", "true")
        self.bp("resume", tid)
        self.assertEqual(self.show(tid)["status"], "working")
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertNotIn("--continue", cmd)
        self.assertTrue(any("Read your brief" in x for x in cmd))

    def test_resume_of_a_set_up_worktree_doesnt_set_it_up_again(self):
        self.bp("config", "--setup", "echo x >> ../setup-count")
        tid = self.start_task()
        wt = Path(self.show(tid)["worktree"])
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)
        self.bp("resume", tid)
        self.assertEqual((wt.parent / "setup-count").read_text(), "x\n")

    def test_the_config_setup_runs_over_the_committed_file(self):
        self.commit_hook("setup", "touch from-file\n")
        self.bp("up")
        self.bp("config", "--setup", "touch from-config")
        tid = self.bp("add", "Fix the thing", "--brief", "Make it work.").stdout.strip()
        p = self.bp("start", tid)
        self.assertIn("running config.json's setup, over .bullpen/setup.sh on origin/main", p.stderr)
        wt = Path(self.show(tid)["worktree"])
        self.assertTrue((wt / "from-config").exists())
        self.assertFalse((wt / "from-file").exists())
        log = next((self.tmp / "state").rglob(f"{tid}.setup.log")).read_text()
        self.assertIn("config.json's setup (over .bullpen/setup.sh on origin/main)", log)
        # cleared, the committed file is used again
        self.bp("config", "--setup", "")
        self.assertIsNone(json.loads(self.bp("config", "--json").stdout)["setup"])
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        self.assertTrue((Path(self.show(tid2)["worktree"]) / "from-file").exists())

    def test_teardown_runs_before_drop_removes_the_worktree(self):
        self.commit_hook("teardown", "echo \"$BULLPEN_TASK $(ls)\" > ../torn-down\ntouch leftover\n")
        tid = self.start_task()
        wt = Path(self.show(tid)["worktree"])
        self.bp("drop", tid)  # what teardown leaves doesn't stop the removal
        self.assertFalse(wt.exists())
        self.assertIn(tid, (wt.parent / "torn-down").read_text())
        self.assertIn("README", (wt.parent / "torn-down").read_text())
        self.assertEqual(self.show(tid)["status"], "dropped")
        log = next((self.tmp / "state").rglob(f"{tid}.teardown.log"))
        self.assertIn(f"teardown: {log}", self.bp("show", tid).stdout)

    def test_a_failed_teardown_never_blocks_accept(self):
        self.bp("config", "--teardown", "echo oops; exit 2")
        tid = self.ready_with_a_commit()
        wt = Path(self.show(tid)["worktree"])
        p = self.bp("accept", tid)
        self.assertIn("teardown failed (exit 2)", p.stderr)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertFalse(wt.exists())
        log = next((self.tmp / "state").rglob(f"{tid}.teardown.log"))
        self.assertIn("oops", log.read_text())


class FooterTest(BullpenTest):
    """The worker's instructions (prompts/worker-footer.md), filled in for its task."""

    def footer(self, tid):
        return self.full_brief(tid).split("## How you work (bullpen)")[1]

    def test_the_footer_names_the_task_its_worktree_and_the_places_to_leave_alone(self):
        tid = self.start_task()
        t = self.show(tid)
        foot = self.footer(tid)
        self.assertIn(f'You are the worker for task {tid} ("Fix the thing").', foot)
        self.assertIn(f"`{t['worktree']}`, on branch `{t['branch']}` (based on `main`)", foot)
        self.assertIn(f"`{self.repo}` is the main checkout", foot)
        briefs = next((self.tmp / "state").rglob(f"{tid}.full.md")).parent
        self.assertIn(f"goes in `{briefs}`, not\n  `/tmp`", foot)
        self.assertIn("fails on `main` too", foot)
        self.assertNotRegex(foot, r"\{[a-z]+\}")  # every field filled in

    def test_the_footer_says_how_to_end_each_kind_of_task(self):
        tid = self.start_task()
        foot = self.footer(tid)
        for end in ("ready", "finish", "finish", "finish --keep-branch"):
            self.assertRegex(foot, rf"/bullpen {end.split()[0]} {tid} " + ("--keep-branch " if "keep" in end else "")
                             + "--summary-file <that file>")
        self.assertIn("only when your brief asks for the branch to be kept", foot)

    def test_the_footer_says_who_types_and_what_not_to_do(self):
        foot = self.footer(self.start_task())
        self.assertIn("come from the coordinator (they start `Coordinator: `)", foot)
        self.assertIn("Sub-agents are fine for reading and searching", foot)
        self.assertIn("two writers in one worktree collide", foot)
        self.assertIn("Run the full suite\n  once, right before `ready`", foot)
        self.assertIn("Don't schedule wake-ups", foot)
        self.assertIn(' later "<a short title>"`', foot)  # the backlog line


class TellTest(BullpenTest):
    """`tell` says who is typing: `Coordinator: ` from a coordinator's pane, bare from the person."""

    def test_tell_from_a_coordinators_pane_starts_with_coordinator(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp_in("blk1", "tell", tid, "Also cover", "the logout path.")
        self.assertEqual(self.typed()[-1], (blk, "Coordinator: Also cover the logout path."))
        self.bp_in("blk1", "tell", tid, "Coordinator: once is enough.")
        self.assertEqual(self.typed()[-1], (blk, "Coordinator: once is enough."))

    def test_tell_from_anywhere_else_types_the_words_bare(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("tell", tid, "Also cover the logout path.")  # the person's own pane
        self.assertEqual(self.typed()[-1], (blk, "Also cover the logout path."))
        self.bp_in("blk2", "tell", tid, "From the board.")  # the board beside the coordinator: the person's keys
        self.assertEqual(self.typed()[-1], (blk, "From the board."))
        del self.env["REX_BLOCK"]  # no pane at all
        self.bp("tell", tid, "From a script.")
        self.assertEqual(self.typed()[-1], (blk, "From a script."))
