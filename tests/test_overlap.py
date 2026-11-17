"""Overlap: news when two open tasks change the same file."""

import json

from support import setUpModule, tearDownModule, odd_name, BullpenTest  # a temp folder for each module


class OverlapTest(BullpenTest):
    def setUp(self):
        super().setUp()
        self.bp("config", "--review", "off")
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")

    def end_turn(self, tid):
        """The task's worker ends a turn (back to work first, as a new turn would be)."""
        self.bp("_event", self.block(tid), "working", "", "Working")
        self.bp("_event", self.block(tid), "idle")

    def overlap_news(self):
        return [l for l in self.news_lines() if "both changed" in l]

    def show_text(self, tid):
        return self.bp("show", tid).stdout

    def test_two_tasks_changing_a_file_are_said_once(self):
        self.commit_file("BP-1", "shared.txt", "one\n")
        self.commit_file("BP-1", "a.txt", "one\n")
        self.commit_file("BP-2", "shared.txt", "two\n")
        self.end_turn("BP-2")
        self.assertEqual(self.overlap_news(), ["bullpen · BP-1 and BP-2 both changed shared.txt"])
        self.end_turn("BP-2")
        self.bp("ready", "BP-1", "--summary", "done")
        self.assertEqual(len(self.overlap_news()), 1)  # once per pair, whoever checks
        self.assertIn("  overlap: BP-2 changed shared.txt too", self.show_text("BP-1"))
        self.assertIn("  overlap: BP-1 changed shared.txt too", self.show_text("BP-2"))

    def test_a_file_whose_name_git_quotes_is_said_readably(self):
        name = odd_name("shared") + ".txt"
        self.commit_file("BP-1", name, "one\n")
        self.commit_file("BP-2", name, "two\n")
        self.end_turn("BP-2")
        shown = name.replace("\t", "\\t").replace("\n", "\\n")
        self.assertEqual(self.overlap_news(), [f"bullpen · BP-1 and BP-2 both changed {shown}"])
        self.assertIn(f"  overlap: BP-2 changed {shown} too\n", self.show_text("BP-1"))

    def test_a_file_joining_adds_to_it(self):
        self.commit_file("BP-1", "shared.txt", "one\n")
        self.commit_file("BP-2", "shared.txt", "two\n")
        self.end_turn("BP-2")
        self.commit_file("BP-1", "b.txt", "one\n")
        self.commit_file("BP-1", "c.txt", "one\n")
        self.commit_file("BP-2", "b.txt", "two\n")
        self.commit_file("BP-2", "c.txt", "two\n")
        self.end_turn("BP-1")
        self.assertEqual(self.overlap_news(), ["bullpen · BP-1 and BP-2 both changed shared.txt",
                                               "bullpen · BP-1 and BP-2 both changed b.txt too (and 1 more)"])
        self.assertIn("  overlap: BP-2 changed b.txt, c.txt, shared.txt too", self.show_text("BP-1"))

    def test_no_shared_files_say_nothing(self):
        self.commit_file("BP-1", "one.txt", "one\n")
        self.commit_file("BP-2", "two.txt", "two\n")
        self.commit_file("BP-1", "Cargo.lock", "one\n")  # a lock file overlaps harmlessly
        self.commit_file("BP-2", "Cargo.lock", "two\n")
        self.end_turn("BP-2")
        self.bp("ready", "BP-1", "--summary", "done")
        self.assertEqual(self.overlap_news(), [])
        self.assertNotIn("overlap:", self.show_text("BP-2"))

    def test_a_closed_task_drops_out(self):
        self.commit_file("BP-1", "shared.txt", "one\n")
        self.commit_file("BP-2", "shared.txt", "two\n")
        self.bp("ready", "BP-1", "--summary", "done")
        self.assertEqual(len(self.overlap_news()), 1)
        self.bp("drop", "BP-1")
        self.assertNotIn("overlap:", self.show_text("BP-2"))
        self.end_turn("BP-2")
        store = next((self.tmp / "state").rglob("overlaps.json"))
        self.assertEqual(json.loads(store.read_text()), {})
        self.assertEqual(len(self.overlap_news()), 1)
