"""What the watcher reports: drift, stuck panes, the usage limit."""

import datetime as dt
import json
from pathlib import Path

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module


class StatusTest(BullpenTest):
    def test_check_prints_nothing_when_all_agree(self):
        tid = self.start_task()
        self.agent(self.block(tid), "working", msg="Reading")
        self.settle()
        p = self.bp("check")
        self.assertEqual((p.returncode, p.stdout), (0, ""))

    def test_check_leaves_a_task_bullpen_just_changed(self):
        # `tell` (or start, resume, a verdict) marked it working a moment ago; its agent's
        # record still says idle from before, and its working event is on the way
        tid = self.start_task()
        self.agent(self.block(tid), "idle", ago=600)
        self.assertEqual(self.check(settle=False), [])
        self.bp("check", "--fix", "--quiet")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_check_skips_a_task_without_a_pane(self):
        self.bp("up")
        self.bp("add", "Later", "--brief", "x")  # todo: no pane
        tid = self.start_task("Now")
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)  # stopped, its pane forgotten
        self.assertEqual(self.show(tid)["status"], "stopped")
        self.settle()
        p = self.bp("check")
        self.assertEqual((p.returncode, p.stdout), (0, ""))

    def test_check_finds_a_missed_end_of_turn_and_fix_reports_it_once(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Running the tests")
        self.agent(blk, "idle", msg="Done")
        self.assertEqual(self.check(), [f"{tid} is marked working but its agent is idle at its prompt: "
                                        "`bullpen peek` it"])
        self.assertEqual(self.bp("check", ok=False).returncode, 1)
        self.assertEqual(self.show(tid)["status"], "working")  # no --fix: nothing changes
        before = len(self.news())
        self.assertEqual(self.bp("check", "--fix", "--quiet").stdout, "")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Ended its turn without saying ready"))
        news = self.news()[before:]
        self.assertEqual(len(news), 1)
        self.assertIn("ended its turn without `ready`", news[0])
        self.bp("check", "--fix", "--quiet")  # agrees now: nothing again
        self.assertEqual(self.check(), [])
        self.assertEqual(len(self.news()), before + 1)

    def test_check_leaves_a_state_that_just_changed(self):
        tid = self.start_task()
        self.agent(self.block(tid), "idle", ago=5)  # the watcher reports it after its own wait
        self.assertEqual(self.check(), [])

    def test_check_finds_a_missed_permission_prompt_and_its_end(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.agent(blk, "blocked", kind="permission", msg="Allow rm?")
        self.assertEqual(self.check(), [f"{tid} is marked working but its agent is waiting on you (permission)"])
        self.check("--fix")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Needs you (permission): Allow rm?"))
        self.assertIn("is at a permission prompt", self.news()[-1])
        self.agent(blk, "working", msg="Removing")
        self.assertEqual(self.check(), [f"{tid} is marked needs (permission) but its agent is working again"])
        self.assertEqual(self.check("--fix"), [f"{tid} is marked needs (permission) but its agent is "
                                               "working again (fixed)"])
        self.assertEqual(self.show(tid)["status"], "working")
        self.assertEqual(self.check(), [])

    def test_check_sees_a_busy_agent_working_again(self):
        # a working agent's record is new every few seconds (its message changes)
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        self.agent(blk, "working", msg="Running rm", ago=3)
        self.assertEqual(self.check(), [f"{tid} is marked needs (permission) but its agent is working again"])
        self.check("--fix", "--quiet")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_check_finds_a_gone_pane(self):
        tid = self.start_task()
        (self.fake / "alive" / self.block(tid)).unlink()
        self.assertEqual(self.check(), [f"{tid} is marked working but its pane is gone"])
        self.check("--fix", "--quiet")
        self.assertEqual(self.show(tid)["status"], "stopped")
        self.assertIn("stopped before saying ready", self.news()[-1])
        self.assertEqual(self.check(), [])

    def test_check_finds_a_reviewer_idle_without_a_verdict(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.agent(self.block(tid), "idle")  # the worker waits: that's fine in review
        self.agent(reviewer, "idle")
        self.assertEqual(self.check(), [f"{tid} is in review but its reviewer is idle without a verdict"])
        self.check("--fix", "--quiet")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Reviewer stopped without a verdict"))
        self.assertIn("stopped without a verdict", self.news()[-1])
        self.assertEqual(self.check(), [])
        self.agent(reviewer, "working")
        self.assertEqual(self.check(), [f"{tid} is marked triage (reviewer stopped without a verdict) but "
                                        "its reviewer is working again"])
        self.check("--fix")
        self.assertEqual(self.show(tid)["status"], "review")

    def test_check_counts_a_background_shell_as_busy(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Running the tests")
        self.agent(blk, "idle")
        (self.fake / f"screen_{blk}").write_text("❯ \n  ⏵⏵ auto mode on · 1 shell · ← 1 agent\n")
        self.assertEqual(self.check(), [])
        # and the watcher's end of turn is held back for it: it takes another turn
        self.bp("_event", blk, "idle", "", "Waiting for the tests")
        self.assertEqual(self.show(tid)["status"], "working")
        self.assertFalse([l for l in self.news() if "ended its turn" in l])
        (self.fake / f"screen_{blk}").unlink()  # the shell ended, and so did its next turn
        self.bp("_event", blk, "working", "", "Reading the results")
        self.bp("_event", blk, "idle", "", "Waiting for the tests")
        self.assertEqual(self.show(tid)["status"], "triage")

    def test_a_background_shell_left_running_is_drift_after_a_while(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Starting the server")
        (self.fake / f"screen_{blk}").write_text("❯ \n  ⏵⏵ auto mode on · 1 shell\n")
        self.agent(blk, "idle", ago=19 * 60)  # stuck_after is 20 min
        self.assertEqual(self.check(), [])
        self.agent(blk, "idle", ago=25 * 60)
        line = (f"{tid} is marked working but its agent has been idle at its prompt for 25 min, "
                "with a background shell still running: `bullpen peek` it")
        self.assertEqual(self.check(), [line])
        self.assertEqual(self.check("--fix"), [line + " (fixed)"])
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Ended its turn without saying ready"))
        self.assertIn("background shell of its own still runs", self.news()[-1])
        before = len(self.news())
        self.check("--fix")
        self.assertEqual((self.check(), len(self.news())), ([], before))

    def test_a_reviewers_background_shell_left_running_is_drift_after_a_while(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        (self.fake / f"screen_{reviewer}").write_text("❯ \n  ⏵⏵ auto mode on · 1 shell\n")
        self.agent(reviewer, "idle", ago=5 * 60)
        self.assertEqual(self.check(), [])
        self.agent(reviewer, "idle", ago=30 * 60)
        self.assertEqual(self.check(), [f"{tid} is in review but its reviewer is idle without a verdict"])

    def test_check_goes_by_the_screen_without_a_status_record(self):
        tid = self.start_task()
        blk = self.block(tid)
        (self.fake / f"screen_{blk}").write_text("✶ Misting… (28s)\n  ⏵⏵ auto mode on · esc to interrupt\n")
        self.assertEqual(self.check(), [])
        (self.fake / f"screen_{blk}").write_text("Done.\n\n❯ \n  ⏵⏵ auto mode on\n")
        self.assertEqual(self.check(), [f"{tid} is marked working but its agent is idle at its prompt: "
                                        "`bullpen peek` it"])
        (self.fake / f"screen_{blk}").write_text("something else\n")  # unsure: say nothing
        self.assertEqual(self.check(), [])

    def test_check_hints_at_finished_work_that_never_ran_ready(self):
        tid = self.start_task()
        blk = self.block(tid)
        wt = self.show(tid)["worktree"]
        (Path(wt) / "fix.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        self.bp("_event", blk, "idle", "", "")
        self.agent(blk, "idle")
        self.assertEqual(self.check(), [f"{tid} is idle with commits and a clean worktree but never ran "
                                        "ready: `bullpen peek` it, then `bullpen tell` it to run ready if "
                                        "it's done"])
        self.assertEqual(self.bp("check").returncode, 0)  # a hint, not drift
        (Path(wt) / "more.txt").write_text("x\n")  # still at it
        self.assertEqual(self.check(), [])

    def test_check_takes_ids_and_refuses_unknown_ones(self):
        tid = self.start_task()
        self.agent(self.block(tid), "idle")
        self.assertEqual(len(self.check(tid)), 1)
        self.assertNotEqual(self.bp("check", "BP-99", ok=False).returncode, 0)

    def test_check_all_repos_is_the_watchers(self):
        tid = self.start_task()
        self.agent(self.block(tid), "idle")
        self.settle()
        self.bp("check", "--fix", "--quiet", "--all-repos", cwd=self.tmp)
        self.assertEqual(self.show(tid)["status"], "triage")

    def test_a_quiet_worker_is_news_once_until_its_next_event(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "working", "", "Running the tests")
        self.bp("_stuck", worker, "20")
        self.bp("_stuck", worker, "21")  # the same quiet spell: no more news
        self.assertEqual(self.news(), ["bullpen · BP-1 has shown no progress for 20 min: `bullpen peek BP-1`"])
        t = self.show(tid)
        self.assertEqual((t["status"], t["stuck"]["who"]), ("working", "worker"))  # still working
        self.assertIn("No progress for 20 min. p shows its screen.", self.bp("ls").stdout)
        self.assertIn("no progress for 20 min", self.bp("show", tid).stdout)
        self.bp("_event", worker, "working", "", "Running the tests")  # a sign of life
        self.assertNotIn("stuck", self.show(tid))
        self.bp("_stuck", worker, "20")  # quiet again: a new spell
        self.assertEqual(len(self.news()), 2)

    def test_any_event_ends_a_quiet_spell(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "blocked", "permission", "Allow rm?")
        self.bp("_event", worker, "working", "", "")
        self.bp("_stuck", worker, "20")
        self.bp("_event", worker, "blocked", "permission", "Allow rm?")
        t = self.show(tid)
        self.assertEqual(t["status"], "needs")
        self.assertNotIn("stuck", t)

    def test_only_a_working_task_can_be_stuck(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "blocked", "permission", "Allow rm?")
        self.bp("_stuck", worker, "20")  # it waits on the person: that's known
        self.bp("_stuck", "blk1", "20")  # the coordinator
        self.bp("_stuck", "nope", "20")  # not bullpen's
        self.assertNotIn("stuck", self.show(tid))
        self.assertEqual(len(self.news()), 1)  # just the permission prompt

    # ----- the usage limit -----

    def limit_msg(self, minutes=60):
        """A limit message whose reset time is this many minutes from now."""
        import datetime as dt
        when = dt.datetime.now() + dt.timedelta(minutes=minutes)
        return f"You've hit your 5-hour limit · resets {when.hour % 12 or 12}:{when.minute:02d}{'am' if when.hour < 12 else 'pm'}"

    def limit_file(self):
        return self.tmp / "state" / "limit.json"

    def reset_passed(self, *tids):
        """Move the noted reset time (and each task's) ten minutes into the past."""
        import datetime as dt
        past = (dt.datetime.now().astimezone() - dt.timedelta(minutes=10)).isoformat(timespec="seconds")
        lim = json.loads(self.limit_file().read_text())
        self.limit_file().write_text(json.dumps({**lim, "resets": past}))
        for tid in tids:
            path = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
            t = json.loads(path.read_text())
            t["limit"]["resets"] = past
            path.write_text(json.dumps(t))

    def test_a_limit_message_marks_the_task_with_one_news_line(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "working", "", "")
        self.bp("_event", worker, "error", "", self.limit_msg())
        self.bp("_event", worker, "idle", "", "")  # what follows says nothing new
        t = self.show(tid)
        self.assertEqual(t["status"], "working")  # not "needs": nothing for the person to do
        self.assertEqual(t["limit"]["who"], "worker")
        self.assertRegex(t["why"], r"^Usage limit, resets (\w{3} )?\d{1,2}:\d\d [AP]M$")
        news = self.news()
        self.assertEqual(len(news), 1)
        self.assertRegex(news[0], r"^bullpen · BP-1 stopped at the usage limit, resets .*[AP]M: nothing to do, "
                                  r"it is continued then\. `bullpen start` refuses meanwhile "
                                  r"\(`--force` overrides, only if the person asks\)$")
        self.assertIn("Usage limit, resets", self.bp("ls").stdout)
        self.assertTrue(self.limit_file().exists())

    def test_a_limit_message_only_on_screen_is_found(self):
        tid = self.start_task()
        (self.fake / "screen").write_text(f"● Working on it\n  ⎿ {self.limit_msg()}\n\n❯ \n")
        self.bp("_event", self.block(tid), "error", "", "API error")
        t = self.show(tid)
        self.assertEqual((t["status"], t["limit"]["who"]), ("working", "worker"))
        self.assertIn("stopped at the usage limit", self.news()[-1])

    def test_a_finished_turn_is_not_a_limit_whatever_the_screen_says(self):
        tid = self.start_task()
        (self.fake / "screen").write_text(f"I added the text '{self.limit_msg()}' to the tests.\n")
        self.bp("_event", self.block(tid), "done", "", "")
        t = self.show(tid)
        self.assertEqual(t["status"], "triage")
        self.assertNotIn("limit", t)
        self.assertFalse(self.limit_file().exists())

    def test_start_is_refused_at_the_limit_unless_forced(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "error", "", self.limit_msg())
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        p = self.bp("start", tid2, ok=False)
        self.assertEqual(p.returncode, 1)
        self.assertRegex(p.stderr, r"usage limit is reached \(resets .*[AP]M\)")
        self.assertIn("--force", p.stderr)
        self.assertEqual(self.show(tid2)["status"], "todo")
        self.assertIsNone(self.show(tid2)["worktree"])
        self.bp("start", tid2, "--force")
        self.assertEqual(self.show(tid2)["status"], "working")

    def test_start_works_again_once_a_limited_worker_is_working(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "error", "", self.limit_msg())
        self.bp("_event", worker, "working", "", "")  # the person continued it, or Claude Code did
        t = self.show(tid)
        self.assertNotIn("limit", t)
        self.assertEqual((t["status"], t["why"]), ("working", None))
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        self.assertEqual(self.show(tid2)["status"], "working")

    def test_start_works_again_after_the_reset_time(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "error", "", self.limit_msg())
        self.reset_passed()
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        self.assertEqual(self.show(tid2)["status"], "working")

    def test_limited_tasks_are_continued_after_the_reset(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "error", "", self.limit_msg())
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", tid2, "--force")
        worker2 = self.block(tid2)
        self.bp("_event", worker2, "error", "", self.limit_msg())
        self.assertIn("BP-2 stopped at the usage limit", self.news()[-1])
        self.assertNotIn("bullpen start", self.news()[-1])  # said once, with the first
        (self.fake / "alive" / worker2).unlink()  # its pane goes meanwhile
        self.bp("_gone", worker2)
        self.assertEqual(self.show(tid2)["status"], "working")  # still waiting for the reset
        before = len(self.typed())
        self.bp("_limit")  # not yet
        self.assertEqual(len(self.typed()), before)
        self.reset_passed(tid, tid2)
        self.bp("_limit")
        self.assertIn((worker, "The usage limit has reset. Carry on where you stopped."), self.typed())
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertIn("--resume", cmd)  # BP-2, resumed in a new pane
        self.assertEqual(self.news()[-1], "bullpen · the usage limit has reset: continued BP-1, "
                                          "BP-2 (its pane was gone: resumed in a new one): nothing to do")
        self.assertIn(" limit-continued: bullpen · the usage limit has reset", self.news_log())  # rides along
        for t in (self.show(tid), self.show(tid2)):
            self.assertEqual((t["status"], t["why"]), ("working", None))
            self.assertNotIn("limit", t)
        self.assertFalse(self.limit_file().exists())
        n = len(self.typed())
        self.bp("_limit")  # nothing left to do
        self.assertEqual(len(self.typed()), n)

    def test_a_limited_worker_the_message_doesnt_reach_goes_to_its_coordinator(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "error", "", self.limit_msg())
        self.reset_passed(tid)
        (self.fake / f"send_fails_{worker}").write_text("")
        self.bp("_limit")
        t = self.show(tid)
        self.assertEqual((t["status"], t["stop"]), ("triage", {"by": "worker", "kind": "limit"}))
        self.assertEqual(t["why"], "Couldn't be continued after the usage limit: rex: couldn't send to the block")
        self.assertIn("BP-1 (not continued, for its coordinator to see to: rex: couldn't send", self.news()[-1])
        self.assertTrue(self.news()[-1].endswith("; `bullpen resume BP-1` continues it)"))
        self.assertIn(" limit-reset: bullpen · the usage limit has reset", self.news_log())  # it wakes
        (self.fake / f"send_fails_{worker}").unlink()
        self.bp("tell", tid, "Carry on.")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_one_agent_working_again_leaves_the_others_to_be_continued(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "error", "", self.limit_msg())
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", tid2, "--force")
        worker2 = self.block(tid2)
        self.bp("_event", worker2, "error", "", self.limit_msg())
        self.bp("_event", worker, "working", "", "")  # before the reset: Claude Code's own continue, or a tell
        tid3 = self.bp("add", "A third", "--brief", "x").stdout.strip()
        self.bp("start", tid3)  # the limit no longer holds start back
        self.bp("_limit")  # BP-2's reset time hasn't come
        self.assertIn("limit", self.show(tid2))
        self.assertTrue(self.limit_file().exists())
        self.reset_passed(tid2)
        self.bp("_limit")
        self.assertIn((worker2, "The usage limit has reset. Carry on where you stopped."), self.typed())
        self.assertNotIn((worker, "The usage limit has reset. Carry on where you stopped."), self.typed())
        self.assertEqual(self.news()[-1], "bullpen · the usage limit has reset: continued BP-2: nothing to do")
        self.assertNotIn("limit", self.show(tid2))
        self.assertFalse(self.limit_file().exists())

    def test_a_blocked_agent_that_mentions_the_limit_needs_you(self):
        tid = self.start_task()
        worker = self.block(tid)
        (self.fake / "screen").write_text(f"  ⎿ {self.limit_msg()}\n")
        self.bp("_event", worker, "blocked", "permission", "Bash: grep -n 'usage limit reached' bin/bullpen")
        t = self.show(tid)
        self.assertEqual(t["status"], "needs")
        self.assertNotIn("limit", t)
        self.assertIn("BP-1 is at a permission prompt", self.news()[-1])
        self.assertFalse(self.limit_file().exists())
        reviewer_tid = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", reviewer_tid)
        reviewer = self.commit_and_ready(reviewer_tid)
        self.bp("_event", reviewer, "blocked", "permission", "Bash: grep 'usage limit reached' x")
        t = self.show(reviewer_tid)
        self.assertEqual((t["status"], t["why"][:18]), ("needs", "Reviewer needs you"))
        self.assertNotIn("limit", t)

    def test_an_unreadable_reset_time_is_not_continued(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.bp("_event", worker, "error", "", "Usage limit reached")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("working", "Usage limit, reset time unknown"))
        self.assertIsNone(t["limit"]["resets"])
        self.assertIn("its reset time couldn't be read, so it won't be continued by itself: tell the person, "
                      "and `bullpen tell BP-1 continue` when they say the limit has reset", self.news()[-1])
        before = len(self.typed())
        self.bp("_limit")
        self.assertEqual(len(self.typed()), before)
        self.assertIn("limit", self.show(tid))
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.assertIn("reset time unknown", self.bp("start", tid2, ok=False).stderr)

    def test_a_limited_reviewer_is_continued_too(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp("_event", reviewer, "error", "", self.limit_msg())
        t = self.show(tid)
        self.assertEqual((t["status"], t["limit"]["who"]), ("review", "reviewer"))
        self.assertIn("BP-1's reviewer stopped at the usage limit", self.news()[-1])
        self.reset_passed(tid)
        self.bp("_limit")
        self.assertIn((reviewer, "The usage limit has reset. Carry on where you stopped."), self.typed())
        self.assertEqual(self.news()[-1], "bullpen · the usage limit has reset: continued BP-1's reviewer: "
                                          "nothing to do")
        self.assertEqual(self.show(tid)["status"], "review")

    def test_a_limit_stays_with_the_screen_that_showed_it(self):
        tid = self.start_task()
        worker = self.block(tid)
        (self.fake / "screen").write_text(f"  ⎿ {self.limit_msg()}\n")
        self.bp("_event", worker, "error", "", "")
        self.bp("_event", worker, "working", "", "")  # continued; the old line is still on screen
        self.bp("_event", worker, "idle", "", "")
        t = self.show(tid)
        self.assertEqual(t["status"], "triage")  # an ordinary end of turn
        self.assertNotIn("limit", t)

    def test_stuck_after_zero_turns_it_off(self):
        tid = self.start_task()
        self.assertEqual(json.loads(self.bp("config", "--json").stdout)["stuck_after"], 20)
        cfg = json.loads(self.bp("config", "--stuck-after", "0", "--json").stdout)
        self.assertEqual(cfg["stuck_after"], 0)
        self.bp("_stuck", self.block(tid), "20")
        self.assertNotIn("stuck", self.show(tid))
        self.assertEqual(self.news(), [])
        self.assertEqual(self.bp("config", "--stuck-after", "-1", ok=False).returncode, 1)

    def test_stuck_after_reaches_the_panes_the_watcher_reads(self):
        tid = self.start_task()
        entry = self.tmp / "state" / "blocks" / f"{self.block(tid)}.json"
        self.assertEqual(json.loads(entry.read_text())["stuck_after"], 20)
        self.bp("config", "--stuck-after", "45")
        self.assertEqual(json.loads(entry.read_text()),
                         {"repo": str(self.repo), "role": tid, "stuck_after": 45})
        tid2 = self.bp("add", "Another", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        entry2 = self.tmp / "state" / "blocks" / f"{self.block(tid2)}.json"
        self.assertEqual(json.loads(entry2.read_text())["stuck_after"], 45)

    def test_the_doing_folder_is_there_for_the_watcher_and_a_panes_file_goes_with_it(self):
        tid = self.start_task()
        doing = self.tmp / "state" / "doing"
        self.assertTrue(doing.is_dir())
        note = doing / f"{self.block(tid)}.json"
        note.write_text('{"state": "working", "text": "Reading", "at": 1}')
        self.assertTrue(note.exists())
        self.bp("drop", tid, "--force")
        self.assertFalse(note.exists())

    def test_a_quiet_reviewer_is_news_in_its_own_words(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp("_stuck", self.block(tid), "20")  # the worker is done: not its turn
        self.assertNotIn("stuck", self.show(tid))
        self.bp("_stuck", reviewer, "25")
        self.bp("_stuck", reviewer, "26")
        self.assertEqual(self.news(), ["bullpen · BP-1's reviewer has shown no progress for 25 min: "
                                       "`bullpen go BP-1 --review` shows it"])
        t = self.show(tid)
        self.assertEqual((t["status"], t["stuck"]["who"]), ("review", "reviewer"))
        self.bp("_event", reviewer, "working", "", "")
        self.assertNotIn("stuck", self.show(tid))
        self.bp("_stuck", reviewer, "20")
        self.verdict(tid, "approve")  # the status changes: the quiet spell is over
        self.assertNotIn("stuck", self.show(tid))

    def test_verdict_needs_a_review(self):
        tid = self.start_task()
        p = self.bp("verdict", tid, "approve", "--summary", "x", ok=False)
        self.assertIn("isn't in review", p.stderr)

    def test_review_off_goes_straight_to_ready(self):
        tid = self.start_task()
        self.bp("config", "--review", "off")
        wt = self.show(tid)["worktree"]
        (Path(wt) / "f.txt").write_text("x\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "x")
        self.bp("ready", tid, cwd=wt)
        self.assertEqual(self.show(tid)["status"], "ready")
        self.assertEqual(len(self.api("session.new_layer")), 1)  # no reviewer

    def test_go_review_brings_up_the_reviewer(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.bp("go", tid, "--review")
        self.assertEqual(self.layer_states(), {"lyr1": "minimized", "lyr2": "normal"})

    def test_note_shows_until_the_status_changes(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "done", "", "")
        self.bp("note", tid, "Waits", "on the CI image.")
        self.assertEqual(self.show(tid)["note"], "Waits on the CI image.")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.bp("_event", self.block(tid), "working", "", "")
        self.assertIsNone(self.show(tid)["note"])

    def windows(self):
        """{window: [labels]} from the fake's session view."""
        R = json.loads((self.fake / "reg.json").read_text())
        out = {w: [] for w in R["windows"]}
        for b, info in R["blocks"].items():
            if (self.fake / "alive" / b).exists() and info["window"] in out:
                out[info["window"]].append(info["label"])
        return {w: sorted(labels) for w, labels in out.items()}

    def test_a_restarted_coordinator_takes_the_workers_with_it(self):
        self.bp("config", "--reviewers", "same")
        tid = self.start_task()
        self.commit_and_ready(tid)  # a reviewer capsule too
        both = sorted(["bullpen: proj", "bullpen: proj board", "bullpen: BP-1 · ctrl+esc hides",
                       "bullpen: BP-1 review · ctrl+esc hides"])
        self.assertEqual(self.windows(), {"w1": both})
        for b in ("blk1", "blk2"):  # the coordinator and board are closed
            (self.fake / "alive" / b).unlink()
        self.bp("up")  # a new coordinator, in a new tab
        self.assertEqual(self.windows(), {"w2": both})  # and the emptied tab is closed
        self.assertEqual(sorted(self.layer_states().items())[-2:],
                         [("lyr3", "minimized"), ("lyr4", "minimized")])

    def test_news_waits_while_the_person_is_typing_to_the_coordinator(self):
        tid = self.start_task()
        (self.fake / "screen").write_text("● Ready.\n────\n❯ drop BP-\n────\n  auto mode on\n")
        self.bp("_event", self.block(tid), "done", "", "")
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))  # not typed into the draft
        self.assertTrue((self.tmp / "state" / "outbox").exists())
        self.bp("_flush")  # still typing: still waiting
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))
        (self.fake / "screen").write_text("● Ready.\n────\n❯ \n────\n  auto mode on\n")
        self.bp("_flush")
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual(len(news), 1)
        self.assertIn("BP-1 ended its turn", news[0])
        self.assertFalse((self.tmp / "state" / "outbox").exists())

    def test_waiting_news_goes_out_in_order(self):
        tid = self.start_task()
        (self.fake / "screen").write_text("❯ hmm\n")
        self.bp("_event", self.block(tid), "blocked", "permission", "first")
        self.bp("_event", self.block(tid), "blocked", "permission", "second")
        (self.fake / "screen").write_text("❯ \n")
        self.bp("_event", self.block(tid), "blocked", "permission", "third")  # delivers all three
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual([n.rsplit(" ", 1)[-1] for n in news], ["first", "second", "third"])

    # How Claude Code draws its input box (`rex capture --format=vt`): a suggestion is
    # faint (SGR 2) after the ❯, text the person typed is not.
    RULE = "\x1b[0m\x1b[38;5;7m────────\r\n"
    SUGGESTION = RULE + "❯\xa0\x1b[0m\x1b[2mdrop BP-18 and BP-20\x1b[0m\r\n" + RULE
    TYPED = RULE + "❯\xa0\x1b[0mdrop BP-\x1b[7m \x1b[0m\r\n" + RULE

    def test_a_prompt_suggestion_doesnt_hold_typed_news(self):
        tid = self.start_task()
        (self.fake / "screen").write_text(self.SUGGESTION + "  \x1b[2m\x1b[38;5;7m⏵⏵ auto mode on\x1b[0m\r\n")
        self.bp("_event", self.block(tid), "done", "", "")
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual(len(news), 1)
        self.assertIn("BP-1 ended its turn", news[0])
        self.assertFalse((self.tmp / "state" / "outbox").exists())
        self.assertIn("--format=vt", [a for a in self.rex_calls() if a[0] == "capture"][-1])

    def test_typed_text_still_holds_typed_news(self):
        tid = self.start_task()
        (self.fake / "screen").write_text(self.TYPED)
        self.ask(tid, "one")
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))
        self.assertTrue((self.tmp / "state" / "outbox").exists())
        # typed text followed by a faint rest of the suggestion is still a draft
        (self.fake / "screen").write_text(self.RULE + "❯\xa0\x1b[0mdrop\x1b[2m BP-18\x1b[0m\r\n")
        self.bp("_flush")
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))
        # faint from the line before, then reset: the ❯ line is the person's
        (self.fake / "screen").write_text("\x1b[2mhint\r\n\x1b[0m❯ half\r\n")
        self.bp("_flush")
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))
        (self.fake / "screen").write_text("\x1b[2mhint\r\n❯ still faint\x1b[22m\r\n")
        self.bp("_flush")
        self.assertEqual(len([l for b, l in self.typed() if b == "blk1"]), 1)
