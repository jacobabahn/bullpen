"""Mission control: `bullpen mission`, its session and its pane."""

import json

from support import setUpModule, tearDownModule, BullpenTest  # a temp folder for each module


class MissionTest(BullpenTest):
    def record(self):
        return json.loads((self.tmp / "state" / "mission.json").read_text())

    def test_mission_makes_its_session_then_focuses_it_and_starts_again_when_its_pane_closed(self):
        out = self.bp("mission", "--from", "blk9", "--session", "session:1", cwd=self.tmp).stdout
        new = [a for a in self.rex_calls() if a[0] == "new"]
        self.assertEqual(len(new), 1)
        self.assertEqual(new[0][:6], ["new", "mission", "--json", "--window", "Mission", "-c"])
        self.assertEqual(new[0][-2:], ["mission", "--here"])  # its pane runs the screen
        self.assertIn(f"BULLPEN_STATE={self.tmp / 'state'}", new[0])  # Rex panes don't inherit it
        rec = self.record()
        self.assertEqual(rec["session"], "session:mission")
        self.assertEqual(out, f"mission control started: {rec['block']}\n")
        self.assertEqual(self.rex_calls()[-1], ["focus", "-s", "session:mission", rec["block"]])
        # again: the one that's running is focused, nothing new
        out = self.bp("mission").stdout
        self.assertEqual(out, f"mission control focused: {rec['block']}\n")
        self.assertEqual(len([a for a in self.rex_calls() if a[0] in ("new", "run")]), 1)
        # its pane closed: a new tab in the mission session, not a second session
        (self.fake / "alive" / rec["block"]).unlink()
        self.bp("mission")
        run = [a for a in self.rex_calls() if a[0] == "run"]
        self.assertEqual(len(run), 1)
        self.assertEqual(run[0][run[0].index("-s") + 1], "session:mission")
        self.assertEqual(run[0][run[0].index("--label") + 1], "bullpen: mission")
        self.assertEqual(len([a for a in self.rex_calls() if a[0] == "new"]), 1)
        self.assertNotEqual(self.record()["block"], rec["block"])

    def test_mission_here_runs_the_board_program_as_mission_control(self):
        fake = self.fake / "board"
        fake.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\"\n")
        fake.chmod(0o755)
        self.env["BULLPEN_BOARD"] = str(fake)
        args = self.bp("mission", "--here", cwd=self.tmp).stdout.splitlines()
        self.assertEqual(args[:3], ["--mission", "--state", str(self.tmp / "state")])
        self.assertEqual(args[3], "--bullpen")
        self.env["BULLPEN_BOARD"] = str(self.tmp / "missing")
        self.assertIn("isn't built", self.bp("mission", "--here", ok=False).stderr)

    def test_go_switch_shows_the_pane_then_has_the_app_show_its_session(self):
        tid = self.start_task()
        blk = self.block(tid)
        before = len(self.rex_calls())
        self.bp("go", tid, "--switch")
        calls = self.rex_calls()[before:]
        session = self.show(tid)["session"]
        select = [a for a in calls if a[:2] == ["do", "session.select"]]
        self.assertEqual(len(select), 1)
        self.assertEqual(select[0][2], f"session_id={session}")
        window = self.reg()["blocks"][blk]["window"]
        self.assertEqual(select[0][3:], [f"window_id={window}"])
        # the pane stays where it is, running
        self.assertNotIn(["block", "close", blk], calls)
        self.assertTrue((self.fake / "alive" / blk).exists())
        # plain go doesn't switch
        before = len(self.rex_calls())
        self.bp("go", tid)
        self.assertNotIn("do", [a[0] for a in self.rex_calls()[before:]])

    def test_go_switch_says_when_the_app_cant_be_asked(self):
        tid = self.start_task()
        (self.fake / "select_fails").write_text("")
        p = self.bp("go", tid, "--switch", ok=False)
        self.assertIn(f"{tid}'s pane is up in its own session, but Rex didn't show it: rex: no app client", p.stderr)

    def reg(self):
        return json.loads((self.fake / "reg.json").read_text())
