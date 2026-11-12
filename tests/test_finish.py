"""finish and close: tasks with nothing to merge."""

from pathlib import Path

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module


class FinishTest(BullpenTest):
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
        self.assertIn("Finished, nothing to merge: Recorded the demo video. a closes it.", self.bp("ls").stdout)

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
        self.assertIn("not ready to accept or finished", self.bp("accept", tid, ok=False).stderr)
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
        self.assertIn("not ready to accept or finished", self.bp("accept", tid, ok=False).stderr)

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
        self.assertIn("BP-1 is working again (was finished)", self.news()[-1])
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
        self.assertIn("BP-1 is working again (was ready)", self.news()[-1])
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
