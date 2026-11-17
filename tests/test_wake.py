"""The backstop for news no coordinator reads (`_wake_check`), news --pause, and the
waiter a coordinator's Stop hook runs (`news --wake`)."""

import datetime as dt
import fcntl
import json
import os
import re
import signal
import subprocess
import time

from support import setUpModule, tearDownModule, BullpenTest, BULLPEN  # a temp folder for each module

STAMP = re.compile(r"^\S+")


def give_input(p, text):
    """Write text to p's stdin and close it. A waiter with nothing to wait for (not a pane
    of bullpen's) exits without reading its input, as a hook may, so its pipe can be gone
    by then: that is no failure. Its exit code and output say how it went, and the tests
    check those."""
    try:
        p.stdin.write(text)
    except BrokenPipeError:
        pass
    try:
        p.stdin.close()  # flushes: where a gone reader shows
    except BrokenPipeError:
        pass  # closed all the same


class WakeCase(BullpenTest):
    """A task, and news that isn't typed."""
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

    def wait_json(self):
        f = self.repo_file("news-c1.wait")
        return json.loads(f.read_text()) if f else {}

    def set_wait(self, w):
        self.repo_file("news.log").with_name("news-c1.wait").write_text(json.dumps(w))


class WakeTest(WakeCase):

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

    def test_pause_refuses_outside_rex(self):
        self.env.pop("REX_BLOCK")  # a shell outside Rex: whose pane it is can't be told
        for flag in ("--pause", "--resume"):
            p = self.bp("news", flag, ok=False)
            self.assertIn("isn't one of this repo's coordinators", p.stderr)
        self.assertIsNone(self.repo_file("news-c1.wait"))

    def test_a_line_logged_while_the_backstop_looks_keeps_its_mark(self):
        p = subprocess.Popen([str(BULLPEN), "_wake_check"], cwd=self.repo,
                             env={**self.env, "BULLPEN_MARK_WAIT": "1.5"})
        self.addCleanup(p.wait)
        time.sleep(0.6)  # it has looked at every repo: nothing waits
        self.ask(self.tid, "Which one?")
        p.wait()
        self.assertTrue(self.state("wake-due").exists())

    def test_typed_news_that_has_to_wait_while_the_watcher_flushes_keeps_its_mark(self):
        self.bp("config", "--news", "type")
        p = subprocess.Popen([str(BULLPEN), "_flush"], cwd=self.repo, env={**self.env, "BULLPEN_MARK_WAIT": "1.5"})
        self.addCleanup(p.wait)
        time.sleep(0.6)  # its round is done: nothing waited
        (self.fake / "screen").write_text("❯ half a message\n")
        self.ask(self.tid, "Which one?")  # held by the draft
        p.wait()
        self.assertTrue(self.state("outbox").exists())

    def test_typed_news_needs_no_backstop(self):
        self.bp("config", "--news", "type")
        self.ask(self.tid, "Which one?")
        self.assertFalse(self.state("wake-due").exists())


