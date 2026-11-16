"""Help, ls and doctor, and the code's layout."""

import json
import os
import re
import shutil
import subprocess
import tempfile
import unittest

from support import setUpModule, tearDownModule, ROOT, BULLPEN, sh, BullpenTest  # a temp folder for each module


class CliTest(BullpenTest):
    def test_help_leaves_out_the_internal_commands(self):
        out = self.bp("--help").stdout
        for name in ("_event", "_stuck", "_flush", "_prs", "_limit", "_gone", "_auto_accept"):
            self.assertNotIn(name, out)
        self.assertIn("\n  accept ", out)
        self.bp("_flush")  # still there for the watcher
        self.bp("_limit")
        self.bp("_prs")

    def test_help_lists_the_commands_in_groups(self):
        env = dict(self.env, COLUMNS="42")
        out = subprocess.run([str(BULLPEN), "--help"], cwd=self.repo, env=env, text=True,
                             capture_output=True).stdout
        lines = out.splitlines()
        self.assertEqual(lines[0], "usage: bullpen COMMAND [ARGS]")
        titles = [l for l in lines if l and not l.startswith(" ") and l.endswith(":")]
        self.assertEqual(titles, ["You:", "Coordinator:", "Worker and reviewer:", "Setup:"])
        self.assertTrue(all(len(l) <= 42 for l in lines), out)
        group = lambda name: next(t for t in reversed(titles) if out.index(t) < out.index(f"\n  {name} "))
        self.assertEqual(group("accept"), "You:")
        self.assertEqual(group("ask"), "Coordinator:")
        self.assertEqual(group("verdict"), "Worker and reviewer:")
        self.assertEqual(group("doctor"), "Setup:")
        self.assertNotIn("(coordinator)", out)  # the group says it
        self.assertIn("(reviewer)", out)  # a mixed group keeps who it's for

    def test_an_unknown_command_is_one_line_with_a_close_match(self):
        p = self.bp("acept", "BP-1", ok=False)
        self.assertEqual(p.returncode, 2)
        self.assertEqual(p.stdout, "")
        self.assertEqual(p.stderr, "bullpen: no command `acept`. Did you mean `accept`? "
                                   "`bullpen --help` lists them.\n")
        p = self.bp("frobnicate", ok=False)
        self.assertEqual(p.stderr, "bullpen: no command `frobnicate`. `bullpen --help` lists them.\n")
        p = self.bp("_evnt", ok=False)  # never offers an internal command
        self.assertEqual(p.stderr, "bullpen: no command `_evnt`. `bullpen --help` lists them.\n")
        p = self.bp("--bogus", ok=False)
        self.assertEqual(p.stderr, "bullpen: `--bogus` goes after a command. `bullpen --help` lists them.\n")
        p = self.bp(ok=False)  # nothing: the help
        self.assertEqual(p.returncode, 2)
        self.assertTrue(p.stdout.startswith("usage: bullpen COMMAND [ARGS]\n"))

    def test_ls_and_show_say_a_task_as_the_board_does(self):
        self.bp("up")
        self.bp("add", "Later", "--brief", "Then this.")
        tid = self.start_task()  # BP-2
        self.bp("_event", self.block(tid), "blocked", "permission", "Allow rm?")
        out = self.bp("ls").stdout.splitlines()  # piped: whole lines, no colour
        self.assertRegex(out[0], rf"^● {tid}  Fix the thing  now  .*Allow rm\?")  # needs you first
        self.assertEqual(out[1], "◌ BP-1  Later  now  Not started. s starts it.")
        self.assertNotIn("\x1b", "".join(out))
        self.assertEqual([t["id"] for t in json.loads(self.bp("ls", "--json").stdout)], ["BP-1", tid])
        self.assertEqual({t["status"] for t in json.loads(self.bp("ls", "--json").stdout)}, {"needs", "todo"})
        show = self.bp("show", tid).stdout.splitlines()
        self.assertEqual(show[0], out[0])
        self.assertNotIn("status:", "".join(show))
        self.assertTrue(show[-1].startswith(f"     next: `bullpen answer {tid} <option>`"), show[-1])
        self.assertEqual(self.bp("show", "BP-1").stdout.splitlines()[-1],
                         "     next: `bullpen start BP-1` (s on the board), or `bullpen drop BP-1` (d)")

    def test_doctor_says_all_good(self):
        self.healthy()
        p, lines = self.doctor()
        self.assertEqual(p.returncode, 0, p.stdout)
        self.assertEqual(p.stdout.splitlines()[-1], "All good.")
        for name in ("rex", "gh", "agents", "board", "keys", "watcher", "state", "config", "coordinators"):
            self.assertEqual(lines[name][0], "ok", (name, lines[name]))
        self.assertEqual(lines["setup"][0], "--")
        self.assertIn("0.1.0-preview.13", lines["rex"][1])
        self.assertIn("c1 (blk1)", lines["coordinators"][1])
        self.assertNotIn("problem", self.bp("up").stdout)

    def test_doctor_names_each_problem_and_its_fix(self):
        self.healthy()
        (self.fake / "server_version").write_text("0.1.0-preview.12")
        (self.fake / "actions.json").write_text(json.dumps({"actions": []}))
        (self.fake / "gh_signed_out").write_text("")
        self.env["BULLPEN_BOARD"] = str(self.tmp / "missing")
        cfg_path = next((self.tmp / "state").rglob("config.json"))
        cfg = json.loads(cfg_path.read_text())
        cfg["worker"] = ["no-such-agent"]
        cfg_path.write_text(json.dumps(cfg))
        (self.fake / "alive" / "blk3").unlink()  # the watcher
        (self.fake / "alive" / "blk2").unlink()  # c1's board
        p, lines = self.doctor()
        self.assertEqual(p.returncode, 1)
        self.assertEqual(p.stdout.splitlines()[-1],
                         "7 problems: rex, gh, agents, board, keys, watcher, coordinators.")
        fixes = {"rex": "preview.13 or later", "keys": "run `bullpen keys --install`", "gh": "gh auth login",
                 "board": "BULLPEN_BOARD", "agents": "no-such-agent isn't on PATH (config.json's worker)",
                 "watcher": "run `bullpen up`", "coordinators": "c1's board (blk2) is gone"}
        for name, fix in fixes.items():
            self.assertEqual(lines[name][0], "FAIL", (name, lines[name]))
            self.assertIn(fix, lines[name][1])

    def test_doctor_on_a_stale_watcher_or_gone_coordinator(self):
        self.healthy()
        w = self.tmp / "state" / "watcher.json"
        data = json.loads(w.read_text())
        data["stamp"]["watch"] = "old"
        w.write_text(json.dumps(data))
        (self.fake / "alive" / "blk1").unlink()  # the coordinator
        _, lines = self.doctor()
        self.assertIn("from an older watch.lua or bullpen", lines["watcher"][1])
        self.assertIn("c1's pane (blk1) is gone", lines["coordinators"][1])

    def test_a_board_from_an_older_build_is_caught_and_up_restarts_it(self):
        self.healthy()
        # c1's board (blk2) runs the program `bullpen board` runs now: it reloads by itself
        (self.fake / "prog_blk2").write_text(self.env["BULLPEN_BOARD"])
        _, lines = self.doctor()
        self.assertEqual(lines["coordinators"][0], "ok")
        # one from an older install's path never reloads
        (self.fake / "prog_blk2").write_text(str(self.tmp / "old" / "release" / "bullpen-board"))
        _, lines = self.doctor()
        self.assertEqual(lines["coordinators"],
                         ("FAIL", "c1's board is from an older build: `bullpen up` restarts it"))
        out = self.bp("up").stdout
        self.assertIn("Board restarted (it ran an older build).", out)
        self.assertFalse((self.fake / "alive" / "blk2").exists())
        self.assertEqual(json.loads(self.bp("config", "--json").stdout)["coordinators"][0]["board"]["block"],
                         "blk4")
        _, lines = self.doctor()
        self.assertEqual(lines["coordinators"][0], "ok")
        self.assertNotIn("board", self.bp("up").stdout)  # nothing to restart now

    def test_a_board_runs_bullpen_board_and_up_leaves_it_be(self):
        self.healthy()
        split = next(a for a in self.rex_calls() if a[0] == "split")
        self.assertIn(f"BULLPEN_BOARD={self.env['BULLPEN_BOARD']}", split)  # Rex panes don't inherit it
        (self.fake / "prog_blk2").write_text(self.env["BULLPEN_BOARD"])  # what that board then runs
        self.assertNotIn("board", self.bp("up").stdout)
        self.assertTrue((self.fake / "alive" / "blk2").exists())

    def test_doctor_says_to_reload_when_only_some_keys_are_loaded(self):
        self.healthy()
        (self.fake / "actions.json").write_text(json.dumps({"actions": [
            {"name": n, "source": "/x/lua/keys.lua:62"}
            for n in ("bullpen_next", "bullpen_board", "bullpen_coordinator")]}))
        _, lines = self.doctor()
        self.assertEqual(lines["keys"],
                         ("FAIL", "bullpen_jump missing: Rex loaded an older keys.lua. Run `rex config reload`"))

    def keys_install(self, ok=True):
        self.env.pop("REX_CONFIG", None)
        return self.bp("keys", "--install", ok=ok)

    def test_keys_install_appends_after_whats_there_and_reloads(self):
        init = self.tmp / "xdg" / "rex" / "init.lua"
        init.parent.mkdir(parents=True)
        first = 'do local r = rex.exec({ "rexurrect", "auto-restore" }) end\nrex.bind("cmd+k", "x")'
        init.write_text(first)  # no newline at the end
        p = self.keys_install()
        self.assertIn(f"added to the end of {init}", p.stdout)
        text = init.read_text()
        self.assertTrue(text.startswith(first + "\n\n"), text)  # what was there stays first, as it was
        self.assertEqual(text.splitlines()[-1],
                         f'dofile("{ROOT / "lua" / "keys.lua"}").setup{{ bin = "{BULLPEN}", '
                         f'state = "{self.tmp / "state"}" }}')
        self.assertIn(["config", "reload"], self.rex_calls())
        p = self.keys_install()  # again: nothing added, still reloaded
        self.assertIn("already in", p.stdout)
        self.assertEqual(init.read_text(), text)
        self.assertEqual(self.rex_calls().count(["config", "reload"]), 2)

    def test_keys_install_makes_the_config_and_honours_rex_config(self):
        init = self.tmp / "elsewhere" / "init.lua"
        self.env["REX_CONFIG"] = str(init)
        self.bp("keys", "--install")
        self.assertTrue(init.read_text().startswith("-- bullpen's keys"))
        self.assertFalse((self.tmp / "xdg" / "rex" / "init.lua").exists())

    def test_keys_install_leaves_another_keys_line_alone(self):
        init = self.tmp / "xdg" / "rex" / "init.lua"
        init.parent.mkdir(parents=True)
        init.write_text('dofile("/old/bullpen/lua/keys.lua").setup{ bin = "/old/bullpen/bin/bullpen" }\n')
        p = self.keys_install(ok=False)
        self.assertEqual(p.returncode, 1)
        self.assertIn("already loads another keys.lua (/old/bullpen/lua/keys.lua)", p.stderr)
        self.assertNotIn(["config", "reload"], self.rex_calls())
        self.assertEqual(len(init.read_text().splitlines()), 1)

    def test_keys_install_says_when_the_reload_fails(self):
        (self.fake / "reload_fails").write_text("")
        p = self.keys_install(ok=False)
        self.assertEqual(p.returncode, 1)
        self.assertIn("added to the end of", p.stdout)
        self.assertIn("`rex config reload` failed: rex: no server", p.stderr)

    def test_go_board_starts_a_board_when_none_runs(self):
        self.bp("up")
        (self.fake / "alive" / "blk2").unlink()  # c1's board, quit
        p = self.bp("go", "--board")
        self.assertIn("board started: blk4", p.stderr)
        self.assertEqual(self.focused(), ["blk4"])
        self.bp("go", "--board")  # running now: just shown
        self.assertEqual(self.focused(), ["blk4", "blk4"])

    def test_go_board_starts_one_for_a_coordinator_without_one(self):
        self.bp("up", "--no-board")  # the coordinator (blk1) and the watcher (blk2)
        self.bp("go", "--board")
        self.assertEqual(self.focused(), ["blk3"])
        self.assertEqual(json.loads(self.bp("config", "--json").stdout)["coordinators"][0]["board"]["block"],
                         "blk3")

    def test_doctor_when_rex_has_no_server(self):
        self.healthy()
        (self.fake / "server_down").write_text("")
        _, lines = self.doctor()
        self.assertEqual(lines["rex"][0], "FAIL")
        self.assertIn("rex server start", lines["rex"][1])

    def test_doctor_cant_tell_on_a_rex_that_doesnt_say(self):
        self.healthy()
        (self.fake / "server_version").unlink()
        (self.fake / "actions.json").unlink()
        p, lines = self.doctor()
        self.assertEqual((lines["rex"][0], lines["keys"][0]), ("--", "--"))
        self.assertEqual(p.returncode, 0)

    def test_doctor_in_local_mode_needs_no_gh(self):
        sh("git", "-C", str(self.repo), "remote", "remove", "origin")
        self.healthy()
        (self.fake / "gh_signed_out").write_text("")
        _, lines = self.doctor()
        self.assertEqual(lines["gh"], ("--", "local mode (no origin remote): gh isn't needed"))

    def test_doctor_on_a_config_that_doesnt_parse(self):
        self.healthy()
        next((self.tmp / "state").rglob("config.json")).write_text("{oops")
        p, lines = self.doctor()
        self.assertEqual(p.returncode, 1)
        self.assertEqual(lines["config"][0], "FAIL")
        self.assertIn("doesn't parse", lines["config"][1])
        for name in ("setup", "teardown", "coordinators"):
            self.assertEqual(lines[name], ("--", "can't tell: config.json doesn't parse"))


