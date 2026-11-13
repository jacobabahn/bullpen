"""Rules, the agents' model and the global config."""

import json
import os
import re
import unittest
from pathlib import Path

from support import setUpModule, tearDownModule, sh, BullpenTest, ROOT  # a temp folder for each module


class ConfigTest(BullpenTest):
    def commit_rules(self, text):
        (self.repo / ".bullpen").mkdir(exist_ok=True)
        (self.repo / ".bullpen" / "rules.md").write_text(text)
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "Rules")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")

    def personal_rules(self, text):
        d = next((self.tmp / "state" / "repos").iterdir())
        (d / "rules.md").write_text(text)

    def test_rules_from_the_repo_go_in_the_brief(self):
        self.commit_rules("- Keep functions small.\n")
        self.start_task()
        brief = self.full_brief()
        self.assertIn("## Rules for this repository\n\n- Keep functions small.\n", brief)
        self.assertLess(brief.index("Make it work."), brief.index("Keep functions small."))
        self.assertLess(brief.index("Keep functions small."), brief.index("## How you work"))

    def test_personal_rules_go_in_the_brief(self):
        self.bp("up")
        self.personal_rules("- Say it plainly.\n")
        self.bp("add", "Fix", "--brief", "Make it work.")
        self.bp("start", "BP-1")
        self.assertIn("## Rules for this repository\n\n- Say it plainly.\n", self.full_brief())

    def test_both_rules_files_repo_first(self):
        self.commit_rules("- Repo rule.\n")
        self.bp("up")
        self.personal_rules("- My rule.\n")
        self.bp("add", "Fix", "--brief", "Make it work.")
        self.bp("start", "BP-1")
        brief = self.full_brief()
        self.assertEqual(brief.count("## Rules for this repository"), 1)
        self.assertLess(brief.index("Repo rule."), brief.index("My rule."))
        out = self.bp("rules").stdout
        self.assertIn("From .bullpen/rules.md on origin/main", out)
        self.assertIn("rules.md\n\n- My rule.", out)
        self.assertLess(out.index("Repo rule."), out.index("My rule."))

    def test_no_rules_no_heading(self):
        self.start_task()
        self.assertNotIn("Rules for this repository", self.full_brief())
        self.commit_and_ready("BP-1")
        prompt = next((self.tmp / "state").rglob("BP-1.review-1.md")).read_text()
        self.assertNotIn("Rules for this repository", prompt)
        self.assertIn("No rules.", self.bp("rules").stdout)

    def test_uncommitted_rules_in_the_main_checkout_are_not_used(self):
        self.commit_rules("- Committed rule.\n")
        (self.repo / ".bullpen" / "rules.md").write_text("- Half-written rule.\n")
        self.start_task()
        brief = self.full_brief()
        self.assertIn("Committed rule.", brief)
        self.assertNotIn("Half-written", brief)
        self.assertNotIn("Half-written", self.bp("rules").stdout)

    def test_reviewer_gets_the_workers_rules(self):
        self.commit_rules("- Repo rule.\n")
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / ".bullpen" / "rules.md").write_text("- Changed by the worker.\n")
        self.commit_and_ready(tid)
        prompt = next((self.tmp / "state").rglob("BP-1.review-1.md")).read_text()
        self.assertIn("## Rules for this repository", prompt)
        self.assertIn("Check that the work follows them.", prompt)
        self.assertIn("- Repo rule.", prompt)
        self.assertNotIn("Changed by the worker", prompt)

    def test_a_restarted_task_keeps_the_base_rules(self):
        self.commit_rules("- Repo rule.\n")
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / ".bullpen" / "rules.md").write_text("- Changed by the worker.\n")
        self.bp("_gone", self.block(tid))
        self.assertEqual(self.show(tid)["status"], "stopped")
        self.bp("start", tid)
        brief = self.full_brief()
        self.assertIn("- Repo rule.", brief)
        self.assertNotIn("Changed by the worker", brief)
        saved = next((self.tmp / "state").rglob("BP-1.rules.md")).read_text()
        self.assertEqual(saved, "- Repo rule.\n")

    def test_rules_edit_opens_the_personal_file(self):
        self.bp("up")
        editor = self.tmp / "ed"
        editor.write_text("#!/bin/sh\necho '- Edited.' > \"$1\"\n")
        editor.chmod(0o755)
        self.env = {**self.env, "EDITOR": str(editor), "VISUAL": ""}
        self.bp("rules", "--edit")
        out = self.bp("rules").stdout
        self.assertIn("- Edited.", out)
        self.assertIn(str(self.tmp / "state" / "repos"), out)

    def test_no_model_set_leaves_the_command_alone(self):
        tid = self.start_task()
        self.assertNotIn("--model", self.last_command())
        self.assertIsNone(self.show(tid).get("model"))
        cfg = json.loads(self.bp("config", "--json").stdout)
        self.assertEqual(cfg["models"], {"worker": "default (Claude Code picks)",
                                         "reviewer": "default (Claude Code picks)"})

    def test_config_model_sets_both_and_the_per_role_flags_one_each(self):
        cfg = json.loads(self.bp("config", "--model", "opus", "--json").stdout)
        self.assertEqual((cfg["worker_model"], cfg["reviewer_model"]), ("opus", "opus"))
        self.assertEqual(cfg["models"], {"worker": "opus", "reviewer": "opus"})
        cfg = json.loads(self.bp("config", "--reviewer-model", "sonnet", "--json").stdout)
        self.assertEqual((cfg["worker_model"], cfg["reviewer_model"]), ("opus", "sonnet"))
        cfg = json.loads(self.bp("config", "--worker-model", "claude-opus-4-1", "--json").stdout)
        self.assertEqual((cfg["worker_model"], cfg["reviewer_model"]), ("claude-opus-4-1", "sonnet"))
        cfg = json.loads(self.bp("config", "--model", "default", "--json").stdout)
        self.assertEqual((cfg["worker_model"], cfg["reviewer_model"]), ("default", "default"))

    def test_model_is_passed_at_start_resume_and_review(self):
        self.bp("config", "--worker-model", "opus", "--reviewer-model", "sonnet")
        tid = self.start_task()
        self.assertEqual(self.models(self.last_command()), ["opus"])
        self.assertEqual(self.show(tid)["model"], "opus")
        self.assertIn("    model: opus", self.bp("show", tid).stdout)
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)
        self.bp("resume", tid)
        cmd = self.last_command()
        self.assertIn("--continue", cmd)
        self.assertEqual(self.models(cmd), ["opus"])
        self.commit_and_ready(tid)
        self.assertEqual(self.models(self.last_command()), ["sonnet"])

    def test_config_model_replaces_one_in_the_command(self):
        self.set_commands(worker=["claude", "--permission-mode", "auto", "--model", "opus"],
                          reviewer=["claude", "--model=haiku", "--permission-mode", "auto"])
        cfg = json.loads(self.bp("config", "--json").stdout)
        self.assertEqual(cfg["models"], {"worker": "opus", "reviewer": "haiku"})
        self.bp("config", "--model", "sonnet")
        tid = self.start_task()
        self.assertEqual(self.models(self.last_command()), ["sonnet"])
        self.assertEqual(self.show(tid)["model"], "sonnet")
        self.commit_and_ready(tid)
        cmd = self.last_command()
        self.assertEqual(self.models(cmd), ["sonnet"])
        self.assertFalse(any(a.startswith("--model=") for a in cmd))

    def test_model_default_drops_the_commands_own(self):
        self.set_commands(worker=["claude", "--permission-mode", "auto", "--model", "opus"],
                          reviewer=["claude", "--permission-mode", "auto", "--model=opus"])
        cfg = json.loads(self.bp("config", "--model", "default", "--json").stdout)
        self.assertEqual(cfg["models"], {"worker": "default (Claude Code picks)",
                                         "reviewer": "default (Claude Code picks)"})
        tid = self.start_task()
        self.assertEqual(self.models(self.last_command()), [])
        self.assertIsNone(self.show(tid).get("model"))
        self.commit_and_ready(tid)
        cmd = self.last_command()
        self.assertEqual(self.models(cmd), [])
        self.assertFalse(any(a.startswith("--model") for a in cmd))

    def test_start_model_is_for_that_task_and_stays_on_resume(self):
        self.bp("config", "--model", "opus")
        self.bp("up")
        tid = self.bp("add", "Fix it", "--brief", "x").stdout.strip()
        self.bp("start", tid, "--model", "haiku")
        self.assertEqual(self.models(self.last_command()), ["haiku"])
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)
        self.bp("resume", tid)
        self.assertEqual(self.models(self.last_command()), ["haiku"])
        tid2 = self.bp("add", "Other", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        self.assertEqual(self.models(self.last_command()), ["opus"])

    def test_add_model_carries_to_a_start_by_itself(self):
        self.bp("config", "--worker-model", "opus")
        tid = self.ready_with_a_commit()
        self.assertEqual(self.models(self.api("session.new_layer")[0]["layout"]["block"]["options"]["command"]), ["opus"])
        nxt = self.bp("add", "Next", "--brief", "x", "--after", tid, "--model", "haiku").stdout.strip()
        self.assertEqual(self.show(nxt)["worker_model"], "haiku")
        self.assertIn("Model: haiku.", self.bp("ls").stdout)
        self.assertIn("    model: haiku (its own, over the config's opus)", self.bp("show", nxt).stdout)
        self.merge_on_github(tid)
        self.bp("accept", tid)
        self.assertEqual(self.show(nxt)["status"], "working")
        self.assertEqual(self.models(self.last_command()), ["haiku"])
        self.assertEqual(self.show(nxt)["model"], "haiku")

    def test_model_sets_and_clears_a_tasks_own(self):
        self.bp("config", "--worker-model", "opus")
        self.bp("up")
        tid = self.bp("add", "Fix it", "--brief", "x").stdout.strip()
        self.assertIn(f"{tid} uses sonnet", self.bp("model", tid, "sonnet").stdout)
        self.bp("start", tid)
        self.assertEqual(self.models(self.last_command()), ["sonnet"])
        out = self.bp("model", tid, "--clear").stdout
        self.assertIn(f"{tid} uses the config's worker model (opus)", out)
        self.assertIn("takes effect at its next start or resume", out)
        self.assertNotIn("worker_model", self.show(tid))
        self.assertNotIn("Model:", self.bp("ls").stdout)
        self.assertEqual(len(self.api("session.new_layer")), 1)  # nothing restarted
        self.bp("model", tid, "haiku")
        self.assertIn("running sonnet until its next start or resume", self.bp("show", tid).stdout)
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)
        self.bp("resume", tid)
        self.assertEqual(self.models(self.last_command()), ["haiku"])
        self.assertNotEqual(self.bp("model", tid, ok=False).returncode, 0)

    def test_start_model_overrides_the_tasks_own(self):
        self.bp("up")
        tid = self.bp("add", "Fix it", "--brief", "x", "--model", "haiku").stdout.strip()
        self.bp("start", tid, "--model", "sonnet")
        self.assertEqual(self.models(self.last_command()), ["sonnet"])
        self.assertEqual(self.show(tid)["worker_model"], "sonnet")
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)
        self.bp("resume", tid)
        self.assertEqual(self.models(self.last_command()), ["sonnet"])

    def test_other_agents_commands_are_left_alone(self):
        self.set_commands(worker=["codex", "--permission-mode", "auto"])
        cfg = json.loads(self.bp("config", "--model", "opus", "--json").stdout)
        self.assertIn("not claude", cfg["models"]["worker"])
        self.assertEqual(cfg["models"]["reviewer"], "opus")
        tid = self.start_task()
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertNotIn("--model", cmd)
        self.assertIsNone(self.show(tid).get("model"))

    # ---------- global config ----------

    def global_cfg(self):
        path = self.tmp / "xdg" / "bullpen" / "config.json"
        return json.loads(path.read_text()) if path.exists() else None

    def cfg(self, *args):
        return json.loads(self.bp("config", *args, "--json").stdout)

    def test_repo_settings_over_global_ones_over_defaults(self):
        self.bp("up")
        cfg = self.cfg("--global", "--merge", "squash", "--max-running", "5", "--worker-model", "opus")
        self.assertEqual(self.global_cfg(), {"merge": "squash", "max_running": 5, "worker_model": "opus"})
        self.assertEqual((cfg["merge"], cfg["max_running"], cfg["worker_model"]), ("squash", 5, "opus"))
        self.assertEqual(cfg["sources"]["merge"], "global")
        self.assertEqual(cfg["sources"]["stuck_after"], "default")
        cfg = self.cfg("--merge", "merge")
        self.assertEqual((cfg["merge"], cfg["sources"]["merge"]), ("merge", "repo"))
        self.assertEqual(self.global_cfg()["merge"], "squash")  # the repo's own doesn't change it
        # a later global change reaches what the repo doesn't set itself
        self.bp("config", "--global", "--max-running", "2")
        cfg = self.cfg()
        self.assertEqual((cfg["max_running"], cfg["merge"]), (2, "merge"))
        out = self.bp("config").stdout
        self.assertRegex(out, r"(?m)^  merge +merge +repo$")
        self.assertRegex(out, r"(?m)^  max_running +2 +global$")
        self.assertRegex(out, r"(?m)^  triage_minutes +10 +default$")
        self.assertIn("models: worker opus;", out)
        # the repo's config.json holds only its own settings
        own = json.loads(next((self.tmp / "state" / "repos").iterdir()).joinpath("config.json").read_text())
        self.assertEqual(own["merge"], "merge")
        self.assertNotIn("max_running", own)
        self.assertNotIn("worker_model", own)

    def test_unset_lets_the_global_value_apply(self):
        self.bp("config", "--global", "--review-rounds", "3")
        self.assertEqual(self.cfg("--review-rounds", "1")["review_rounds"], 1)
        cfg = self.cfg("--unset", "review_rounds")
        self.assertEqual((cfg["review_rounds"], cfg["sources"]["review_rounds"]), (3, "global"))
        cfg = self.cfg("--global", "--unset", "review_rounds")
        self.assertEqual((cfg["review_rounds"], cfg["sources"]["review_rounds"]), (2, "default"))
        self.assertEqual(self.global_cfg(), {})
        self.bp("config", "--unset", "merge")  # nothing of its own: nothing to do
        p = self.bp("config", "--unset", "nope", ok=False)
        self.assertIn("no setting nope", p.stderr)
        p = self.bp("config", "--unset", "prefix", ok=False)
        self.assertIn("prefix is this repository's own: change it with --prefix", p.stderr)
        self.assertEqual(self.cfg("--base", "dev")["base"], "dev")
        self.assertIsNone(self.cfg("--unset", "base")["base"])

    def test_global_refuses_what_only_a_repository_has(self):
        for flags in (["--prefix", "XY"], ["--base", "dev"], ["--news", "monitor"],
                      ["--board-others", "on", "--coordinator", "c1"]):
            p = self.bp("config", "--global", *flags, ok=False)
            self.assertIn("per repository, so not for the global config", p.stderr)
        p = self.bp("config", "--global", "--unset", "prefix", ok=False)
        self.assertIn("prefix is this repository's own: it can't be in the global config.json", p.stderr)
        self.assertIsNone(self.global_cfg())
        # one written by hand is ignored there
        path = self.tmp / "xdg" / "bullpen" / "config.json"
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps({"prefix": "XY", "merge": "squash"}))
        cfg = self.cfg()
        self.assertEqual((cfg["prefix"], cfg["merge"]), ("BP", "squash"))

    def test_new_flags_check_their_numbers(self):
        for flag in ("--max-running", "--review-rounds"):
            p = self.bp("config", flag, "0", ok=False)
            self.assertIn("1 or more", p.stderr)
            p = self.bp("config", "--global", flag, "0", ok=False)
            self.assertIn("1 or more", p.stderr)
        self.assertEqual(self.cfg("--max-running", "4")["max_running"], 4)

    def test_doctor_and_config_write_home_as_a_tilde(self):
        self.healthy()
        self.env["HOME"] = str(self.tmp)
        out = self.bp("doctor").stdout
        self.assertNotIn(str(self.tmp), out)
        self.assertIn("ok    state: ~/state is writable", out)
        out = self.bp("config").stdout
        self.assertNotIn(str(self.tmp), out)
        self.assertEqual(out.splitlines()[0], "repo:   ~/proj")
        self.assertIn("global: ~/xdg/bullpen/config.json (none yet)", out)
        self.assertIn("no task BP-9 in ~/proj: `bullpen ls --all` lists them", self.bp("show", "BP-9", ok=False).stderr)

    def test_config_lists_the_settings_in_groups(self):
        out = self.bp("config").stdout
        titles = [l for l in out.splitlines() if l and not l.startswith(" ") and ":" not in l]
        self.assertEqual(titles, ["Tasks", "Agents", "Review", "Board", "News"])  # no Other: each has a group
        self.assertIn("Board\n  board_layout      grouped  default\n", out)
        self.assertRegex(out, r"(?m)^  worker +claude --permission-mode auto +default$")

    def test_global_config_outside_a_repository(self):
        elsewhere = self.tmp / "elsewhere"
        elsewhere.mkdir()
        p = self.bp("config", "--global", "--notify", "all", cwd=elsewhere)
        self.assertIn("none: not in a git repository", p.stdout)
        self.assertRegex(p.stdout, r"(?m)^  notify +all +global$")
        self.assertNotRegex(p.stdout, r"(?m)^ *prefix")
        self.assertEqual(self.global_cfg(), {"notify": "all"})

    def test_global_stuck_after_reaches_the_panes_the_watcher_reads(self):
        tid = self.start_task()
        entry = self.tmp / "state" / "blocks" / f"{self.block(tid)}.json"
        elsewhere = self.tmp / "elsewhere"
        elsewhere.mkdir()
        self.bp("config", "--global", "--stuck-after", "45", cwd=elsewhere)
        self.assertEqual(json.loads(entry.read_text())["stuck_after"], 45)

    def test_a_config_from_before_the_global_one_keeps_only_what_isnt_the_default(self):
        self.bp("up")
        path = next((self.tmp / "state" / "repos").iterdir()) / "config.json"
        old = json.loads(path.read_text())
        old.pop("layered")
        for k in ("merge", "review_rounds", "notify", "max_running"):  # every setting, as bullpen wrote them
            old[k] = {"merge": "rebase", "review_rounds": 2, "notify": "needs", "max_running": 3}[k]
        old["stuck_after"] = 45
        path.write_text(json.dumps(old))
        self.bp("config", "--global", "--merge", "squash")
        cfg = self.cfg()
        self.assertEqual((cfg["merge"], cfg["sources"]["merge"]), ("squash", "global"))
        self.assertEqual((cfg["stuck_after"], cfg["sources"]["stuck_after"]), (45, "repo"))
        own = json.loads(path.read_text())
        self.assertTrue(own["layered"])
        for k in ("merge", "review_rounds", "notify", "max_running"):
            self.assertNotIn(k, own)
        self.assertEqual(len(cfg["coordinators"]), 1)

    def test_doctor_checks_the_global_config(self):
        self.healthy()
        _, lines = self.doctor()
        self.assertEqual(lines["global"][0], "--")
        self.bp("config", "--global", "--merge", "squash")
        p, lines = self.doctor()
        self.assertEqual(lines["global"][0], "ok", p.stdout)
        path = self.tmp / "xdg" / "bullpen" / "config.json"
        path.write_text(json.dumps({"prefix": "XY"}))
        _, lines = self.doctor()
        self.assertEqual(lines["global"][0], "FAIL")
        self.assertIn("prefix, which only a repository can have", lines["global"][1])
        path.write_text("{oops")
        p, lines = self.doctor()
        self.assertEqual(p.returncode, 1)
        self.assertEqual(lines["global"][0], "FAIL")
        self.assertIn("doesn't parse", lines["global"][1])
        self.assertEqual(lines["setup"], ("--", "can't tell: the global config.json doesn't parse"))
        p = self.bp("ls", ok=False)
        self.assertIn(f"{path} doesn't parse", p.stderr)

    def test_doctor_on_an_unwritable_state_folder(self):
        if os.geteuid() == 0:
            self.skipTest("root writes anywhere")
        self.healthy()
        state = self.tmp / "state"
        state.chmod(0o555)
        self.addCleanup(state.chmod, 0o755)
        _, lines = self.doctor()
        self.assertEqual(lines["state"][0], "FAIL")
        self.assertIn(f"can't write to {state}", lines["state"][1])

    def test_doctor_outside_a_repository(self):
        self.healthy()
        elsewhere = self.tmp / "elsewhere"
        elsewhere.mkdir()
        p, lines = self.doctor(cwd=elsewhere)
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
        self.assertEqual(lines["setup"][0], "--")

    def test_doctor_checks_the_setup_hook(self):
        self.healthy()
        self.commit_hook("setup", "#!/bin/sh\nnpm ci\n")
        self.assertEqual(self.doctor()[1]["setup"], ("ok", ".bullpen/setup.sh on origin/main"))
        self.commit_hook("setup", "#!/usr/bin/env -S sh -e\nnpm ci\n")  # env's options aren't the program
        self.assertEqual(self.doctor()[1]["setup"], ("ok", ".bullpen/setup.sh on origin/main"))
        self.commit_hook("setup", "#!/usr/bin/env -S sh -e\nif true; then\n")  # and sh still checks it
        self.assertIn("isn't valid sh", self.doctor()[1]["setup"][1])
        self.commit_hook("setup", "#!/usr/bin/env no-such-shell\necho hi\n")
        line = self.doctor()[1]["setup"]
        self.assertEqual(line[0], "FAIL")
        self.assertIn("runs under no-such-shell", line[1])
        self.commit_hook("setup", "if true; then\n")  # no #!: sh runs it, and it doesn't parse
        line = self.doctor()[1]["setup"]
        self.assertEqual(line[0], "FAIL")
        self.assertIn("isn't valid sh", line[1])
        (self.repo / ".bullpen" / "teardown.sh").write_text("echo bye\n")  # never committed
        line = self.doctor()[1]["teardown"]
        self.assertEqual(line[0], "FAIL")
        self.assertIn("isn't committed on origin/main", line[1])

    def test_doctor_checks_the_setup_command(self):
        self.healthy()
        self.bp("config", "--setup", "cd . && echo hi")
        self.assertEqual(self.doctor()[1]["setup"], ("ok", "config.json's setup: cd . && echo hi"))
        self.bp("config", "--setup", "no-such-tool install")
        line = self.doctor()[1]["setup"]
        self.assertEqual(line[0], "FAIL")
        self.assertIn("runs no-such-tool, which isn't on PATH", line[1])

    def test_up_hints_at_doctor_without_its_slow_checks(self):
        self.healthy()
        (self.fake / "gh_signed_out").write_text("")  # gh needs the network: up doesn't ask
        self.assertNotIn("problem", self.bp("up").stdout)
        self.env["BULLPEN_BOARD"] = str(self.tmp / "missing")
        (self.fake / "actions.json").write_text(json.dumps({"actions": []}))
        out = self.bp("up").stdout.splitlines()
        self.assertEqual(out[-1], "2 problems: run `bullpen doctor`")

    # ---------- one table of settings ----------

    def test_rebase_others_and_gh_have_flags(self):
        cfg = self.cfg("--rebase-others", "off", "--gh", "/opt/gh/bin/gh")
        self.assertEqual((cfg["rebase_others"], cfg["gh"]), (False, "/opt/gh/bin/gh"))
        self.assertEqual(cfg["sources"]["gh"], "repo")
        cfg = self.cfg("--gh", "")  # "": back to the gh on PATH
        self.assertIsNone(cfg["gh"])
        self.bp("config", "--global", "--rebase-others", "off")
        self.assertEqual(self.global_cfg(), {"rebase_others": False})

    def test_config_help_states_the_defaults(self):
        out = self.bp("config", "--help").stdout
        out = " ".join(out.split())
        for says in ("(default 3)", "(default 40)", "(default sonnet)", "(default rebase)",
                     "(default on)", "(default off)", "(default 20)", "(default needs)", "(default background)",
                     "(default 1 while workers or reviewers float, else 0)"):
            self.assertIn(says, out)
        self.assertNotIn("--coordinators", out)  # the command lists and the coordinators are file-only
        self.assertNotIn("--worker ", out)


