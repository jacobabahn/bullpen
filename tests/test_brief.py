"""`bullpen brief`: a task's brief printed, replaced or added to; its worker told what
changed, and its reviewer's next round told that it did."""

import re
from pathlib import Path

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module


class BriefTest(BullpenTest):
    def briefs(self):
        return next((self.tmp / "state" / "repos").iterdir()) / "briefs"

    def test_it_prints_the_brief_and_a_task_not_started_just_gets_the_new_one(self):
        self.bp("up")
        tid = self.bp("add", "Fix the thing", "--brief", "Make it work.").stdout.strip()
        self.assertEqual(self.bp("brief", tid).stdout, "Make it work.\n")
        typed = len(self.typed())
        out = self.bp("brief", tid, "--add", "Also cover logout.").stdout
        self.assertEqual(out, "BP-1's brief changed: its worker starts with it as it is now\n")
        new = Path(self.tmp / "new.md")
        new.write_text("Make it work on Tuesdays.\n\n")
        self.bp("brief", tid, "--file", str(new))
        self.assertEqual(len(self.typed()), typed)  # no worker to tell
        self.assertEqual(self.bp("brief", tid).stdout, "Make it work on Tuesdays.\n")
        self.bp("start", tid)
        self.assertIn("# BP-1: Fix the thing\n\nMake it work on Tuesdays.\n", self.full_brief())
        self.assertIn("brief: ", self.bp("show", tid).stdout)
        self.assertIn("(changed 2 times since it was added)", self.bp("show", tid).stdout)

    def test_a_running_worker_is_told_what_was_added_and_its_brief_has_it(self):
        tid = self.start_task()
        out = self.bp("brief", tid, "--add", "Also cover logout.").stdout
        self.assertEqual(out, "BP-1's brief changed, and its worker is told what changed\n")
        block, msg = self.typed()[-1]
        self.assertEqual(block, self.block(tid))
        self.assertTrue(msg.startswith("Your brief changed: follow it as it is now. This was added to it"), msg)
        self.assertIn("you needn't read the rest again", msg)
        self.assertRegex(msg, r"\n\n## Added \d{4}-\d\d-\d\d \d\d:\d\d\n\nAlso cover logout\.$")
        self.assertRegex(self.bp("brief", tid).stdout,
                         r"^Make it work\.\n\n## Added [\d\- :]+\n\nAlso cover logout\.\n$")
        full = self.full_brief()
        self.assertIn("Also cover logout.\n", full)
        self.assertIn("## How you work (bullpen)", full)  # the footer is still there
        self.assertEqual(self.show(tid)["status"], "working")
        kept = self.briefs() / "BP-1.brief-1.md"
        self.assertTrue(kept.read_text().endswith("Also cover logout.\n"))
        self.assertEqual([c["how"] for c in self.show(tid)["brief_changes"]], ["added"])

    def test_a_replaced_brief_is_told_as_a_diff_and_the_same_text_changes_nothing(self):
        tid = self.start_task()
        new = self.tmp / "new.md"
        new.write_text("Make it work.\n")
        typed = len(self.typed())
        self.assertIn("nothing changed", self.bp("brief", tid, "--file", str(new)).stdout)
        self.assertEqual(len(self.typed()), typed)
        new.write_text("Make it work, and fast.\n")
        self.bp("brief", tid, "--file", str(new))
        msg = self.typed()[-1][1]
        self.assertIn("This is what changed in it", msg)
        self.assertTrue(msg.endswith("@@ -1 +1 @@\n-Make it work.\n+Make it work, and fast."), msg)
        self.assertIn("Make it work, and fast.\n", self.full_brief())

    def test_a_long_change_is_read_from_its_file(self):
        tid = self.start_task()
        self.bp("brief", tid, "--add", "\n".join(f"Step {i}." for i in range(60)))
        msg = self.typed()[-1][1]
        self.assertNotIn("\n", msg)
        self.assertIn(f"What changed is in {self.briefs() / 'BP-1.brief-1.md'}", msg)

    def test_a_ready_or_closed_task_is_refused_with_what_to_do(self):
        tid = self.ready_with_a_commit()
        p = self.bp("brief", tid, "--add", "More.", ok=False)
        self.assertEqual(p.returncode, 1)
        self.assertIn("BP-1 is ready to accept: its brief can't change under work that's done", p.stderr)
        self.assertIn('`bullpen tell BP-1 "..."`', p.stderr)
        self.assertIn("follow-up task", p.stderr)
        self.assertEqual(self.bp("brief", tid).stdout, "Make it work.\n")  # printing still works
        done = self.done_dependency()
        p = self.bp("brief", done, "--add", "More.", ok=False)
        self.assertIn("is closed (done): add a follow-up task", p.stderr)
        p = self.bp("brief", tid, "--add", "x", "--file", "y", ok=False)
        self.assertIn("give --file or --add, not both", p.stderr)

    def test_a_worker_that_isnt_running_or_is_at_a_prompt_isnt_typed_to(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        typed = len(self.typed())
        out = self.bp("brief", tid, "--add", "More.").stdout
        self.assertIn("Its worker is at a prompt, so it isn't told", out)
        self.assertIn('`bullpen tell BP-1 "Your brief changed', out)
        self.assertEqual(self.show(tid)["status"], "needs")
        (self.fake / "alive" / blk).unlink()
        out = self.bp("brief", tid, "--add", "And more.").stdout
        self.assertIn("Its worker isn't running, so it isn't told", out)
        self.assertIn('`bullpen resume BP-1 "Your brief changed', out)
        self.assertIn("BP-1.brief-2.md", out)
        self.assertEqual(len(self.typed()), typed)
        self.assertEqual(len(self.show(tid)["brief_changes"]), 2)

    def test_a_task_in_triage_goes_back_to_work(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.bp("brief", tid, "--add", "More.")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_a_message_that_doesnt_reach_the_worker_says_so(self):
        tid = self.start_task()
        (self.fake / f"send_fails_{self.block(tid)}").write_text("")
        p = self.bp("brief", tid, "--add", "More.", ok=False)
        self.assertEqual(p.returncode, 1)
        self.assertIn("BP-1's brief changed, but the message didn't reach its worker", p.stderr)
        self.assertIn("More.\n", self.bp("brief", tid).stdout)


class BriefReviewTest(BullpenTest):
    def prompt(self, name):
        return next((self.tmp / "state").rglob(name)).read_text()

    def test_a_review_started_after_the_change_reads_the_brief_as_it_is_now(self):
        tid = self.start_task()
        self.bp("brief", tid, "--add", "Also cover logout.")
        self.commit_and_ready(tid)
        prompt = self.prompt("BP-1.review-1.md")
        brief = re.search(r"The worker's brief: `(.*)`", prompt).group(1)
        self.assertIn("Also cover logout.", Path(brief).read_text())
        self.assertNotIn("The brief changed since", prompt)  # a first round: it has seen no other
        self.assertNotIn("{brief_changed}", prompt)

    def test_a_follow_up_round_is_told_the_brief_changed_since_the_last_one(self):
        self.set_commands(review_rounds=3)
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.verdict(tid, "changes", "The fix misses the empty case.")
        self.bp("brief", tid, "--add", "Also cover logout.")
        self.commit_and_ready(tid, 2)
        prompt = self.prompt("BP-1.review-2.md")
        self.assertIn("The brief changed since round 1 looked (what changed: `", prompt)
        self.assertIn("BP-1.brief-1.md`). Check the work against the brief as it is now", prompt)
        self.verdict(tid, "changes", "Still misses it.")
        self.commit_and_ready(tid, 3)
        self.assertNotIn("The brief changed since", self.prompt("BP-1.review-3.md"))  # round 2 saw it

    def test_a_change_during_a_review_stops_it_and_sends_the_task_back(self):
        tid = self.start_task()
        worker = self.block(tid)
        reviewer = self.commit_and_ready(tid)
        self.bp("brief", tid, "--add", "Also cover logout.")
        self.assertFalse((self.fake / "alive" / reviewer).exists())  # it reviewed the brief as it was
        self.assertEqual(self.show(tid)["status"], "working")
        block, msg = self.typed()[-1]
        self.assertEqual(block, worker)
        self.assertIn("Also cover logout.", msg)
        self.assertIn("has been stopped: when you've done what changed, run your ready command again", msg)
        self.commit_and_ready(tid, 2)
        self.assertEqual(self.show(tid)["review"]["round"], 1)  # the stopped round isn't counted


class BriefGuideTest(BullpenTest):
    def test_the_guide_has_the_command_and_the_rule(self):
        guide = self.bp("guide").stdout
        self.assertIn("\nbullpen brief BP-3 --add \"...\"", guide)
        self.assertIn("Never edit files in bullpen's state folder", guide)
