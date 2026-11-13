"""News: the log, news --follow, and telling coordinators their guide changed."""

import hashlib
import json
import shutil
import subprocess
from pathlib import Path

from support import setUpModule, tearDownModule, BULLPEN, BullpenTest  # a temp folder for each module


class NewsTest(BullpenTest):
    def restart_rex(self, new_session=False):
        """Rex restarts: every placed pane comes back with a new ID, detached ones don't.
        With new_session, the session gets a new ID too (its label stays)."""
        reg = self.fake / "reg.json"
        R = json.loads(reg.read_text())
        if new_session:
            R["session"] = "session:2"
            self.env["REX_SESSION"] = "session:2"
        for b, info in list(R["blocks"].items()):
            if (self.fake / "alive" / b).exists():
                (self.fake / "alive" / b).unlink()
                if info["layer"] != "detached":
                    n = f"blk{len(R['blocks']) + 1}"
                    R["blocks"][n] = dict(info)
                    (self.fake / "alive" / n).write_text("")
        reg.write_text(json.dumps(R))

    def stop(self, p):
        p.terminate()
        p.wait()

    def test_follow_prints_each_new_line_as_it_is_logged(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        p, lines, quiet = self.follow()
        self.assertTrue(quiet())  # nothing logged yet (and no news.log): nothing printed
        self.ask(tid, "first")
        self.assertEqual(lines(1), ["bullpen · BP-1 is at a prompt (question): first"])
        self.ask(tid, "second")
        self.ask(tid, "third")
        self.assertEqual([l.rsplit(" ", 1)[-1] for l in lines(2)], ["second", "third"])
        self.assertIsNone(p.poll())  # still running

    def test_a_restarted_follower_prints_what_it_missed_once(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        p, lines, _ = self.follow()
        self.ask(tid, "one")
        self.assertEqual(len(lines(1)), 1)
        self.stop(p)
        self.ask(tid, "two")  # logged while no follower runs
        self.ask(tid, "three")
        p, lines, quiet = self.follow()
        self.assertEqual([l.rsplit(" ", 1)[-1] for l in lines(2)], ["two", "three"])
        self.ask(tid, "four")
        self.assertEqual([l.rsplit(" ", 1)[-1] for l in lines(1)], ["four"])
        self.stop(p)
        p, lines, quiet = self.follow()
        self.assertTrue(quiet())  # nothing new: nothing again

    def test_a_second_follower_waits_and_takes_over(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        first, lines1, _ = self.follow()
        self.ask(tid, "one")
        self.assertEqual(len(lines1(1)), 1)
        second, lines2, quiet2 = self.follow()
        self.ask(tid, "two")
        self.assertEqual([l.rsplit(" ", 1)[-1] for l in lines1(1)], ["two"])
        self.assertTrue(quiet2())  # not printed twice
        p = self.bp_in("blk1", "news", ok=False)  # a one-off read doesn't wait
        self.assertIn("followed already", p.stderr)
        self.stop(first)
        self.ask(tid, "three")
        self.assertEqual([l.rsplit(" ", 1)[-1] for l in lines2(1)], ["three"])

    def test_news_prints_the_lines_since_last_time_and_exits(self):
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")  # no news.log yet
        tid = self.start_task()
        self.ask(tid, "one")
        self.ask(tid, "two")
        out = self.bp_in("blk1", "news").stdout.splitlines()
        self.assertEqual(out, ["bullpen · BP-1 is at a prompt (question): one",
                               "bullpen · BP-1 is at a prompt (question): two"])
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")
        self.ask(tid, "three")
        self.assertEqual(self.bp_in("blk1", "news").stdout, "bullpen · BP-1 is at a prompt (question): three\n")

    def test_news_never_replays_what_was_logged_before_it_first_ran(self):
        tid = self.start_task()
        self.ask(tid, "old")  # typed, in the default mode
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")
        self.ask(tid, "older still")
        self.bp_in("blk1", "news")
        self.ask(tid, "typed too")
        self.bp("config", "--news", "monitor")  # switching starts from here
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")
        self.ask(tid, "new")
        self.assertEqual(self.bp_in("blk1", "news").stdout, "bullpen · BP-1 is at a prompt (question): new\n")

    def test_monitor_mode_types_nothing_and_keeps_no_outbox(self):
        tid = self.start_task()
        p = self.bp("config", "--news", "monitor", "--json")
        self.assertEqual(json.loads(p.stdout)["news"], "monitor")
        self.assertIn("news --follow", p.stderr)
        self.ask(tid, "Which one?")
        self.bp("_event", self.block(tid), "working", "", "")
        self.bp("_event", self.block(tid), "done", "", "")
        self.bp("_flush")
        typed = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual(len(typed), 1)  # only the line that said news moved to the monitor
        self.assertIn("news now comes by monitor", typed[0])
        state = self.tmp / "state"
        self.assertFalse((state / "outbox").exists())
        self.assertEqual(list(state.rglob("outbox*.txt")), [])
        log = next(state.rglob("news.log")).read_text().splitlines()
        self.assertEqual(len(log), 2)
        self.assertIn("BP-1 needs-you: bullpen · BP-1 is at a prompt (question): Which one?", log[0])

    def test_type_is_the_default_and_can_be_set_back(self):
        tid = self.start_task()
        self.assertEqual(json.loads(self.bp("config", "--json").stdout)["news"], "type")
        self.bp("config", "--news", "monitor")
        self.bp("config", "--news", "type")
        self.ask(tid, "Which one?")
        self.assertEqual([l for b, l in self.typed() if b == "blk1"][1:],
                         ["bullpen · BP-1 is at a prompt (question): Which one?"])

    def test_switching_to_type_ends_a_running_follower_with_one_line(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        p, lines, quiet = self.follow()
        self.ask(tid, "one")
        self.assertEqual(len(lines(1)), 1)
        self.bp("config", "--news", "type")
        self.assertTrue(lines(1)[0].startswith("bullpen · news is typed into your pane now"))
        self.assertEqual(p.wait(timeout=10), 0)
        self.ask(tid, "two")  # typed, and not printed by a follower started again
        self.assertIn("two", self.typed()[-1][1])
        p, lines, quiet = self.follow()
        self.assertEqual(len(lines(1)), 1)  # the same one line, then it's gone
        self.assertEqual(p.wait(timeout=10), 0)
        self.assertEqual(p.stdout.read(), b"")
        self.bp("config", "--news", "monitor")  # switching again starts after "two": it was typed
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")

    def test_switching_to_monitor_tells_a_running_coordinator_by_one_last_typed_line(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        told = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual(len(told), 1)
        self.assertTrue(told[0].startswith("bullpen · news now comes by monitor"))
        self.assertIn(f"{BULLPEN} news --follow", told[0])
        self.bp("config", "--news", "monitor")  # no switch: not said again
        self.ask(tid, "one")
        self.assertEqual(len([l for b, l in self.typed() if b == "blk1"]), 1)

    def test_the_switch_line_waits_for_an_empty_input_like_other_typed_news(self):
        self.start_task()
        (self.fake / "screen").write_text("❯ half a message\n")
        self.bp("config", "--news", "monitor")
        self.assertFalse(any(b == "blk1" for b, _ in self.typed()))
        (self.fake / "screen").write_text("❯ \n")
        self.bp("_flush")
        self.assertEqual(len([l for b, l in self.typed() if b == "blk1"]), 1)

    def test_switching_to_monitor_types_nothing_from_the_coordinators_pane_or_without_one(self):
        self.bp("config", "--news", "monitor")  # no coordinator yet
        self.bp("config", "--news", "type")
        self.bp("up", "--here")  # this pane ("me") is the coordinator
        p = self.bp("config", "--news", "monitor")
        self.assertIn("news --follow", p.stderr)
        self.assertEqual(self.typed(), [])
        self.assertEqual(list((self.tmp / "state").rglob("outbox*.txt")), [])
        self.assertIn("News comes by monitor: start it first", self.bp("up", "--here").stdout)

    def test_up_says_one_plain_line_and_verbose_names_the_panes(self):
        self.assertEqual(self.bp("up").stdout,
                         "Coordinator, board and watcher started. Tell the coordinator what you want.\n")
        self.assertEqual(self.bp("up").stdout, "Coordinator already running.\n")
        (self.fake / "alive" / "blk2").unlink()  # the board, quit
        self.assertEqual(self.bp("up").stdout, "Coordinator already running. Board started.\n")
        self.assertEqual(self.bp("up", "--verbose").stdout.splitlines(),
                         ["coordinator already running: c1 (blk1)",
                          "news: typed into the coordinator's pane (`bullpen config --news monitor`: "
                          "by Claude Code's Monitor tool instead)."])

    def test_errors_say_a_status_in_the_boards_words(self):
        self.bp("up")
        for n in range(1, 5):
            self.bp("add", f"Task {n}", "--brief", "b")
        self.bp("start", "BP-1")

        def set_status(tid, status):
            f = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
            f.write_text(json.dumps({**json.loads(f.read_text()), "status": status}))
        set_status("BP-2", "stopped")
        set_status("BP-3", "ready")
        self.bp("after", "BP-4", "BP-3")
        said = {args: self.bp(*args.split(), ok=False).stderr.strip() for args in (
            "accept BP-1", "go BP-4", "diff BP-4", "ok BP-1", "ok BP-2", "close BP-3", "start BP-4", "tour BP-2")}
        self.assertEqual(said, {
            "accept BP-1": "bullpen: BP-1 is working, not ready to accept or finished",
            "go BP-4": "bullpen: BP-4 has no pane: it's queued (todo)",
            "diff BP-4": "bullpen: BP-4 is queued (todo), with no worktree, so there's nothing to diff",
            "ok BP-1": "bullpen: BP-1 is working, not waiting on its coordinator",
            "ok BP-2": "bullpen: BP-2 needs you (stopped), not waiting on its coordinator",
            "close BP-3": "bullpen: BP-3 is ready to accept, not finished: `accept BP-3` merges it",
            "start BP-4": "bullpen: BP-4 waits on BP-3 (ready to accept): it starts by itself once they're "
                          "accepted; `start BP-4 --force` starts it now",
            "tour BP-2": "bullpen: BP-2 needs you (stopped): a tour is only for a task that is ready or in "
                         "review, whose code isn't changing"})

    def test_up_says_how_news_arrives(self):
        out = self.bp("up", "--verbose").stdout.splitlines()
        self.assertEqual(out[:3], ["coordinator started: blk1", "board started: blk2", "watcher started: blk3"])
        self.assertTrue(out[-1].startswith("news: typed into the coordinator's pane"))
        self.bp("config", "--news", "monitor")
        for args in (["up"], ["up", "--here"]):
            last = self.bp(*args).stdout.splitlines()[-1]
            self.assertTrue(last.startswith("news: by monitor"))
            self.assertIn(f"{BULLPEN} news --follow", last)

    def test_the_guide_names_the_follow_command_in_full(self):
        guide = self.bp("guide").stdout
        self.assertIn(f"{BULLPEN} news --follow`", guide)
        self.assertNotIn("{{", guide)
        self.bp("config", "--news", "monitor")
        self.bp("up")
        run = next(a for a in self.rex_calls() if a[0] == "run")
        prompt = Path(run[run.index("--append-system-prompt-file") + 1]).read_text()
        self.assertEqual(prompt.strip(), guide.strip())
        self.assertIn("News comes by monitor", run[-1])

    def test_hide_puts_a_worker_away_and_goes_to_the_board(self):
        tid = self.start_task()
        self.bp("go", tid)
        self.assertEqual(self.layer_states(), {"lyr1": "normal"})
        self.bp("hide", "--from", self.block(tid))
        self.assertEqual(self.layer_states(), {"lyr1": "minimized"})
        self.assertEqual(self.focused()[-1], "blk2")  # the board

    def test_hide_lets_the_key_through_elsewhere(self):
        self.start_task()
        for pane in ("blk1", "blk2", "someone-elses"):  # coordinator, board, not ours
            p = self.bp("hide", "--from", pane, ok=False)
            self.assertEqual(p.returncode, 3)

    def test_go_focuses_coordinator_and_board(self):
        self.start_task()
        self.bp("go")
        self.bp("go", "--board")
        self.assertEqual(self.focused(), ["blk1", "blk2"])

    def test_workers_start_as_minimized_floating_panes(self):
        tid = self.start_task()
        self.assertEqual([a[0] for a in self.rex_calls()].count("run"), 1)  # only the coordinator
        layer = self.api("session.new_layer")[0]
        self.assertEqual(layer["window_id"], "w1")  # the coordinator's window
        self.assertEqual(self.layer_states(), {"lyr1": "minimized"})

    def test_go_brings_one_worker_up_at_a_time(self):
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        self.bp("go", "BP-1")
        self.assertEqual(self.layer_states(), {"lyr1": "normal", "lyr2": "minimized"})
        self.assertTrue(self.api("session.set_layer_state")[-1].get("focus"))
        self.bp("go", "BP-2")
        self.assertEqual(self.layer_states(), {"lyr1": "minimized", "lyr2": "normal"})
        self.bp("go")  # back to the coordinator: workers go back to capsules
        self.assertEqual(self.layer_states(), {"lyr1": "minimized", "lyr2": "minimized"})
        self.assertEqual(self.focused(), ["blk1"])

    def test_workers_tabs_setting(self):
        self.bp("up")
        self.bp("config", "--workers", "tabs")
        self.bp("add", "T", "--brief", "x")
        self.bp("start", "BP-1")
        run = [a for a in self.rex_calls() if a[0] == "run"][-1]
        self.assertIn("bullpen: BP-1", run)
        self.assertEqual(self.api("session.new_layer"), [])

    def test_up_finds_its_panes_again_after_a_rex_restart(self):
        a = self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        (self.fake / "alive" / self.block("BP-2")).unlink()  # closed while Rex was down
        self.restart_rex()
        runs = [x for x in self.rex_calls() if x[0] == "run"]
        self.bp("up")
        self.assertEqual([x for x in self.rex_calls() if x[0] == "run"], runs)  # no second coordinator
        cfg = json.loads(self.bp("config", "--json").stdout)
        self.assertEqual(cfg["coordinators"][0]["block"], "blk6")
        self.assertEqual(cfg["coordinators"][0]["board"]["block"], "blk7")
        self.assertEqual(self.block(a), "blk8")
        self.assertEqual(self.show("BP-2")["status"], "stopped")
        self.assertEqual(len(self.api("session.new_block")), 2)  # the watcher, again
        self.bp("_event", "blk8", "blocked", "permission", "ok?")  # events reach the task
        self.assertEqual(self.show(a)["status"], "needs")

    def test_a_pane_from_before_the_hide_hint_is_found_again(self):
        tid = self.start_task()
        reg = self.fake / "reg.json"
        R = json.loads(reg.read_text())
        R["blocks"][self.block(tid)]["label"] = "bullpen: BP-1"  # started by an older bullpen
        reg.write_text(json.dumps(R))
        self.restart_rex()
        self.bp("go", tid)
        self.assertEqual(self.block(tid), "blk7")

    def test_worker_panes_in_tabs_have_no_hide_hint(self):
        self.bp("up")
        self.bp("config", "--workers", "tabs")
        self.bp("add", "Fix", "--brief", "x")
        self.bp("start", "BP-1")
        run = [a for a in self.rex_calls() if a[0] == "run"][-1]
        self.assertEqual(run[run.index("--label") + 1], "bullpen: BP-1")  # ctrl+esc only hides floating ones

    def test_up_after_a_restart_that_renamed_the_session(self):
        tid = self.start_task()
        self.restart_rex(new_session=True)
        self.bp("up")
        self.assertEqual([x[0] for x in self.rex_calls()].count("run"), 1)  # no second coordinator
        cfg = json.loads(self.bp("config", "--json").stdout)
        c = cfg["coordinators"][0]
        self.assertEqual((c["block"], c["session"]), ("blk5", "session:2"))
        self.assertEqual(self.show(tid)["block"], "blk7")
        self.assertEqual(self.show(tid)["session"], "session:2")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_go_after_a_restart_that_renamed_the_session(self):
        tid = self.start_task()
        self.restart_rex(new_session=True)
        env = {k: v for k, v in self.env.items() if k != "REX_SESSION"}  # as a key binding runs it
        p = subprocess.run([str(BULLPEN), "go", tid, "--session", "session:2", "--from", "x"],
                           cwd=self.tmp, env=env, text=True, capture_output=True)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(self.block(tid), "blk7")

    def test_go_finds_a_pane_again_after_a_rex_restart(self):
        tid = self.start_task()
        self.restart_rex()
        self.bp("go", tid)
        self.assertEqual(self.block(tid), "blk7")

    def test_news_is_one_line(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "blocked", "question", "Which one?\nA or B")
        line = self.typed()[-1][1]
        self.assertEqual(line, "bullpen · BP-1 is at a prompt (question): Which one? A or B")

    # ---------- telling coordinators that their guide changed ----------

    def guide(self):
        return self.bp("guide").stdout[:-1]  # print's own newline

    def give_old_guide(self, block, text, keep=True):
        """The coordinator in pane block was given the guide text (None: from before
        hashes); keep=False: its text wasn't kept."""
        cfg_path = next((self.tmp / "state" / "repos").glob("*/config.json"))
        cfg = json.loads(cfg_path.read_text())
        c = next(c for c in cfg["coordinators"] if c["block"] == block)
        c.pop("guide", None)
        if text is not None:
            c["guide"] = hashlib.sha256(text.encode()).hexdigest()
            if keep:
                (self.tmp / "state" / "guides" / f"{c['guide']}.md").write_text(text)
        cfg_path.write_text(json.dumps(cfg))

    def guide_hashes(self):
        return {c["block"]: c.get("guide") for c in self.coordinators()}

    def guide_news(self, block="blk1"):
        return [l for b, l in self.typed() if b == block and "coordinator guide" in l]

    def test_a_short_guide_change_is_told_once_with_its_diff(self):
        self.bp("up")
        now = self.guide()
        # two lines of the guide now in place of one: two changed lines in one section
        old = now.replace("- When there's nothing to add, reply with a single short line or nothing at all.\n"
                          "  Several news lines at once get one reply.", "- Reply to each news line.")
        self.assertNotEqual(old, now)
        self.give_old_guide("blk1", old)
        self.bp("up")
        [msg] = self.guide_news()
        lines = msg.split("\n")
        self.assertEqual(lines[0], "bullpen · Your coordinator guide changed (section: News). "
                                   "Read the change below and follow it from now on:")
        self.assertEqual(lines[1:3], ["--- guide (yours)", "+++ guide (now)"])
        self.assertIn("-- Reply to each news line.", lines)
        self.assertIn("+  Several news lines at once get one reply.", lines)
        self.assertEqual(self.guide_hashes()["blk1"], hashlib.sha256(now.encode()).hexdigest())
        self.bp("up")
        self.assertEqual(len(self.guide_news()), 1)  # once

    def test_an_unchanged_guide_is_no_news(self):
        tid = self.ready_with_a_commit()
        self.bp("up")
        self.bp("accept", tid)
        self.assertEqual(self.guide_news(), [])
        self.assertNotIn(" guide guide: ", self.news_log())

    def test_a_long_guide_change_names_its_sections_and_points_at_the_guide(self):
        self.bp("up")
        now = self.guide()
        old = now.replace("## Review\n", "## Review\n" + "An old rule.\n" * 50)
        old = old.replace("- When there's nothing to add, reply with a single short line or nothing at all.\n"
                          "  Several news lines at once get one reply.", "- Reply to each news line.")
        old = old.replace("When a worker runs `ready`, its PR opens (or, when the repo has no remote, its\n"
                          "local branch is marked ready)", "When a worker is ready")
        self.assertEqual(old.count("When a worker is ready"), 1)
        self.give_old_guide("blk1", old)
        self.bp("up")
        [msg] = self.guide_news()
        self.assertNotIn("\n", msg)
        self.assertRegex(msg, r"^bullpen · Your coordinator guide changed \(sections: Review, News\), "
                              r"\d+ lines of diff: run `bullpen guide` to read it all, and follow it from now on\.$")

    def test_a_coordinator_from_before_guide_hashes_is_told_once_without_a_diff(self):
        self.bp("up")
        self.give_old_guide("blk1", None)
        self.bp("up")
        self.bp("up")
        self.assertEqual(self.guide_news(), [
            "bullpen · Your coordinator guide may have changed since you were given it: run "
            "`bullpen guide` to read it all, and follow it from now on."])
        self.give_old_guide("blk1", "a guide whose text wasn't kept", keep=False)
        self.bp("up")
        self.assertEqual(len(self.guide_news()), 2)
        self.assertIn("may have changed", self.guide_news()[1])

    def test_a_guide_change_is_told_to_each_running_coordinator_alone(self):
        tid = self.ready_with_a_commit()
        other = self.new_pane()
        self.bp_in(other, "up", "--here")
        third = self.new_pane()
        self.bp_in(third, "up", "--here")
        now = self.guide()
        self.assertEqual(set(self.guide_hashes().values()), {hashlib.sha256(now.encode()).hexdigest()})
        old = now.replace("## News\n", "## News\nOld words.\n")
        self.give_old_guide("blk1", old)
        self.give_old_guide(other, old)
        self.give_old_guide(third, old)
        (self.fake / "alive" / third).unlink()  # its agent is closed: nobody to tell
        before = self.typed()
        self.bp("accept", tid)
        told = [(b, l.split("\n")[0]) for b, l in self.typed()[len(before):] if "coordinator guide" in l]
        line = "bullpen · Your coordinator guide changed (section: News). Read the change below and follow it from now on:"
        self.assertEqual(sorted(told), sorted([("blk1", line), (other, line)]))
        self.assertEqual(self.guide_hashes()[third], hashlib.sha256(old.encode()).hexdigest())
        self.bp("up")  # drops the closed one; nothing more to tell
        self.assertEqual(len([l for b, l in self.typed() if "coordinator guide" in l]), 2)

    def test_a_guide_diff_comes_after_its_line_by_monitor(self):
        self.bp("up")
        self.bp("config", "--news", "monitor")
        self.bp_in("blk1", "news")  # its cursor starts here
        now = self.guide()
        self.give_old_guide("blk1", now.replace("## News\n", "## News\nOld words.\n"))
        self.bp("up")
        out = self.bp_in("blk1", "news").stdout.splitlines()
        self.assertEqual(out[0], "bullpen · Your coordinator guide changed (section: News). "
                                 "Read the change below and follow it from now on:")
        self.assertEqual(out[1:3], ["--- guide (yours)", "+++ guide (now)"])
        self.assertIn("-Old words.", out)
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")
        self.assertEqual(self.guide_news(), [])  # nothing typed

    def test_go_refuses_a_task_without_a_pane(self):
        self.bp("up")
        tid = self.bp("add", "Later", "--brief", "x").stdout.strip()
        p = self.bp("go", tid, ok=False)
        self.assertIn("no pane", p.stderr)

    def test_go_next_cycles_through_tasks_needing_you(self):
        a = self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        b = "BP-2"
        self.bp("start", b)
        self.start_task("Third")  # working: never a target
        p = self.bp("go", "--next", "--from", "elsewhere")  # nothing needs you: the board
        self.assertIn("nothing needs you", p.stderr)
        self.assertEqual(self.focused()[-1], "blk2")
        self.bp("_event", self.block(b), "blocked", "permission", "Allow?")
        self.bp("_event", self.block(a), "blocked", "question", "Which?")
        nxt = lambda frm: json.loads(self.bp("go", "--next", "--json", "--from", frm).stdout)["block"]
        first = nxt("elsewhere")
        self.assertEqual(first, self.block(b))  # waiting longest comes first
        self.assertEqual(nxt(first), self.block(a))
        self.assertEqual(nxt(self.block(a)), self.block(b))  # wraps around

    def test_go_next_falls_through_to_a_ready_task_on_the_board(self):
        self.start_task("Working")
        for title in ("Finished", "Ready", "Ready too"):
            self.bp("add", title, "--brief", "x")
        d = next((self.tmp / "state").rglob("config.json")).parent
        for tid, status, since in (("BP-2", "finished", "2026-01-01T00:00:00+00:00"),
                                   ("BP-3", "ready", "2026-01-03T00:00:00+00:00"),
                                   ("BP-4", "ready", "2026-01-02T00:00:00+00:00")):
            f = d / "tasks" / f"{tid}.json"
            f.write_text(json.dumps({**json.loads(f.read_text()), "status": status, "since": since}))
        sel = d / "select" / "c1"
        # nothing needs you: the board, on the ready task that has waited longest
        p = self.bp("go", "--next", "--from", "elsewhere")
        self.assertIn("nothing needs you; BP-4 is ready, on the board", p.stderr)
        self.assertEqual(self.focused()[-1], "blk2")
        self.assertEqual(sel.read_text(), "BP-4\n")
        self.assertEqual(json.loads(self.bp("go", "--next", "--json", "--from", "x").stdout)["block"], "blk2")
        # a finished one once nothing is ready
        for tid in ("BP-3", "BP-4"):
            self.bp("drop", tid)
        p = self.bp("go", "--next", "--from", "elsewhere")
        self.assertIn("BP-2 is finished, on the board", p.stderr)
        self.assertEqual(sel.read_text(), "BP-2\n")
        # a board that was quit starts again, on the task
        (self.fake / "alive" / "blk2").unlink()
        p = self.bp("go", "--next", "--from", "elsewhere")
        self.assertIn("BP-2 is finished, on the board", p.stderr)
        board = self.coordinators()[0]["board"]["block"]
        self.assertNotEqual(board, "blk2")
        self.assertEqual(self.focused()[-1], board)
        self.assertEqual(sel.read_text(), "BP-2\n")
        # what needs you still comes first
        self.bp("_event", self.block("BP-1"), "blocked", "question", "Which?")
        self.assertEqual(json.loads(self.bp("go", "--next", "--json").stdout)["block"], self.block("BP-1"))

    def test_ls_all_repos_lists_each_repo_with_something_open(self):
        self.bp("add", "Here", "--brief", "x")
        shop = self.other_repo("shop")
        self.bp("add", "Sell it", "--brief", "x", cwd=shop)
        self.bp("add", "Ship it", "--brief", "x", cwd=shop)
        quiet = self.other_repo("quiet")
        self.bp("add", "Done already", "--brief", "x", cwd=quiet)
        self.bp("drop", "BP-1", cwd=quiet)  # nothing open: left out
        gone = self.other_repo("gone")
        self.bp("add", "Lost", "--brief", "x", cwd=gone)
        shutil.rmtree(gone)  # its checkout is gone: left out
        out = self.bp("ls", "--all-repos", cwd=self.tmp).stdout  # works outside a repository
        self.assertEqual([l.split()[:2] for l in out.splitlines() if l], [
            ["proj", f"({self.repo})"], ["◌", "BP-1"],
            ["shop", f"({shop})"], ["◌", "BP-1"], ["◌", "BP-2"]])
        self.assertIn("  ◌ BP-2  Ship it  now  Not started. s starts it.", out)
        self.assertNotIn("Done already", out)
        with_closed = self.bp("ls", "--all-repos", "--all").stdout
        self.assertIn("Done already", with_closed)
        self.assertNotIn("Lost", with_closed)
        js = json.loads(self.bp("ls", "--all-repos", "--json").stdout)
        self.assertEqual([(g["repo"], [t["id"] for t in g["tasks"]]) for g in js],
                         [(str(self.repo), ["BP-1"]), (str(shop), ["BP-1", "BP-2"])])
        self.assertEqual(self.bp("ls").stdout.count("BP-"), 1)  # plain ls: this repo's alone
        self.bp("drop", "BP-1")
        self.bp("drop", "BP-1", cwd=shop)
        self.bp("drop", "BP-2", cwd=shop)
        self.assertEqual(self.bp("ls", "--all-repos").stdout, "no open tasks in any repository "
                         "(--all shows finished ones)\n")

    def test_go_next_all_repos_goes_across_repos(self):
        here = self.start_task("Here")
        shop = self.other_repo("shop")
        self.bp("up", cwd=shop)
        self.bp("add", "Sell it", "--brief", "x", cwd=shop)
        self.bp("start", "BP-1", cwd=shop)
        theirs = json.loads(self.bp("show", "BP-1", "--json", cwd=shop).stdout)["block"]
        p = self.bp("go", "--next", "--all-repos", "--from", "elsewhere")
        self.assertIn("nothing needs you in any repository; showing the board", p.stderr)
        self.bp("_event", self.block(here), "blocked", "question", "Which?")
        self.bp("_event", theirs, "blocked", "question", "Which?", cwd=shop)
        go = lambda frm, *more: json.loads(self.bp("go", "--next", "--json", "--from", frm, *more).stdout)
        first = go("elsewhere", "--all-repos")
        self.assertEqual((first["block"], first["name"]), (self.block(here), here))  # waiting longest
        second = go(first["block"], "--all-repos")
        self.assertEqual((second["block"], second["name"]), (theirs, "shop BP-1"))
        self.assertEqual(go(theirs, "--all-repos")["block"], self.block(here))  # back from the other repo
        self.assertEqual(go(self.block(here))["block"], self.block(here))  # without it: this repo only
        self.bp("go", "--next", "--all-repos", "--from", self.block(here))
        layer = json.loads((self.fake / "reg.json").read_text())["blocks"][theirs]["layer"]
        self.assertEqual(self.api("session.set_layer_state")[-1], {"layer_id": layer, "state": "normal", "focus": True})
        self.assertIn("goes with --next", self.bp("go", "--all-repos", ok=False).stderr)
