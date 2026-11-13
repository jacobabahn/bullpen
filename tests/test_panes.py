"""Where a pane lives: the placements, through the commands that show and hide panes (Rust's
cli/src/panes/, with its own tests of placement choice), and Python's twin (bullpen/panes.py)
called as launch, review and `up` call it."""

import json
import subprocess
import sys
import textwrap

from support import ROOT, BullpenTest, setUpModule, tearDownModule  # noqa: F401


class PanesBase(BullpenTest):
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

    def reg(self):
        return json.loads((self.fake / "reg.json").read_text())

    def made_block(self, label):
        """The newest block with this label."""
        return [b for b, i in self.reg()["blocks"].items() if i["label"] == label][-1]

    def where(self, blk):
        """"detached", or (window, the floating layer's state, or None when tiled)."""
        R = self.reg()
        info = R["blocks"][blk]
        if info["layer"] == "detached":
            return "detached"
        return info["window"], R["layers"].get(info["layer"]) if info["layer"] else None


class PanesTest(PanesBase):
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
        got = [self.bp("hide", "--from", b, ok=False).returncode for b in (blk, "blk1", "nosuch")]
        self.assertEqual(got, [0, 3, 3])  # a worker's capsule, the coordinator's tab, nothing
        self.assertEqual(self.api("session.set_layer_state")[-1]["state"], "minimized")

    def test_hide_lets_the_key_through_in_a_tab_of_its_own(self):
        self.bp("config", "--workers", "tabs")
        tid = self.start_task()
        before = len(self.rex_calls())
        self.assertEqual(self.bp("hide", "--from", self.block(tid), ok=False).returncode, 3)
        self.assertNotIn("focus", [a[0] for a in self.rex_calls()[before:]])

    def test_show_brings_one_float_up_and_puts_the_others_away(self):
        a = self.start_task()
        self.bp("add", "Another", "--brief", "B.")
        self.bp("start", "BP-2")
        self.bp("go", a)
        self.bp("go", "BP-2")
        states = self.layer_states()
        layer = {b: i["layer"] for b, i in json.loads((self.fake / "reg.json").read_text())["blocks"].items()}
        self.assertEqual(states[layer[self.block(a)]], "minimized")
        self.assertEqual(states[layer[self.block("BP-2")]], "normal")
        up = [a for a in self.api("session.set_layer_state") if a["layer_id"] == layer[self.block("BP-2")]]
        self.assertTrue(up[-1].get("focus"))

    def test_show_focuses_a_pane_in_a_tab(self):
        self.bp("config", "--workers", "tabs")
        tid = self.start_task()
        self.bp("go", tid)
        self.assertEqual(self.rex_calls()[-1][:2], ["focus", "-s"])

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


class BackgroundTest(PanesBase):
    """--workers background and --reviewers: detached panes that `go` floats over the
    coordinator's tab and the hide key detaches again."""

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


