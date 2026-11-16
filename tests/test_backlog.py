"""The backlog: `bullpen later`, the list ideas go on (docs/backlog.md)."""

import json

from support import setUpModule, tearDownModule, BullpenTest  # a temp folder for each module


class BacklogTest(BullpenTest):
    def later(self, *args, ok=True):
        return self.bp("later", *args, ok=ok)

    def items(self, *args):
        return json.loads(self.later("--json", *args).stdout)

    def item(self, iid):
        return next(i for i in self.items("--all") if i["id"] == iid)

    def brief(self, text="Do the thing.\n"):
        path = self.tmp / "b.md"
        path.write_text(text)
        return str(path)

    def test_ideas_go_on_the_list_in_order(self):
        self.assertIn("the backlog is empty", self.later().stdout)
        self.assertEqual(self.later("Tidy the watcher's errors").stdout, "L1\n")
        self.assertEqual(self.later("Fix the flaky test", "--top").stdout, "L2\n")
        self.later("Split the board", "--brief-file", self.brief())
        self.assertEqual(self.later().stdout.splitlines(), [
            "◇ L2  Fix the flaky test",
            "◇ L1  Tidy the watcher's errors",
            "◇ L3  Split the board · has a brief",
        ])
        i = self.item("L1")
        self.assertEqual((i["by"], i["state"], i["brief"], i["task"]), ("person", "kept", None, None))
        self.assertEqual(self.item("L3")["brief"], "L3.md")
        state = next((self.tmp / "state" / "repos").iterdir())
        self.assertEqual((state / "briefs" / "L3.md").read_text(), "Do the thing.\n")
        self.assertEqual(json.loads((state / "backlog.json").read_text())["next"], 4)
        # moves, among the open ones
        self.assertEqual(self.later("L3", "--top").stdout, "L3 is 1 of 3 on the list\n")
        self.later("l2", "--down")
        self.assertEqual([i["id"] for i in self.items()], ["L3", "L1", "L2"])
        self.later("L3", "--bottom")
        self.later("L2", "--up")
        self.assertEqual([i["id"] for i in self.items()], ["L2", "L1", "L3"])
        self.assertIn("no item L9", self.later("L9", "--top", ok=False).stderr)

    def test_keep_drop_skip_and_one_item_in_full(self):
        self.later("One")
        self.later("Two")
        self.assertIn("L1 dropped: One", self.later("L1", "--drop", "not worth it").stdout)
        self.assertEqual((self.item("L1")["state"], self.item("L1")["note"]), ("dropped", "not worth it"))
        self.assertEqual([i["id"] for i in self.items()], ["L2"])
        self.assertIn("· L1  One · dropped", self.later("--all").stdout)
        self.assertIn("--keep` puts it back", self.later("L1", "--drop", ok=False).stderr)
        self.later("L1", "--keep")
        self.assertEqual(self.item("L1")["state"], "kept")
        self.later("L2", "--skip", "overlaps BP-3")
        self.assertIn("· skipped: overlaps BP-3", self.later().stdout)
        self.assertIn("say why", self.later("L2", "--skip", " ", ok=False).stderr)
        self.assertIn("one of --keep, --drop at a time", self.later("L2", "--keep", "--drop", ok=False).stderr)
        self.assertIn("only for --drop", self.later("L2", "why", ok=False).stderr)
        self.assertIn("is for an item already on the list", self.later("Three", "--start", ok=False).stderr)
        shown = self.later("L2").stdout
        self.assertIn("L2  Two\n    state: kept, added by you", shown)
        self.assertIn("  skipped: overlaps BP-3", shown)
        self.assertIn("next: `bullpen later L2 --brief-file FILE` gives it a brief", shown)
        self.later("L2", "--brief-file", self.brief())
        self.assertIn("next: `bullpen later L2 --start`", self.later("L2").stdout)

    def test_start_makes_it_a_task_that_keeps_the_link(self):
        self.bp("up")
        self.later("Tidy the errors")
        p = self.later("L1", "--start", ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("L1 has no brief", p.stderr)
        self.assertEqual(self.bp("ls", "--json").stdout.strip(), "[]")  # no task made
        p = self.later("L1", "--start", "--brief-file", self.brief("Tidy them.\n"))
        self.assertTrue(p.stdout.startswith("L1 is BP-1 now, started in "), p.stdout)
        t = self.show("BP-1")
        self.assertEqual((t["title"], t["status"]), ("Tidy the errors", "working"))
        self.assertEqual((t["from"], t["origin"], t["started_by"]), ("L1", "backlog", "person"))
        self.assertIn("Tidy them.", self.full_brief("BP-1"))
        i = self.item("L1")
        self.assertEqual((i["state"], i["task"]), ("started", "BP-1"))
        self.assertIn("     from: the backlog (L1), started by you", self.bp("show", "BP-1").stdout)
        # a dim ↻ after its mark, wherever it's said, and its sentence says where it came from
        line = self.bp("ls").stdout.splitlines()[0]
        self.assertTrue(line.startswith("○↻BP-1  Tidy the errors"), line)
        self.assertTrue(line.endswith("From the backlog (L1)."), line)
        picked = self.bp("pick", "--filter", "Tidy", "--from", "nowhere").stdout
        self.assertEqual(picked, "○↻BP-1  Tidy the errors\n")
        self.assertEqual(self.later().stdout, "↳ L1  Tidy the errors · BP-1, working\n")
        self.assertIn("a task already (BP-1)", self.later("L1", "--start", ok=False).stderr)
        # from the coordinator's pane, the task says so
        self.later("Another", "--brief-file", self.brief())
        self.bp_in("blk1", "later", "L2", "--start")
        self.assertEqual(self.show("BP-2")["started_by"], "c1")
        self.assertIn("started by c1", self.bp("show", "BP-2").stdout)
        # once its task closes, the item leaves the list
        self.bp("drop", "BP-1")
        self.assertNotIn("L1", self.later().stdout)
        self.assertEqual(self.item("L1")["state"], "started")

    def test_a_start_that_fails_still_records_the_task(self):
        self.later("Tidy", "--brief-file", self.brief())  # no `up`: no Rex session
        env, self.env = self.env, {k: v for k, v in self.env.items() if k != "REX_SESSION"}
        try:
            p = self.later("L1", "--start", ok=False)
        finally:
            self.env = env
        self.assertIn("L1 is BP-1 now, but it didn't start", p.stderr)
        self.assertEqual(self.show("BP-1")["status"], "todo")
        self.assertEqual(self.item("L1")["task"], "BP-1")

    def test_a_workers_item_asks_and_its_coordinator_hears(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.assertIn(' later "<a short title>"`', self.full_brief(tid))  # the footer says how
        p = self.bp_in(blk, "later", "Split watch.lua's timers")
        self.assertEqual(p.stdout, "L1\n")
        i = self.item("L1")
        self.assertEqual((i["by"], i["state"]), (tid, "asks"))
        self.assertEqual(self.news()[-1], "bullpen · BP-1 added L1 to the backlog: Split watch.lua's timers "
                         "(`bullpen later L1 --keep` puts it on the list, `--drop` says no)")
        self.assertIn("? L1  Split watch.lua's timers · asks: from BP-1", self.later().stdout)
        # a worker only adds
        self.assertIn("only adds", self.bp_in(blk, "later", "L1", "--keep", ok=False).stderr)
        # --from, from anywhere
        self.later("Another aside", "--from", "1")
        self.assertEqual(self.item("L2")["state"], "asks")
        # the coordinator says yes
        self.bp_in("blk1", "later", "L1", "--keep")
        self.assertEqual(self.item("L1")["state"], "kept")
        # a coordinator's own item is kept from the start
        self.bp_in("blk1", "later", "Its own idea")
        self.assertEqual((self.item("L3")["by"], self.item("L3")["state"]), ("c1", "kept"))

    def test_a_title_starting_with_a_dash_starts(self):
        self.bp("up")
        self.later("--brief-file", self.brief(), "--", "--x marks it")
        p = self.later("L1", "--start")
        self.assertTrue(p.stdout.startswith("L1 is BP-1 now, started in "), p.stdout)
        self.assertEqual(self.show("BP-1")["title"], "--x marks it")

    def test_a_worker_may_view_an_item(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.later("Tidy")
        self.assertIn("L1  Tidy\n    state: kept, added by you", self.bp_in(blk, "later", "L1").stdout)
        self.assertIn("only adds", self.bp_in(blk, "later", "L1", "--brief-file", self.brief(), ok=False).stderr)
        self.assertIn("only adds", self.bp_in(blk, "later", "L1", "--drop", ok=False).stderr)

    def test_an_items_flag_with_no_item_says_so(self):
        self.later("Tidy")
        for flags in (["--top"], ["--keep"], ["--drop"], ["--start"], ["--skip", "why"], ["--brief-file", "f"]):
            p = self.later(*flags, ok=False)
            self.assertIn(f"{flags[0]} needs an item: `bullpen later L7 {flags[0]}`", p.stderr)
            self.assertEqual(p.stdout, "")
