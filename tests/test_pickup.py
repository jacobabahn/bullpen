"""Idle pickup: the backlog's next item starts by itself when there's room, under caps
(docs/backlog.md)."""

import datetime as dt
import json

from support import setUpModule, tearDownModule, BullpenTest  # a temp folder for each module


class PickupTest(BullpenTest):
    def later(self, *args, ok=True):
        return self.bp("later", *args, ok=ok)

    def brief(self, text="Do the thing.\n"):
        path = self.tmp / "b.md"
        path.write_text(text)
        return str(path)

    def item(self, iid):
        return next(i for i in json.loads(self.later("--json", "--all").stdout) if i["id"] == iid)

    def tasks(self):
        """Every task, closed ones too, by ID."""
        files = sorted((self.tmp / "state").rglob("tasks/*.json"), key=lambda f: int(f.stem.split("-")[1]))
        return [json.loads(f.read_text()) for f in files]

    def set_status(self, tid, status):
        f = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
        f.write_text(json.dumps({**json.loads(f.read_text()), "status": status}))

    def on(self, *more):
        self.bp("up")
        self.bp("config", "--pickup", "on", *more)

    def test_off_by_default_and_on_by_config(self):
        cfg = json.loads(self.bp("config", "--json").stdout)
        self.assertEqual({k: cfg[k] for k in cfg if k.startswith("pickup")}, {
            "pickup": False, "pickup_running": 1, "pickup_waiting": 2, "pickup_per_day": 4,
            "pickup_model": None, "pickup_auto_accept": False})
        self.bp("up")
        self.later("Tidy", "--brief-file", self.brief())
        self.bp("_pickup")
        self.assertEqual(self.tasks(), [])  # off: nothing starts by itself
        self.assertNotIn("Pickup", self.later().stdout)
        flag = self.tmp / "state" / "pickup"
        self.assertFalse(flag.exists())
        self.bp("config", "--pickup", "on")
        self.assertTrue(flag.exists())  # the watcher's timer looks
        self.bp("_pickup")
        t = self.show("BP-1")
        self.assertEqual((t["status"], t["started_by"], t["from"], t["auto_accept"]), ("working", "itself", "L1", False))
        self.assertIn("bullpen · BP-1 started by itself from the backlog (L1: Tidy)", self.news())
        self.assertIn("     from: the backlog (L1), started by itself", self.bp("show", "BP-1").stdout)
        self.bp("_pickup")  # nothing kept is left: the flag goes
        self.assertFalse(flag.exists())

    def test_one_runs_at_a_time_and_a_hand_start_is_not_capped(self):
        self.on("--pickup-model", "haiku")
        self.later("First", "--brief-file", self.brief())  # the add runs pickup
        self.assertEqual(self.show("BP-1")["started_by"], "itself")
        self.assertEqual(self.show("BP-1")["worker_model"], "haiku")
        self.later("Second", "--brief-file", self.brief())
        self.assertEqual(len(self.tasks()), 1)  # pickup_running 1
        self.assertIn("Pickup waits for room", self.later().stdout)
        self.set_status("BP-1", "review")  # in review still counts
        self.bp("_pickup")
        self.assertEqual(len(self.tasks()), 1)
        # a hand start is the person's: not capped, and the config's auto-accept
        self.bp("config", "--auto-accept", "on")
        self.later("L2", "--start")
        t = self.show("BP-2")
        self.assertEqual((t["started_by"], t.get("auto_accept")), ("person", None))
        # pickup_auto_accept: self-started tasks may accept by themselves
        self.set_status("BP-1", "done")
        self.bp("config", "--pickup-auto-accept", "on", "--max-running", "5")
        self.later("Third", "--brief-file", self.brief())
        self.assertEqual((self.show("BP-3")["started_by"], self.show("BP-3")["auto_accept"]), ("itself", True))

    def test_it_waits_while_anything_needs_you_and_pauses_at_the_pile(self):
        self.on("--pickup-running", "3", "--max-running", "5")
        self.later("One", "--brief-file", self.brief())
        self.later("Two", "--brief-file", self.brief())
        self.assertEqual([t["id"] for t in self.tasks()], ["BP-1", "BP-2"])
        # a hand-started task needs you: nothing new starts
        self.bp("add", "Hand", "--brief", "x")
        self.set_status("BP-3", "needs")
        self.later("Three", "--brief-file", self.brief())
        self.assertEqual(len(self.tasks()), 3)
        self.assertIn("Pickup waits: a task needs you", self.later().stdout)
        self.set_status("BP-3", "dropped")
        # two self-started tasks wait for you: paused, said once
        self.set_status("BP-1", "ready")
        self.set_status("BP-2", "finished")
        self.bp("_pickup")
        self.bp("_pickup")
        self.assertEqual(len(self.tasks()), 3)  # Three waits
        paused = [l for l in self.news() if "Pickup paused" in l]
        self.assertEqual(paused, ["bullpen · Pickup paused: BP-1 and BP-2 wait for you"])
        log = next((self.tmp / "state").rglob("notify.log")).read_text()
        self.assertIn("Pickup paused: BP-1 and BP-2 wait for you", log)
        self.assertIn("Pickup paused: BP-1 and BP-2 wait for you", self.later().stdout)
        # one accepted (here: closed): room again, and a later pause is said again
        self.set_status("BP-1", "done")
        self.bp("_pickup")
        self.assertEqual(self.show("BP-4")["started_by"], "itself")

    def test_an_item_with_no_brief_asks_the_coordinator_once(self):
        self.on()
        self.later("Tidy the errors")
        self.later("Later one", "--brief-file", self.brief())
        ask = ("bullpen · Room for one from the backlog: L1 Tidy the errors. Write its brief and give it with "
               "`bullpen later L1 --brief-file <f>` (pickup then starts it), or `bullpen later L1 --skip \"why\"`")
        self.bp("_pickup")
        self.assertEqual([l for l in self.news() if "Room for one" in l], [ask])  # once, and the order holds
        self.assertEqual(self.tasks(), [])
        # the coordinator writes the brief: pickup starts it
        self.bp_in("blk1", "later", "L1", "--brief-file", self.brief("Tidy them.\n"))
        t = self.show("BP-1")
        self.assertEqual((t["from"], t["started_by"], t["status"]), ("L1", "itself", "working"))

    def test_a_skip_passes_over_an_item_until_the_next_start(self):
        self.on("--max-running", "5", "--pickup-running", "2")
        self.later("Clashes", "--brief-file", self.brief())
        self.set_status("BP-1", "done")  # it started; make room again
        self.later("Overlaps BP-9")
        self.later("Fine", "--brief-file", self.brief())
        self.later("Also fine", "--brief-file", self.brief())
        self.bp_in("blk1", "later", "L2", "--skip", "overlaps BP-9")
        self.assertEqual(self.show("BP-2")["from"], "L3")  # the skip ran pickup: the next one
        self.assertNotIn("skipped", self.item("L2"))  # until the next start
        self.assertNotIn("BP-3", [t["id"] for t in self.tasks()])  # L2 has no brief: it asks

    def test_with_no_coordinator_the_next_item_with_a_brief_starts(self):
        self.on()
        (self.fake / "alive" / "blk1").unlink()  # the coordinator is gone
        self.later("Needs a brief")
        self.later("Has one", "--brief-file", self.brief())
        self.assertEqual(self.show("BP-1")["from"], "L2")
        self.assertIn("Room for one from the backlog: L1", self.news_log())  # logged for the next one

    def test_the_days_budget_and_the_usage_limit(self):
        self.on("--pickup-per-day", "1")
        self.later("One", "--brief-file", self.brief())
        self.set_status("BP-1", "done")
        self.later("Two", "--brief-file", self.brief())
        self.bp("_pickup")
        self.assertEqual(len(self.tasks()), 1)
        spent = [l for l in self.news() if "for today" in l]
        self.assertEqual(spent, ["bullpen · Pickup started its 1 for today (pickup_per_day): "
                                 "the backlog waits for tomorrow"])
        self.assertIn("Pickup done for today (1 of 1 started)", self.later().stdout)
        # yesterday's don't count; the usage limit stops it too
        f = next((self.tmp / "state").rglob("tasks/BP-1.json"))
        yesterday = (dt.datetime.now().astimezone() - dt.timedelta(days=1)).isoformat(timespec="seconds")
        f.write_text(json.dumps({**json.loads(f.read_text()), "created": yesterday}))
        resets = (dt.datetime.now().astimezone() + dt.timedelta(hours=1)).isoformat(timespec="seconds")
        (self.tmp / "state" / "limit.json").write_text(json.dumps({"at": resets, "resets": resets, "text": "x"}))
        self.bp("_pickup")
        self.assertEqual(len(self.tasks()), 1)
        self.assertIn("Pickup waits: the usage limit is reached", self.later().stdout)
        (self.tmp / "state" / "limit.json").unlink()
        self.bp("_pickup")
        self.assertEqual(self.show("BP-2")["started_by"], "itself")

    def test_viewing_the_list_or_an_item_starts_nothing(self):
        self.bp("up")
        self.later("Tidy", "--brief-file", self.brief())
        self.bp("config", "--pickup", "on")  # on, with room: the next change would start L1
        self.later("L1")
        self.later()
        self.later("--json")
        self.later("L1", "--json")
        self.assertEqual(self.tasks(), [])  # a read never starts work
        self.later("L1", "--top")  # a change does
        self.assertEqual(self.show("BP-1")["from"], "L1")

    def test_an_unanswered_ask_is_told_once_and_the_order_holds(self):
        self.on()
        self.later("Tidy the errors")
        self.later("Has one", "--brief-file", self.brief())
        self.assertIn("Pickup waits for a brief for L1 (asked at ", self.later().stdout)
        self.assertEqual([l for l in self.news() if "waits for a brief" in l], [])  # not yet
        # half an hour on, nobody wrote it: the person hears, once
        f = next((self.tmp / "state").rglob("pickup.json"))
        st = json.loads(f.read_text())
        st["asked"]["L1"] = (dt.datetime.now().astimezone() - dt.timedelta(minutes=31)).isoformat(timespec="seconds")
        f.write_text(json.dumps(st))
        self.bp("_pickup")
        self.bp("_pickup")
        told = [l for l in self.news() if "waits for a brief" in l]
        self.assertEqual(len(told), 1)
        self.assertRegex(told[0], r"^bullpen · Pickup waits for a brief for L1 \(asked at .+\): give it with "
                                  r"`bullpen later L1 --brief-file <f>`, or `bullpen later L1 --skip \"why\"` "
                                  r"to pass over it$")
        log = next((self.tmp / "state").rglob("notify.log")).read_text()
        self.assertEqual(log.count("Pickup waits for a brief for L1"), 1)
        self.assertEqual(self.tasks(), [])  # L2 doesn't go ahead of L1: the order is the person's
        self.assertIn("Pickup waits for a brief for L1", self.later().stdout)
        # a skip passes over it: L2 starts, and the line says so no more
        self.later("L1", "--skip", "later")
        self.assertEqual(self.show("BP-1")["from"], "L2")
        self.assertNotIn("waits for a brief", self.later().stdout)

    def test_the_pause_is_said_once_through_a_task_that_needs_you(self):
        self.on("--pickup-running", "3", "--max-running", "5")
        self.later("One", "--brief-file", self.brief())
        self.later("Two", "--brief-file", self.brief())
        self.set_status("BP-1", "ready")
        self.set_status("BP-2", "ready")
        self.later("Three", "--brief-file", self.brief())  # paused: Three waits
        # a hand task needs you, then is answered: still the same pause
        self.bp("add", "Hand", "--brief", "x")
        self.set_status("BP-3", "needs")
        self.bp("_pickup")
        self.set_status("BP-3", "working")
        self.bp("_pickup")
        self.assertEqual(len(self.tasks()), 3)
        self.assertEqual(len([l for l in self.news() if "Pickup paused" in l]), 1)
        log = next((self.tmp / "state").rglob("notify.log")).read_text()
        self.assertEqual(log.count("Pickup paused"), 1)
