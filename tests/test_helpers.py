"""bullpen's functions, called directly from the package's modules."""

import argparse
import datetime as dt
import re
import unittest

from support import setUpModule, tearDownModule, ROOT  # a temp folder for each module
from bullpen import (
    agents, cli, core, notifications,
)


class HelpersTest(unittest.TestCase):
    def test_the_docs_tables_are_the_boards(self):
        # docs/board.md's marks and keys tables say what the board's `?` list says
        use = (ROOT / "docs" / "board.md").read_text()
        rows = [r.split(" | ") for r in re.findall(r"^\| (.+) \|$", use, re.M)]
        cells = {r[1]: r[0] for r in rows if len(r) == 2}
        # every still mark (the board's mark() in board/src/model/groups.rs), each in docs/board.md beside its words
        src = (ROOT / "board" / "src" / "model" / "groups.rs").read_text()
        body = src[src.index("pub fn mark(status: &str)"):]
        marks = dict(re.findall(r'"(\w+)" => "(.)"', body[:body.index("\n}\n")]))
        self.assertGreaterEqual(len(marks), 10)
        for st, mark in marks.items():
            self.assertTrue(any(f"`{mark}`" in c for c in cells.values()), (st, mark))
        for words in ("needs you", "ready to accept", "dropped", "in review", "working"):
            self.assertIn(words, cells)
        self.assertIn("`◐`", cells["in review"])
        self.assertIn("`○`", cells["working"])
        # the turning marks: the first frame of each animation in board/src/ui/mod.rs
        ui = (ROOT / "board" / "src" / "ui" / "mod.rs").read_text()
        for name, words in (("REVIEW", "in review"), ("WORKING", "working")):
            first = re.search(name + r': &\[&str\] = &\["(.)"', ui).group(1)
            self.assertIn(f"`{first}`", cells[words])
        spin = re.search(r'SPINNER: &\[&str\] = &\["(.)"', ui).group(1)
        self.assertTrue(any(c.startswith(f"`{spin}`") for c in cells.values()))
        # every key of the board (BOARD_KEYS in board/src/ui/help.rs, the tour's aside), in its order
        help = (ROOT / "board" / "src" / "ui" / "help.rs").read_text()
        keys = help[help.index("pub const BOARD_KEYS"):help.index('("In a tour"')]
        board = [k.split()[0] for k in re.findall(r'\("([^"]+)", "', keys)]
        table = [re.match(r"`([^`]+)`", c).group(1) for c in cells.values() if re.match(r"`[^`]+`", c)]
        listed = [k for k in table if k in board]
        self.assertEqual(listed, board)

    # ---------- help and ls ----------

    def test_every_command_is_in_one_help_group(self):
        sub = next(a for a in cli.parser()._actions if a.choices)
        titles = [title for title, _ in cli.GROUPS]
        grouped = {n for n, p in sub.choices.items() if p.group}
        self.assertTrue(all(p.group in titles for p in sub.choices.values() if p.group))
        self.assertEqual(grouped, {n for n in sub.choices if not n.startswith("_")})

    def test_every_command_and_option_has_help_but_the_internal_ones(self):
        sub = next(a for a in cli.parser()._actions if a.choices)
        listed = {a.dest for a in sub._choices_actions}
        for name, p in sub.choices.items():
            self.assertEqual(name in listed, not name.startswith("_"), name)
            if name.startswith("_"):
                continue
            for a in p._actions:
                if a.dest != "help" and a.help != argparse.SUPPRESS:
                    self.assertTrue(a.help, f"{name} {a.option_strings or a.dest}")

    def test_status_words_are_the_boards(self):
        w = core.status_words
        self.assertEqual(w("needs"), "needs you")
        self.assertEqual(w("stopped"), "needs you (stopped)")
        self.assertEqual(w("triage"), "coordinator on it (triage)")
        self.assertEqual(w("review"), "in review")
        self.assertEqual(w("ready"), "ready to accept")
        self.assertEqual(w("todo"), "queued (todo)")
        self.assertEqual(w("dropped"), "closed (dropped)")
        self.assertEqual(set(core.BOARD_WORDS), {"todo", "working", "review", "ready", "finished",
                                                   "triage", "needs", "stopped", "done", "dropped"})

    # ---------- the task lifecycle ----------

    def stopped(self, status, by, kind, **more):
        return {"status": status, "stop": {"by": by, "kind": kind}, **more}

    def test_whether_a_worker_is_between_turns_goes_by_its_stop(self):
        idle = core.worker_idle
        self.assertTrue(idle({"status": "working"}))
        self.assertTrue(idle(self.stopped("triage", "worker", "ended")))
        self.assertTrue(idle(self.stopped("triage", "worker", "not_sent")))
        self.assertTrue(idle(self.stopped("triage", "reviewer", "ended")))
        self.assertTrue(idle(self.stopped("needs", "reviewer", "prompt")))
        self.assertFalse(idle(self.stopped("needs", "worker", "prompt")))
        self.assertFalse(idle(self.stopped("triage", "worker", "failed")))
        self.assertFalse(idle(self.stopped("triage", "reviewer", "limit")))
        # the person asked about it: the question is its why, the stop still counts
        self.assertFalse(idle(self.stopped("needs", "worker", "prompt", why="Delete it?", ask=True)))
        self.assertTrue(idle({"status": "needs", "why": "Delete it?", "ask": True}))
        self.assertTrue(idle(self.stopped("stopped", "worker", "exited")))

    def test_what_a_status_change_tells_the_person(self):
        n = notifications.notice
        t = lambda status, **more: {"id": "BP-3", "title": "Fix it", "status": status, **more}
        self.assertEqual(n(t("needs", why="Needs you (permission): Allow rm?"), t("working")),
                         ("needs", "BP-3 needs you (permission): Allow rm?"))
        self.assertEqual(n(t("needs", why="Reviewer needs you (input)"), t("review")),
                         ("needs", "BP-3's reviewer needs you (input)"))
        self.assertEqual(n(t("needs", why="Failed: boom (coordinator didn't resolve it)"), t("triage")),
                         ("needs", "BP-3 needs you: Failed: boom (coordinator didn't resolve it)"))
        prompt = t("needs", why="Needs you (permission): Allow rm?")
        self.assertIsNone(n(prompt, prompt))  # no change
        asked = t("needs", why="Use SQLite?", ask=True)
        self.assertEqual(n(asked, prompt), ("needs", "BP-3 needs you: Use SQLite?"))  # asked at a prompt
        self.assertIsNone(n(asked, asked))
        self.assertEqual(n(t("needs", why="Or Postgres?", ask=True), asked), ("needs", "BP-3 needs you: Or Postgres?"))
        self.assertIsNone(n(t("triage", why="Ended its turn without saying ready"), t("working")))
        self.assertEqual(n(t("ready"), t("review")), ("news", "BP-3 is ready: Fix it"))
        self.assertIsNone(n(t("ready"), t("ready")))
        self.assertEqual(n(t("finished"), t("working")), ("news", "BP-3 finished: Fix it"))
        self.assertEqual(n(t("done", auto_accepted=True), t("ready")), ("news", "BP-3 was auto-accepted: Fix it"))
        self.assertIsNone(n(t("done", auto_accepted=False), t("ready")))  # the person accepted it
        self.assertIsNone(n(t("working"), {}))

    def test_a_task_is_on_its_reviewer_by_its_stop(self):
        on = agents.on_reviewer
        self.assertTrue(on(self.stopped("needs", "reviewer", "prompt")))
        self.assertTrue(on(self.stopped("triage", "reviewer", "exited")))
        self.assertTrue(on(self.stopped("needs", "reviewer", "ended", why="Delete it?", ask=True)))
        self.assertFalse(on(self.stopped("triage", "reviewer", "limit")))  # seen to as its worker's
        self.assertFalse(on(self.stopped("triage", "worker", "ended")))
        self.assertFalse(on({"status": "review"}))
        t = self.stopped("needs", "reviewer", "ended", review={"block": "blk2", "session": "s"}, block="blk1")
        self.assertEqual(agents.waited_on(t), ("reviewer", "s", "blk2"))
        t["stop"] = {"by": "worker", "kind": "failed"}
        self.assertEqual(agents.waited_on(t), ("worker", None, "blk1"))

    def test_a_task_waits_on_its_reviewer_in_review_or_for_its_stop(self):
        w = agents.waits_on
        self.assertEqual(w({"status": "review"}), "reviewer")
        self.assertEqual(w(self.stopped("needs", "reviewer", "prompt")), "reviewer")
        self.assertEqual(w(self.stopped("triage", "reviewer", "limit")), "worker")  # seen to as its worker's
        self.assertEqual(w(self.stopped("triage", "worker", "exited")), "worker")
        self.assertIsNone(w(self.stopped("stopped", "worker", "pane_gone")))  # its pane is gone
        self.assertEqual(w({"status": "working"}), "worker")
        for status in ("todo", "ready", "finished", "done", "dropped"):
            self.assertIsNone(w({"status": status}))
        lim = {"who": "reviewer", "text": "limit"}
        self.assertIsNone(w({"status": "review", "limit": lim}))  # waits on the usage limit
        self.assertEqual(agents.whose_turn({"status": "review", "limit": lim}), "reviewer")
        self.assertIsNone(w({"status": "working", "limit": {**lim, "who": "worker"}}))

    def test_a_reviewer_pane_falls_back_to_its_workers_session(self):
        t = {"status": "review", "session": "s1", "block": "b1", "review": {"block": "b2"}}
        self.assertEqual(agents.pane(t, "worker"), ("s1", "b1"))
        self.assertEqual(agents.pane(t, "reviewer"), ("s1", "b2"))
        t["review"]["session"] = "s2"
        self.assertEqual(agents.pane(t, "reviewer"), ("s2", "b2"))
        self.assertEqual(agents.pane({"status": "todo"}, "reviewer"), (None, None))
        self.assertIsNone(agents.waited_on({"status": "review", "block": "b1"}))  # no reviewer pane
        self.assertFalse(agents.running({"status": "ready", "block": "b1"}))  # waits on no agent

    def test_a_new_status_ends_ask_ok_and_the_stop(self):
        t = self.stopped("triage", "worker", "ended", why="Ended its turn without saying ready", ok=True)
        core.ask(t, "Delete it?")
        self.assertEqual((t["status"], t["why"], t["ask"], t["stop"]),
                         ("needs", "Delete it?", True, {"by": "worker", "kind": "ended"}))
        self.assertNotIn("ok", t)
        core.ask(t, "Or keep it?")  # asked again: still asked
        self.assertTrue(t["ask"])
        core.set_status(t, "working")
        self.assertEqual((t["status"], t["why"]), ("working", None))
        self.assertFalse({"ask", "ok", "stop"} & set(t))
        t = self.stopped("triage", "reviewer", "ended")
        core.ok(t)
        self.assertEqual((t["status"], t["ok"]), ("review", True))
        self.assertNotIn("stop", t)
        t = self.stopped("triage", "worker", "failed")
        core.ok(t)
        self.assertEqual(t["status"], "working")

    def test_old_whys_say_the_stop(self):
        f = core.stop_from_why
        self.assertEqual(f("Needs you (permission): Allow?"), {"by": "worker", "kind": "prompt"})
        self.assertEqual(f("Reviewer needs you (input)"), {"by": "reviewer", "kind": "prompt"})
        self.assertEqual(f("Failed (coordinator didn't resolve it)"), {"by": "worker", "kind": "failed"})
        self.assertEqual(f("Reviewer exited without a verdict"), {"by": "reviewer", "kind": "exited"})
        self.assertEqual(f("Its pane was gone"), {"by": "worker", "kind": "pane_gone"})
        self.assertEqual(f("Couldn't be continued after the usage limit: x"), {"by": "worker", "kind": "limit"})
        self.assertEqual(f("Something else"), {"by": "worker", "kind": "other"})
        self.assertIsNone(f(None))

    def test_news_doesnt_say_local_branch_in_local_mode(self):
        self.assertEqual(core.where({"status": "ready", "pr": None, "local": True}), "")