class EntryTest(BullpenTest):
    """bin/bullpen and the Rust entry (target/bullpen) it runs (docs/code.md)."""

    def call(self, *args):
        """bullpen, whatever its exit."""
        return subprocess.run([str(BULLPEN), *args], cwd=self.repo, env=self.env, text=True, capture_output=True)

    def test_the_table_lists_every_command(self):
        table = json.loads(self.bp("_commands").stdout)
        self.assertEqual({c["name"] for c in table},
                         {"_nop", "_commands", "go", "hide", "adopt", "board", "ls", "show", "diff", "add", "after",
                          "model", "review-mode", "peek", "ask", "ok", "note", "news", "guide", "up", "build",
                          "_build_board", "doctor", "_flush", "start", "resume", "tell", "say", "rules", "pick",
                          "config", "ready", "review", "verdict", "close", "drop", "finish", "_push", "answer",
                          "reply", "prompt", "check", "_event", "_stuck", "_limit", "_gone", "_coordinator_gone",
                          "_tour", "tour-comment", "accept", "_auto_accept", "keys", "_prs", "tour", "stats",
                          "repos", "later", "_pickup"})

    def test_help_and_an_unknown_command(self):
        p = self.call("--help")
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertTrue(p.stdout.startswith("usage: bullpen COMMAND [ARGS]\n"))
        p = self.call("acept")
        self.assertEqual((p.returncode, p.stderr), (2, "bullpen: no command `acept`. Did you mean `accept`? "
                                                       "`bullpen --help` lists them.\n"))
        p = self.call("build-board")  # build's old name, gone with the Python
        self.assertEqual((p.returncode, p.stderr), (2, "bullpen: no command `build-board`. Did you mean `build`? "
                                                       "`bullpen --help` lists them.\n"))
        self.assertEqual(self.call("_nop").returncode, 0)

    def test_config_and_pick_are_rusts_with_argparses_rules(self):
        self.assertIn("worker_model", json.loads(self.call("config", "--js").stdout))  # argparse's prefix rules
        p = self.call("config", "--reviewer-")
        self.assertEqual(p.stderr.splitlines()[-1], "bullpen config: error: ambiguous option: --reviewer- could match "
                                                    "--reviewer-model, --reviewer-followup-model")
        p = self.call("config", "--bogus")
        self.assertEqual(p.returncode, 2)
        self.assertEqual(p.stderr.splitlines()[-1], "bullpen: error: unrecognized arguments: --bogus")  # argparse says it from the top
        p = self.call("config", "--max-running", "x")
        self.assertEqual((p.returncode, p.stderr.splitlines()[-1]),
                         (2, "bullpen config: error: argument --max-running: invalid int value: 'x'"))
        self.assertEqual(self.call("config", "--max-running", " 4 ", "--json").returncode, 0)  # int()'s spaces
        p = self.call("config", "--merge", "fast")
        self.assertEqual(p.stderr.splitlines()[-1], "bullpen config: error: argument --merge: invalid choice: 'fast' "
                                                    "(choose from rebase, squash, merge)")
        p = self.call("pick")  # no terminal
        self.assertEqual((p.returncode, p.stderr),
                         (1, "bullpen: pick needs a terminal (--filter prints the matching tasks instead)\n"))
        self.assertEqual(self.call("pick", "--fil", "zzzz").returncode, 0)

    def test_ls_and_show_are_rusts_with_argparses_rules(self):
        self.assertEqual(json.loads(self.call("ls", "--js").stdout), [])
        p = self.call("ls", "--al")
        self.assertEqual((p.returncode, p.stderr.splitlines()[-1]),
                         (2, "bullpen ls: error: ambiguous option: --al could match --all, --all-repos"))
        p = self.call("ls", "--bogus")  # argparse says it from the top
        self.assertEqual((p.returncode, p.stderr),
                         (2, "usage: bullpen COMMAND [ARGS]\nbullpen: error: unrecognized arguments: --bogus\n"))
        p = self.call("show", "BP-9")
        self.assertEqual(p.returncode, 1)
        self.assertTrue(p.stderr.startswith("bullpen: no task BP-9 in "), p.stderr)

    def test_the_pane_commands_are_rusts_with_argparses_rules(self):
        tid = self.start_task()
        p = self.call("peek", tid, "--lines", "x")
        self.assertEqual((p.returncode, p.stderr.splitlines()[-1]),
                         (2, "bullpen peek: error: argument --lines: invalid int value: 'x'"))
        self.assertEqual(self.call("peek", tid, "--li", " 1 ").stdout, "line two\n")
        p = self.call("go", "--all-r")
        self.assertEqual((p.returncode, p.stderr), (1, "bullpen: --all-repos goes with --next\n"))
        self.assertEqual(self.call("hide", "--from", "nosuch").returncode, 3)  # not bullpen's: the key goes through
        p = self.call("adopt")
        self.assertEqual((p.returncode, p.stderr.splitlines()[-1]),
                         (2, "bullpen adopt: error: the following arguments are required: ID"))
        self.assertEqual(self.call("diff", tid, "--st").stdout, f"{tid} has no commits against origin/main.\n")

    def test_without_a_build_bin_bullpen_says_to_install(self):
        home = self.tmp / "bullpen"
        shutil.copytree(ROOT / "bin", home / "bin")
        for args in (["ls"], ["--help"]):
            p = subprocess.run([str(home / "bin" / "bullpen"), *args], cwd=self.repo, env=self.env, text=True,
                               capture_output=True)
            self.assertEqual((p.returncode, p.stdout), (1, ""))
            self.assertEqual(p.stderr, f"bullpen: bullpen isn't built: run {home / 'install.sh'} (it needs cargo)\n")


