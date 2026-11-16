"""`config --news wake`: the hook in the repo's .claude/settings.local.json that parks the
waiter (written, kept current by `up`, removed with another value), and doctor's `news`."""

import json
import subprocess

from support import setUpModule, tearDownModule, BullpenTest, BULLPEN  # a temp folder for each module

FILE = ".claude/settings.local.json"


class HookCase(BullpenTest):
    def settings_file(self):
        return self.repo / FILE

    def settings(self):
        return json.loads(self.settings_file().read_text())

    def entry(self):
        return {"hooks": [{"type": "command", "command": self.command(), "asyncRewake": True, "timeout": 86400}]}

    def command(self):
        """What the hook runs: this bullpen's waiter, with the test's own state."""
        return self.settings()["hooks"]["Stop"][-1]["hooks"][-1]["command"]

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repo), *args], text=True, capture_output=True)


class ConfigNewsWakeTest(HookCase):
    def test_wake_writes_both_entries_and_git_leaves_the_file_out(self):
        p = self.bp("config", "--news", "wake", "--json")
        self.assertEqual(json.loads(p.stdout)["news"], "wake")
        self.assertIn("news: by wake", p.stderr)
        st = self.settings()
        self.assertEqual(st, {"hooks": {"Stop": [self.entry()], "StopFailure": [self.entry()]}})
        self.assertTrue(self.command().endswith(f"{BULLPEN} news --wake"))
        self.assertIn(f"BULLPEN_STATE={self.tmp / 'state'}", self.command())  # the test's own state, as --follow's
        self.assertIn(f"/{FILE}\n", (self.repo / ".git/info/exclude").read_text())
        self.assertEqual(self.git("status", "--porcelain").stdout, "")
        before = self.settings_file().read_text()
        self.bp("config", "--news", "wake")  # again: nothing to change
        self.assertEqual(self.settings_file().read_text(), before)
        self.assertEqual((self.repo / ".git/info/exclude").read_text().count(FILE), 1)
        self.assertEqual([f.name for f in self.settings_file().parent.iterdir()], ["settings.local.json"])

    def test_it_touches_nothing_else_in_the_file_and_another_value_takes_it_out_again(self):
        theirs = {"zeta": 1, "permissions": {"allow": ["Bash(ls:*)"]},
                  "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "true"}]}],
                            "Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]},
                  "alpha": "é"}
        self.settings_file().parent.mkdir()
        self.settings_file().write_text(json.dumps(theirs, indent=2, ensure_ascii=False) + "\n")
        self.settings_file().chmod(0o600)
        self.bp("config", "--news", "wake")
        st = self.settings()
        self.assertEqual(list(st), list(theirs))  # the keys in their order
        self.assertEqual(list(st["hooks"]), ["PreToolUse", "Stop", "StopFailure"])
        self.assertEqual(st["hooks"]["Stop"], [theirs["hooks"]["Stop"][0], self.entry()])
        self.assertEqual(st["hooks"]["PreToolUse"], theirs["hooks"]["PreToolUse"])
        self.assertIn('"é"', self.settings_file().read_text())
        self.assertEqual(self.settings_file().stat().st_mode & 0o777, 0o600)
        self.bp("config", "--news", "type")
        self.assertEqual(self.settings(), theirs)
        self.assertEqual(list(self.settings()), list(theirs))

    def test_another_value_removes_the_hook_and_a_file_left_empty(self):
        for back in (["--news", "type"], ["--news", "monitor"]):
            self.bp("config", "--news", "wake")
            self.assertTrue(self.settings_file().exists())
            self.bp("config", *back)
            self.assertFalse(self.settings_file().exists(), back)

    def test_wake_is_refused_for_a_coordinator_that_isnt_claude_code(self):
        self.set_commands(coordinator_cmd=["codex", "--full-auto"])
        p = self.bp("config", "--news", "wake", ok=False)
        self.assertEqual(p.returncode, 1)
        self.assertIn("`wake` needs Claude Code as the coordinator", p.stderr)
        self.assertIn("coordinator_cmd runs `codex`: use `bullpen config --news type`", p.stderr)
        self.assertEqual(json.loads(self.bp("config", "--json").stdout)["news"], "type")
        self.assertFalse(self.settings_file().exists())
        self.set_commands(coordinator_cmd=["/opt/bin/claude", "--model", "opus"])  # by its path: Claude Code
        self.bp("config", "--news", "wake")
        self.assertTrue(self.settings_file().exists())

    def test_a_settings_file_that_doesnt_parse_is_left_alone_and_wake_refused(self):
        self.settings_file().parent.mkdir()
        self.settings_file().write_text("{not json")
        p = self.bp("config", "--news", "wake", ok=False)
        self.assertIn(f"{FILE} doesn't parse", p.stderr)
        self.assertEqual(self.settings_file().read_text(), "{not json")
        self.assertEqual(json.loads(self.bp("config", "--json").stdout)["news"], "type")

    def test_a_running_coordinator_hears_of_the_switch_by_one_last_typed_line(self):
        tid = self.start_task()
        self.ask(tid, "typed")
        typed = len(self.news())
        self.bp("config", "--news", "wake")
        told = self.news()[typed:]
        self.assertEqual(len(told), 1)
        self.assertTrue(told[0].startswith("bullpen · news now wakes you by itself and is no longer typed"))
        self.assertEqual(self.bp_in("blk1", "news").stdout, "")  # what was typed isn't news again
        self.ask(tid, "not typed")
        self.assertEqual(len(self.news()), typed + 1)
        self.assertIn("not typed", self.bp_in("blk1", "news").stdout)

    def test_from_monitor_to_wake_unread_news_stays_unread(self):
        tid = self.start_task()
        self.bp("config", "--news", "monitor")
        self.ask(tid, "Which one?")
        self.bp("config", "--news", "wake")
        self.assertIn("Which one?", self.bp_in("blk1", "news").stdout)

    def test_the_coordinators_own_pane_is_typed_nothing(self):
        self.bp("up", "--here")  # this pane ("me") is the coordinator
        self.bp("config", "--news", "wake")
        self.assertEqual(self.typed(), [])


