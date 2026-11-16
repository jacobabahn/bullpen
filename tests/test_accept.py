"""After ready: following the PR, auto-accept, rebasing the others."""

import json
import unittest
from pathlib import Path

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module


class AcceptTest(BullpenTest):
    def gh_views(self):
        log = self.fake / "gh.log"
        calls = [json.loads(l)["argv"] for l in log.read_text().splitlines()] if log.exists() else []
        return [a for a in calls if a[:2] == ["pr", "view"]]

    def sent_back_by(self, tid):
        """Who the board says sent a working task back (core/src/words.rs reads
        the same fields): "github", "review", or None."""
        t = self.show(tid)
        if (t.get("gh") or {}).get("sent"):
            return "github"
        return "review" if (t.get("review") or {}).get("verdict") == "changes" else None

    FAILING = [{"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "FAILURE"},
               {"__typename": "CheckRun", "name": "lint", "status": "COMPLETED", "conclusion": "SUCCESS"},
               {"__typename": "StatusContext", "context": "ci/build", "state": "ERROR"}]

    def test_a_failing_check_goes_back_to_the_worker_once(self):
        tid = self.start_task()
        worker = self.block(tid)
        reviewer = self.commit_and_ready(tid)
        self.assertTrue((self.tmp / "state" / "prs").exists())
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · BP-1's PR checks failed: test, ci/build. Sent back to its worker.")
        line = [l for b, l in self.typed() if b == worker][-1]
        self.assertIn("test, ci/build", line)
        self.assertIn("ready command again", line)
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        self.assertFalse((self.fake / "alive" / reviewer).exists())  # its review is moot
        self.assertEqual(t["review"]["round"], 0)  # and doesn't use up a round
        self.assertIn("Fixing what GitHub said. Checks failing.", self.bp("show", tid).stdout)
        self.assertEqual((self.sent_back_by(tid), self.show(tid)["gh"]["checks"]), ("github", "failing"))
        # the worker runs ready again without fixing them: same commit, same failure
        self.bp("ready", tid, "--summary", "Did it.", cwd=t["worktree"])
        self.assertEqual(self.show(tid)["review"]["round"], 1)
        self.assertNotIn("sent", self.show(tid)["gh"])
        before = len(self.typed())
        self.bp("_prs")
        self.bp("_prs")
        self.assertEqual(len(self.typed()), before)
        self.assertEqual(self.show(tid)["status"], "review")
        # a new push that fails again is news again
        self.github(statusCheckRollup=self.FAILING, headRefOid="def")
        self.bp("_prs")
        self.assertIn("checks failed: test, ci/build", self.news_lines()[-1])

    def test_review_changes_after_the_watcher_looked_say_review(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.github(statusCheckRollup=[
            {"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "SUCCESS"}])
        self.bp("_prs")
        self.assertEqual(self.show(tid)["gh"]["checks"], "passing")
        self.verdict(tid, "changes", "The fix misses the empty case.")
        self.assertEqual(self.sent_back_by(tid), "review")
        self.assertNotIn("github:", self.bp("show", tid).stdout)

    def test_review_changes_after_a_github_send_back_say_review(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        self.assertEqual((self.sent_back_by(tid), self.show(tid)["gh"]["checks"]), ("github", "failing"))
        self.commit_and_ready(tid, 2)
        self.verdict(tid, "changes", "The fix misses the empty case.")
        self.assertEqual(self.sent_back_by(tid), "review")
    PASSING = [{"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "SUCCESS"}]

    def approved_with_auto_accept(self, *flags):
        """A task started with auto-accept on (and flags on its start), approved by review."""
        self.bp("up")
        self.bp("config", "--auto-accept", "on")
        tid = self.bp("add", "Fix the thing", "--brief", "Make it work.").stdout.strip()
        self.bp("start", tid, *flags)
        wt = self.show(tid)["worktree"]
        self.commit_and_ready(tid)
        self.bp("verdict", tid, "approve", "--summary", "Looks right.", cwd=wt)  # from the reviewer's worktree
        return tid, wt

    def merges(self):
        return [a for a in (json.loads(l)["argv"] for l in (self.fake / "gh.log").read_text().splitlines())
                if a[:2] == ["pr", "merge"]]

    def test_auto_accept_merges_an_approved_pr(self):
        self.assertFalse(json.loads(self.bp("config", "--json").stdout)["auto_accept"])  # off by default
        tid, wt = self.approved_with_auto_accept()
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertFalse(Path(wt).exists())
        self.assertEqual(len(self.merges()), 1)
        self.assertIn("bullpen · BP-1 approved by review and auto-accepted (merged PR #7, rebase)",
                      self.news_lines())

    def test_auto_accept_says_the_reviewers_notes(self):
        self.bp("up")
        self.bp("config", "--auto-accept", "on")
        tid = self.bp("add", "Fix the thing", "--brief", "Make it work.").stdout.strip()
        self.bp("start", tid)
        self.commit_and_ready(tid)
        self.verdict(tid, "approve", "## Findings\n\nChecked it.\n\n## Notes\n\n- A stale rule in docs.\n")
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertIn("bullpen · BP-1 approved by review with 1 note and auto-accepted (merged PR #7, rebase)",
                      self.news_lines())
        self.assertIn("    notes: - A stale rule in docs.", self.bp("show", tid).stdout)

    def test_auto_accept_notifies_with_notify_all(self):
        read = self.terminal("blk2")  # the board: the worker's pane closes on accept
        self.bp("config", "--notify", "all")
        tid, _ = self.approved_with_auto_accept()
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(read(), "\x1b]777;notify;bullpen · proj;BP-1 is ready: Fix the thing\x07"
                                 "\x1b]777;notify;bullpen · proj;BP-1 was auto-accepted: Fix the thing\x07")

    def test_auto_accept_waits_for_running_checks_then_merges(self):
        self.github(statusCheckRollup=self.RUNNING)
        tid, _ = self.approved_with_auto_accept()
        t = self.show(tid)
        self.assertEqual((t["status"], t["auto_wait"]), ("ready", True))
        self.assertIn("auto-accepts once its PR's checks pass", self.news_lines()[-1])
        self.bp("_prs")  # still running
        self.assertEqual(self.show(tid)["status"], "ready")
        self.github(statusCheckRollup=self.PASSING)
        self.bp("_prs")
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(len(self.merges()), 1)
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · BP-1 approved by review and auto-accepted (merged PR #7, rebase)")

    def test_auto_accept_waits_for_running_checks_that_then_fail(self):
        self.github(statusCheckRollup=self.RUNNING)
        tid, _ = self.approved_with_auto_accept()
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        t = self.show(tid)
        self.assertNotEqual(t["status"], "done")
        self.assertNotIn("auto_wait", t)
        self.assertIn("bullpen · BP-1 not auto-accepted: its PR's checks failed", self.news_lines())
        self.assertFalse(any(a[:2] == ["pr", "merge"] for a in self.merges()))
        timeline = next((self.tmp / "state").rglob(f"timeline/{tid}.jsonl")).read_text().splitlines()
        refused = [json.loads(l) for l in timeline if '"accept_refused"' in l]
        self.assertEqual([(e["reason"], e["why"]) for e in refused], [("checks", "its PR's checks failed")])

    def test_a_waiting_auto_accept_ends_when_the_task_leaves_ready(self):
        self.github(statusCheckRollup=self.RUNNING)
        tid, wt = self.approved_with_auto_accept()
        self.bp("tell", tid, "One more thing.")
        self.assertNotIn("auto_wait", self.show(tid))
        self.bp("config", "--review", "off")
        self.commit_and_ready(tid, 2)  # ready with no approval
        self.github(statusCheckRollup=self.PASSING)
        self.bp("_prs")
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertEqual(self.merges(), [])

    def test_auto_accept_skips_failing_checks(self):
        self.github(statusCheckRollup=self.FAILING)
        tid, _ = self.approved_with_auto_accept()
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertIn("not auto-accepted: its PR's checks are failing", self.news_lines()[-1])
        self.assertEqual(self.merges(), [])

    def test_auto_accept_refused_by_github_leaves_it_ready(self):
        (self.fake / "merge_refused").write_text(
            "Pull request is not mergeable: the merge commit cannot be cleanly created")
        tid, wt = self.approved_with_auto_accept()
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertTrue(Path(wt).exists())
        self.assertIn("auto-accept was refused: merge refused: Pull request is not mergeable",
                      self.news_lines()[-1])
        self.assertNotIn("conflict_sends", t)  # it merges cleanly: not a conflict, so not sent back

    def test_auto_accept_refused_for_a_conflict_sends_it_back_to_rebase(self):
        (self.fake / "merge_refused").write_text(
            "Pull request is not mergeable: the merge commit cannot be cleanly created")
        self.bp("up")
        self.bp("config", "--auto-accept", "on")
        tid = self.bp("add", "Fix the thing", "--brief", "Make it work.").stdout.strip()
        self.bp("start", tid)
        wt = self.show(tid)["worktree"]
        self.commit_and_ready(tid)
        (self.repo / "fix1.txt").write_text("fixed on main\n")  # what another task merged meanwhile
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "Clash")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "HEAD:main")
        self.bp("verdict", tid, "approve", "--summary", "Looks right.", cwd=wt)
        t = self.show(tid)
        self.assertEqual((t["status"], t["conflict_sends"]), ("working", 1))
        line = [l for b, l in self.typed() if b == t["block"]][-1]
        self.assertIn("doesn't rebase cleanly onto main: conflicts in fix1.txt", line)
        self.assertIn("run `git fetch origin`, then rebase your branch onto origin/main; where both sides changed "
                      "the same place, keep both. Run the full suite again, commit, and run your ready command "
                      "again.", line)
        self.assertEqual(self.news_lines()[-1], "bullpen · BP-1 approved by review, but the auto-accept was refused: "
                                                "it conflicts with main in fix1.txt. Sent back to its worker to "
                                                "rebase (trip 1 of 3)")

    def test_no_auto_accept_keeps_a_task_for_the_person(self):
        tid, _ = self.approved_with_auto_accept("--no-auto-accept")
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · BP-1 is ready: Fix the thing (1 commit, PR #7, approved by light review: 1 line in 1 file)"
                         "; risk: low · a small change (1 line in 1 file)")
        self.assertIn("kept for you", self.bp("show", tid).stdout)

    def test_auto_accept_on_a_task_wins_over_the_config(self):
        self.bp("up")
        tid = self.bp("add", "Fix the thing", "--brief", "x", "--auto-accept").stdout.strip()
        self.bp("start", tid)
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.assertEqual(self.show(tid)["status"], "done")

    def test_auto_accept_off_leaves_an_approved_task_ready(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertFalse((self.fake / "gh.log").exists() and self.merges())

    def test_a_new_comment_stops_a_waiting_auto_accept(self):
        self.github(statusCheckRollup=self.RUNNING)
        tid, _ = self.approved_with_auto_accept()
        self.github(statusCheckRollup=self.PASSING, comments=[
            {"id": "c1", "author": {"login": "pat"}, "body": "Hold on, rename it."}])
        self.bp("_prs")
        self.assertNotEqual(self.show(tid)["status"], "done")
        self.assertIn("bullpen · BP-1 not auto-accepted: its PR has new comments", self.news_lines())
        self.assertEqual(self.merges(), [])

    def test_a_new_comment_is_forwarded_once(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.github(comments=[
            {"id": "c1", "author": {"login": "ann"}, "body": "Please also\ncover the logout path."},
            {"id": "c2", "author": {"login": "codecov[bot]"}, "body": "Coverage 90%"}],
            reviews=[{"id": "r1", "author": {"login": "bo"}, "state": "APPROVED", "body": "LGTM"}])
        self.bp("_prs")
        self.assertEqual(self.news_lines()[-1], "bullpen · BP-1's PR has 1 new comment: "
                         "ann Please also cover the logout path. Sent back to its worker.")
        line = [l for b, l in self.typed() if b == worker][-1]
        self.assertIn("Please also cover the logout path.", line)
        self.assertNotIn("\n", line)
        self.assertNotIn("Coverage", line)
        self.assertNotIn("LGTM", line)
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        self.assertIn("1 new comment", self.bp("show", tid).stdout)
        self.commit_and_ready(tid, 2)
        before = len(self.typed())
        self.bp("_prs")
        self.assertEqual(len(self.typed()), before)  # already seen
        self.github(comments=[{"id": "c1", "author": {"login": "ann"}, "body": "Please also cover it."}],
                    reviews=[{"id": "r2", "author": {"login": "bo"}, "state": "CHANGES_REQUESTED", "body": ""}])
        self.bp("_prs")
        self.assertIn("bo requested changes", self.news_lines()[-1])

    def test_a_pr_merged_outside_bullpen_is_news_once(self):
        tid = self.start_task()
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        self.github(state="MERGED")
        self.bp("_prs")
        self.bp("_prs")
        merged = [l for l in self.news_lines() if "merged outside bullpen" in l]
        self.assertEqual(merged, ["bullpen · BP-1's PR #7 was merged outside bullpen: "
                                  "`bullpen accept BP-1` cleans up"])
        self.assertEqual(len(self.gh_views()), 1)  # a merged PR isn't looked at again
        self.assertFalse((self.tmp / "state" / "prs").exists())
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")  # nothing changes until the person says
        self.assertTrue(Path(t["worktree"]).exists())
        self.bp("accept", tid)
        gh = [json.loads(l)["argv"] for l in (self.fake / "gh.log").read_text().splitlines()]
        self.assertFalse(any(a[:2] == ["pr", "merge"] for a in gh))
        self.assertEqual(self.show(tid)["status"], "done")

    def test_a_pr_closed_outside_bullpen_points_at_drop(self):
        tid = self.start_task()
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        self.github(state="CLOSED")
        self.bp("_prs")
        self.assertEqual(self.news_lines()[-1], "bullpen · BP-1's PR #7 was closed outside bullpen: "
                                                "`bullpen drop BP-1` cleans up")

    def test_feedback_for_a_worker_that_is_gone_goes_to_the_coordinator(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        (self.fake / "alive" / worker).unlink()
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        self.assertFalse(any(b == worker for b, _ in self.typed()))
        t = self.show(tid)
        self.assertEqual(t["status"], "triage")
        self.assertEqual(t["why"], "PR checks failed: test, ci/build")
        self.assertIn("`bullpen resume BP-1` restarts it", self.news_lines()[-1])

    def test_feedback_that_doesnt_reach_the_worker_goes_to_the_coordinator(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        (self.fake / f"send_fails_{worker}").write_text("")
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "PR checks failed: test, ci/build"))
        self.assertNotIn("sent", t.get("gh") or {})
        line = self.news_lines()[-1]
        self.assertIn("The message didn't reach its worker (rex: couldn't send to the block)", line)
        self.assertNotIn("rex send", line)  # the reason, not the command
        self.assertIn("`bullpen tell BP-1` it", line)
        (self.fake / f"send_fails_{worker}").unlink()
        self.bp("tell", tid, "Fix the checks.")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_feedback_isnt_typed_into_a_prompt_the_person_was_asked_about(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        self.bp("_event", worker, "blocked", "permission", "Allow rm?")
        self.bp("ask", tid, "Should it delete the cache?")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"], t["stop"]),
                         ("needs", "Should it delete the cache?", {"by": "worker", "kind": "prompt"}))
        typed = len([1 for b, _ in self.typed() if b == worker])
        self.github(comments=[{"id": "c1", "author": {"login": "ann"}, "body": "Cover logout too."}])
        self.bp("_prs")
        self.assertEqual(len([1 for b, _ in self.typed() if b == worker]), typed)  # still at its prompt
        self.assertIn("Not sent to its worker", self.news_lines()[-1])
        self.assertEqual(self.show(tid)["why"], "Should it delete the cache?")

    def test_feedback_during_review_that_doesnt_reach_the_worker_goes_to_the_coordinator(self):
        tid = self.start_task()
        worker = self.block(tid)
        reviewer = self.commit_and_ready(tid)
        (self.fake / f"send_fails_{worker}").write_text("")
        self.github(statusCheckRollup=self.FAILING)
        self.bp("_prs")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "PR checks failed: test, ci/build"))
        self.assertFalse((self.fake / "alive" / reviewer).exists())  # moot once the work changes
        self.assertIn("The message didn't reach its worker", self.news_lines()[-1])
        (self.fake / f"send_fails_{worker}").unlink()
        self.bp("tell", tid, "Fix the checks.")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_feedback_is_not_typed_into_a_worker_blocked_on_a_prompt(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.commit_and_ready(tid)
        self.verdict(tid, "changes", "Fix the empty case.")  # back to the worker
        self.bp("_event", worker, "blocked", "permission", "Allow rm?")
        typed_to_worker = len([1 for b, _ in self.typed() if b == worker])
        self.github(comments=[{"id": "c1", "author": {"login": "ann"}, "body": "Cover logout too."}])
        self.bp("_prs")
        self.assertEqual(len([1 for b, _ in self.typed() if b == worker]), typed_to_worker)
        t = self.show(tid)
        self.assertEqual(t["status"], "needs")
        self.assertIn("Allow rm?", t["why"])
        self.assertIn("Not sent to its worker", self.news_lines()[-1])
        self.bp("_prs")  # recorded: not news again
        self.assertIn("Not sent to its worker", self.news_lines()[-1])
        self.assertEqual(len([l for l in self.news_lines() if "new comment" in l]), 1)

    def test_a_task_accepted_while_gh_answers_stays_accepted(self):
        tid = self.start_task()
        self.bp("config", "--review", "off")
        self.commit_and_ready_no_review(tid)
        self.github(statusCheckRollup=self.FAILING)
        # gh is slow: the person accepts the task while it answers (meanwhile, below)
        accepted = json.loads(self.bp("show", tid, "--json").stdout)
        accepted.update(status="done", block=None, worktree=None)
        path = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
        tmp = self.tmp / "accepted.json"
        tmp.write_text(json.dumps(accepted))
        (self.fake / "meanwhile").write_text(json.dumps(["cp", str(tmp), str(path)]))
        self.bp("_prs")
        t = self.show(tid)
        self.assertEqual(t["status"], "done")
        self.assertFalse(any("checks failed" in l for l in self.news_lines()))

    def test_nothing_to_follow_calls_no_gh(self):
        tid = self.start_task()
        self.bp("_prs")  # working, no PR yet
        self.assertEqual(self.gh_views(), [])
        self.assertFalse((self.tmp / "state" / "prs").exists())

    # ---------- rebase after accept ----------

    def two_tasks(self, one="one.txt", two="two.txt", same=False):
        """BP-1 and BP-2 started, review off, each with a commit (BP-2's: a file named
        two, written as BP-1 writes its own when same); BP-1 ready."""
        self.bp("config", "--review", "off")
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        for tid, name in (("BP-1", one), ("BP-2", two)):
            wt = self.show(tid)["worktree"]
            (Path(wt) / name).write_text("BP-1\n" if same else f"{tid}\n")
            sh("git", "-C", wt, "add", ".")
            sh("git", "-C", wt, "commit", "-qm", f"{tid} work")
        self.bp("ready", "BP-1", "--summary", "done")

    def accept_first(self):
        """Accept BP-1 (merged on GitHub): the line typed into BP-2's pane, or None."""
        self.merge_on_github("BP-1")
        self.bp("accept", "BP-1")
        said = [line for block, line in self.typed() if block == self.block("BP-2")]
        return said[-1] if said else None

    def test_after_accept_a_ready_task_is_rebased_and_pushed(self):
        self.two_tasks()
        self.bp("ready", "BP-2", "--summary", "done")
        self.merge_on_github("BP-1")
        self.bp("accept", "BP-1")
        t = self.show("BP-2")
        self.assertEqual(t["status"], "ready")
        main = sh("git", "-C", str(self.repo), "rev-parse", "origin/main").strip()
        pushed = sh("git", "-C", str(self.tmp / "origin.git"), "rev-parse", t["branch"]).strip()
        self.assertEqual(sh("git", "-C", t["worktree"], "rev-parse", "HEAD").strip(), pushed)
        sh("git", "-C", t["worktree"], "merge-base", "--is-ancestor", main, pushed)
        self.assertEqual(t["rebased"], "main")  # the board says "Rebased onto main."
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 rebased.")

    def test_show_says_when_a_ready_task_no_longer_rebases_onto_origins_base(self):
        self.two_tasks()
        self.assertNotIn("rebase:", self.bp("show", "BP-1").stdout)
        (self.repo / "one.txt").write_text("landed first\n")  # what another change merged meanwhile
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "Clash")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "HEAD:main")
        sh("git", "-C", str(self.repo), "fetch", "-q", "origin")
        self.assertIn("   rebase: no longer rebases onto main: conflicts in one.txt. `bullpen tell BP-1` it to "
                      "rebase and run ready again\n", self.bp("show", "BP-1").stdout)
        self.assertNotIn("rebase:", self.bp("show", "BP-2").stdout)  # working: its next ready sees to it

    def test_after_accept_a_working_task_with_no_overlap_is_left_alone(self):
        self.two_tasks()
        wt = self.show("BP-2")["worktree"]
        head = sh("git", "-C", wt, "rev-parse", "HEAD")
        self.assertIsNone(self.accept_first())
        self.assertEqual(sh("git", "-C", wt, "rev-parse", "HEAD"), head)
        self.assertEqual(self.show("BP-2")["status"], "working")
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 left alone (no overlap).")

    def test_after_accept_a_working_task_that_merges_cleanly_is_left_alone(self):
        self.two_tasks("shared.txt", "shared.txt", same=True)  # the same file, the same change
        self.assertIsNone(self.accept_first())
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 left alone (no conflict).")

    def test_after_accept_a_working_task_that_conflicts_is_told_the_files(self):
        self.two_tasks("shared.txt", "shared.txt")
        wt = self.show("BP-2")["worktree"]
        head = sh("git", "-C", wt, "rev-parse", "HEAD")
        line = self.accept_first()
        self.assertEqual(sh("git", "-C", wt, "rev-parse", "HEAD"), head)  # its worktree isn't touched
        self.assertIn("main moved: BP-1 (First) was just accepted, and your branch conflicts with it in shared.txt. "
                      "At a good point, ", line)
        self.assertIn("run `git fetch origin`, then rebase your branch onto origin/main; where both sides changed "
                      "the same place, keep both.", line)
        self.assertNotIn("suite", line)
        self.assertEqual(self.show("BP-2")["status"], "working")
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · After BP-1: BP-2 told to rebase (conflicts in shared.txt).")

    def test_after_accept_uncommitted_changes_in_a_touched_file_are_told(self):
        self.two_tasks()
        wt = self.show("BP-2")["worktree"]
        (Path(wt) / "one.txt").write_text("mine\n")  # untracked, where BP-1 added it
        line = self.accept_first()
        self.assertIn("it changed one.txt, where you have uncommitted changes. Commit them at a good point, then "
                      "run `git fetch origin`, then rebase your branch onto origin/main.", line)
        self.assertNotIn("conflict", line)
        self.assertEqual((Path(wt) / "one.txt").read_text(), "mine\n")
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · After BP-1: BP-2 told to rebase (uncommitted changes in one.txt).")

    def test_after_accept_uncommitted_changes_elsewhere_leave_it_alone(self):
        self.two_tasks()
        wt = self.show("BP-2")["worktree"]
        (Path(wt) / "README").write_text("edited\n")
        self.assertIsNone(self.accept_first())
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 left alone (no overlap).")
