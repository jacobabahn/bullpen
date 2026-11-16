"""The backstop for news no coordinator reads (`_wake_check`), and news --pause."""

import datetime as dt
import fcntl
import json
import re

from support import setUpModule, tearDownModule, BullpenTest  # a temp folder for each module

STAMP = re.compile(r"^\S+")


class WakeTest(BullpenTest):
    def setUp(self):
        super().setUp()
        self.tid = self.start_task()
        self.bp("config", "--news", "monitor")  # typed once: the line saying so
        self.told = len(self.news())

    def state(self, name):
        return self.tmp / "state" / name

    def repo_file(self, name):
        return next((self.tmp / "state" / "repos").glob(f"*/{name}"), None)

    def age_news(self, seconds):
        """Every news line was logged this many seconds ago (same length: cursors hold)."""
        at = (dt.datetime.now().astimezone() - dt.timedelta(seconds=seconds)).isoformat(timespec="seconds")
        log = self.repo_file("news.log")
        log.write_text("".join(l if l.startswith("\t") else STAMP.sub(at, l) for l in
                               log.read_text().splitlines(keepends=True)))

    def age_wait(self, key, seconds):
        """news-c1.wait's stamp under key (or held's) is this many seconds old."""
        f = self.repo_file("news-c1.wait")
        w = json.loads(f.read_text())
        at = (dt.datetime.now().astimezone() - dt.timedelta(seconds=seconds)).isoformat(timespec="seconds")
        if key == "held":
            w["held"]["since"] = at
        else:
            w[key] = at
        f.write_text(json.dumps(w))

    def unread_typed(self):
        return self.news()[self.told:]

    def test_unread_news_is_typed_once_after_a_minute(self):
        self.ask(self.tid, "Which one?")
        self.assertTrue(self.state("wake-due").exists())
        self.bp("_wake_check")
        self.assertEqual(self.unread_typed(), [])  # under a minute: a reader may come
        self.assertTrue(self.state("wake-due").exists())
        self.age_news(61)
        self.bp("_wake_check")
        self.bp("_wake_check")
        self.assertEqual(self.unread_typed(), ["bullpen · BP-1 is at a prompt (question): Which one?"])
        self.assertFalse(self.state("wake-due").exists())
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")  # the cursor moved past it

    def test_bookkeeping_wakes_nobody_and_rides_along_with_the_next_wake(self):
        self.bp("add", "Other", "--brief", "x")
        self.bp("start", "BP-2")
        self.bp("drop", "BP-2")  # "dropped": not worth a wake
        self.assertFalse(self.state("wake-due").exists())
        self.ask(self.tid, "Which one?")
        self.age_news(61)
        self.bp("_wake_check")
        typed = self.unread_typed()
        self.assertEqual(len(typed), 2)
        self.assertIn("BP-2", typed[0])  # dropped
        self.assertIn("Which one?", typed[1])

    def test_a_live_reader_holds_the_backstop(self):
        self.ask(self.tid, "Which one?")
        self.age_news(61)
        with open(self.repo_file("news.log").parent / "news-c1.lock", "a") as f:
            fcntl.flock(f, fcntl.LOCK_EX)  # a `news --follow` runs
            self.bp("_wake_check")
            self.assertEqual(self.unread_typed(), [])
            self.assertTrue(self.state("wake-due").exists())
        self.bp("_wake_check")
        self.assertEqual(len(self.unread_typed()), 1)

    def test_a_follower_reads_it_and_nothing_is_typed(self):
        p, lines, _ = self.follow()
        self.ask(self.tid, "Which one?")
        self.assertEqual(len(lines(1)), 1)
        self.stop_follower(p)
        self.age_news(61)
        self.bp("_wake_check")
        self.assertEqual(self.unread_typed(), [])
        self.assertFalse(self.state("wake-due").exists())

    def stop_follower(self, p):
        p.terminate()
        p.wait()

    def test_a_draft_holds_it_and_the_person_hears_once_after_ten_minutes(self):
        (self.fake / "screen").write_text("❯ half a message\n")
        self.ask(self.tid, "Which one?")
        self.age_news(61)
        self.bp("_wake_check")
        self.bp("_wake_check")
        self.assertEqual(self.unread_typed(), [])
        self.assertNotIn("hasn't seen", self.repo_file("notify.log").read_text())  # not yet
        self.age_wait("held", 601)
        self.bp("_wake_check")
        self.ask(self.tid, "And this?")  # the same batch: no second notification
        self.age_news(61)
        self.bp("_wake_check")
        log = self.repo_file("notify.log").read_text()
        self.assertEqual(log.count("has news it hasn't seen"), 1)
        (self.fake / "screen").write_text("❯ \n")
        self.bp("_wake_check")
        self.assertEqual(len(self.unread_typed()), 2)

    def test_a_pause_quiets_the_backstop_until_resume(self):
        p = self.bp_in("blk1", "news", "--pause")
        self.assertIn("news for c1 is paused", p.stdout)
        self.assertIn("paused already", self.bp_in("blk1", "news", "--pause").stdout)
        self.ask(self.tid, "Which one?")
        self.age_news(61)
        self.bp("_wake_check")
        self.assertEqual(self.unread_typed(), [])
        self.assertFalse(self.state("wake-due").exists())  # nothing for the watcher to do
        self.assertIn("news for c1 is on again", self.bp_in("blk1", "news", "--resume").stdout)
        self.assertIn("wasn't paused", self.bp_in("blk1", "news", "--resume").stdout)
        self.bp("_wake_check")
        self.assertEqual(self.unread_typed(), [])  # a minute from the resume for a reader
        self.age_wait("resumed", 61)
        self.bp("_wake_check")
        self.assertEqual(len(self.unread_typed()), 1)

    def test_up_ends_a_pause_with_one_line(self):
        self.bp_in("blk1", "news", "--pause")
        out = self.bp("up").stdout
        self.assertEqual(out.count("was paused: `up` ended the pause"), 1)
        self.assertNotIn("paused", self.bp("up").stdout)

    def test_pause_refuses_typed_news_and_other_panes(self):
        p = self.bp("news", "--pause", ok=False)  # from "me", not a coordinator
        self.assertIn("isn't one of this repo's coordinators", p.stderr)
        self.bp("config", "--news", "type")
        p = self.bp_in("blk1", "news", "--pause", ok=False)
        self.assertIn("nothing to pause", p.stderr)

    def test_typed_news_needs_no_backstop(self):
        self.bp("config", "--news", "type")
        self.ask(self.tid, "Which one?")
        self.assertFalse(self.state("wake-due").exists())
