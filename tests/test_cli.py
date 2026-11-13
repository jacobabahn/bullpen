"""Help, ls and doctor, and the code's layout."""

import ast
import json
import os
import shutil
import subprocess
import sys
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
    """bin/bullpen, the Rust entry (target/bullpen) and its hand-over to the Python (docs/code.md)."""

    def strict(self, *args):
        """bullpen run with the hand-over to Python made a failure (as cli/check/compare.sh runs it)."""
        return subprocess.run([str(BULLPEN), *args], cwd=self.repo, env=dict(self.env, BULLPEN_RUST_STRICT="1"),
                              text=True, capture_output=True)

    def test_the_table_is_the_commands_python_declares(self):
        from bullpen import cli
        table = json.loads(self.bp("_commands").stdout)
        self.assertEqual([c["name"] for c in table if c["owner"] == "python"], list(cli.COMMANDS))
        sub = next(a for a in cli.parser()._actions if a.choices)
        helps = {a.dest: a.help for a in sub._choices_actions}
        for c in table:
            if c["owner"] == "python":
                self.assertEqual((c["group"], c["help"] or None),
                                 (sub.choices[c["name"]].group, helps.get(c["name"])), c["name"])
        self.assertEqual({c["name"]: c["owner"] for c in table if c["owner"] != "python"},
                         {"build-board": "alias of build", "_nop": "rust", "_commands": "rust"})

    def test_help_and_an_unknown_command_come_from_rust(self):
        p = self.strict("--help")
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertTrue(p.stdout.startswith("usage: bullpen COMMAND [ARGS]\n"))
        p = self.strict("acept")
        self.assertEqual((p.returncode, p.stderr), (2, "bullpen: no command `acept`. Did you mean `accept`? "
                                                       "`bullpen --help` lists them.\n"))
        self.assertEqual(self.strict("_nop").returncode, 0)
        p = self.strict("ls")  # Python's: handed over, which strict mode refuses
        self.assertEqual((p.returncode, p.stderr), (3, "bullpen-rust: not in Rust: ls\n"))

    def test_a_python_command_reaches_python_whole(self):
        self.assertEqual(json.loads(self.bp("ls", "--js").stdout), [])  # argparse's prefix rules
        p = self.bp("ls", "--al", ok=False)
        self.assertEqual(p.stderr.splitlines()[-1], "bullpen ls: error: ambiguous option: --al could match --all, --all-repos")
        p = self.bp("ls", "--bogus", ok=False)
        self.assertEqual(p.returncode, 2)
        self.assertEqual(p.stderr.splitlines()[-1], "bullpen: error: unrecognized arguments: --bogus")  # argparse says it from the top
        # the old name of build is build, as Python owns it
        self.assertTrue(self.bp("build-board", "--help").stdout.startswith("usage: bullpen build [-h]"))

    def test_python_says_when_a_command_isnt_in_this_build(self):
        # an older build hands Python a command Python no longer has (here, one Rust owns)
        p = subprocess.run(["python3", "-S", str(ROOT / "bin" / "bullpen-py"), "_nop"], cwd=self.repo, env=self.env,
                           text=True, capture_output=True)
        self.assertEqual(p.returncode, 2)
        self.assertEqual(p.stderr, "bullpen: `_nop` isn't in this build of bullpen: rebuild it with `bullpen build`\n")

    def test_without_a_build_bin_bullpen_runs_the_python(self):
        home = self.tmp / "bullpen"
        for d in ("bin", "bullpen"):
            shutil.copytree(ROOT / d, home / d, ignore=shutil.ignore_patterns("__pycache__"))
        run = lambda *a: subprocess.run([str(home / "bin" / "bullpen"), *a], cwd=self.repo, env=self.env,
                                        text=True, capture_output=True)
        p = run("ls", "--json")
        self.assertEqual((p.returncode, p.stdout), (0, "[]\n"))
        p = run("--help")
        self.assertEqual(p.returncode, 2)
        self.assertEqual(p.stderr, f"bullpen: bullpen isn't built: run `{home / 'bin' / 'bullpen'} build` "
                                   f"(it needs cargo)\n")


