"""Dependencies and the run queue."""

import json

from support import setUpModule, tearDownModule, BullpenTest  # a temp folder for each module


class QueueTest(BullpenTest):
    def set_config(self, **kv):
        path = next((self.tmp / "state" / "repos").iterdir()) / "config.json"
        path.write_text(json.dumps({**json.loads(path.read_text()), **kv}))

    def test_add_after_takes_repeats_and_commas_and_refuses_unknown_ids(self):
        for title in ("A", "B", "C"):
            self.bp("add", title, "--brief", "x")
        self.bp("add", "D", "--brief", "x", "--after", "BP-1", "--after", "2,bp-3")
        self.assertEqual(self.show("BP-4")["after"], ["BP-1", "BP-2", "BP-3"])
        p = self.bp("add", "E", "--brief", "x", "--after", "BP-9", ok=False)
        self.assertIn("no task BP-9", p.stderr)
        self.assertEqual(self.bp("add", "E", "--brief", "x").stdout.strip(), "BP-5")  # no ID was used up
        self.assertNotIn("after", self.show("BP-5"))

    def test_after_sets_changes_and_clears_and_refuses_cycles(self):
        for title in ("A", "B", "C"):
            self.bp("add", title, "--brief", "x")
        self.bp("after", "BP-2", "BP-1")
        self.bp("after", "BP-3", "BP-2")
        self.assertEqual(self.show("BP-3")["after"], ["BP-2"])
        p = self.bp("after", "BP-1", "BP-3", ok=False)
        self.assertIn("BP-3 already waits on BP-1 (BP-3 → BP-2 → BP-1): that would be a cycle", p.stderr)
        self.assertIn("can't wait on itself", self.bp("after", "BP-1", "BP-1", ok=False).stderr)
        self.assertIn("no task BP-7", self.bp("after", "BP-1", "BP-7", ok=False).stderr)
        self.assertNotEqual(self.bp("after", "BP-1", ok=False).returncode, 0)  # neither tasks nor --none
        self.bp("after", "BP-3", "BP-1,BP-2")
        self.assertEqual(self.show("BP-3")["after"], ["BP-1", "BP-2"])
        self.bp("after", "BP-3", "--none")
        self.assertNotIn("after", self.show("BP-3"))

    def test_start_refuses_a_task_whose_dependencies_arent_done_unless_forced(self):
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x", "--after", "BP-1")
        p = self.bp("start", "BP-2", ok=False)
        self.assertIn("BP-2 waits on BP-1 (working)", p.stderr)
        self.assertIn("--force", p.stderr)
        self.assertEqual(self.show("BP-2")["status"], "todo")
        self.bp("start", "BP-2", "--force")
        self.assertEqual(self.show("BP-2")["status"], "working")
        self.assertIn("### BP-1: First (working, not accepted yet)", self.full_brief("BP-2"))

    def test_ls_and_show_say_what_a_todo_task_waits_on(self):
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x", "--after", "BP-1")
        self.assertIn("Second  now  Waits on BP-1.", self.bp("ls").stdout)
        self.assertIn("waits: Waits on BP-1", self.bp("show", "BP-2").stdout)
        self.bp("drop", "BP-1", "--force")
        self.assertIn("Waits on BP-1 (dropped).", self.bp("ls").stdout)

    def test_accept_starts_queued_tasks_in_id_order_within_max_running(self):
        tid = self.ready_with_a_commit()
        self.bp("add", "Plain", "--brief", "x")  # BP-2: no dependencies, waits for the coordinator
        for title in ("Next", "Then", "Later"):
            self.bp("add", title, "--brief", "x", "--after", tid)
        self.bp("add", "Other", "--brief", "x")
        self.bp("start", "BP-6")
        self.set_config(max_running=3)
        self.merge_on_github(tid)
        self.bp("accept", tid)
        status = {t: self.show(t)["status"] for t in ("BP-2", "BP-3", "BP-4", "BP-5")}
        self.assertEqual(status, {"BP-2": "todo", "BP-3": "working", "BP-4": "working", "BP-5": "todo"})
        news = self.news_lines()
        self.assertIn("bullpen · BP-3 started (BP-1 was accepted)", news)
        self.assertIn("bullpen · BP-4 started (BP-1 was accepted)", news)
        self.assertFalse(any("BP-5 started" in l or "BP-2 started" in l for l in news))
        # BP-6 is dropped: its slot goes to BP-5, which was queued; BP-2 still waits
        self.bp("drop", "BP-6", "--force")
        self.assertEqual(self.show("BP-5")["status"], "working")
        self.assertEqual(self.show("BP-2")["status"], "todo")
        self.assertIn("bullpen · BP-5 started (BP-6 was dropped, freeing a slot)", self.news_lines())

    def test_a_queued_task_starts_from_the_coordinators_pane_too(self):
        tid = self.ready_with_a_commit()
        self.bp("add", "Next", "--brief", "x", "--after", tid)
        self.env["REX_BLOCK"] = "blk1"
        self.merge_on_github(tid)
        self.bp("accept", tid)
        self.assertEqual(self.news_lines()[-1], "bullpen · BP-2 started (BP-1 was accepted)")

    def test_a_task_waits_for_all_its_dependencies(self):
        tid = self.ready_with_a_commit()
        self.bp("add", "Other", "--brief", "x")
        self.bp("add", "Both", "--brief", "x", "--after", f"{tid},BP-2")
        self.merge_on_github(tid)
        self.bp("accept", tid)
        self.assertEqual(self.show("BP-3")["status"], "todo")

    def test_a_dropped_dependency_starts_nothing_and_asks_once(self):
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x", "--after", "BP-1")
        self.bp("add", "Third", "--brief", "x", "--after", "BP-1")
        self.bp("drop", "BP-1", "--force")
        self.assertEqual((self.show("BP-2")["status"], self.show("BP-3")["status"]), ("todo", "todo"))
        asks = [l for l in self.news_lines() if "which was dropped" in l]
        self.assertEqual(len(asks), 1)
        self.assertIn("BP-2, BP-3 wait on BP-1, which was dropped, so they stay queued", asks[0])
        self.assertIn("bullpen after BP-2 --none", asks[0])
        self.assertIn("was dropped", self.bp("after", "BP-2", "BP-1", ok=False).stderr)

    def test_a_started_dependent_hears_what_landed(self):
        tid = self.ready_with_a_commit()
        briefs = next((self.tmp / "state" / "repos").iterdir()) / "briefs"
        (briefs / f"{tid}.review-1.findings.md").write_text("Looks fine.\n")
        self.bp("add", "Next", "--brief", "Build on it.", "--after", tid)
        self.merge_on_github(tid)
        self.bp("accept", tid)
        brief = self.full_brief("BP-2")
        self.assertIn("## What this builds on", brief)
        self.assertIn("### BP-1: Fix the thing (accepted, PR #7)", brief)
        self.assertIn("done", brief.split("### BP-1")[1])  # its summary
        self.assertIn(f"Review findings: {briefs / 'BP-1.review-1.findings.md'}", brief)
        self.assertLess(brief.index("Build on it."), brief.index("## What this builds on"))
        self.assertNotIn("## What this builds on", self.full_brief("BP-1"))  # no dependencies, no section

    def test_queued_tasks_wait_at_the_usage_limit(self):
        tid = self.ready_with_a_commit()
        self.bp("add", "Next", "--brief", "x", "--after", tid)
        (self.tmp / "state" / "limit.json").write_text(json.dumps(
            {"at": "2026-01-01T00:00:00+00:00", "resets": "2999-01-01T00:00:00+00:00", "text": "x"}))
        self.merge_on_github(tid)
        self.bp("accept", tid)
        self.assertEqual(self.show("BP-2")["status"], "todo")
        self.assertIn("BP-2 can start (BP-1 was accepted), but the usage limit is reached", self.news_lines()[-1])

    def test_review_frees_a_slot_and_a_task_sent_back_takes_one_again(self):
        self.bp("config", "--max-running", "1")
        self.start_task("First")  # BP-1 works: the one slot is taken
        dep = self.done_dependency()  # BP-2
        self.bp("add", "Next", "--brief", "x", "--after", dep)  # BP-3: queued, nothing frees a slot yet
        self.assertEqual(self.show("BP-3")["status"], "todo")
        # BP-1 goes to review: review doesn't take a slot, so BP-3 starts
        self.commit_and_ready("BP-1")
        self.assertEqual((self.show("BP-1")["status"], self.show("BP-3")["status"]), ("review", "working"))
        self.assertIn("bullpen · BP-3 started (BP-1 went to review, freeing a slot)", self.news_lines())
        self.bp("add", "Later", "--brief", "x", "--after", dep)  # BP-4
        # sent back with findings, BP-1 works again, over the limit: allowed, it's finishing its work
        self.verdict("BP-1", "changes", "Misses the empty case.")
        self.assertEqual(self.show("BP-1")["status"], "working")
        # BP-3 goes to review, but BP-1 still takes the one slot: BP-4 waits
        self.commit_and_ready("BP-3")
        self.assertEqual((self.show("BP-3")["status"], self.show("BP-4")["status"]), ("review", "todo"))
        self.assertFalse(any("BP-4 started" in l for l in self.news_lines()))
        # BP-1 back in review: nothing works, so BP-4 starts
        self.commit_and_ready("BP-1", 2)
        self.assertEqual(self.show("BP-4")["status"], "working")
        self.assertIn("bullpen · BP-4 started (BP-1 went to review, freeing a slot)", self.news_lines())

    def test_the_queue_keeps_to_max_running_among_working_tasks(self):
        self.bp("config", "--max-running", "2")
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        dep = self.done_dependency()  # BP-3
        for title in ("A", "B", "C"):  # BP-4, BP-5, BP-6
            self.bp("add", title, "--brief", "x", "--after", dep)
        self.commit_and_ready("BP-1")  # BP-2 still works: one slot, for BP-4
        status = {t: self.show(t)["status"] for t in ("BP-4", "BP-5", "BP-6")}
        self.assertEqual(status, {"BP-4": "working", "BP-5": "todo", "BP-6": "todo"})

    def test_ready_without_review_and_finish_free_a_slot(self):
        self.bp("config", "--review", "off")
        self.bp("config", "--max-running", "1")
        self.start_task("First")
        dep = self.done_dependency()  # BP-2
        self.bp("add", "Next", "--brief", "x", "--after", dep)  # BP-3
        self.commit_and_ready_no_review("BP-1")
        self.assertEqual(self.show("BP-3")["status"], "working")
        self.assertIn("bullpen · BP-3 started (BP-1 is ready, freeing a slot)", self.news_lines())
        self.bp("add", "Then", "--brief", "x", "--after", dep)  # BP-4
        self.bp("finish", "BP-3", "--summary", "Answered.", cwd=self.show("BP-3")["worktree"])
        self.assertEqual(self.show("BP-4")["status"], "working")
        self.assertIn("bullpen · BP-4 started (BP-3 finished, freeing a slot)", self.news_lines())

    def test_the_queue_starts_after_the_news_of_what_freed_the_slot(self):
        tid = self.ready_with_a_commit()
        self.bp("add", "Next", "--brief", "x", "--after", tid)
        self.merge_on_github(tid)
        self.bp("accept", tid)
        news = self.news_lines()
        accepted = news.index("bullpen · BP-1 accepted (merged PR #7, rebase)")
        self.assertGreater(news.index("bullpen · BP-2 started (BP-1 was accepted)"), accepted)

    def test_a_stop_frees_no_slot(self):
        # a worker waiting on the person comes back to its task: its slot stays taken
        self.bp("config", "--max-running", "1")
        self.start_task("First")
        dep = self.done_dependency()  # BP-2
        self.bp("add", "Next", "--brief", "x", "--after", dep)  # BP-3
        self.ask("BP-1", "Which one?")
        self.assertEqual(self.show("BP-1")["status"], "needs")
        self.assertEqual(self.show("BP-3")["status"], "todo")

    def test_the_queue_moves_by_bullpen_queue(self):
        # what a save that frees a slot runs once its command is done (at later-time)
        dep = self.done_dependency()  # BP-1
        self.bp("add", "Next", "--brief", "x", "--after", dep)  # BP-2: added after BP-1 was done
        self.assertEqual(self.show("BP-2")["status"], "todo")
        self.bp("_queue", "--repo", str(self.repo), "--by", dep, "--how", "accepted")
        self.assertEqual(self.show("BP-2")["status"], "working")
        log = next((self.tmp / "state" / "repos").iterdir()) / "news.log"
        self.assertIn(f"bullpen · BP-2 started ({dep} was accepted)", log.read_text())