class WaiterTest(WakeCase):
    """`news --wake` as a process: what a Stop hook sees of it."""
    def setUp(self):
        super().setUp()
        self.bp("config", "--news", "wake")  # one more typed line: the switch
        self.told = len(self.news())

    def wake(self, block="blk1", event="Stop", *args):
        """Start `bullpen news --wake` as the hook of a pane's session would."""
        p = subprocess.Popen([str(BULLPEN), "news", "--wake", "--every", "0.05", *args], cwd=self.repo,
                             env={**self.env, "REX_BLOCK": block}, stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addCleanup(lambda: (p.kill(), p.wait(), p.stdout.close(), p.stderr.close()))
        give_input(p, json.dumps({"session_id": "sess-1", "hook_event_name": event}))
        return p

    def parked(self, p):
        """Wait until the waiter p owns c1's wait."""
        end = time.time() + 10
        while time.time() < end:
            f = self.repo_file("news-c1.wait")
            try:
                if f and json.loads(f.read_text()).get("waiter", {}).get("pid") == p.pid:
                    return
            except ValueError:
                pass
            if p.poll() is not None:
                self.fail(f"the waiter ended ({p.returncode}): {p.stderr.read()}")
            time.sleep(0.02)
        self.fail("the waiter never took the wait")

    def test_news_ends_it_with_exit_2_and_the_lines_on_stderr(self):
        p = self.wake()
        self.parked(p)
        self.assertEqual(self.wait_json()["waiter"]["session"], "sess-1")
        self.bp("add", "Other", "--brief", "x")
        self.bp("start", "BP-2")
        self.bp("drop", "BP-2")  # rides along
        time.sleep(0.3)
        self.assertIsNone(p.poll())  # bookkeeping: still parked
        self.ask(self.tid, "Which one?")
        self.assertEqual(p.wait(timeout=15), 2)
        lines = p.stderr.read().splitlines()
        self.assertEqual(len(lines), 2, lines)
        self.assertIn("BP-2", lines[0])
        self.assertEqual(lines[1], "bullpen · BP-1 is at a prompt (question): Which one?")
        self.assertEqual(p.stdout.read(), "")
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")  # the cursor is past them
        self.assertNotIn("waiter", self.wait_json())
        self.age_news(61)
        self.bp("_wake_check")
        self.assertEqual(self.unread_typed(), [])  # nothing left for the backstop

    def test_it_exits_at_once_where_there_is_nothing_to_wait_for(self):
        self.assertEqual(self.wake("me").wait(timeout=10), 0)  # not a coordinator's pane
        self.assertEqual(self.wake("blk9").wait(timeout=10), 0)  # not a pane of bullpen's
        self.bp_in("blk1", "news", "--pause")
        self.assertEqual(self.wake().wait(timeout=10), 0)  # paused
        self.bp_in("blk1", "news", "--resume")
        for news in ("type", "monitor"):  # only `wake` has a waiter
            self.bp("config", "--news", news)
            p = self.wake()
            self.assertEqual(p.wait(timeout=10), 0, news)
            self.assertEqual(p.stderr.read(), "")
            self.assertNotIn("waiter", self.wait_json())

    def test_a_signal_ends_it_quietly_and_the_cursor_stays(self):
        for sig in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
            p = self.wake()
            self.parked(p)
            self.ask(self.tid, f"Which one ({sig.name})?")
            time.sleep(0.3)  # the batch is settling
            p.send_signal(sig)
            self.assertEqual(p.wait(timeout=10), 0, sig)
            self.assertEqual(p.stderr.read(), "")
            self.assertNotIn("waiter", self.wait_json())
            self.assertIn(sig.name, self.bp_in("blk1", "news").stdout)  # the next reader has it

    def test_the_newest_waiter_wins(self):
        old = self.wake()
        self.parked(old)
        new = self.wake()
        self.assertEqual(old.wait(timeout=10), 0)
        self.assertEqual(old.stderr.read(), "")
        self.parked(new)
        self.ask(self.tid, "Which one?")
        self.assertEqual(new.wait(timeout=15), 2)

    def test_a_pause_ends_a_parked_waiter(self):
        p = self.wake()
        self.parked(p)
        self.bp_in("blk1", "news", "--pause")
        self.assertEqual(p.wait(timeout=10), 0)
        self.assertEqual(list(self.wait_json()), ["paused"])

    def test_switching_away_from_wake_ends_a_parked_waiter(self):
        p = self.wake()
        self.parked(p)
        self.bp("config", "--news", "type")
        self.assertEqual(p.wait(timeout=10), 0)
        self.assertEqual(p.stderr.read(), "")
        self.assertNotIn("waiter", self.wait_json())

    def test_follow_leaves_it_to_the_waiter(self):
        p = self.bp_in("blk1", "news", "--follow")
        self.assertEqual(p.stdout, "bullpen · news wakes you by itself now (`bullpen config --news wake`): "
                                   "don't start the monitor again\n")

    def test_the_backstop_leaves_news_to_a_live_waiter(self):
        self.ask(self.tid, "Which one?")
        self.age_news(61)
        self.bp("_wake_check")
        self.assertEqual(len(self.unread_typed()), 1)  # no waiter: typed
        self.set_wait({"woke": {"at": dt.datetime.now().astimezone().isoformat(timespec="seconds")}})
        p = self.wake()  # woke a moment ago: it holds its next wake for a minute
        self.parked(p)
        self.ask(self.tid, "And this?")
        self.age_news(61)
        self.bp("_wake_check")
        self.assertEqual(len(self.unread_typed()), 1)  # the waiter's to deliver
        self.assertTrue(self.state("wake-due").exists())
        self.assertIsNone(p.poll())
        p.terminate()
        self.assertEqual(p.wait(timeout=10), 0)
        self.bp("_wake_check")
        self.assertEqual(len(self.unread_typed()), 2)  # the waiter is gone: typed

    def test_a_dead_waiters_entry_doesnt_hold_the_backstop(self):
        self.ask(self.tid, "Which one?")
        self.age_news(61)
        dead = subprocess.Popen(["true"])
        dead.wait()
        until = (dt.datetime.now().astimezone() + dt.timedelta(hours=1)).isoformat(timespec="seconds")
        self.set_wait({"waiter": {"pid": dead.pid, "started": "x", "until": until}})
        self.bp("_wake_check")
        self.assertEqual(len(self.unread_typed()), 1)

    def test_a_waiter_left_by_a_dead_session_goes_and_takes_no_news(self):
        p = self.wake()
        self.parked(p)
        p.stdout.close()  # the session was killed outright: no signal, only its pipes closing
        p.stderr.close()
        self.assertEqual(p.wait(timeout=10), 0)
        self.assertNotIn("waiter", self.wait_json())
        self.ask(self.tid, "Which one?")
        self.age_news(61)
        self.bp("_wake_check")
        self.assertEqual(len(self.unread_typed()), 1)  # the backstop types it
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")  # once

    def test_wake_goes_alone(self):
        p = self.bp_in("blk1", "news", "--wake", "--follow", ok=False)
        self.assertIn("--wake and --follow don't go together", p.stderr)
        p = self.bp_in("blk1", "news", "--timeout", "5", ok=False)
        self.assertIn("--timeout goes with --wake", p.stderr)

    def test_it_expires_before_its_timeout(self):
        p = self.wake("blk1", "Stop", "--timeout", "60.4")  # a minute before: 0.4 s from now
        self.assertEqual(p.wait(timeout=10), 0)
        self.assertEqual(p.stderr.read(), "")