class UpKeepsTheHookTest(HookCase):
    def test_up_writes_a_missing_hook_and_brings_an_old_one_up_to_date(self):
        self.bp("config", "--news", "wake")
        want = self.settings()
        self.assertNotIn("News hook", self.bp("up").stdout)  # current: nothing said
        self.settings_file().unlink()
        self.assertIn(f"News hook written to {FILE}", self.bp("up").stdout)
        self.assertEqual(self.settings(), want)
        old = json.loads(json.dumps(want).replace(str(BULLPEN), "/old/checkout/bin/bullpen"))
        self.settings_file().write_text(json.dumps(old))
        self.assertIn("News hook written", self.bp("up").stdout)
        self.assertEqual(self.settings(), want)

    def test_up_says_why_it_couldnt_and_goes_on(self):
        self.bp("config", "--news", "wake")
        self.settings_file().write_text("[]")
        out = self.bp("up").stdout
        self.assertIn(f"The news hook isn't in place: {FILE} isn't a JSON object. See `bullpen doctor`.", out)
        self.assertIn("oordinator", out)  # up did the rest

    def test_up_writes_no_hook_for_other_news(self):
        self.bp("up")
        self.bp("config", "--news", "monitor")
        self.bp("up")
        self.assertFalse(self.settings_file().exists())