class SettingsCopiesTest(unittest.TestCase):
    """The board and the watcher keep their own short copies of some settings: they
    must agree with the table in bullpen/core.py."""

    @classmethod
    def setUpClass(cls):
        from bullpen import core
        cls.bp = core

    def test_the_boards_repo_only_settings_agree(self):
        src = (ROOT / "board" / "src" / "model" / "config.rs").read_text()
        m = re.search(r"const REPO_ONLY: \[&str; \d+\] = \[([^\]]*)\]", src)
        self.assertEqual(sorted(re.findall(r'"(\w+)"', m.group(1))), sorted(self.bp.REPO_ONLY))

    def test_the_boards_defaults_agree(self):
        src = (ROOT / "board" / "src" / "model" / "config.rs").read_text()
        body = src[src.index("pub fn from_value"):]
        body = body[:body.index("\n    }\n")]
        found = re.findall(r'v\["(\w+)"\]\.as_(\w+)\(\)\.unwrap_or\(([^)]*)\)', body)
        self.assertGreaterEqual(len(found), 7)
        for key, kind, default in found:
            want = self.bp.DEFAULT_CONFIG[key]
            got = {"bool": lambda s: s == "true", "str": lambda s: json.loads(s),
                   "u64": int}[kind](default)
            self.assertEqual(got, want, key)

    def test_the_watchers_stuck_after_agrees(self):
        src = (ROOT / "lua" / "watch.lua").read_text()
        m = re.search(r'"stuck_after":%s\*\(%d\+\)\'\)\) or (\d+)', src)
        self.assertEqual(int(m.group(1)), self.bp.DEFAULT_CONFIG["stuck_after"])
