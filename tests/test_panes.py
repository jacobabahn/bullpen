"""Where a pane lives: bullpen/panes.py's placements, called as the commands call them."""

import json
import subprocess
import sys
import textwrap

from support import ROOT, BullpenTest, setUpModule, tearDownModule  # noqa: F401


class PanesTest(BullpenTest):
    def panes(self, code):
        """Run code with bullpen.panes imported as panes and the repo as repo, against the
        fake rex; what it prints, as JSON."""
        src = textwrap.dedent("""\
            import json, sys
            sys.path.insert(0, %r)
            from bullpen import panes
            from bullpen.core import Repo
            from pathlib import Path
            repo = Repo(Path(%r))
            """) % (str(ROOT), str(self.repo)) + textwrap.dedent(code)
        p = subprocess.run([sys.executable, "-c", src], cwd=self.repo, env=self.env,
                           text=True, capture_output=True)
        self.assertEqual(p.returncode, 0, p.stderr)
        return json.loads(p.stdout) if p.stdout.strip() else None

    def labels(self):
        return {b: i["label"] for b, i in json.loads((self.fake / "reg.json").read_text())["blocks"].items()}

    def test_float_places_a_capsule_in_the_coordinators_window_with_the_hint(self):
        tid = self.start_task()
        b = self.panes(f"""\
            t = repo.task({tid!r})
            print(json.dumps([panes.place(repo, t, "session:1", "bullpen: X", ["true"])[0],
                              panes.place(repo, t, "session:1", "bullpen: Y", ["true"], hint=False)[0]]))
            """)
        self.assertEqual([self.labels()[x] for x in b], ["bullpen: X · ctrl+esc hides", "bullpen: Y"])
        layers = self.api("session.new_layer")[-2:]
        self.assertEqual({l.get("window_id") for l in layers}, {"w1"})  # the coordinator's
        self.assertEqual([self.layer_states()[l] for l in ("lyr2", "lyr3")], ["minimized", "minimized"])

    def test_tabs_places_a_tab_without_the_hint(self):
        tid = self.start_task()
        self.bp("config", "--workers", "tabs")
        self.panes(f"""\
            t = repo.task({tid!r})
            panes.place(repo, t, "session:1", "bullpen: X", ["true"])
            panes.place(repo, t, "session:1", "bullpen: Y", ["true"], role="diff", focus=True)
            """)
        runs = [a for a in self.rex_calls() if a[0] == "run"][-2:]
        self.assertEqual([a[a.index("--label") + 1] for a in runs], ["bullpen: X", "bullpen: Y"])
        self.assertEqual([a[2] for a in runs], ["--focus=false", "--focus=true"])

    def test_hide_minimizes_a_float_and_refuses_anything_else(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("go", tid)
        got = self.panes(f"""\
            print(json.dumps([panes.hide(repo, "session:1", {blk!r}), panes.hide(repo, "session:1", "blk1"),
                              panes.hide(repo, "session:1", "nosuch")]))
            """)
        self.assertEqual(got, [True, False, False])  # a worker's capsule, the coordinator's tab, nothing
        self.assertEqual(self.api("session.set_layer_state")[-1]["state"], "minimized")

    def test_show_brings_one_float_up_and_puts_the_others_away(self):
        a = self.start_task()
        self.bp("add", "Another", "--brief", "B.")
        self.bp("start", "BP-2")
        self.bp("go", a)
        self.panes(f'panes.show(repo, "session:1", {self.block("BP-2")!r})\n')
        states = self.layer_states()
        layer = {b: i["layer"] for b, i in json.loads((self.fake / "reg.json").read_text())["blocks"].items()}
        self.assertEqual(states[layer[self.block(a)]], "minimized")
        self.assertEqual(states[layer[self.block("BP-2")]], "normal")
        self.assertTrue(self.api("session.set_layer_state")[-1].get("focus"))

    def test_show_focuses_a_pane_in_a_tab(self):
        self.bp("config", "--workers", "tabs")
        tid = self.start_task()
        self.panes(f'panes.show(repo, "session:1", {self.block(tid)!r})\n')
        self.assertEqual(self.rex_calls()[-1][0], "focus")

    def test_find_by_id_or_by_label_with_or_without_the_hint(self):
        tid = self.start_task()
        blk = self.block(tid)
        got = self.panes(f"""\
            from bullpen.rex import rex_view
            v = rex_view("session:1")
            print(json.dumps([(panes.find("session:1", v, block={blk!r}) or [None])[-1],
                              (panes.find("session:1", v, label="bullpen: {tid}") or [None])[-1],
                              panes.find("session:1", v, label="bullpen: nobody")]))
            """)
        self.assertEqual(got, [blk, blk, None])

    def test_gather_with_tabs_leaves_the_panes_alone(self):
        self.bp("config", "--workers", "tabs")
        self.start_task()
        before = len(self.rex_calls())
        self.panes('panes.gather(repo, "session:1")\n')
        self.assertEqual(self.rex_calls()[before:], [])

    def test_gather_brings_a_stray_float_back_as_a_capsule(self):
        tid = self.start_task()
        blk = self.block(tid)
        reg = self.fake / "reg.json"
        R = json.loads(reg.read_text())
        R["windows"].append("w9")
        R["blocks"][blk].update(layer=None, window="w9")  # moved to a tab of its own
        reg.write_text(json.dumps(R))
        self.panes('panes.gather(repo, "session:1")\n')
        R = json.loads(reg.read_text())
        self.assertEqual(R["blocks"][blk]["window"], "w1")
        self.assertEqual(R["layers"][R["blocks"][blk]["layer"]], "minimized")
        self.assertNotIn("w9", R["windows"])  # the tab it left was empty, so it closed


class BackgroundTest(PanesTest):
    """--workers background and --reviewers: detached panes that `go` floats over the
    coordinator's tab and the hide key detaches again."""

    def reg(self):
        return json.loads((self.fake / "reg.json").read_text())

    def where(self, blk):
        """"detached", or (window, the floating layer's state, or None when tiled)."""
        R = self.reg()
        info = R["blocks"][blk]
        if info["layer"] == "detached":
            return "detached"
        return info["window"], R["layers"].get(info["layer"]) if info["layer"] else None

    def background_task(self):
        self.bp("config", "--workers", "background")
        tid = self.start_task()
        return tid, self.block(tid)

    def test_a_background_worker_starts_detached_and_records_it(self):
        tid, blk = self.background_task()
        self.assertEqual(self.where(blk), "detached")
        self.assertEqual(self.show(tid)["pane"], "background")
        self.assertEqual(self.made()[-1]["label"], f"bullpen: {tid} · ctrl+esc hides")
        self.assertEqual(self.made()[-1]["options"]["cwd"], self.show(tid)["worktree"])
        self.assertEqual(self.api("session.new_layer"), [])

    def test_go_floats_it_over_the_coordinators_tab_and_hide_detaches_it(self):
        tid, blk = self.background_task()
        self.bp("go", tid)
        self.assertEqual(self.api("session.move_block")[-1], {"block_id": blk, "anchor_block_id": "blk1"})
        self.assertEqual(self.where(blk), ("w1", "normal"))
        self.assertTrue(self.api("session.set_layer_state")[-1].get("focus"))
        self.bp("go", tid)  # up already: no second move
        self.assertEqual(len(self.api("session.move_block")), 1)
        self.bp("hide", "--from", blk)
        self.assertEqual(self.where(blk), "detached")
        self.assertEqual(self.api("session.detach_block"), [{"block_id": blk}])
        self.assertEqual(self.focused()[-1], "blk2")  # back on the board

    def test_showing_one_pane_puts_away_a_background_one_and_a_capsule(self):
        tid, blk = self.background_task()
        self.bp("config", "--workers", "float")
        self.bp("add", "Another", "--brief", "B.")
        self.bp("start", "BP-2")
        cap = self.block("BP-2")
        self.bp("go", tid)
        self.bp("go", "BP-2")
        self.assertEqual(self.where(blk), "detached")  # a setting change leaves the old pane background
        self.assertEqual(self.where(cap), ("w1", "normal"))
        self.bp("go", tid)
        self.assertEqual(self.where(cap), ("w1", "minimized"))
        self.assertEqual(self.where(blk), ("w1", "normal"))

    def test_a_capsule_made_before_the_switch_still_minimizes(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("config", "--workers", "background")
        self.bp("go", tid)
        self.bp("hide", "--from", blk)
        self.assertEqual(self.where(blk), ("w1", "minimized"))
        self.assertEqual(self.api("session.detach_block"), [])

    def test_find_by_id_or_label_in_the_background(self):
        tid, blk = self.background_task()
        got = self.panes(f"""\
            from bullpen.rex import rex_view
            v = rex_view("session:1")
            print(json.dumps([panes.find("session:1", v, block={blk!r}),
                              (panes.find("session:1", v, label="bullpen: {tid}") or [None])[-1],
                              panes.find("session:1", v, label="bullpen: nobody")]))
            """)
        self.assertEqual(got, [[None, {"kind": "detached"}, blk], blk, None])
        self.assertEqual(len(self.api("session.list_blocks")), 1)  # once, for the whole view

    def test_relink_finds_a_background_pane_by_its_label(self):
        tid, blk = self.background_task()
        reg = self.fake / "reg.json"
        R = json.loads(reg.read_text())
        R["blocks"]["blk99"] = R["blocks"].pop(blk)  # a new ID, as after a restart
        reg.write_text(json.dumps(R))
        (self.fake / "alive" / blk).rename(self.fake / "alive" / "blk99")
        self.bp("go", tid)
        self.assertEqual((self.block(tid), self.show(tid)["status"]), ("blk99", "working"))

    def test_a_background_pane_gone_after_a_rex_restart_stops_the_task(self):
        tid, blk = self.background_task()
        (self.fake / "alive" / blk).unlink()
        self.bp("go", tid, ok=False)
        self.assertEqual(self.show(tid)["status"], "stopped")

    def test_gather_leaves_background_panes_alone(self):
        self.background_task()
        before = len(self.rex_calls())
        self.panes('panes.gather(repo, "session:1")\n')
        self.assertEqual([a for a in self.rex_calls()[before:] if "session.view" not in a], [])

    def test_peek_tell_answer_and_the_watcher_work_in_the_background(self):
        tid, blk = self.background_task()
        (self.fake / f"screen_{blk}").write_text(
            " Do you want to proceed?\n ❯ 1. Yes\n   2. No\n\n Esc to cancel\n")
        self.assertIn("Do you want to proceed?", self.bp("peek", tid).stdout)
        self.bp("_event", blk, "blocked", "permission", "Bash command")  # the watcher's event
        self.assertEqual(self.show(tid)["status"], "needs")
        pid = json.loads(self.bp("prompt", tid).stdout)["id"]
        self.bp("answer", tid, "2", "--prompt", pid)
        self.assertEqual(self.typed()[-1], (blk, "2"))
        self.bp("tell", tid, "One more thing.")
        self.assertEqual(self.typed()[-1], (blk, "One more thing."))
        self.assertEqual(self.where(blk), "detached")  # none of it showed the pane

    def test_a_notification_comes_from_the_board_not_a_background_pane(self):
        tid, blk = self.background_task()
        worker, board = self.terminal(blk), self.terminal("blk2")
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        self.assertIn("BP-1 needs you (permission): Allow rm?", board())
        self.assertEqual(worker(), "")


class ReviewersTest(PanesTest):
    def test_reviewers_default_to_the_background_whatever_workers_do(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.assertEqual(self.show(tid)["review"]["pane"], "background")
        R = json.loads((self.fake / "reg.json").read_text())
        self.assertEqual(R["blocks"][reviewer]["layer"], "detached")

    def test_reviewers_same_follows_workers_and_float_floats(self):
        self.bp("config", "--workers", "tabs", "--reviewers", "same")
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.assertEqual(self.show(tid)["review"]["pane"], "tabs")
        self.bp("config", "--reviewers", "float")
        self.bp("add", "Another", "--brief", "B.")
        self.bp("start", "BP-2")
        self.commit_and_ready("BP-2")
        self.assertEqual(self.show("BP-2")["review"]["pane"], "float")
        self.assertEqual(self.api("session.new_layer")[-1]["layout"]["block"]["label"],
                         "bullpen: BP-2 review · ctrl+esc hides")

    def test_go_next_brings_up_a_reviewer_that_needs_you(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp("_event", reviewer, "blocked", "permission", "Bash command")
        self.assertEqual(self.show(tid)["status"], "needs")
        self.bp("go", "--next")
        R = json.loads((self.fake / "reg.json").read_text())
        self.assertEqual(R["layers"][R["blocks"][reviewer]["layer"]], "normal")
        self.bp("hide", "--from", reviewer)
        R = json.loads((self.fake / "reg.json").read_text())
        self.assertEqual(R["blocks"][reviewer]["layer"], "detached")

    def test_the_diff_pane_never_goes_to_the_background(self):
        self.bp("config", "--workers", "background")
        tid = self.start_task()
        self.panes(f"""\
            t = repo.task({tid!r})
            print(json.dumps(panes.place(repo, t, "session:1", "bullpen: {tid} diff", ["true"], role="diff", hint=False)))
            """)
        self.assertEqual(self.api("session.new_layer")[-1]["layout"]["block"]["label"], f"bullpen: {tid} diff")
