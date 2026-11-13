"""The timeline each task records, and `bullpen stats`."""

import datetime as dt
import json
import textwrap

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module


class StatsTest(BullpenTest):
    def timeline_path(self, tid):
        return next((self.tmp / "state").rglob(f"timeline/{tid}.jsonl"))

    def timeline(self, tid):
        return [json.loads(l) for l in self.timeline_path(tid).read_text().splitlines()]

    def through_a_question_two_reviews_and_accept(self):
        """A task that starts, asks a question, is ready, gets a review that asks for
        changes, a second that approves, and is accepted."""
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "")
        self.bp("_event", blk, "blocked", "question", "Which file?")
        self.bp("_event", blk, "working", "", "")  # answered
        self.commit_and_ready(tid)
        self.bp("_event", blk, "done", "", "")  # it ends its turn after ready
        self.verdict(tid, "changes", "The fix misses the empty case.")
        self.bp("_event", blk, "working", "", "")
        self.commit_and_ready(tid, 2)
        self.verdict(tid, "approve")
        self.bp("accept", tid)
        return tid

    def test_the_timeline_of_a_task_through_review_to_accept(self):
        tid = self.through_a_question_two_reviews_and_accept()
        said = []
        for e in self.timeline(tid):
            self.assertTrue(dt.datetime.fromisoformat(e["at"]))
            if e["event"] == "status":
                said.append(("status", e["from"], e["to"], e.get("back"), (e.get("stop") or {}).get("kind")))
            elif e["event"] in ("review_started", "review_finished"):
                said.append((e["event"], e["round"], e["kind"], e.get("verdict")))
            else:
                said.append((e["event"],))
        self.assertEqual(said, [
            ("added",),
            ("status", "todo", "working", None, None),
            ("status", "working", "needs", None, "prompt"),
            ("status", "needs", "working", None, None),
            ("status", "working", "review", None, None),
            ("review_started", 1, "light", None),
            ("turn_ended",),
            ("review_finished", 1, "light", "changes"),
            ("status", "review", "working", "review", None),
            ("status", "working", "review", None, None),
            ("review_started", 2, "followup", None),
            ("review_finished", 2, "followup", "approved"),
            ("status", "review", "ready", None, None),
            ("status", "ready", "done", None, None),
        ])
        self.assertEqual(self.timeline(tid)[-1]["outcome"], "accepted")
        self.assertNotIn("back", self.show(tid))  # the mark is the save's, not the task's
        self.bp("ls")
        self.bp("show", tid)
        self.assertEqual(len(self.timeline(tid)), len(said))  # reading tasks writes nothing

    def spread_out(self, tid, minutes):
        """Stamp the timeline's entries minutes[i] after 10:00 on a day."""
        start = dt.datetime(2026, 11, 2, 10, 0).astimezone()
        es = self.timeline(tid)
        self.assertEqual(len(es), len(minutes))
        for e, m in zip(es, minutes):
            e["at"] = (start + dt.timedelta(minutes=m)).isoformat()
        self.timeline_path(tid).write_text("".join(json.dumps(e) + "\n" for e in es))

    def test_stats_totals_for_that_task(self):
        tid = self.through_a_question_two_reviews_and_accept()
        # added, started 5 min later, asks at 20, answered at 30, ready at 60 (reviewed
        # until 80), back to work until 100, reviewed until 110, accepted at 150
        self.spread_out(tid, [0, 5, 20, 30, 60, 60, 61, 80, 80, 100, 100, 110, 110, 150])
        st = json.loads(self.bp("stats", tid, "--json").stdout)
        mins = {k: v // 60 for k, v in st["time"].items()}
        self.assertEqual(mins, {"working": 15 + 30 + 20, "rebasing": 0, "review_wait": 0, "review": 20 + 10,
                                "person": 10 + 40, "coordinator": 0, "stalled": 0, "queued": 5})
        self.assertEqual((st["review_rounds"], st["reviews"]), (2, {"light": 1, "followup": 1}))
        self.assertEqual((st["trips_back"], st["questions"], st["turns"], st["stalls"]), ({"review": 1}, 1, 1, 0))
        self.assertEqual((st["outcome"], st["timeline"], st["commits"]), ("accepted", "whole", 2))
        self.assertEqual(st["lines"], {"added": 2, "removed": 0})
        self.assertEqual(st["events"][0]["event"], "added")
        self.assertEqual(st["elapsed"], 150 * 60)
        out = self.bp("stats", tid).stdout
        self.assertIn("BP-1 Fix the thing", out)
        self.assertIn("sent back to work (review)", out)
        self.assertIn("light review asked for changes", out)
        self.assertRegex(out, r"working +1h 05m")
        self.assertRegex(out, r"waiting for the person +50m")
        self.assertIn("Review rounds 2 (reviews: light 1, followup 1); trips back: review 1", out)
        self.assertIn("Changes: 2 commits, +2 −0", out)
        self.assertIn("accepted, 2h 30m from added to closed", out)
        # across closed tasks
        every = json.loads(self.bp("stats", "--json").stdout)
        self.assertEqual((every["count"], every["with_timeline"], every["outcomes"]), (1, 1, {"accepted": 1}))
        self.assertEqual(every["phases"]["working"]["median"], 65 * 60)
        self.assertEqual(every["phases"]["working"]["slowest"], [{"id": tid, "seconds": 65 * 60}])
        self.assertEqual(every["trips_back"], {"review": 1})
        out = self.bp("stats").stdout
        self.assertIn("1 closed task: accepted 1.", out)
        self.assertIn("Trips back: review 1 (1 of 1 tasks went back at least once)", out)
        self.assertIn("No closed tasks (since", self.bp("stats", "--since", "2099-01-01").stdout)
        self.assertEqual(json.loads(self.bp("stats", "--last", "0", "--json").stdout)["count"], 0)

    def test_a_task_from_before_timelines_has_its_news_and_unknown_phases(self):
        tid = self.through_a_question_two_reviews_and_accept()
        self.timeline_path(tid).unlink()
        st = json.loads(self.bp("stats", tid, "--json").stdout)
        self.assertEqual(st["timeline"], "none")
        self.assertEqual(set(st["time"].values()), {None})
        self.assertEqual((st["trips_back"], st["review_rounds"]), (None, 2))
        self.assertTrue(st["events"])
        self.assertEqual({e["event"] for e in st["events"]}, {"news"})
        self.assertIn("No timeline", self.bp("stats", tid).stdout)
        every = json.loads(self.bp("stats", "--json").stdout)
        self.assertEqual((every["count"], every["with_timeline"]), (1, 0))

    def test_an_auto_accept_refused_for_a_conflict_is_on_the_timeline(self):
        gh = self.fake / "gh"
        gh.write_text(gh.read_text() + textwrap.dedent("""\
            if sys.argv[1:3] == ["pr", "merge"]:
                print("Pull request is not mergeable: the merge commit cannot be cleanly created", file=sys.stderr)
                sys.exit(1)
        """))
        self.bp("config", "--auto-accept", "on")
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        self.commit_and_ready(tid)
        (self.repo / "fix1.txt").write_text("fixed on main\n")  # what another task merged meanwhile
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "Clash")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "HEAD:main")
        self.bp("verdict", tid, "approve", "--summary", "Looks right.", cwd=wt)
        es = self.timeline(tid)
        self.assertEqual([e["reason"] for e in es if e["event"] == "accept_refused"], ["conflict"])
        self.assertEqual([(e["from"], e["back"]) for e in es if e.get("back")], [("ready", "rebase")])
        st = json.loads(self.bp("stats", tid, "--json").stdout)
        self.assertEqual((st["accept_refused"], st["trips_back"]), ({"conflict": 1}, {"rebase": 1}))