class CodeTest(unittest.TestCase):
    """bin/bullpen and the bullpen package (docs/code.md)."""

    def test_bin_bullpen_finds_its_checkout_through_a_link(self):
        with tempfile.TemporaryDirectory() as d:
            for link in (os.path.join(d, "bullpen"), os.path.join(d, "bin", "bullpen")):  # install.sh's, in a bin/
                os.makedirs(os.path.dirname(link), exist_ok=True)
                os.symlink(BULLPEN, link)
                p = subprocess.run([link, "--help"], cwd=d, text=True, capture_output=True)
                self.assertEqual(p.returncode, 0, p.stderr)
                self.assertIn("usage: bullpen COMMAND", p.stdout)

    def test_the_entry_scripts_stay_small(self):
        # sh forks nothing on the usual path; Python compiles the script it runs on every
        # call, but caches a package's bytecode
        self.assertLess(len(BULLPEN.read_text().splitlines()), 10)
        self.assertLess(len((ROOT / "bin" / "bullpen-py").read_text().splitlines()), 20)

    def test_each_module_stays_under_a_thousand_lines(self):
        files = [*(ROOT / "bullpen").glob("*.py"), *(ROOT / "core").rglob("*.rs"), *(ROOT / "cli").rglob("*.rs")]
        long = {str(f.relative_to(ROOT)): n for f in sorted(files) if (n := len(f.read_text().splitlines())) >= 1000}
        self.assertEqual(long, {})

    def modules(self):
        return {f.stem: ast.parse(f.read_text()) for f in sorted((ROOT / "bullpen").glob("*.py"))}

    def test_the_modules_import_each_other_without_a_loop(self):
        # each module imports the ones below it, at the top: none inside a function
        deps = {}
        for name, tree in self.modules().items():
            deps[name] = {n.module for n in tree.body if isinstance(n, ast.ImportFrom) and n.level == 1
                          and n.module}
            inner = [n.lineno for f in tree.body if isinstance(f, (ast.FunctionDef, ast.ClassDef))
                     for n in ast.walk(f) if isinstance(n, ast.ImportFrom) and n.level == 1]
            self.assertEqual(inner, [], f"{name}.py imports a module of bullpen inside a function")
        done, path = set(), []

        def visit(name):
            self.assertNotIn(name, path, "an import loop: " + " -> ".join(path + [name]))
            if name in done:
                return
            path.append(name)
            for d in deps.get(name, ()):
                visit(d)
            path.pop()
            done.add(name)

        for name in deps:
            visit(name)

    def test_only_the_parser_calls_a_command_function(self):
        # other code calls the plain function a command runs, never cmd_...(Namespace)
        for name, tree in self.modules().items():
            calls = [n.func.id for n in ast.walk(tree) if isinstance(n, ast.Call)
                     and isinstance(n.func, ast.Name) and n.func.id.startswith("cmd_")]
            self.assertEqual(calls, [], f"{name}.py calls a command function")

    def test_saving_a_task_tells_the_person_and_marks_its_pr(self):
        from bullpen import cli, core, notifications, prs
        cli.load_savers()
        self.assertIn(notifications.notify, core.SAVED)
        self.assertIn(prs.follow_saved, core.SAVED)

    def test_the_save_hooks_run_in_one_order_whichever_module_loaded_first(self):
        from bullpen import cli, core, notifications, prs, start
        core.SAVED.reverse()
        cli.load_savers()
        self.assertEqual(core.SAVED[:3], [start.move_queue, notifications.notify, prs.follow_saved])

    def test_each_command_is_found_in_the_module_that_declares_it(self):
        # main imports only a command's module: COMMANDS says which, in --help's order
        from bullpen import cli
        declared = []
        for m in cli.MODULES:
            sub = next(a for a in cli.parser([m])._actions if a.choices)
            declared += [(n, m) for n in sub.choices]
        self.assertEqual(declared, list(cli.COMMANDS.items()))

    def test_a_command_imports_only_what_it_needs(self):
        code = ("import sys; sys.argv = ['bullpen', 'rules', '--help']\n"
                "from bullpen import cli\n"
                "try: cli.main()\n"
                "except SystemExit: pass\n"
                "print(' '.join(m for m in sys.modules if m.startswith('bullpen.')))")
        p = subprocess.run([sys.executable, "-c", code], cwd=ROOT, capture_output=True, text=True)
        loaded = set(p.stdout.split())
        self.assertIn("bullpen.rules", loaded)
        self.assertNotIn("bullpen.accept", loaded)
        self.assertNotIn("bullpen.board", loaded)

    def calls(self, name):
        """(module, function) for each call of name in the package."""
        out = []
        for mod, tree in self.modules().items():
            for f in ast.walk(tree):
                if isinstance(f, ast.FunctionDef):
                    out += [(mod, f.name) for n in ast.walk(f) if isinstance(n, ast.Call)
                            and getattr(n.func, "id", getattr(n.func, "attr", None)) == name]
        return out

    def test_the_run_queue_moves_only_from_a_status_change(self):
        # no caller has to remember it: saving a task that frees a slot starts the queue
        self.assertEqual(self.calls("start_queued"), [("start", "move_queue")])
        from bullpen import cli, core, start  # noqa: F401
        self.assertIn(start.move_queue, core.SAVED)

    def test_only_the_mode_adapters_push(self):
        # one way to push: quiet_git, which never waits on a prompt, in modes.py (and the
        # push remote's background push)
        pushes = []
        for mod, tree in self.modules().items():
            for n in ast.walk(tree):
                f = isinstance(n, ast.Call) and getattr(n.func, "id", getattr(n.func, "attr", None))
                if f in ("git", "quiet_git", "run") and any(
                        isinstance(a, ast.Constant) and a.value == "push" for x in n.args for a in ast.walk(x)):
                    pushes.append((mod, f))
        self.assertEqual({m for m, _ in pushes}, {"modes", "push_remote"})
        self.assertEqual({f for _, f in pushes}, {"quiet_git"})

    def test_close_accept_and_drop_share_one_close_step(self):
        self.assertEqual(sorted(f for m, f in self.calls("close") if m in ("accept", "close")),
                         ["accept", "close_task", "cmd_drop"])
        self.assertEqual(self.calls("remove_worktree"), [("close", "close")])