class ReviewersTest(PanesBase):
    def test_a_reviewer_gone_after_a_rex_restart_goes_to_the_coordinator(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        (self.fake / "alive" / reviewer).unlink()  # detached blocks don't survive a restart
        self.bp("up")
        t = self.show(tid)
        self.assertEqual((t["status"], t["stop"], t["review"]["block"]),
                         ("triage", {"by": "reviewer", "kind": "pane_gone"}, None))
        self.assertIn(f"`bullpen review {tid}` starts a new one", self.news_lines()[-1])
        self.bp("review", tid)  # what the coordinator does
        self.assertEqual(self.show(tid)["status"], "review")

    def test_an_approved_task_keeps_its_status_when_its_reviewer_is_gone(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.verdict(tid, "approve")  # which closes the reviewer
        (self.fake / "alive" / reviewer).unlink(missing_ok=True)
        self.bp("up")
        self.assertEqual(self.show(tid)["status"], "ready")

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
        self.bp("diff", tid, "--pane")
        made = self.api("session.new_layer")[-1]["layout"]["block"]
        self.assertEqual(made["label"], f"bullpen: {tid} diff")
        self.assertEqual(made["options"]["command"][-3:], ["diff", tid, "--back"])
        layer = self.reg()["blocks"][self.made_block(f"bullpen: {tid} diff")]["layer"]
        self.assertEqual(self.reg()["layers"][layer], "normal")  # shown at once

    def test_a_second_diff_pane_replaces_the_first(self):
        tid = self.start_task()
        self.bp("diff", tid, "--pane")
        first = self.made_block(f"bullpen: {tid} diff")
        self.bp("diff", tid, "--pane")
        self.assertFalse((self.fake / "alive" / first).exists())
        self.assertNotEqual(self.made_block(f"bullpen: {tid} diff"), first)


class TabTest(PanesBase):
    """--workers tab: capsules in one workers tab per coordinator, out of the board's tab."""

    def two_tab_tasks(self):
        self.bp("config", "--workers", "tab")
        a = self.start_task()
        self.bp("add", "Another", "--brief", "B.")
        self.bp("start", "BP-2")
        return a, "BP-2"

    def test_workers_share_one_tab_made_without_focus(self):
        a, b = self.two_tab_tasks()
        made = self.api("session.new_window")
        self.assertEqual(len(made), 1)
        self.assertEqual((made[0]["focus"], made[0]["window_label"]), (False, "bullpen: proj workers"))
        self.assertEqual(made[0]["layout"]["block"]["label"], f"bullpen: {a} · ctrl+esc hides")
        tab = self.where(self.block(a))[0]
        self.assertNotEqual(tab, "w1")  # not the coordinator's
        # Rex keeps one pane up in a tab of floats: the first stays up, the next is a capsule
        self.assertEqual([self.where(self.block(t)) for t in (a, b)], [(tab, "normal"), (tab, "minimized")])
        self.assertEqual([self.show(t)["pane"] for t in (a, b)], ["tab", "tab"])
        self.assertEqual(self.focused(), [])

    def test_go_switches_to_the_tab_and_hide_goes_back_to_the_board(self):
        a, b = self.two_tab_tasks()
        blk = self.block(b)
        self.bp("go", b)
        tab = self.where(blk)[0]
        self.assertEqual([self.where(self.block(t)) for t in (a, b)], [(tab, "minimized"), (tab, "normal")])
        self.assertEqual(self.api("session.set_layer_state")[-2], {"layer_id": self.reg()["blocks"][blk]["layer"],
                                                                   "state": "normal", "focus": True})
        self.bp("hide", "--from", blk)  # the last one up in its tab: Rex keeps it up
        self.assertEqual(self.focused()[-1], "blk2")  # back on the board, in its tab
        self.bp("go", a)
        self.bp("hide", "--from", self.block(a))
        self.assertEqual(self.focused()[-1], "blk2")

    def test_a_float_comes_up_over_the_board_and_a_tab_pane_is_put_away(self):
        a, b = self.two_tab_tasks()
        self.bp("config", "--workers", "float")
        self.bp("add", "Third", "--brief", "C.")
        self.bp("start", "BP-3")
        self.bp("go", a)
        self.bp("go", "BP-3")
        self.assertEqual(self.where(self.block("BP-3")), ("w1", "normal"))
        self.assertEqual(self.where(self.block(a))[1], "normal")  # its tab's last one up
        self.assertEqual(self.where(self.block(b))[1], "minimized")

    def test_gather_moves_a_stray_pane_back_into_the_workers_tab(self):
        a, b = self.two_tab_tasks()
        blk, tab = self.block(b), self.where(self.block(a))[0]
        R = self.reg()
        R["windows"].append("w9")
        R["blocks"][blk].update(layer=None, window="w9")  # dragged to a tab of its own
        (self.fake / "reg.json").write_text(json.dumps(R))
        (self.fake / "focused").write_text("blk1")
        self.panes('panes.gather(repo, "session:1")\n')
        self.assertEqual(self.where(blk), (tab, "minimized"))
        layer = self.reg()["blocks"][blk]["layer"]
        alone = [b2 for b2, i in self.reg()["blocks"].items()
                 if i["layer"] == layer and (self.fake / "alive" / b2).exists()]
        self.assertEqual(alone, [blk])  # the stand-in it was placed beside closed
        stand_in = self.api("session.new_layer")[-1]
        self.assertEqual(stand_in["window_id"], tab)
        self.assertNotIn("w9", self.reg()["windows"])  # emptied, so closed
        self.assertEqual(self.focused()[-1], "blk1")  # moving it took focus: given back

    def test_gather_makes_the_workers_tab_again_when_it_is_gone(self):
        a, _ = self.two_tab_tasks()
        blk = self.block(a)
        R = self.reg()
        R["labels"] = {}  # its tab was renamed
        (self.fake / "reg.json").write_text(json.dumps(R))
        self.panes('panes.gather(repo, "session:1")\n')
        made = self.api("session.new_window")[-1]
        self.assertEqual((made["focus"], made["window_label"]), (False, "bullpen: proj workers"))
        self.assertEqual(made["layout"]["block"]["label"], "bullpen: stand-in")
        tab = self.where(blk)[0]
        self.assertEqual(self.where(self.block("BP-2"))[0], tab)  # both, into the one new tab
        self.assertEqual(len(self.api("session.new_window")), 2)

    def test_reviewers_can_go_to_the_tab(self):
        self.bp("config", "--reviewers", "tab")
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.assertEqual(self.show(tid)["review"]["pane"], "tab")
        self.assertEqual(self.reg()["labels"][self.where(reviewer)[0]], "bullpen: proj workers")


class PaneChoiceTest(PanesBase):
    """start/add/resume --pane: a task's own placement, over config --workers."""

    def test_start_pane_goes_over_the_setting_and_show_says_so(self):
        self.bp("up")
        self.bp("add", "One", "--brief", "A.")
        self.bp("start", "BP-1", "--pane", "background")
        self.assertEqual(self.where(self.block("BP-1")), "detached")
        t = self.show("BP-1")
        self.assertEqual((t["placement"], t["pane"]), ("background", "background"))
        self.assertIn("pane: background (its own", self.bp("show", "BP-1").stdout)
        self.bp("add", "Two", "--brief", "B.")
        self.bp("start", "BP-2")
        self.assertEqual(self.show("BP-2")["pane"], "float")  # the setting, for the others
        self.assertNotIn("pane:", self.bp("show", "BP-2").stdout)

    def test_add_pane_is_kept_until_the_task_starts(self):
        self.bp("up")
        self.bp("add", "One", "--brief", "A.", "--pane", "tabs")
        self.assertEqual(self.show("BP-1")["placement"], "tabs")
        self.bp("start", "BP-1")
        self.assertEqual(self.show("BP-1")["pane"], "tabs")

    def test_resume_pane_changes_where_the_new_pane_goes(self):
        tid = self.start_task()
        (self.fake / "alive" / self.block(tid)).unlink()
        self.bp("resume", tid, "--pane", "tab")
        t = self.show(tid)
        self.assertEqual((t["placement"], t["pane"]), ("tab", "tab"))
        self.assertEqual(self.reg()["labels"][self.where(t["block"])[0]], "bullpen: proj workers")

    def test_a_reviewer_follows_the_tasks_choice_only_with_reviewers_same(self):
        self.bp("up")
        self.bp("add", "One", "--brief", "A.", "--pane", "tabs")
        self.bp("start", "BP-1")
        self.commit_and_ready("BP-1")
        self.assertEqual(self.show("BP-1")["review"]["pane"], "background")  # the reviewers setting
        self.bp("config", "--reviewers", "same")
        self.bp("add", "Two", "--brief", "B.", "--pane", "tab")
        self.bp("start", "BP-2")
        self.commit_and_ready("BP-2")
        self.assertEqual(self.show("BP-2")["review"]["pane"], "tab")

    def test_the_diff_pane_gets_a_tab_when_the_task_has_a_tab_each(self):
        self.bp("up")
        self.bp("add", "One", "--brief", "A.", "--pane", "tabs")
        self.bp("start", "BP-1")
        self.bp("diff", "BP-1", "--pane")
        run = [a for a in self.rex_calls() if a[0] == "run"][-1]
        self.assertEqual((run[run.index("--label") + 1], run[2]), ("bullpen: BP-1 diff", "--focus=true"))
