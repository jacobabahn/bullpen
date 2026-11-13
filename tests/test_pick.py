"""Going to a task: go, next and the picker."""

import json
import os
import select
import shutil
import subprocess
import time
from pathlib import Path

from support import setUpModule, tearDownModule, BULLPEN, sh, BullpenTest  # a temp folder for each module


class PickTest(BullpenTest):
    def test_pick_filter_lists_open_tasks_of_every_repo_best_first(self):
        self.bp("add", "Fix the login redirect", "--brief", "x")
        self.bp("add", "Cover logout", "--brief", "x")
        self.bp("add", "Gone soon", "--brief", "x")
        self.bp("drop", "BP-3")  # closed: not listed
        shop = self.other_repo("shop")
        self.bp("add", "Sell it", "--brief", "x", cwd=shop)
        pick = lambda q, cwd=None: self.bp("pick", "--filter", q, "--from", "nowhere", cwd=cwd).stdout.splitlines()
        self.assertEqual(pick(""), [  # this repository's first, with each one's name
            "proj  ◌ BP-1  Fix the login redirect",  # the board's mark, not its words
            "proj  ◌ BP-2  Cover logout",
            "shop  ◌ BP-1  Sell it"])
        self.assertEqual([l.split()[:3] for l in pick("", cwd=shop)][0], ["shop", "◌", "BP-1"])  # from shop: shop's first
        self.assertEqual(pick("lgn"), [pick("")[0]])  # letters in order, not together
        self.assertEqual(pick("shop bp1"), [pick("")[2]])  # every word must match
        self.assertEqual(pick("p-2"), [pick("")[1]])
        self.assertEqual(pick("zzz"), [])
        shutil.rmtree(shop)  # a repository whose checkout is gone is left out, and with one repo, no names
        self.assertEqual(pick("")[0], "◌ BP-1  Fix the login redirect")

    def test_pick_lists_what_needs_you_then_what_is_ready_first(self):
        for title in ("Migrate sessions table", "Emails: digest", "Rate-limit resets", "Docs", "Logs"):
            self.bp("add", title, "--brief", "x")
        d = next((self.tmp / "state").rglob("config.json")).parent
        for tid, status in {"BP-1": "working", "BP-2": "ready", "BP-3": "triage", "BP-4": "stopped",
                            "BP-5": "needs"}.items():
            f = d / "tasks" / f"{tid}.json"
            f.write_text(json.dumps({**json.loads(f.read_text()), "status": status}))
        pick = lambda q: [l.split()[1] for l in self.bp("pick", "--filter", q, "--from", "nowhere").stdout.splitlines()]
        self.assertEqual(pick(""), ["BP-4", "BP-5", "BP-2", "BP-3", "BP-1"])  # triage is the coordinator's
        self.assertEqual(pick("digest"), ["BP-2"])  # not "miGrate sEssionS Table"
        self.assertEqual(pick("rl"), ["BP-3"])  # in one word: "Rate-limit"
        self.assertEqual(pick("lgs"), ["BP-5"])

    def test_go_board_with_a_task_selects_it_on_its_board(self):
        self.start_task()
        self.bp("add", "Later", "--brief", "x")
        sel = next((self.tmp / "state").rglob("config.json")).parent / "select" / "c1"
        self.bp("go", "BP-2", "--board", "--json")
        self.assertFalse(sel.exists())  # --json only says where
        self.bp("go", "bp-2", "--board")
        self.assertEqual(self.focused()[-1], "blk2")
        self.assertEqual(sel.read_text(), "BP-2\n")

    def test_go_board_with_a_task_uses_a_board_that_shows_it(self):
        other = self.two_coordinators()
        board1 = self.coordinators()[0]["board"]["block"]
        theirs = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        f = next((self.tmp / "state").rglob("config.json"))
        cfg = json.loads(f.read_text())
        cfg["coordinators"][1]["board"] = None  # c2 has no board
        f.write_text(json.dumps(cfg))
        sel = f.parent / "select"
        p = self.bp("go", theirs, "--board", ok=False)  # c1's board hides c2's tasks
        self.assertIn(f"no board shows {theirs}: it is c2's, which has no board (o on a board shows it)", p.stderr)
        self.assertFalse(sel.exists())
        self.bp("config", "--board-others", "on", "--coordinator", "c1")  # now c1's shows it
        self.bp("go", theirs, "--board")
        self.assertEqual(self.focused()[-1], board1)
        self.assertEqual((sel / "c1").read_text(), f"{theirs}\n")

    def test_go_repo_goes_to_a_task_of_another_repo(self):
        here = self.start_task("Here")
        shop = self.other_repo("shop")
        self.bp("up", cwd=shop)
        self.bp("add", "Sell it", "--brief", "x", cwd=shop)
        self.bp("start", "BP-1", cwd=shop)
        theirs = json.loads(self.bp("show", "BP-1", "--json", cwd=shop).stdout)["block"]
        go = lambda *a: json.loads(self.bp("go", "BP-1", "--json", "--from", self.block(here), *a).stdout)
        self.assertEqual(go()["block"], self.block(here))  # the same ID, this pane's repository
        self.assertEqual(go("--repo", str(shop))["block"], theirs)
        p = self.bp("go", "BP-1", "--repo", str(self.tmp), ok=False)
        self.assertIn(f"bullpen has no tasks for {self.tmp}", p.stderr)

    def test_pick_pane_opens_the_picker_over_the_pane_you_are_in(self):
        self.start_task()
        self.bp("pick", "--pane", "--from", "blk1", "--session", "session:1", cwd=self.tmp)
        layer = self.api("session.new_layer")[-1]
        block = layer["layout"]["block"]
        self.assertEqual(block["label"], "bullpen: jump")
        self.assertEqual(block["options"]["command"][-5:], ["pick", "--from", "blk1", "--session", "session:1"])
        self.assertEqual(layer["window_id"], "w1")  # the coordinator's window
        lid = json.loads((self.fake / "reg.json").read_text())["blocks"]["blk5"]["layer"]
        self.assertEqual(self.api("session.set_layer_state")[-1], {"layer_id": lid, "state": "normal", "focus": True})
        self.bp("pick", "--pane", "--from", "blk1", "--session", "session:1")  # again: one picker at a time
        self.assertFalse((self.fake / "alive" / "blk5").exists())
        self.assertTrue((self.fake / "alive" / "blk6").exists())

    def pick_on_a_terminal(self, keys, cwd=None):
        """Run `bullpen pick` on a terminal of its own, type keys, and return how it exited."""
        import pty
        master, slave = pty.openpty()
        p = subprocess.Popen([str(BULLPEN), "pick", "--from", "nowhere"], cwd=cwd or self.repo, env=self.env,
                             stdin=slave, stdout=slave, stderr=slave)
        os.close(slave)
        seen = b""
        for k in keys:
            while not seen.endswith(b"H") or b"jump to>" not in seen:  # drawn: ready for a key
                r, _, _ = select.select([master], [], [], 10)
                self.assertTrue(r, f"the picker drew nothing: {seen!r}")
                seen += os.read(master, 65536)
            seen = b""
            os.write(master, k.encode())
            time.sleep(0.1)  # an arrow's bytes, not esc then letters
        try:
            while select.select([master], [], [], 10)[0] and os.read(master, 65536):
                pass
        except OSError:
            pass  # the terminal closed with the picker
        os.close(master)
        return p.wait(10)

    def test_pick_goes_to_the_task_you_choose(self):
        self.start_task("Here")
        shop = self.other_repo("shop")
        self.bp("up", cwd=shop)
        self.bp("add", "Sell it", "--brief", "x", cwd=shop)
        self.bp("start", "BP-1", cwd=shop)
        theirs = json.loads(self.bp("show", "BP-1", "--json", cwd=shop).stdout)["block"]
        lid = lambda b: json.loads((self.fake / "reg.json").read_text())["blocks"][b]["layer"]
        self.assertEqual(self.pick_on_a_terminal(["s", "e", "l", "\r"]), 0)
        self.assertEqual(self.api("session.set_layer_state")[-1], {"layer_id": lid(theirs), "state": "normal", "focus": True})
        before = len(self.api("session.set_layer_state"))
        self.assertEqual(self.pick_on_a_terminal(["\x1b[B", "\x1b[A", "\r"]), 0)  # down and up again: the first
        self.assertEqual(self.api("session.set_layer_state")[-1]["layer_id"], lid(self.block("BP-1")))
        before = len(self.api("session.set_layer_state"))
        self.assertEqual(self.pick_on_a_terminal(["x", "\x1b"]), 0)  # esc: nothing happens
        self.assertEqual(len(self.api("session.set_layer_state")), before)

    def test_pick_opens_the_board_on_a_task_with_no_pane(self):
        self.start_task("Here")
        self.bp("add", "Queued", "--brief", "x")
        self.assertEqual(self.pick_on_a_terminal(["q", "u", "e", "\r"]), 0)  # it closes: not a dead end
        self.assertEqual(self.focused()[-1], "blk2")  # the board
        sel = next((self.tmp / "state").rglob("config.json")).parent / "select" / "c1"
        self.assertEqual(sel.read_text(), "BP-2\n")  # with BP-2 selected

    def test_board_repos_is_a_setting_global_too(self):
        cfg = lambda: json.loads(self.bp("config", "--json").stdout)
        self.assertEqual((cfg()["board_repos"], cfg()["sources"]["board_repos"]), (True, "default"))
        self.bp("config", "--global", "--board-repos", "off")
        self.assertEqual((cfg()["board_repos"], cfg()["sources"]["board_repos"]), (False, "global"))
        self.bp("config", "--board-repos", "on")
        self.assertEqual((cfg()["board_repos"], cfg()["sources"]["board_repos"]), (True, "repo"))

    def change_things(self, tid):
        """A committed new file, an uncommitted edit and an untracked file."""
        wt = self.show(tid)["worktree"]
        (Path(wt) / "fix.txt").write_text("one\ntwo\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        (Path(wt) / "README").write_text("changed\n")
        (Path(wt) / "scratch.txt").write_text("x\n")
        return wt

    def test_diff_stat(self):
        tid = self.start_task()
        self.assertEqual(self.bp("diff", tid, "--stat").stdout.strip(), "BP-1 has no commits against origin/main.")
        wt = self.change_things(tid)
        sh("git", "-C", str(self.repo), "commit", "-q", "--allow-empty", "-m", "Later on main")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")
        sh("git", "-C", wt, "fetch", "-q", "origin")  # the base moved on: only the task's changes show
        out = self.bp("diff", tid, "--stat").stdout
        committed, loose = out.split("\n\n")
        self.assertIn("fix.txt | 2 ++", committed)
        self.assertIn("1 file changed, 2 insertions(+)", committed)
        self.assertNotIn("README", committed)
        self.assertTrue(loose.startswith("Uncommitted"))
        self.assertIn("README | 2 +-", loose)
        self.assertIn("untracked: scratch.txt", loose)
        self.assertEqual(self.rex_calls()[-1][0], "api")  # no pane, no pager: nothing new in Rex

    def test_diff_prints_the_changes_when_not_on_a_terminal(self):
        tid = self.start_task()
        self.change_things(tid)
        out = self.bp("diff", tid).stdout
        self.assertIn("+++ b/fix.txt", out)
        self.assertIn("Uncommitted (not in any commit yet):", out)
        self.assertIn("+changed", out)
        self.assertIn("untracked: scratch.txt", out)
        self.assertNotIn("\x1b[", out)  # no colour codes into a file or pipe

    def test_diff_of_a_task_without_a_worktree(self):
        self.bp("up")
        tid = self.bp("add", "Later", "--brief", "x").stdout.strip()
        for args in ((), ("--stat",), ("--pane",)):
            p = self.bp("diff", tid, *args, ok=False)
            self.assertEqual(p.returncode, 1)
            self.assertIn("BP-1 is queued (todo), with no worktree", p.stderr)
        self.assertEqual(self.api("session.new_layer"), [])

    def test_diff_pane_opens_over_the_board_and_goes_back_after(self):
        tid = self.start_task()  # its worker is the capsule lyr1
        wt = self.show(tid)["worktree"]
        self.bp("go", tid)  # the worker is up: the diff puts it away
        self.bp("diff", tid, "--pane")
        layer = self.api("session.new_layer")[-1]
        block = layer["layout"]["block"]
        self.assertEqual(block["label"], "bullpen: BP-1 diff")
        self.assertEqual(block["options"]["cwd"], wt)
        self.assertEqual(block["options"]["command"][-3:], ["diff", "BP-1", "--back"])
        self.assertEqual(layer["window_id"], "w1")
        self.assertEqual(self.layer_states(), {"lyr1": "minimized", "lyr2": "normal"})
        self.assertTrue(self.api("session.set_layer_state")[-1].get("focus"))
        self.bp("diff", tid, "--pane")  # again: the old diff pane closes first
        self.assertFalse((self.fake / "alive" / "blk5").exists())
        self.bp("diff", tid, "--back")  # what the pane runs: the board after the pager
        self.assertEqual(self.focused()[-1], "blk2")

    def test_ready_records_the_size_of_the_change(self):
        tid = self.start_task()
        self.change_things(tid)
        wt = self.show(tid)["worktree"]
        (Path(wt) / "README").write_text("hi\n")
        (Path(wt) / "scratch.txt").unlink()
        self.bp("ready", tid, cwd=wt)
        self.assertEqual(self.show(tid)["diffstat"], "1 file, +2 −0")
        self.assertIn("diffstat: 1 file, +2 −0", self.bp("show", tid).stdout)

    def test_board_runs_the_rust_board_on_this_repos_state(self):
        fake = self.fake / "board"
        fake.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\"\n")
        fake.chmod(0o755)
        self.env["BULLPEN_BOARD"] = str(fake)
        args = self.bp("board").stdout.splitlines()
        cfg = json.loads(self.bp("config", "--json").stdout)
        self.assertEqual(args[:4], ["--dir", cfg["state"], "--root", str(self.repo)])
        self.assertEqual(args[4], "--bullpen")

    def test_board_says_when_it_isnt_built(self):
        self.env["BULLPEN_BOARD"] = str(self.tmp / "missing")
        p = self.bp("board", ok=False)
        self.assertIn("isn't built", p.stderr)

    def test_board_layout_is_a_setting(self):
        self.bp("config", "--board-layout", "compact")
        self.assertEqual(json.loads(self.bp("config", "--json").stdout)["board_layout"], "compact")

    def test_board_bottom_gap_is_a_setting(self):
        self.assertEqual(json.loads(self.bp("config", "--json").stdout)["board_bottom_gap"], 1)
        self.bp("config", "--board-bottom-gap", "0")
        self.assertEqual(json.loads(self.bp("config", "--json").stdout)["board_bottom_gap"], 0)
        p = self.bp("config", "--board-bottom-gap", "-1", ok=False)
        self.assertIn("takes rows", p.stderr)

    def test_peek_vt_keeps_colours_and_drops_blank_coloured_lines(self):
        tid = self.start_task()
        (self.fake / "screen").write_text("\x1b[0m\x1b[31mred\x1b[0m\r\n\x1b[0mplain\n\x1b[0m\r\n\n")
        out = self.bp("peek", tid, "--vt").stdout
        self.assertEqual(out, "\x1b[0m\x1b[31mred\x1b[0m\n\x1b[0mplain\n")
        self.assertIn("--format=vt", [a for a in self.rex_calls() if a[0] == "capture"][-1])
        # Rex writes a run of spaces as a cursor move: the board would run the words together
        (self.fake / "screen").write_text("\x1b[3C\x1b[0m4.\x1b[1CChat\x1b[C\x1b[1mabout\r\n")
        self.assertEqual(self.bp("peek", tid, "--vt").stdout, "   \x1b[0m4. Chat \x1b[1mabout\n")

    def test_go_finds_the_repo_from_any_directory(self):
        tid = self.start_task()
        out = self.bp("go", tid, "--json", "--from", "nowhere", cwd=self.tmp).stdout
        self.assertEqual(json.loads(out)["block"], self.block(tid))
