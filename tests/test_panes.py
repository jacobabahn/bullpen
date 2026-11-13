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
            print(json.dumps([panes.place(repo, t, "session:1", "bullpen: X", ["true"]),
                              panes.place(repo, t, "session:1", "bullpen: Y", ["true"], hint=False)]))
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
            print(json.dumps([panes.hide("session:1", {blk!r}), panes.hide("session:1", "blk1"),
                              panes.hide("session:1", "nosuch")]))
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
