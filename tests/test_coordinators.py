"""Several coordinators, in one repo and across repos."""

import json

from support import setUpModule, tearDownModule, BullpenTest  # a temp folder for each module


class CoordinatorsTest(BullpenTest):
    def test_a_second_up_here_keeps_the_first_coordinator(self):
        other = self.two_coordinators()
        cs = self.coordinators()
        self.assertEqual([(c["id"], c["block"]) for c in cs], [("c1", "blk1"), ("c2", other)])
        splits = [a for a in self.rex_calls() if a[0] == "split"]
        self.assertEqual([a[a.index("-b") + 1] for a in splits], ["blk1", other])  # a board each
        self.assertEqual([a[-1] for a in splits], ["c1", "c2"])
        self.assertTrue(cs[0]["board"]["block"] and cs[1]["board"]["block"])
        out = self.bp_in(other, "up", "--here").stdout  # again: nothing changes
        self.assertIn("coordinator c2: this pane", out)
        self.assertEqual(len(self.coordinators()), 2)
        self.assertEqual(len([a for a in self.rex_calls() if a[0] == "split"]), 2)
        out = self.bp("up").stdout  # from elsewhere: as before, nothing new
        self.assertEqual(out, "Coordinators c1 and c2 already running.\n")
        self.assertIn("coordinators already running: c1 (blk1), c2 (", self.bp("up", "--verbose").stdout)
        self.assertEqual([a[0] for a in self.rex_calls()].count("run"), 1)

    def test_say_types_into_the_only_coordinator_from_another_repo(self):
        self.bp("up")
        self.bp("config", "--news", "monitor")  # what you say is typed all the same
        shop = self.other_repo("shop")
        p = self.bp("say", "--repo", str(self.repo), "Look", "at", "BP-2 first", cwd=shop)
        self.assertEqual(p.stdout, "typed to c1 in proj\n")
        self.assertEqual(self.typed()[-1], ("blk1", "Look at BP-2 first"))
        self.assertIn(["send-key", "-s", "session:1", "-b", "blk1", "enter"], self.rex_calls())
        self.bp("say", "From", "here")  # in the repo: its coordinator
        self.assertEqual(self.typed()[-1], ("blk1", "From here"))
        p = self.bp_in("blk1", "say", "hi", ok=False)
        self.assertIn("this is c1's own pane", p.stderr)
        p = self.bp("say", "--repo", str(shop), "hi", ok=False)
        self.assertIn(f"bullpen has no tasks for {shop}", p.stderr)

    def test_say_waits_for_an_empty_input(self):
        self.bp("up")
        (self.fake / "screen").write_text("❯ half a message\n")
        p = self.bp("say", "Two\nlines")
        self.assertIn("half-typed in c1 in proj's input: this is typed once it's empty", p.stdout)
        self.assertEqual(self.typed(), [])
        (self.fake / "screen").write_text("❯ \n")
        self.bp("_flush")
        self.assertEqual(self.typed(), [("blk1", "Two\nlines")])

    def test_say_with_several_coordinators_wants_one_named(self):
        other = self.two_coordinators()
        p = self.bp("say", "hi", ok=False)
        self.assertIn("proj has 2 coordinators (c1, c2): say which with --coordinator", p.stderr)
        self.bp("say", "--coordinator", "c2", "hi")
        self.assertEqual(self.typed()[-1], (other, "hi"))
        p = self.bp("say", "--coordinator", "c7", "hi", ok=False)
        self.assertIn("proj has no coordinator c7; it has c1, c2", p.stderr)

    def test_say_says_when_the_coordinator_is_gone_or_refuses_typing(self):
        self.assertIn("run `bullpen up`", self.bp("say", "hi", ok=False).stderr)  # no bullpen here yet
        self.bp("add", "Here", "--brief", "x")
        p = self.bp("say", "hi", ok=False)
        self.assertIn("proj has no coordinator: `bullpen up` there starts one", p.stderr)
        self.bp("up")
        (self.fake / "send_fails_blk1").write_text("")
        p = self.bp("say", "hi", ok=False)
        self.assertIn("couldn't type into c1 in proj's pane: rex: couldn't send to the block", p.stderr)
        (self.fake / "alive" / "blk1").unlink()
        p = self.bp("say", "hi", ok=False)
        self.assertIn("c1's pane in proj is gone: `bullpen up` there starts a coordinator again", p.stderr)

    def test_board_others_is_a_setting_per_board(self):
        other = self.two_coordinators()
        self.assertNotIn("board_others", self.coordinators()[0])  # off by default
        self.bp("config", "--board-others", "on", "--coordinator", "c1")  # as the board runs it
        self.assertEqual([c.get("board_others") for c in self.coordinators()], [True, None])
        self.bp_in(other, "config", "--board-others", "on")  # in c2's pane: c2's
        self.bp("config", "--board-others", "off", "--coordinator", "c1")
        self.assertEqual([c.get("board_others") for c in self.coordinators()], [False, True])
        board = self.coordinators()[1]["board"]["block"]
        self.bp_in(board, "config", "--board-others", "off")  # on c2's board: c2's
        self.assertEqual(self.coordinators()[1]["board_others"], False)
        p = self.bp("config", "--board-others", "on", ok=False)  # whose?
        self.assertIn("per board", p.stderr)
        p = self.bp("config", "--board-others", "on", "--coordinator", "c7", ok=False)
        self.assertIn("no coordinator c7", p.stderr)

    def test_go_next_skips_tasks_the_board_hides(self):
        other = self.two_coordinators()
        mine = self.bp("add", "Mine", "--brief", "x").stdout.strip()  # c1's
        theirs = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        for tid in (theirs, mine):
            self.bp("start", tid)
            self.bp("_event", self.block(tid), "blocked", "question", "Which?")
        nxt = lambda frm: json.loads(self.bp("go", "--next", "--json", "--from", frm).stdout)["block"]
        c1_board = self.coordinators()[0]["board"]["block"]
        self.assertEqual(nxt(c1_board), self.block(mine))
        self.assertEqual(nxt(self.block(mine)), self.block(mine))  # c2's isn't on c1's board
        self.assertEqual(nxt(other), self.block(theirs))  # nor c1's on c2's
        self.assertEqual(nxt("elsewhere"), self.block(theirs))  # not a coordinator's pane: every task
        self.bp("config", "--board-others", "on", "--coordinator", "c1")
        self.assertEqual(nxt(self.block(mine)), self.block(theirs))
        # c2's board is unchanged
        self.assertEqual(nxt(other), self.block(theirs))
        self.assertEqual(nxt(self.block(theirs)), self.block(theirs))

    def test_a_task_belongs_to_the_coordinator_that_added_it(self):
        other = self.two_coordinators()
        mine = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        primary = self.bp("add", "Elsewhere", "--brief", "x").stdout.strip()  # not a coordinator's pane
        self.assertEqual(self.show(mine)["coordinator"], "c2")
        self.assertEqual(self.show(primary)["coordinator"], "c1")
        self.assertIn("coord: c2", self.bp("show", mine).stdout)

    def test_news_goes_to_the_tasks_own_coordinator(self):
        other = self.two_coordinators()
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        b = self.bp("add", "Primary's", "--brief", "x").stdout.strip()
        self.bp("add", "Waits", "--brief", "x", "--after", a)
        self.bp("start", a)
        self.bp("start", b)
        self.ask(a, "for c2?")
        self.ask(b, "for c1?")
        self.assertEqual(self.typed(), [(other, "bullpen · BP-1 is at a prompt (question): for c2?"),
                                        ("blk1", "bullpen · BP-2 is at a prompt (question): for c1?")])
        self.bp("drop", a, "--force")
        typed = self.typed()[2:]
        self.assertEqual(typed[0], (other, "bullpen · BP-1 dropped"))  # its own coordinator alone
        # news about more than one task goes to every coordinator
        waits = [(blk, l) for blk, l in typed if "waits on BP-1, which was dropped" in l]
        self.assertEqual(sorted(blk for blk, _ in waits), sorted(["blk1", other]))

    def test_news_from_a_coordinators_pane_goes_to_the_owner_only(self):
        other = self.two_coordinators()
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", a)
        self.bp_in(other, "drop", a, "--force")  # c2 did it: no news to itself
        self.assertEqual(self.typed(), [])
        b = self.bp_in(other, "add", "Theirs too", "--brief", "x").stdout.strip()
        self.bp("start", b)
        self.bp_in("blk1", "drop", b, "--force")  # c1 dropped c2's task: c2 hears of it
        self.assertEqual(self.typed(), [(other, "bullpen · BP-2 dropped")])

    def test_each_coordinator_reads_its_own_news(self):
        other = self.two_coordinators()
        self.bp("config", "--news", "monitor")
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        b = self.bp("add", "Primary's", "--brief", "x").stdout.strip()
        self.bp("start", a)
        self.bp("start", b)
        self.ask(a, "one")
        self.ask(b, "two")
        self.ask(a, "three")
        self.assertEqual(self.bp_in(other, "news").stdout.splitlines(),
                         ["bullpen · BP-1 is at a prompt (question): one", "bullpen · BP-1 is at a prompt (question): three"])
        self.assertEqual(self.bp_in("blk1", "news").stdout, "bullpen · BP-2 is at a prompt (question): two\n")
        self.assertEqual(self.bp_in(other, "news").stdout, "")
        self.ask(b, "four")
        self.assertEqual(self.bp_in(other, "news").stdout, "")
        self.assertIn("isn't one of this repo's coordinators", self.bp("news", ok=False).stderr)  # another pane
        del self.env["REX_BLOCK"]  # outside Rex: the primary's
        self.assertEqual(self.bp("news").stdout, "bullpen · BP-2 is at a prompt (question): four\n")
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")  # one cursor for c1, wherever it's read
        # two followers at once, one per coordinator: neither waits for the other
        self.bp_in(other, "news")
        self.bp_in("blk1", "news")

    def test_a_monitor_in_another_pane_doesnt_take_the_coordinators_news(self):
        # What held BP-20's line for 15 minutes: an agent that used to be the
        # coordinator kept starting its monitor. Its follower held the lock, so the
        # coordinator's own waited, and the line was printed in the other pane.
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        stray, stray_lines, _ = self.follow("me")
        self.assertEqual(stray_lines(1), ["bullpen · this pane isn't one of this repo's coordinators, so it "
                                          "doesn't read their news: don't start the monitor again "
                                          "(`bullpen up --here` here makes it one)"])
        stray.wait(timeout=10)
        p, lines, _ = self.follow()  # the coordinator's own gets the lock at once
        self.ask(tid, "one")
        self.assertEqual(lines(1), ["bullpen · BP-1 is at a prompt (question): one"])
        self.assertIn("isn't one of", self.bp("news", ok=False).stderr)  # a one-off read neither

    def test_a_monitor_stops_when_its_pane_is_no_longer_a_coordinator(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        p, lines, _ = self.follow()
        self.ask(tid, "one")
        self.assertEqual(lines(1), ["bullpen · BP-1 is at a prompt (question): one"])
        path = next((self.tmp / "state" / "repos").iterdir()) / "config.json"
        cfg = json.loads(path.read_text())
        cfg["coordinators"][0]["block"] = "blk9"  # another pane took the role
        path.write_text(json.dumps(cfg))
        self.assertIn("isn't one of this repo's coordinators", lines(1)[0])
        p.wait(timeout=10)

    def test_an_old_config_with_one_coordinator_still_loads(self):
        self.bp("up")  # blk1, board blk2
        path = next((self.tmp / "state" / "repos").iterdir()) / "config.json"
        cfg = json.loads(path.read_text())
        for k in ("coordinators", "next_coordinator"):
            cfg.pop(k)
        cfg["coordinator"] = {"block": "blk1", "session": "session:1"}
        cfg["board"] = {"block": "blk2", "session": "session:1"}
        path.write_text(json.dumps(cfg))
        (path.parent / "news.log").write_text("")
        (path.parent / "news.cursor").write_text("0\n")  # its one cursor
        cs = self.coordinators()
        self.assertEqual(cs, [{"id": "c1", "block": "blk1", "session": "session:1",
                               "board": {"block": "blk2", "session": "session:1"}}])
        self.assertNotIn("coordinator", json.loads(self.bp("config", "--json").stdout))
        self.assertIn("coordinator already running: c1 (blk1)\n", self.bp("up", "--verbose").stdout)
        self.assertEqual([a[0] for a in self.rex_calls()].count("split"), 1)  # its board stays
        self.bp("config", "--news", "monitor")
        tid = self.bp("add", "T", "--brief", "x").stdout.strip()
        self.assertEqual(self.show(tid)["coordinator"], "c1")
        other = self.new_pane()
        self.bp_in(other, "up", "--here")
        self.assertEqual([c["id"] for c in self.coordinators()], ["c1", "c2"])

    def test_an_old_cursor_carries_on_for_the_first_coordinator(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        d = next((self.tmp / "state" / "repos").iterdir())
        self.ask(tid, "one")
        for p in d.glob("news-*.cursor"):  # as an older bullpen left it
            (d / "news.cursor").write_text(p.read_text())
            p.unlink()
        self.assertEqual(self.bp_in("blk1", "news").stdout, "bullpen · BP-1 is at a prompt (question): one\n")
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")

    def test_a_gone_coordinators_tasks_fall_back_to_the_others(self):
        other = self.two_coordinators()
        third = self.new_pane()
        self.bp_in(third, "up", "--here")
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", a)
        board2 = self.coordinators()[1]["board"]["block"]
        (self.fake / "alive" / other).unlink()  # c2's agent is closed
        self.ask(a, "anyone?")
        self.assertEqual(sorted(self.typed()), sorted([("blk1", "bullpen · BP-1 is at a prompt (question): anyone?"),
                                                       (third, "bullpen · BP-1 is at a prompt (question): anyone?")]))
        self.bp("up")  # drops c2, and closes its board
        self.assertEqual([c["id"] for c in self.coordinators()], ["c1", "c3"])
        self.assertIn(["block", "close", board2, "-s", "session:1"], self.rex_calls())
        self.assertIn("coord: c2, gone: every running coordinator sees to it", self.bp("show", a).stdout)
        self.bp_in(third, "adopt", a)
        self.assertEqual(self.show(a)["coordinator"], "c3")
        self.ask(a, "c3?")
        self.assertEqual(self.typed()[-1], (third, "bullpen · BP-1 is at a prompt (question): c3?"))
        self.assertIn("coordinator's own pane", self.bp("adopt", a, ok=False).stderr)

    def test_waiting_news_for_a_gone_coordinator_goes_to_the_others(self):
        other = self.two_coordinators()
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", a)
        (self.fake / "screen").write_text("❯ half a message\n")  # the person is typing: news waits
        self.ask(a, "waiting")
        self.assertEqual(self.typed(), [])
        (self.fake / "alive" / other).unlink()
        (self.fake / "screen").write_text("❯ \n")
        self.bp("_flush")
        self.assertEqual(self.typed(), [("blk1", "bullpen · BP-1 is at a prompt (question): waiting")])
        self.assertEqual(list((self.tmp / "state").rglob("outbox*.txt")), [])

    def test_news_while_no_coordinator_is_alive_is_typed_once(self):
        other = self.two_coordinators()
        third = self.new_pane()
        self.bp_in(third, "up", "--here")
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", a)
        for b in ("blk1", other, third):
            (self.fake / "alive" / b).unlink()  # every coordinator's agent is closed
        self.ask(a, "nobody home")
        self.assertEqual(self.typed(), [])
        (self.fake / "alive" / "blk1").write_text("")  # one comes back
        self.bp("_flush")
        self.assertEqual(self.typed(), [("blk1", "bullpen · BP-1 is at a prompt (question): nobody home")])
        self.assertEqual(list((self.tmp / "state").rglob("outbox*.txt")), [])
        self.assertFalse((self.tmp / "state" / "outbox").exists())

    def test_go_and_hide_use_the_tasks_own_coordinator(self):
        other = self.two_coordinators()
        board2 = self.coordinators()[1]["board"]["block"]
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", a)
        self.assertEqual(self.api("session.new_layer")[-1]["window_id"], "w2")  # c2's tab
        self.bp_in(board2, "go")  # the board's own coordinator
        self.assertEqual(self.focused()[-1], other)
        self.bp_in(board2, "go", "--board")
        self.assertEqual(self.focused()[-1], board2)
        self.bp("go", a)
        self.bp("hide", "--from", self.block(a))
        self.assertEqual(self.focused()[-1], board2)
        self.bp("go", "--from", self.block(a))
        self.assertEqual(self.focused()[-1], other)
        self.bp("go")  # from elsewhere: the primary one
        self.assertEqual(self.focused()[-1], "blk1")

    # ---------- who sees to a task ----------

    def test_a_gone_owners_task_is_on_the_running_coordinators_boards(self):
        other = self.two_coordinators()
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", a)
        c1_board = self.coordinators()[0]["board"]["block"]
        self.ask(a, "anyone?")
        self.assertIn("nothing needs you", self.bp("go", "--next", "--from", c1_board, ok=False).stderr)
        (self.fake / "alive" / other).unlink()  # c2's agent is closed: c1 sees to its task
        out = json.loads(self.bp("go", "--next", "--json", "--from", c1_board).stdout)
        self.assertEqual(out["block"], self.block(a))

    def gone_now(self, block):
        """The watcher reports a pane gone; bullpen's later look happens right away."""
        self.env["BULLPEN_GONE_WAIT"] = "0"
        self.bp("_gone", block)

    def test_a_coordinator_whose_pane_closes_is_dropped(self):
        other = self.two_coordinators()
        board2 = self.coordinators()[1]["board"]["block"]
        a = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        (self.fake / "alive" / other).unlink()
        self.gone_now(other)
        self.assertEqual([c["id"] for c in self.coordinators()], ["c1"])
        self.assertIn(["block", "close", board2, "-s", "session:1"], self.rex_calls())
        self.assertIn("coord: c2, gone", self.bp("show", a).stdout)

    def test_a_coordinator_still_running_is_kept(self):
        other = self.two_coordinators()
        self.gone_now(other)  # the pane came back, or the event was for something else
        self.assertEqual([c["id"] for c in self.coordinators()], ["c1", "c2"])

    def test_a_rex_restart_drops_no_coordinator(self):
        other = self.two_coordinators()
        reg = self.fake / "reg.json"
        R = json.loads(reg.read_text())
        # Rex restarted and its pane came back under a new ID: `up` finds it again
        (self.fake / "alive" / "blk1").unlink()
        R["blocks"]["blk99"] = dict(R["blocks"]["blk1"])
        (self.fake / "alive" / "blk99").write_text("")
        reg.write_text(json.dumps(R))
        self.gone_now("blk1")
        self.assertEqual([c["id"] for c in self.coordinators()], ["c1", "c2"])
        # Rex is down, or came back with a new session: nobody can tell, so nobody is dropped
        R["session"] = "session:2"
        reg.write_text(json.dumps(R))
        (self.fake / "alive" / other).unlink()
        self.gone_now(other)
        self.assertEqual([c["id"] for c in self.coordinators()], ["c1", "c2"])