class CodeTest(unittest.TestCase):
    """bin/bullpen and the Rust code's layout (docs/code.md)."""

    def test_bin_bullpen_finds_its_checkout_through_a_link(self):
        with tempfile.TemporaryDirectory() as d:
            for link in (os.path.join(d, "bullpen"), os.path.join(d, "bin", "bullpen")):  # install.sh's, in a bin/
                os.makedirs(os.path.dirname(link), exist_ok=True)
                os.symlink(BULLPEN, link)
                p = subprocess.run([link, "--help"], cwd=d, text=True, capture_output=True)
                self.assertEqual(p.returncode, 0, p.stderr)
                self.assertIn("usage: bullpen COMMAND", p.stdout)

    def test_the_entry_script_stays_small(self):
        # sh forks nothing on the usual path
        self.assertLess(len(BULLPEN.read_text().splitlines()), 10)

    def test_each_module_stays_under_a_thousand_lines(self):
        files = [*(ROOT / "core").rglob("*.rs"), *(ROOT / "cli").rglob("*.rs")]
        long = {str(f.relative_to(ROOT)): n for f in sorted(files) if (n := len(f.read_text().splitlines())) >= 1000}
        self.assertEqual(long, {})

    def rust_calls(self, names):
        """(file, function called, its arguments' text) for each call of one of names in
        cli/src (not its tests; not a method or a path's, like libc::close)."""
        out = []
        for f in sorted((ROOT / "cli" / "src").rglob("*.rs")):
            if f.name.startswith("test_"):
                continue
            text = f.read_text()
            for m in re.finditer(r"(?<![\w:.])(%s)\(" % "|".join(map(re.escape, names)), text):
                if text[:m.start()].endswith("fn "):
                    continue  # its definition
                depth, i = 1, m.end()
                while depth and i < len(text):
                    depth += {"(": 1, ")": -1}.get(text[i], 0)
                    i += 1
                out.append((str(f.relative_to(ROOT / "cli" / "src")), m.group(1), text[m.end():i - 1]))
        return out

    def test_only_the_mode_adapters_push(self):
        # one way to push: quiet_git, which never waits on a prompt, in modes (and the
        # push remote's background push)
        calls = self.rust_calls(["git", "quiet_git", "quiet_git_in", "run", "run_in", "run_with", "run_input"])
        pushes = {(f, name) for f, name, args in calls if '"push"' in args}
        self.assertEqual({f for f, _ in pushes}, {"modes.rs", "push_remote.rs"})
        self.assertEqual({name for _, name in pushes}, {"quiet_git", "quiet_git_in"})
        self.assertNotIn('"push"', "".join(args for _, name, args in calls if name in ("git", "run_with")))

    def test_close_accept_and_drop_share_one_close_step(self):
        rust = [(f, args.split(",")[0]) for f, _, args in self.rust_calls(["close"])]
        # accept's, close_task's, drop's and close --keep-branch's
        self.assertEqual(rust, [("accept/mod.rs", "repo"), ("close.rs", "repo"), ("close.rs", "&repo"),
                                ("keep.rs", "repo")])
        self.assertEqual([f for f, _, _ in self.rust_calls(["remove_worktree"])], ["close.rs"])
