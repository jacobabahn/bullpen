"""Cheaper review: kept approvals, follow-up rounds, light reviews."""

import json
import subprocess
import time
from pathlib import Path

from support import setUpModule, tearDownModule, sh, BullpenTest, BULLPEN  # a temp folder for each module


class ReviewTest(BullpenTest):
    def review_prompt(self, tid, n):  # n: 2, or "1.rebase" for a rebase check
        return next((self.tmp / "state").rglob(f"{tid}.review-{n}.md")).read_text()

    def test_an_approval_is_kept_when_ready_runs_again_on_the_same_commit(self):
        tid = self.approved_task()
        blk = self.block(tid)
        self.bp("_event", blk, "idle", "", "")
        self.bp("_event", blk, "working", "", "A stray message")
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["verdict"]), ("working", "approved"))
        self.assertIn("its approval holds if the code doesn't change", self.news()[-1])
        out = self.ready(tid).stdout
        self.assertIn("approval carries over (the same commit)", out)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["round"], t["review"]["verdict"]), ("ready", 1, "approved"))
        self.assertEqual(list((self.tmp / "state").rglob(f"{tid}.review-2.md")), [])  # no new round
        self.assertEqual(self.news()[-1], "bullpen · BP-1 is ready again: Fix the thing (1 commit, PR #7, "
                                          "its approval carried over: the same commit)")
        self.assertIn("kept 1 time as the code didn't change", self.bp("show", tid).stdout)

    def test_a_turn_that_changes_no_code_leaves_a_ready_task_ready(self):
        tid = self.approved_task()
        blk = self.block(tid)
        self.bp("_event", blk, "idle", "", "")
        self.bp("_event", blk, "working", "", "Answering a question")
        self.bp("_event", blk, "idle", "", "")
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["verdict"]), ("ready", "approved"))
        self.assertIn("BP-1 is ready again: Fix the thing (its worker's turn left the code as approved: the same commit)",
                      self.news()[-1])
        # a turn that does change it ends as usual, and its next ready is reviewed again
        self.bp("_event", blk, "working", "", "Doing more")
        self.commit_file(tid, "more.txt", "more\n")
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.ready(tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["kind"], r["verdict"]), (2, "followup", None))

    def test_an_approval_is_kept_after_a_clean_rebase_with_the_same_patch(self):
        tid = self.approved_task()
        wt = self.show(tid)["worktree"]
        (self.repo / "other.txt").write_text("other\n")
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "Other work")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")
        before = sh("git", "-C", wt, "rev-parse", "HEAD")
        sh("git", "-C", wt, "fetch", "-q", "origin")
        sh("git", "-C", wt, "rebase", "-q", "origin/main")
        self.assertNotEqual(sh("git", "-C", wt, "rev-parse", "HEAD"), before)
        self.bp("tell", tid, "Rebase onto main.")
        self.assertIn("same patch after a clean rebase onto origin/main", self.ready(tid).stdout)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["round"]), ("ready", 1))
        self.assertEqual(t["review"]["commit"], sh("git", "-C", wt, "rev-parse", "HEAD").strip())

    def test_a_real_code_change_is_reviewed_again(self):
        tid = self.approved_task()
        self.bp("tell", tid, "Handle the empty case too.")
        self.commit_file(tid, "fix1.txt", "fixed better\n")
        self.assertIn("A reviewer checks it now (follow-up review)", self.ready(tid).stdout)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["round"], t["review"]["kind"]), ("review", 2, "followup"))
        prompt = self.review_prompt(tid, 2)
        self.assertIn("Round 1 approved the change. Its notes", prompt)
        self.assertIn("Looks right.", prompt)
        self.assertIn("+fixed better", prompt)

    def test_round_two_follows_up_on_the_findings_with_the_diff_since(self):
        self.bp("config", "--reviewer-model", "opus")
        tid = self.start_task()
        self.bp("review-mode", tid, "full")
        self.commit_file(tid, "big.py", "print('one')\n")
        self.ready(tid)
        self.assertEqual(self.models(self.last_command()), ["opus"])  # the first round: the reviewer model
        first = self.show(tid)["review"]["commit"]
        self.verdict(tid, "changes", "big.py misses the empty case.")
        self.commit_file(tid, "big.py", "print('one')\nprint('empty')\n", "Handle the empty case")
        self.ready(tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["kind"], r["model"]), (2, "followup", "sonnet"))
        self.assertEqual(self.models(self.last_command()), ["sonnet"])
        prompt = self.review_prompt(tid, 2)
        self.assertIn("round 2 of 2 (follow-up review)", prompt)
        self.assertIn("Round 1 asked for changes. Its findings", prompt)
        self.assertIn("big.py misses the empty case.", prompt)
        self.assertIn(f"diff {first[:12]}..HEAD", prompt)
        self.assertIn("+print('empty')", prompt)
        self.assertNotIn("+print('one')", prompt)  # round 1 saw that: not in the diff since
        self.assertIn("confirm that each finding is fixed", prompt)
        self.bp("config", "--reviewer-followup-model", "haiku")
        self.verdict(tid, "approve")
        self.assertIn("approved by follow-up review", self.news()[-1])

    def test_a_small_or_docs_only_change_gets_a_light_review(self):
        tid = self.start_task()
        self.commit_file(tid, "fix.py", "x = 1\n")
        self.ready(tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["kind"], r["why"], r["model"]), ("light", "1 line in 1 file", "sonnet"))
        self.assertIn("This is a light review", self.review_prompt(tid, 1))
        self.assertIn("review:", self.bp("show", tid).stdout)
        self.assertIn("(light review (1 line in 1 file) on sonnet)", self.bp("show", tid).stdout)
        self.verdict(tid, "approve")
        self.assertIn("Fix the thing (1 commit, PR #7, approved by light review: 1 line in 1 file)", self.news()[-1])
        self.bp("add", "Docs", "--brief", "x")
        self.bp("start", "BP-2")
        self.commit_file("BP-2", "docs/guide.md", "line\n" * 300)
        self.commit_file("BP-2", "prompts/x.txt", "line\n" * 300)
        self.ready("BP-2")
        self.assertEqual(self.show("BP-2")["review"]["kind"], "light")
        self.assertEqual(self.show("BP-2")["review"]["why"], "docs only")

    def test_a_bigger_change_gets_a_full_review(self):
        self.bp("config", "--light-review-lines", "40")
        tid = self.start_task()
        self.commit_file(tid, "big.py", "x = 1\n" * 41)
        self.ready(tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["kind"], r["model"]), ("full", None))
        self.assertNotIn("--model", self.last_command())
        self.assertIn("What to check", self.review_prompt(tid, 1))
        self.assertNotIn("light review", self.review_prompt(tid, 1))
        # small, but in three files: full too
        self.bp("add", "Three", "--brief", "x")
        self.bp("start", "BP-2")
        for name in ("a.py", "b.py", "c.py"):
            self.commit_file("BP-2", name, "x\n")
        self.ready("BP-2")
        self.assertEqual(self.show("BP-2")["review"]["kind"], "full")

    def test_the_review_mode_overrides_auto_both_ways(self):
        self.bp("up")
        tid = self.bp("add", "Small", "--brief", "x", "--review", "full").stdout.strip()
        self.bp("start", tid)
        self.commit_file(tid, "fix.py", "x = 1\n")
        self.ready(tid)
        self.assertEqual(self.show(tid)["review"]["kind"], "full")
        self.assertIn("mode: full first review", self.bp("show", tid).stdout)
        self.bp("add", "Big", "--brief", "x")
        self.bp("review-mode", "BP-2", "light")
        self.bp("start", "BP-2")
        self.commit_file("BP-2", "big.py", "x = 1\n" * 200)
        self.ready("BP-2")
        r = self.show("BP-2")["review"]
        self.assertEqual((r["kind"], r["why"]), ("light", "set for BP-2"))
        self.bp("review-mode", "BP-2", "auto")
        self.assertNotIn("review_mode", self.show("BP-2"))
        self.assertNotEqual(self.bp("review-mode", "BP-2", "medium", ok=False).returncode, 0)

    def test_a_light_review_that_asks_for_changes_is_followed_up(self):
        tid = self.start_task()
        self.commit_file(tid, "fix.py", "x = 1\n")
        self.ready(tid)
        self.verdict(tid, "changes", "x should be 2.")
        self.commit_file(tid, "fix.py", "x = 2\n")
        self.ready(tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["kind"]), (2, "followup"))
        self.assertIn("x should be 2.", self.review_prompt(tid, 2))

    def test_the_reviewer_gets_a_scratch_folder_the_checkout_and_the_suite_rule_once(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        self.commit_file(tid, "fix.py", "x = 1\n")
        self.ready(tid)
        scratch = Path(self.show(tid)["review"]["scratch"])
        self.assertTrue(scratch.is_dir())
        self.assertFalse(scratch.is_relative_to(wt))
        for n, v in ((1, "changes"), (2, "approve")):
            prompt = self.review_prompt(tid, n)
            self.assertNotIn("{", prompt.replace("{}", ""))  # every placeholder filled
            self.assertIn(f"Your own files go in\n  `{scratch}`", prompt)
            self.assertIn(f"don't run anything against `{Path(BULLPEN).parent.parent}`", prompt)
            self.assertIn(f"`cd {wt}` first", prompt)
            self.assertEqual(prompt.count("suite"), 2)  # the one rule: "a whole suite", "the full suite"
            (scratch / "notes.md").write_text("mine\n")
            self.verdict(tid, v, "x should be 2.")
            self.assertFalse(scratch.exists())  # the findings are copied; its own files go
            if n == 1:
                self.commit_file(tid, "fix.py", "x = 2\n")
                self.ready(tid)
                self.assertTrue(scratch.is_dir())  # a new round, a new one
        for kind in ("full", "light", "followup", "tour", "rebase"):
            self.assertNotIn("suite", (Path(BULLPEN).parent.parent / "prompts" / f"review-{kind}.md").read_text())

    def test_a_replaced_reviewer_keeps_its_rounds_kind(self):
        self.set_commands(review_rounds=3)
        tid = self.start_task()
        self.commit_file(tid, "fix.py", "x = 1\n")
        self.ready(tid)
        self.verdict(tid, "changes", "x should be 2.")
        self.commit_file(tid, "fix.py", "x = 2\n")
        self.ready(tid)
        reviewer = self.show(tid)["review"]["block"]
        self.bp("_event", reviewer, "idle", "", "")
        self.bp("review", tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["kind"], r["prev"]["round"]), (3, "followup", 1))
        self.assertIn("x should be 2.", self.review_prompt(tid, 3))

    def test_ready_again_during_review_of_the_same_commit_leaves_the_reviewer(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        out = self.ready(tid).stdout
        self.assertIn("A reviewer is already checking this commit", out)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["block"]), (1, reviewer))
        self.assertTrue((self.fake / "alive" / reviewer).exists())

    def test_a_reviewer_with_a_background_shell_stays_in_review(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        (self.fake / f"screen_{reviewer}").write_text("❯ \n  ⏵⏵ auto mode on · 1 shell\n")
        self.bp("_event", reviewer, "idle", "", "Waiting for the tests")
        self.assertEqual(self.show(tid)["status"], "review")
        self.assertFalse([l for l in self.news() if "without a verdict" in l])
        self.agent(reviewer, "idle", ago=5 * 60)
        self.assertEqual(self.check(), [])
        self.agent(reviewer, "idle", ago=30 * 60)  # past stuck_after: not the tests any more
        self.check("--fix", "--quiet")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Reviewer stopped without a verdict"))
        self.assertIn("background shell of its own still runs", self.news()[-1])
        # its shell ended, and so did its turn: that's a stop
        (self.fake / f"screen_{reviewer}").unlink()
        self.bp("_event", reviewer, "working", "", "")
        self.bp("_event", reviewer, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")

    def test_after_accept_an_approval_survives_a_clean_rebase(self):
        tid = self.approved_task()
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        self.commit_file("BP-2", "two.txt", "two\n")
        self.ready("BP-2")
        self.verdict("BP-2", "approve")
        self.merge_on_github(tid)
        self.bp("accept", tid)
        t = self.show("BP-2")
        self.assertEqual((t["status"], t["review"]["round"]), ("ready", 1))
        self.assertIn("BP-2 rebased; its approval carried over (the same patch after a clean rebase onto "
                      "origin/main)", self.news()[-1])

    def base_file(self, name, text):
        """A file on origin's main, before the tasks start."""
        (self.repo / name).parent.mkdir(parents=True, exist_ok=True)
        (self.repo / name).write_text(text)
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", f"Add {name}")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")

    def two_approved(self, name, first, second):
        """BP-1 and BP-2 each change file name (to first, second) and are approved, BP-2's
        worker idle; then BP-1 is accepted, which rebases BP-2. Returns BP-2's worktree."""
        tid = self.start_task()
        self.commit_file(tid, name, first)
        self.ready(tid)
        self.verdict(tid, "approve")
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        self.bp("_event", self.block("BP-2"), "working", "", "")
        wt = self.commit_file("BP-2", name, second)
        self.ready("BP-2")
        self.verdict("BP-2", "approve")
        self.bp("_event", self.block("BP-2"), "idle", "", "")
        self.merge_on_github(tid)
        self.bp("accept", tid)
        return wt

    def resolve(self, wt, files, more=None):
        """The worker rebases BP-2 onto origin/main by hand and resolves the conflict:
        {path: text}; more is a file it changes besides."""
        sh("git", "-C", wt, "fetch", "-q", "origin")
        p = subprocess.run(["git", "-C", wt, "rebase", "origin/main"], capture_output=True, text=True)
        self.assertNotEqual(p.returncode, 0)
        for name, text in files.items():
            (Path(wt) / name).write_text(text)
        sh("git", "-C", wt, "add", ".")
        subprocess.run(["git", "-C", wt, "-c", "core.editor=true", "rebase", "--continue"], check=True,
                       capture_output=True, env={**self.env, "GIT_EDITOR": "true"})
        if more:
            self.commit_file("BP-2", more, "more\n")

    def test_after_accept_a_rebase_that_changes_only_context_keeps_the_approval(self):
        self.base_file("list.txt", "a\nb\nc\nd\ne\nf\n")
        # two lines apart: it rebases cleanly, but BP-1's line is in BP-2's patch's context
        self.two_approved("list.txt", "A\nb\nc\nd\ne\nf\n", "a\nb\nc\nD\ne\nf\n")
        t = self.show("BP-2")
        self.assertEqual((t["status"], t["review"]["round"], t["review"]["verdict"]), ("ready", 1, "approved"))
        self.assertEqual(list((self.tmp / "state").rglob("BP-2.review-2*")), [])  # no reviewer
        self.assertIn("BP-2 rebased; its approval carried over (only the base moved)", self.news()[-1])

    def test_after_accept_a_conflict_resolved_by_keeping_both_sides_keeps_the_approval(self):
        self.base_file("list.txt", "a\nb\n")
        wt = self.two_approved("list.txt", "a\nb\nc\n", "a\nb\nd\n")
        t = self.show("BP-2")
        self.assertEqual((t["status"], t["review"]["conflict"]), ("working", ["list.txt"]))
        self.resolve(wt, {"list.txt": "a\nb\nc\nd\n"})
        self.assertIn("approval carries over (only the base moved)", self.ready("BP-2").stdout)
        t = self.show("BP-2")
        self.assertEqual((t["status"], t["review"]["round"]), ("ready", 1))
        self.assertNotIn("conflict", t["review"])
        self.assertIn("BP-2 is ready again after a rebase: Second (1 commit, PR #7, its approval carried over: "
                      "only the base moved)", self.news()[-1])

    def test_after_accept_a_conflict_resolved_by_hand_gets_a_rebase_check(self):
        self.base_file("list.txt", "a\nb\n")
        wt = self.two_approved("list.txt", "a\nb\nc\n", "a\nb\nd\n")
        self.resolve(wt, {"list.txt": "a\nb\nd\n"})  # drops BP-1's line: its own lines changed there
        self.assertIn("A reviewer checks it now (rebase check)", self.ready("BP-2").stdout)
        r = self.show("BP-2")["review"]
        self.assertEqual((r["round"], r["kind"], r["model"], r["rebase"]), (1, "rebase", "sonnet", ["list.txt"]))
        prompt = self.review_prompt("BP-2", "1.rebase")
        self.assertIn("round 1 of 2 (rebase check)", prompt)
        self.assertIn("This is a rebase check", prompt)
        self.assertIn("conflicts were resolved", prompt)
        self.assertIn("<<<<<<<", prompt)  # the conflicts as the rebase met them
        self.assertIn(" b\n-c\n+d", prompt)  # the change in list.txt now
        self.assertIn("range-diff", prompt)
        self.verdict("BP-2", "approve")
        t = self.show("BP-2")
        self.assertEqual((t["status"], t["review"]["round"]), ("ready", 1))
        self.assertIn("a rebase check approved the conflict resolution", self.news()[-1])

    def test_a_rebase_check_that_asks_for_changes_sends_it_back_without_a_round(self):
        self.base_file("list.txt", "a\nb\n")
        wt = self.two_approved("list.txt", "a\nb\nc\n", "a\nb\nd\n")
        self.resolve(wt, {"list.txt": "a\nb\nd\n"})
        self.ready("BP-2")
        self.verdict("BP-2", "changes", "BP-1's c is gone.")
        t = self.show("BP-2")
        self.assertEqual((t["status"], t["review"]["round"]), ("working", 1))
        self.assertIn("A rebase check says the conflict resolution isn't right", self.typed()[-1][1])
        self.commit_file("BP-2", "list.txt", "a\nb\nc\nd\n")
        self.ready("BP-2")
        r = self.show("BP-2")["review"]
        self.assertEqual((r["round"], r["kind"]), (2, "followup"))

    def test_after_accept_a_worker_that_changed_more_than_the_conflict_gets_a_follow_up(self):
        self.base_file("list.txt", "a\nb\n")
        wt = self.two_approved("list.txt", "a\nb\nc\n", "a\nb\nd\n")
        self.resolve(wt, {"list.txt": "a\nb\nc\nd\n"}, more="other.txt")
        self.assertIn("A reviewer checks it now (follow-up review)", self.ready("BP-2").stdout)
        r = self.show("BP-2")["review"]
        self.assertEqual((r["round"], r["kind"]), (2, "followup"))

    def test_after_accept_both_sides_adding_to_a_doc_are_kept_by_bullpen(self):
        self.base_file("docs/notes.md", "# Notes\n\nOne\n=======\n")
        wt = self.two_approved("docs/notes.md", "# Notes\n\nOne\n=======\nFrom BP-1\n",
                               "# Notes\n\nOne\n=======\nFrom BP-2\n")
        t = self.show("BP-2")
        self.assertEqual((t["status"], t["review"]["round"], t["review"]["verdict"]), ("ready", 1, "approved"))
        self.assertEqual((Path(wt) / "docs/notes.md").read_text(),
                         "# Notes\n\nOne\n=======\nFrom BP-1\nFrom BP-2\n")
        self.assertEqual(sh("git", "-C", wt, "status", "--porcelain").strip(), "")
        self.assertIn("BP-2 rebased, keeping both sides of a conflict in docs/notes.md; its approval carried "
                      "over (only the base moved)", self.news()[-1])

    def test_after_accept_a_doc_both_sides_changed_goes_back_to_the_worker(self):
        self.base_file("docs/notes.md", "# Notes\n\nOne\n")
        wt = self.two_approved("docs/notes.md", "# Notes\n\nOne by BP-1\n", "# Notes\n\nOne by BP-2\n")
        self.assertEqual(self.show("BP-2")["status"], "working")
        self.assertEqual(sh("git", "-C", wt, "status", "--porcelain").strip(), "")
        self.assertIn("BP-2 conflicts in docs/notes.md, sent back", self.news()[-1])

    def test_an_approval_carried_over_frees_a_slot_while_auto_accept_waits(self):
        self.bp("config", "--auto-accept", "on")
        self.bp("config", "--max-running", "1")
        tid = self.start_task()
        self.bp("_event", self.block(tid), "working", "", "Fixing")
        self.github(statusCheckRollup=self.RUNNING)
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.bp("_event", self.block(tid), "idle", "", "")
        self.bp("tell", tid, "Run ready again.")
        self.assertEqual(self.show(tid)["status"], "working")
        dep = self.done_dependency()  # BP-2
        self.bp("add", "Next", "--brief", "x", "--after", dep)  # BP-3: BP-1 takes the one slot
        self.assertEqual(self.show("BP-3")["status"], "todo")
        self.ready(tid)  # the approval carries over; auto-accept waits on the running checks
        started = "bullpen · BP-3 started (BP-1 is ready, freeing a slot)"
        for _ in range(100):  # it's saved working, then its news is written
            if self.show("BP-3")["status"] == "working" and started in self.news_lines():
                break
            time.sleep(0.1)
        self.assertEqual((self.show(tid)["status"], self.show(tid).get("auto_wait")), ("ready", True))
        self.assertEqual(self.show("BP-3")["status"], "working")
        self.assertIn(started, self.news_lines())

    def test_ready_again_frees_its_slot(self):
        self.bp("config", "--max-running", "1")
        tid = self.approved_task()
        blk = self.block(tid)
        self.bp("_event", blk, "idle", "", "")
        self.bp("_event", blk, "working", "", "Answering a question")  # BP-1 takes the one slot
        dep = self.done_dependency()  # BP-2
        self.bp("add", "Next", "--brief", "x", "--after", dep)  # BP-3
        self.bp("_event", blk, "idle", "", "")  # ready again, its code as approved
        self.assertEqual((self.show(tid)["status"], self.show("BP-3")["status"]), ("ready", "working"))
        self.assertIn("bullpen · BP-3 started (BP-1 is ready, freeing a slot)", self.news())

    def test_an_approval_carried_over_auto_accepts(self):
        self.bp("config", "--auto-accept", "on")
        tid = self.start_task()
        self.bp("_event", self.block(tid), "working", "", "Fixing")
        (self.fake / "pr.json").write_text(json.dumps({"state": "OPEN", "statusCheckRollup": [
            {"name": "test", "status": "COMPLETED", "conclusion": "FAILURE"}]}))
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.assertEqual(self.show(tid)["status"], "ready")  # its checks failed
        (self.fake / "pr.json").write_text(json.dumps({"state": "OPEN"}))
        self.bp("_event", self.block(tid), "idle", "", "")
        self.bp("tell", tid, "Run ready again.")
        self.ready(tid)
        for _ in range(100):
            if self.show(tid)["status"] == "done":
                break
            time.sleep(0.1)
        self.assertEqual(self.show(tid)["status"], "done")
