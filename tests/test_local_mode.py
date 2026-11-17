"""A repository without an origin remote: no push, no gh; accept merges locally."""

import json
import subprocess
import unittest
from pathlib import Path

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module


class LocalModeTest(unittest.TestCase):
    """A repository without an origin remote: no push, no gh; accept merges locally."""

    setUp_with_origin = BullpenTest.setUp
    bp = BullpenTest.bp
    rex_calls = BullpenTest.rex_calls
    api = BullpenTest.api
    made = BullpenTest.made
    typed = BullpenTest.typed
    show = BullpenTest.show
    start_task = BullpenTest.start_task

    def setUp(self):
        self.setUp_with_origin()
        sh("git", "-C", str(self.repo), "remote", "remove", "origin")

    def tearDown(self):
        self.assertFalse((self.fake / "gh.log").exists(), "gh was called in local mode")

    def git(self, *args, cwd=None):
        return sh("git", "-C", str(cwd or self.repo), *args).strip()

    def commit(self, where, name, text, msg):
        (Path(where) / name).write_text(text)
        self.git("add", ".", cwd=where)
        self.git("commit", "-qm", msg, cwd=where)

    def ready_task(self, review=False):
        """A started task with one commit (fix.txt), and an unrelated commit on main
        made since it started, then run ready."""
        if not review:
            self.bp("config", "--review", "off")
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        self.commit(wt, "fix.txt", "fixed\n", "Fix it")
        self.commit(self.repo, "other.txt", "other\n", "Other work on main")
        self.bp("ready", tid, "--summary", "Fixed the thing.", cwd=wt)
        return tid, wt

    def test_finish_and_close_without_a_remote(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        self.commit(self.repo, "other.txt", "other\n", "Other work on main")
        f = self.tmp / "f.md"
        f.write_text("Answered the question.")
        self.bp("finish", tid, "--summary-file", str(f), cwd=wt)
        self.assertEqual(self.show(tid)["status"], "finished")
        self.bp("close", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertFalse(Path(wt).exists())
        self.assertEqual(self.git("log", "-1", "--format=%s", "main"), "Other work on main")  # nothing merged

    def test_accept_closes_a_finished_task_without_a_remote(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        f = self.tmp / "f.md"
        f.write_text("Answered the question.")
        self.bp("finish", tid, "--summary-file", str(f), cwd=wt)
        main = self.git("rev-parse", "main")
        self.bp("accept", tid)
        t = self.show(tid)
        self.assertEqual((t["status"], t["closed"]), ("done", True))
        self.assertFalse(Path(wt).exists())
        self.assertEqual(self.git("rev-parse", "main"), main)  # nothing merged

    def test_start_uses_the_local_base(self):
        tid = self.start_task()
        t = self.show(tid)
        self.assertEqual(t["base"], "main")
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=t["worktree"]), self.git("rev-parse", "main"))

    def test_setup_and_teardown_without_a_remote(self):
        d = self.repo / ".bullpen"
        d.mkdir()
        (d / "setup.sh").write_text("touch set-up\n")
        (d / "teardown.sh").write_text("touch ../torn-down\n")
        self.git("add", ".")
        self.git("commit", "-qm", "Add hooks")
        tid, wt = self.ready_task()
        self.assertTrue((Path(wt) / "set-up").exists())
        log = next((self.tmp / "state").rglob(f"{tid}.setup.log")).read_text()
        self.assertIn(".bullpen/setup.sh on main", log)
        self.bp("accept", tid)
        self.assertFalse(Path(wt).exists())
        self.assertTrue((Path(wt).parent / "torn-down").exists())

    def test_ready_marks_the_local_branch_ready_without_push_or_gh(self):
        tid, wt = self.ready_task()
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertIsNone(t["pr"])
        self.assertTrue(t["local"])
        self.assertEqual(t["commits"], 1)
        self.bp("_prs")  # no PR to follow: no gh call (tearDown checks)
        self.assertFalse((self.tmp / "state" / "prs").exists())
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-1 is ready: Fix the thing (1 commit); risk: low · a small change (1 line in 1 file)"))
        ls = self.bp("ls").stdout
        self.assertIn("Local branch bullpen/bp-1-fix-the-thing.", ls)
        self.assertNotIn("None", ls)
        show = self.bp("show", tid).stdout
        self.assertIn("local branch", show)
        self.assertNotIn("None", show)

    def test_a_task_ready_once_more_is_ready_again_and_says_after_a_rebase(self):
        RISK = "; risk: low · a small change (1 line in 1 file)"
        tid, wt = self.ready_task()
        self.bp("ready", tid, "--summary", "Same.", cwd=wt)
        self.git("rebase", "-q", "main", cwd=wt)
        self.bp("ready", tid, "--summary", "Rebased.", cwd=wt)
        self.bp("ready", tid, "--summary", "Same.", cwd=wt)
        self.bp("ready", tid, "--summary", "Same.", cwd=wt)  # word for word the last: no news
        self.assertEqual([l for b, l in self.typed() if b == "blk1" and "ready" in l],
                         [f"bullpen · BP-1 is ready: Fix the thing (1 commit){RISK}",
                          f"bullpen · BP-1 is ready again: Fix the thing (1 commit){RISK}",
                          f"bullpen · BP-1 is ready again after a rebase: Fix the thing (1 commit){RISK}",
                          f"bullpen · BP-1 is ready again: Fix the thing (1 commit){RISK}"])

    def test_ready_still_refuses_uncommitted_changes_and_handles_no_commits(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "dirty.txt").write_text("x\n")
        self.assertIn("uncommitted", self.bp("ready", tid, cwd=wt, ok=False).stderr)
        (Path(wt) / "dirty.txt").unlink()
        self.bp("ready", tid, cwd=wt)
        self.assertEqual(self.show(tid)["why"], "Nothing to merge (no commits)")

    def test_rules_from_the_local_base(self):
        (self.repo / ".bullpen").mkdir()
        self.commit(self.repo, ".bullpen/rules.md", "- Local rule.\n", "Rules")
        self.start_task()
        self.assertIn("- Local rule.", next((self.tmp / "state").rglob("BP-1.full.md")).read_text())
        self.assertIn("From .bullpen/rules.md on main", self.bp("rules").stdout)

    def test_ready_starts_the_reviewer(self):
        tid, wt = self.ready_task(review=True)
        t = self.show(tid)
        self.assertEqual(t["status"], "review")
        self.assertEqual(self.made()[-1]["label"], "bullpen: BP-1 review · ctrl+esc hides")
        prompt = next((self.tmp / "state").rglob("BP-1.review-1.md")).read_text()
        self.assertIn(f"diff main...HEAD", prompt)
        self.bp("verdict", tid, "approve", "--summary", "Fine.")
        self.assertEqual(self.typed()[-1][1], "bullpen · BP-1 is ready: Fix the thing (1 commit, approved by "
                                              "light review: 1 line in 1 file); risk: low · a small change (1 line in 1 file)")

    def accepted(self, tid, wt):
        t = self.show(tid)
        self.assertEqual(t["status"], "done")
        self.assertFalse(Path(wt).exists())
        self.assertNotIn("bullpen/", self.git("branch"))
        self.assertEqual(self.git("show", "main:fix.txt"), "fixed")
        self.assertEqual(self.git("show", "main:other.txt"), "other")
        self.assertEqual(self.git("status", "--porcelain"), "")  # the checkout moved along

    def test_accept_rebase(self):
        tid, wt = self.ready_task()
        self.bp("accept", tid)
        self.accepted(tid, wt)
        self.assertEqual(self.git("log", "--format=%s", "main"), "Fix it\nOther work on main\ninit")
        self.assertEqual(self.git("rev-list", "--merges", "main"), "")

    def test_accept_starts_a_queued_task_on_the_new_local_base(self):
        tid, wt = self.ready_task()
        self.bp("add", "Next", "--brief", "x", "--after", tid)
        self.bp("accept", tid)
        t = self.show("BP-2")
        self.assertEqual(t["status"], "working")
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=t["worktree"]), self.git("rev-parse", "main"))
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-2 started (BP-1 was accepted)"))
        brief = next((self.tmp / "state").rglob("BP-2.full.md")).read_text()
        self.assertIn("### BP-1: Fix the thing (accepted, merged into main)", brief)
        self.assertIn("Fixed the thing.", brief)

    def test_accept_squash(self):
        self.bp("config", "--merge", "squash")
        tid, wt = self.ready_task()
        self.commit(wt, "more.txt", "more\n", "More")  # two commits become one
        self.bp("ready", tid, "--summary", "Fixed the thing.", cwd=wt)
        self.bp("accept", tid)
        self.accepted(tid, wt)
        self.assertEqual(self.git("log", "--format=%s", "main"), "Fix the thing (BP-1)\nOther work on main\ninit")
        self.assertEqual(self.git("log", "-1", "--format=%b", "main"), "Fixed the thing.")
        self.assertEqual(self.git("show", "main:more.txt"), "more")

    def test_accept_merge(self):
        self.bp("config", "--merge", "merge")
        tid, wt = self.ready_task()
        self.bp("accept", tid)
        self.accepted(tid, wt)
        parents = self.git("log", "-1", "--format=%P", "main").split()
        self.assertEqual(len(parents), 2)
        self.assertIn("BP-1", self.git("log", "-1", "--format=%s", "main"))

    def test_accept_merges_locally_even_if_an_origin_was_added_after_ready(self):
        tid, wt = self.ready_task()
        origin = self.tmp / "later.git"
        sh("git", "init", "-q", "--bare", "-b", "main", str(origin))
        self.git("remote", "add", "origin", str(origin))
        self.bp("accept", tid)
        self.accepted(tid, wt)

    def test_accept_keeps_unrelated_uncommitted_changes_in_the_main_checkout(self):
        tid, wt = self.ready_task()
        (self.repo / "README").write_text("edited, not committed\n")
        self.bp("accept", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual((self.repo / "README").read_text(), "edited, not committed\n")
        self.assertEqual(self.git("show", "main:fix.txt"), "fixed")
        self.assertEqual(self.git("status", "--porcelain"), "M README")

    def assert_untouched(self, tid, wt, main):
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertTrue(Path(wt).is_dir())
        self.assertEqual(t["worktree"], wt)
        self.assertIn("bullpen/bp-1-fix-the-thing", self.git("branch"))
        self.assertEqual(self.git("rev-parse", "main"), main)
        self.assertEqual(self.git("status", "--porcelain", cwd=wt), "")
        rebase = self.git("rev-parse", "--path-format=absolute", "--git-path", "rebase-merge", cwd=wt)
        self.assertFalse(Path(rebase).exists())  # no rebase left half done

    # ---- auto-accept ----

    def news_tail(self):
        return [l for b, l in self.typed() if b == "blk1"][-1]

    def test_auto_accept_merges_an_approved_branch(self):
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.ready_task(review=True)
        self.bp("verdict", tid, "approve", "--summary", "Fine.", cwd=wt)
        self.accepted(tid, wt)
        self.assertEqual(self.news_tail(),
                         "bullpen · BP-1 approved by review and auto-accepted (merged into main, rebase)")

    def test_auto_accept_never_takes_open_findings_after_the_last_round(self):
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.ready_task(review=True)
        self.bp("verdict", tid, "changes", "--summary", "Missing a case.", cwd=wt)
        self.commit(wt, "fix2.txt", "more\n", "Fix more")
        self.bp("ready", tid, "--summary", "Fixed.", cwd=wt)
        self.bp("verdict", tid, "changes", "--summary", "Still missing.", cwd=wt)
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertIn("after 2 rounds", t["why"])
        self.assertTrue(Path(wt).exists())

    def test_auto_accept_never_takes_a_task_with_review_off(self):
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.ready_task()
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertTrue(Path(wt).exists())

    def test_show_says_when_a_ready_task_no_longer_rebases(self):
        tid, wt = self.ready_task()  # main gained other.txt since: it still rebases
        self.assertNotIn("rebase:", self.bp("show", tid).stdout)
        self.commit(self.repo, "fix.txt", "fixed differently\n", "Clash")
        head = self.git("rev-parse", "HEAD", cwd=wt)
        self.assertIn(f"   rebase: no longer rebases onto main: conflicts in fix.txt. `bullpen tell {tid}` it to "
                      "rebase and run ready again\n", self.bp("show", tid).stdout)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)  # only a trial: nothing is written
        self.assertEqual(self.git("status", "--porcelain", cwd=wt), "")

    def approved_with_a_conflict(self, sends=0):
        """An auto-accepted task whose branch conflicts with main (fix.txt), approved,
        after sends earlier trips back to rebase."""
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.ready_task(review=True)
        self.commit(self.repo, "fix.txt", "fixed differently\n", "Clash")
        if sends:
            f = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
            t = json.loads(f.read_text())
            f.write_text(json.dumps({**t, "conflict_sends": sends}))
        head = self.git("rev-parse", "HEAD", cwd=wt)
        self.bp("verdict", tid, "approve", "--summary", "Fine.", cwd=wt)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)  # its branch as it was
        self.assertEqual(self.git("status", "--porcelain", cwd=wt), "")
        return tid, wt

    def test_an_auto_accept_refused_for_a_conflict_sends_it_back_to_rebase(self):
        main = self.git("rev-parse", "HEAD")
        tid, wt = self.approved_with_a_conflict()
        t = self.show(tid)
        self.assertEqual((t["status"], t["conflict_sends"], t["review"]["conflict"]), ("working", 1, ["fix.txt"]))
        block, line = [(b, l) for b, l in self.typed() if b == t["block"]][-1]
        self.assertIn("Review approved your work, but the auto-accept was refused: your branch doesn't rebase "
                      "cleanly onto main: conflicts in fix.txt.", line)
        self.assertIn("Now rebase your branch onto main; where both sides changed the same place, keep both. "
                      "Run the full suite again, commit, and run your ready command again.", line)
        self.assertEqual(self.news_tail(), "bullpen · BP-1 approved by review, but the auto-accept was refused: "
                                           "it conflicts with main in fix.txt. Sent back to its worker to rebase "
                                           "(trip 1 of 3)")
        self.assertIn("Approved, rebasing onto main: its branch conflicted.", self.bp("show", tid).stdout)
        self.assertNotEqual(self.git("rev-parse", "main"), main)  # only the clash: nothing merged
        self.assertEqual(self.git("show", "main:fix.txt"), "fixed differently")
        # the worker rebases and fixes it: a rebase check, then the auto-accept again
        subprocess.run(["git", "-C", wt, "rebase", "-q", "main"], capture_output=True)  # stops at the conflict
        (Path(wt) / "fix.txt").write_text("fixed differently, and fixed\n")
        self.git("add", "fix.txt", cwd=wt)
        sh("git", "-C", wt, "-c", "core.editor=true", "rebase", "--continue")
        self.bp("ready", tid, "--summary", "Rebased.", cwd=wt)
        self.assertEqual(self.show(tid)["review"]["kind"], "rebase")
        self.bp("verdict", tid, "approve", "--summary", "Resolved right.", cwd=wt)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(self.git("show", "main:fix.txt"), "fixed differently, and fixed")

    def test_an_auto_accept_refused_for_a_conflict_resumes_a_worker_that_exited(self):
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.ready_task(review=True)
        self.commit(self.repo, "fix.txt", "fixed differently\n", "Clash")
        (self.fake / "alive" / self.show(tid)["block"]).unlink()
        self.bp("verdict", tid, "approve", "--summary", "Fine.", cwd=wt)
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertIn("--continue", cmd)
        self.assertTrue(any("conflicts in fix.txt" in x for x in cmd))
        self.assertEqual(self.show(tid)["status"], "working")

    def test_an_auto_accept_refused_for_a_conflict_too_often_is_left_for_the_person(self):
        tid, wt = self.approved_with_a_conflict(sends=3)
        t = self.show(tid)
        self.assertEqual((t["status"], t["conflict_sends"]), ("ready", 3))
        self.assertFalse([l for b, l in self.typed() if b == t["block"] and "rebase" in l])
        self.assertIn("(approved by review), but the auto-accept was refused: it still conflicts with main in "
                      "fix.txt after 3 trips back to rebase. Left for you", self.news_tail())

    def test_an_auto_accept_refused_but_not_for_a_conflict_leaves_it_ready(self):
        self.bp("config", "--auto-accept", "on")
        tid, wt = self.ready_task(review=True)
        (Path(wt) / "scratch.txt").write_text("half done\n")
        main = self.git("rev-parse", "main")
        self.bp("verdict", tid, "approve", "--summary", "Fine.", cwd=wt)
        t = self.show(tid)
        self.assertEqual((t["status"], t.get("conflict_sends")), ("ready", None))
        self.assertEqual(self.git("rev-parse", "main"), main)
        self.assertIn("(approved by review), but the auto-accept was refused: merge refused: BP-1 has uncommitted "
                      "changes", self.news_tail())

    def test_a_manual_accept_refused_for_a_conflict_sends_nothing_back(self):
        tid, wt = self.ready_task()
        self.commit(self.repo, "fix.txt", "fixed differently\n", "Clash")
        before = self.typed()
        p = self.bp("accept", tid, ok=False)
        self.assertIn("merge refused: bullpen/bp-1-fix-the-thing doesn't rebase cleanly onto main", p.stderr)
        t = self.show(tid)
        self.assertEqual((t["status"], t.get("conflict_sends")), ("ready", None))
        self.assertEqual(self.typed(), before)

    def test_accept_refuses_a_conflict_and_leaves_the_task(self):
        for mode in ("rebase", "squash", "merge"):
            with self.subTest(mode=mode):
                self.setUp()
                self.bp("config", "--merge", mode)
                tid, wt = self.ready_task()
                self.commit(self.repo, "fix.txt", "fixed differently\n", "Clash")
                main = self.git("rev-parse", "main")
                p = self.bp("accept", tid, ok=False)
                self.assertNotEqual(p.returncode, 0)
                self.assertIn("merge refused", p.stderr)
                self.assert_untouched(tid, wt, main)

    def test_accept_refuses_when_a_dirty_file_in_the_main_checkout_would_be_overwritten(self):
        tid, wt = self.ready_task()
        (self.repo / "fix.txt").write_text("mine\n")  # untracked, in the way
        main = self.git("rev-parse", "main")
        p = self.bp("accept", tid, ok=False)
        self.assertIn("can't fast-forward main", p.stderr)
        self.assert_untouched(tid, wt, main)
        self.assertEqual((self.repo / "fix.txt").read_text(), "mine\n")

    def test_accept_when_the_base_is_not_checked_out(self):
        tid, wt = self.ready_task()
        self.git("switch", "-q", "-c", "elsewhere")
        self.bp("accept", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(self.git("show", "main:fix.txt"), "fixed")
        self.assertEqual(self.git("branch", "--show-current"), "elsewhere")

    def test_accept_with_nothing_to_merge(self):
        tid = self.start_task()
        self.bp("ready", tid)
        main = self.git("rev-parse", "main")
        self.bp("accept", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(self.git("rev-parse", "main"), main)
        self.assertEqual(self.typed()[-1][1], "bullpen · BP-1 accepted (nothing to merge)")

    def test_diff_against_the_local_base(self):
        tid, wt = self.ready_task()  # main moved on since: only the task's own change shows
        out = self.bp("diff", tid, "--stat").stdout
        self.assertIn("fix.txt | 1 +", out)
        self.assertNotIn("other.txt", out)
        self.assertNotIn("Uncommitted", out)
        self.assertEqual(self.show(tid)["diffstat"], "1 file, +1 −0")

    def test_drop_closes_no_pr(self):
        tid, wt = self.ready_task()
        self.bp("drop", tid)
        self.assertEqual(self.show(tid)["status"], "dropped")
        self.assertFalse(Path(wt).exists())
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-1 dropped"))

    def test_accept_is_news_naming_the_base_and_the_merge(self):
        self.bp("config", "--merge", "squash")
        tid, wt = self.ready_task()
        self.bp("accept", tid)
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-1 accepted (merged into main, squash)"))

    def test_accept_from_the_coordinators_pane_is_no_news(self):
        tid, wt = self.ready_task()
        before = self.typed()
        self.env["REX_BLOCK"] = "blk1"
        self.bp("accept", tid)
        self.accepted(tid, wt)
        self.assertEqual(self.typed(), before)

    def second_ready(self, name, text):
        """BP-2, started from main before BP-1's accept, with one commit, ready."""
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        wt = self.show("BP-2")["worktree"]
        self.commit(wt, name, text, "Second")
        self.bp("ready", "BP-2", "--summary", "done", cwd=wt)
        return wt

    def news_lines(self):
        return [l for b, l in self.typed() if b == "blk1"]

    def test_after_accept_a_ready_task_is_rebased(self):
        tid, _ = self.ready_task()
        wt = self.second_ready("two.txt", "two\n")
        self.bp("accept", tid)
        t = self.show("BP-2")
        self.assertEqual(t["status"], "ready")
        self.git("merge-base", "--is-ancestor", "main", "HEAD", cwd=wt)
        self.assertEqual(self.git("log", "--format=%s", "-1", cwd=wt), "Second")
        self.assertEqual(t["rebased"], "main")  # the board says "Rebased onto main."
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 rebased.")
        self.bp("ready", "BP-2", "--summary", "again", cwd=wt)
        self.assertNotIn("rebased", self.show("BP-2"))  # said once, until its next ready

    def test_after_accept_a_conflict_goes_back_with_the_branch_untouched(self):
        tid, _ = self.ready_task()
        wt = self.second_ready("fix.txt", "fixed differently\n")
        head = self.git("rev-parse", "HEAD", cwd=wt)
        self.bp("accept", tid)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)
        self.assertEqual(self.git("status", "--porcelain", cwd=wt), "")
        rebase = self.git("rev-parse", "--path-format=absolute", "--git-path", "rebase-merge", cwd=wt)
        self.assertFalse(Path(rebase).exists())
        t = self.show("BP-2")
        self.assertEqual(t["status"], "working")
        block, line = self.typed()[-2]
        self.assertEqual(block, t["block"])
        self.assertIn("conflicts in fix.txt", line)
        self.assertIn("rebase your branch onto main", line)
        self.assertNotIn("fetch", line)
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · After BP-1: BP-2 conflicts in fix.txt, sent back.")

    def test_after_accept_a_conflict_that_cant_be_sent_leaves_the_task(self):
        tid, _ = self.ready_task()
        self.second_ready("fix.txt", "fixed differently\n")
        (self.fake / f"send_fails_{self.show('BP-2')['block']}").write_text("")
        self.bp("accept", tid)
        self.assertEqual(self.show("BP-2")["status"], "ready")
        line = self.news_lines()[-1]
        self.assertIn("BP-2 conflicts in fix.txt, but the message didn't reach its worker", line)
        self.assertIn("`bullpen tell BP-2` it to rebase onto main and fix them", line)

    def test_after_accept_a_conflict_resumes_a_worker_that_exited(self):
        tid, _ = self.ready_task()
        self.second_ready("fix.txt", "fixed differently\n")
        (self.fake / "alive" / self.show("BP-2")["block"]).unlink()
        self.bp("accept", tid)
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertIn("--continue", cmd)
        self.assertTrue(any("conflicts in fix.txt" in x for x in cmd))
        self.assertEqual(self.show("BP-2")["status"], "working")

    def test_after_accept_a_dirty_worktree_is_skipped(self):
        tid, _ = self.ready_task()
        wt = self.second_ready("two.txt", "two\n")
        head = self.git("rev-parse", "HEAD", cwd=wt)
        (Path(wt) / "scratch.txt").write_text("x\n")
        self.bp("accept", tid)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)
        self.assertEqual(self.show("BP-2")["status"], "ready")
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · After BP-1: BP-2 has uncommitted changes, not rebased.")

    def test_after_accept_several_tasks_in_one_line(self):
        tid, _ = self.ready_task()
        self.second_ready("fix.txt", "fixed differently\n")
        self.bp("add", "Third", "--brief", "x")
        self.bp("start", "BP-3")  # working, its pane up
        self.bp("accept", tid)
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 conflicts in fix.txt, "
                         "sent back; BP-3 left alone (no overlap).")

    def test_after_accept_a_worker_waiting_on_a_prompt_is_not_rebased(self):
        tid, _ = self.ready_task()
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        t = self.show("BP-2")
        self.commit(t["worktree"], "two.txt", "two\n", "Second")
        head = self.git("rev-parse", "HEAD", cwd=t["worktree"])
        self.bp("_event", t["block"], "blocked", "permission", "Allow rm?")
        self.bp("accept", tid)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=t["worktree"]), head)
        self.assertEqual(self.show("BP-2")["status"], "needs")
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · After BP-1: BP-2 waiting on the person, not rebased.")

    def test_after_accept_a_task_whose_reviewer_is_stuck_is_left_alone(self):
        tid, _ = self.ready_task()
        self.bp("config", "--review", "on")
        wt = self.second_ready("fix.txt", "fixed differently\n")  # would conflict
        head = self.git("rev-parse", "HEAD", cwd=wt)
        reviewer = self.show("BP-2")["review"]["block"]
        self.bp("_event", reviewer, "blocked", "permission", "Allow rm?")
        self.assertEqual(self.show("BP-2")["status"], "needs")
        self.bp("accept", tid)
        t = self.show("BP-2")
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)
        self.assertEqual(t["status"], "needs")
        self.assertEqual(t["review"]["block"], reviewer)
        self.assertTrue((self.fake / "alive" / reviewer).exists())
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 left in review.")

    def in_review_at_an_accept(self, name, text):
        """BP-1 (fix.txt) approved, and BP-2 (a commit to name) in review when BP-1 is
        accepted. Returns BP-2's worktree and its commit."""
        tid, _ = self.ready_task(review=True)
        self.bp("verdict", tid, "approve", "--summary", "Fine.")
        wt = self.second_ready(name, text)
        self.assertEqual(self.show("BP-2")["status"], "review")
        head = self.git("rev-parse", "HEAD", cwd=wt)
        self.bp("accept", tid)
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 left in review.")
        return wt, head

    def test_a_task_left_in_review_that_conflicts_is_sent_back_when_approved(self):
        wt, head = self.in_review_at_an_accept("fix.txt", "fixed differently\n")
        self.bp("verdict", "BP-2", "approve", "--summary", "Fine.")
        t = self.show("BP-2")
        self.assertEqual((t["status"], t["review"]["conflict"]), ("working", ["fix.txt"]))
        self.assertNotIn("left_in_review", t)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)  # its branch as it was
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-1: BP-2 conflicts in fix.txt, sent back.")
        self.assertFalse([l for l in self.news_lines() if "BP-2 is ready" in l])
        line = [l for b, l in self.typed() if b == t["block"]][-1]
        self.assertIn("main moved (BP-1 was accepted) and your branch doesn't rebase cleanly onto it: conflicts in "
                      "fix.txt. Your branch is as it was. Now rebase your branch onto main", line)
        # the worker rebases and resolves it by hand: a rebase check, as after any rebase
        subprocess.run(["git", "-C", wt, "rebase", "-q", "main"], capture_output=True)
        (Path(wt) / "fix.txt").write_text("fixed, and fixed differently\n")
        self.git("add", "fix.txt", cwd=wt)
        sh("git", "-C", wt, "-c", "core.editor=true", "rebase", "--continue")
        self.bp("ready", "BP-2", "--summary", "Rebased.", cwd=wt)
        r = self.show("BP-2")["review"]
        self.assertEqual((r["kind"], r["round"]), ("rebase", 1))
        self.bp("verdict", "BP-2", "approve", "--summary", "Both kept.")
        self.assertEqual(self.show("BP-2")["status"], "ready")
        # its first ready news: the approval before the rebase never said it
        self.assertIn("bullpen · BP-2 is ready: Second (1 commit, a rebase check approved the conflict resolution)",
                      self.news_lines()[-1])

    def test_a_task_left_in_review_that_rebases_cleanly_is_ready_as_it_is(self):
        wt, head = self.in_review_at_an_accept("two.txt", "two\n")
        self.bp("verdict", "BP-2", "approve", "--summary", "Fine.")
        t = self.show("BP-2")
        self.assertEqual(t["status"], "ready")
        self.assertNotIn("left_in_review", t)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)  # not rebased
        self.assertNotIn("rebased", t)
        self.assertIn("bullpen · BP-2 is ready: Second", self.news_lines()[-1])

    def test_a_task_left_in_review_that_gets_findings_is_left_to_its_worker(self):
        wt, head = self.in_review_at_an_accept("fix.txt", "fixed differently\n")
        self.bp("verdict", "BP-2", "changes", "--summary", "Fix the typo.")
        t = self.show("BP-2")
        self.assertEqual(t["status"], "working")
        self.assertNotIn("left_in_review", t)
        self.assertNotIn("conflict", t["review"])
        self.assertFalse([l for l in self.news_lines() if "conflicts in" in l])

    def test_a_task_left_in_review_by_two_accepts_names_both(self):
        tid, _ = self.ready_task(review=True)
        self.bp("verdict", tid, "approve", "--summary", "Fine.")
        wt = self.second_ready("fix.txt", "fixed differently\n")
        self.bp("add", "Third", "--brief", "x")
        self.bp("start", "BP-3")
        self.commit(self.show("BP-3")["worktree"], "three.txt", "three\n", "Third")
        self.bp("ready", "BP-3", "--summary", "done", cwd=self.show("BP-3")["worktree"])
        self.bp("verdict", "BP-3", "approve", "--summary", "Fine.")
        self.bp("accept", tid)
        self.bp("accept", "BP-3")
        self.assertEqual(self.news_lines()[-1], "bullpen · After BP-3: BP-2 left in review.")
        self.assertEqual(self.show("BP-2")["left_in_review"], ["BP-1", "BP-3"])
        self.bp("verdict", "BP-2", "approve", "--summary", "Fine.")
        t = self.show("BP-2")
        self.assertEqual(t["status"], "working")
        self.assertEqual(self.news_lines()[-1],
                         "bullpen · After BP-1 and BP-3: BP-2 conflicts in fix.txt, sent back.")
        line = [l for b, l in self.typed() if b == t["block"]][-1]
        self.assertIn("main moved (BP-1 and BP-3 were accepted)", line)

    def test_rebase_others_off(self):
        tid, _ = self.ready_task()
        wt = self.second_ready("two.txt", "two\n")
        head = self.git("rev-parse", "HEAD", cwd=wt)
        cfg = next((self.tmp / "state").rglob("config.json"))
        cfg.write_text(json.dumps({**json.loads(cfg.read_text()), "rebase_others": False}))
        self.bp("accept", tid)
        self.assertEqual(self.git("rev-parse", "HEAD", cwd=wt), head)
        self.assertEqual(self.news_lines()[-1], "bullpen · BP-1 accepted (merged into main, rebase)")

    def test_a_task_published_locally_stays_local_when_an_origin_is_added(self):
        tid, wt = self.ready_task()
        self.assertIs(self.show(tid)["local"], True)
        origin = self.tmp / "later.git"
        sh("git", "init", "-q", "--bare", "-b", "main", str(origin))
        self.git("remote", "add", "origin", str(origin))
        self.commit(wt, "fix2.txt", "more\n", "Fix more")
        out = self.bp("ready", tid, "--summary", "More.", cwd=wt).stdout
        self.assertIn("no remote, so nothing pushed", out)  # and no gh (tearDown)
        self.assertEqual(sh("git", "-C", str(origin), "branch"), "")  # nothing pushed
        self.assertIsNone(self.show(tid)["pr"])
        self.bp("accept", tid)
        self.assertEqual(self.git("show", "main:fix2.txt"), "more")