class DoctorNewsTest(HookCase):
    def setUp(self):
        super().setUp()
        self.healthy()

    def news_line(self):
        return self.doctor()[1]["news"]

    def claude_says(self, version):
        (self.fake / "claude").write_text(f"#!/bin/sh\necho '{version}'\n")

    def test_no_hook_to_check_with_other_news(self):
        self.assertEqual(self.news_line(), ("--", "news is `type`: no hook to check"))

    def test_wake_with_its_hook_in_place_is_ok(self):
        self.bp("config", "--news", "wake")
        self.claude_says("2.1.325 (Claude Code)")
        p, lines = self.doctor()
        self.assertEqual(lines["news"], ("ok", f"by wake: the hook is in {FILE} (Stop, StopFailure)"))
        self.assertEqual(p.returncode, 0, p.stdout)

    def test_a_missing_or_old_hook_fails_and_up_is_the_fix(self):
        self.bp("config", "--news", "wake")
        st = self.settings()
        self.settings_file().write_text(json.dumps(st).replace(str(BULLPEN), "/old/bin/bullpen"))
        mark, text = self.news_line()
        self.assertEqual(mark, "FAIL")
        self.assertIn("/old/bin/bullpen news --wake`, not `", text)
        self.assertIn("`bullpen up` fixes it", text)
        self.settings_file().write_text("{}")
        self.assertIn(f"the news hook isn't in {FILE}: `bullpen up` writes it", self.news_line()[1])
        self.settings_file().unlink()
        self.assertIn(f"{FILE} isn't there", self.news_line()[1])
        self.settings_file().write_text("{nope")
        self.assertIn("doesn't parse", self.news_line()[1])
        self.settings_file().unlink()
        self.bp("up")
        self.assertEqual(self.news_line()[0], "ok")

    def test_a_committed_settings_file_isnt_ignored(self):
        self.bp("config", "--news", "wake")
        self.git("add", "-f", FILE)
        mark, text = self.news_line()
        self.assertEqual(mark, "FAIL")
        self.assertIn(f"git doesn't ignore {FILE}", text)

    def test_a_coordinator_that_isnt_claude_code_and_an_old_claude_code(self):
        self.bp("config", "--news", "wake")
        self.claude_says("2.1.300 (Claude Code)")
        mark, text = self.news_line()
        self.assertEqual(mark, "FAIL")
        self.assertIn("Claude Code 2.1.300 is older than 2.1.325", text)
        self.set_commands(coordinator_cmd=["codex"])
        self.assertIn("`wake` needs Claude Code as the coordinator", self.news_line()[1])

    def test_an_idle_coordinator_with_no_waiter_is_said(self):
        self.bp("config", "--news", "wake")
        doing = self.tmp / "state" / "doing"
        doing.mkdir(exist_ok=True)
        (doing / "blk1.json").write_text(json.dumps({"state": "working", "text": "", "at": 1}))
        self.assertEqual(self.news_line()[0], "ok")  # in a turn: its end parks one
        (doing / "blk1.json").write_text(json.dumps({"state": "idle", "text": "", "at": 1}))
        mark, text = self.news_line()
        self.assertEqual(mark, "FAIL")
        self.assertIn("c1 is idle with no news waiter: it hears news by typing until its next turn", text)
        self.bp_in("blk1", "news", "--pause")
        self.assertEqual(self.news_line()[0], "ok")  # paused: it wants none
        self.bp_in("blk1", "news", "--resume")
        p = subprocess.Popen([str(BULLPEN), "news", "--wake"], cwd=self.repo, env={**self.env, "REX_BLOCK": "blk1"},
                             stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(lambda: (p.kill(), p.wait(), p.stdout.close(), p.stderr.close()))
        for _ in range(200):
            if self.news_line()[0] == "ok":
                break
        self.assertEqual(self.news_line()[0], "ok")  # a waiter is parked

    def test_fix_removes_a_hook_left_with_other_news_and_writes_a_missing_one(self):
        self.bp("config", "--news", "wake")
        st = self.settings_file().read_text()
        self.bp("config", "--news", "type")
        self.settings_file().write_text(st)  # left behind (an older bullpen, a copied file)
        mark, text = self.news_line()
        self.assertEqual(mark, "FAIL")
        self.assertIn(f"news is `type`, but {FILE} still has bullpen's news hook: `bullpen doctor --fix` removes it",
                      text)
        p = self.bp("doctor", "--fix")
        self.assertIn(f"fixed  news: removed bullpen's news hook from {FILE}", p.stdout)
        self.assertFalse(self.settings_file().exists())
        self.assertNotIn("fixed", self.bp("doctor", "--fix").stdout)  # nothing left to fix
        self.bp("config", "--news", "wake")
        self.settings_file().unlink()
        p = self.bp("doctor", "--fix", "--json")
        self.assertIn("fixed  news: wrote the news hook", p.stderr)
        self.assertEqual(json.loads(p.stdout)["problems"], [])
        self.assertTrue(self.settings_file().exists())
        p = self.bp("doctor", "--fix", "--all-repos", ok=False)
        self.assertIn("--fix is for one repository", p.stderr)
