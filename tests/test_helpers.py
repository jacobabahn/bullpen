"""bullpen's functions, called directly from the package's modules."""

import argparse
import datetime as dt
import re
import unittest

from support import setUpModule, tearDownModule, ROOT  # a temp folder for each module
from bullpen import (
    agents, cli, core, line_edit, notifications, stepper,
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

    def stepper(self, comments=()):
        tour = {"commit": "c1", "steps": [{"title": "One", "note": "Why one.", "hunks": [{"file": "a", "lines": [1, 2]}]},
                                          {"title": "Two", "note": "", "hunks": [{"file": "b"}]}]}
        return stepper.Stepper("BP-1", "Fix it", tour, list(comments), 0,
                               lambda i, w: [f"diff line {n} of step {i + 1}" for n in range(30)])

    def test_stepper_keys(self):
        st = self.stepper()
        self.assertIsNone(st.key("n"))
        self.assertEqual(st.at, 1)
        st.key("\x1b[C")  # → past the last step: the summary
        self.assertTrue(st.at_end())
        st.key("n")  # no further
        self.assertTrue(st.at_end())
        st.key("p")
        st.key("\x1b[D")
        self.assertEqual(st.at, 0)
        st.key("j")
        st.key(" ", rows=10)
        self.assertEqual(st.top, 9)
        st.key("k")
        self.assertEqual(st.top, 8)
        st.key("n")
        self.assertEqual(st.top, 0)  # a new step starts at its top
        self.assertIsNone(st.key("c"))
        for k in "Line 3 is off":
            st.key(k)
        st.key("\x7f")
        self.assertEqual(st.key("q"), None)  # typed, not quit
        self.assertEqual(st.key("\r"), ("comment", "Line 3 is ofq"))
        st.key("c")
        st.key("x")
        self.assertIsNone(st.key("\x1b"))  # esc drops what's typed
        self.assertEqual((st.mode, st.msg), ("read", "Comment not saved."))
        st.key("c")
        self.assertIsNone(st.key("\r"))  # nothing typed: nothing saved
        self.assertEqual(st.key("\x1b"), "close")
        self.assertEqual(st.key("q"), "close")

    def test_stepper_end_sends_or_accepts(self):
        st = self.stepper()
        st.go(2)
        self.assertIsNone(st.key("s"))
        self.assertIn("No comments to send", st.msg)
        self.assertEqual(st.key("a"), "accept")  # nothing unsent: no warning
        st = self.stepper([{"id": 1, "step": 1, "title": "One", "commit": "c1", "text": "Why?"}])
        st.go(2)
        self.assertIsNone(st.key("a"))
        self.assertIn("1 comment not sent: a again accepts anyway", st.msg)
        self.assertEqual(st.key("a"), "accept")
        st.key("a")
        st.key("j")  # another key in between: it asks again
        self.assertIsNone(st.key("a"))
        self.assertEqual(st.key("s"), "send")
        self.assertIsNone(st.key("c"))  # no comments on the summary
        self.assertEqual(st.mode, "read")

    def test_stepper_passes_the_boards_keys_through(self):
        st = self.stepper()
        # ⏎ and v close it and open the task's pane or diff, as on the board
        self.assertEqual(st.key("\r"), "open")
        self.assertEqual(st.key("v"), "diff")
        # t types a line for the worker, sent now; esc drops it
        self.assertIsNone(st.key("t"))
        self.assertEqual(st.prompt(), "Tell BP-1: ")
        for k in "Use the helper":
            st.key(k)
        self.assertEqual(st.key("\r"), ("tell", "Use the helper"))
        self.assertEqual(st.mode, "read")
        st.key("t")
        st.key("x")
        self.assertIsNone(st.key("\x1b"))
        self.assertEqual((st.mode, st.msg), ("read", "Not sent."))
        st.key("t")
        out = [re.sub(r"\x1b\[[0-9;]*m", "", line) for line in st.screen(60, 12)]
        self.assertEqual(out[-1].strip(), "⏎ sends it now · esc cancels")
        st.key("\x1b")
        # a before the end accepts too, on a second a
        self.assertIsNone(st.key("a"))
        self.assertEqual(st.msg, "a again accepts BP-1 before the end.")
        self.assertEqual(st.key("a"), "accept")
        st = self.stepper([{"id": 1, "step": 1, "title": "One", "commit": "c1", "text": "Why?"}])
        self.assertIsNone(st.key("a"))
        self.assertEqual(st.msg, "1 comment not sent: a again accepts BP-1 before the end.")
        out = [re.sub(r"\x1b\[[0-9;]*m", "", line).rstrip() for line in st.screen(42, 12)]
        self.assertEqual(out[-1], " n p · c comment · t tell · esc close")  # the shortest keys at 42
        st.key("n")  # another key in between: it asks again
        self.assertIsNone(st.key("a"))
        # c and s keep their own meanings: a comment, and at the end, send
        self.assertIsNone(st.key("c"))
        self.assertEqual(st.mode, "comment")

    def test_stepper_screen(self):
        st = self.stepper([{"id": 1, "step": 1, "title": "One", "commit": "c1", "text": "Why?"},
                           {"id": 2, "step": 1, "title": "One", "commit": "c0", "text": "From an old tour."}])
        plain = lambda lines: [re.sub(r"\x1b\[[0-9;]*m", "", line).rstrip() for line in lines]
        out = plain(st.screen(60, 12))
        self.assertEqual(len(out), 12)
        self.assertTrue(out[0].startswith(" BP-1  Fix it") and out[0].endswith("step 1 of 2 · 2 comments"), out)
        self.assertEqual(out[2:9], [" One", " a 1-2", "", " Why one.", "", " ▍ Why?", ""])
        self.assertEqual(out[9], " diff line 0 of step 1")
        self.assertIn("↓ 29 more", out[10])
        self.assertIn("c comment", out[11])
        st.go(2)
        out = plain(st.screen(60, 30))
        self.assertTrue(out[0].endswith("end · 2 comments"))
        self.assertIn(" Your 2 comments, not sent yet:", out)
        self.assertIn(" s send · a accept · p back · t tell · esc close", out)  # the short keys at 60
        out = plain(st.screen(100, 30))
        self.assertIn(" s send them back · a accept · p back · t tell · v diff · ⏎ open · esc close", out)
        st.go(0)
        st.key("G")
        out = plain(st.screen(60, 12))
        self.assertEqual(out[9], " diff line 29 of step 1")  # the bottom, no further
        st.key("c")
        for k in "x" * 80:
            st.key(k)
        out = plain(st.screen(60, 12))
        # the prompt on its own row, what's typed wrapped under it
        self.assertEqual(out[8:11], [" Comment on step 1:", " " + "x" * 57, " " + "x" * 23])
        self.assertEqual(out[11], " ⏎ saves · ctrl+j new line · esc cancels")
        self.assertEqual(st.cursor(60, 12), (11, 25))
        st.key("\x1b")
        st.diff = lambda i, w: ["\x1b[32m+" + "y" * 100 + "\x1b[0m", "宽" * 40]
        st.cache.clear()
        st.top = 0
        out = plain(st.screen(20, 20))
        self.assertIn(" +" + "y" * 17, out)  # cut before the last column
        self.assertIn(" " + "宽" * 9, out)
        self.assertTrue(all(len(line) <= 19 for line in out if "宽" not in line), out)

    def test_clip_line_counts_columns_not_codes(self):
        clip = stepper.clip_line
        self.assertEqual(clip("\x1b[1mabc\x1b[0mdef", 4), "\x1b[1mabc\x1b[0md")
        self.assertEqual(clip("ab", 5), "ab")
        self.assertEqual(clip("宽宽宽", 5), "宽宽")

    def test_line_edit_moves_and_cuts_at_the_cursor(self):
        e = line_edit.LineEdit()
        shown = lambda: e.text[:e.at] + "|" + e.text[e.at:]
        for k in ["u", "s", "e", " ", "p", "g", "\x1b[D", "\x1b[D", "X"]:
            self.assertTrue(e.key(k), k)
        self.assertEqual(shown(), "use X|pg")
        for keys, want in [(["\x01"], "|use Xpg"), (["\x1b[F"], "use Xpg|"), (["\x1bb"], "use |Xpg"),
                           (["\x1b[1;5C"], "use Xpg|"), (["\x17"], "use |"), (["\x1b[H", "\x1b[3~"], "|se "),
                           (["\x06", "\x0b"], "s|"), (["\x15"], "|"), (["\x1b[C", "\x7f"], "|")]:
            for k in keys:
                e.key(k)
            self.assertEqual(shown(), want, keys)
        for k in ["\r", "\x1b", "\x03", "\x1b[Z"]:  # for the caller
            self.assertFalse(e.key(k), repr(k))
        # no cap
        e.key("\x1b[200~" + "x" * 3000 + "\x1b[201~")
        self.assertEqual(len(e.text), 3000)

    def test_line_edit_takes_a_paste_on_one_line_or_with_its_lines(self):
        e = line_edit.LineEdit("[]")
        e.key("\x1b[D")
        e.key("\x1b[200~one\r\ntwo\nthree\tfour\x07\x1b[201~")
        self.assertEqual((e.text, e.at), ("[one two three four]", 19))
        e = line_edit.LineEdit(lines=True)
        e.key("\x1b[200~one\r\ntwo\x1b[201~")
        self.assertEqual(e.text, "one\ntwo")
        e.key("\n")  # ctrl+j: a new line
        e.key("x")
        e.key("\x1b[A")  # up: the same column, or the line's end
        self.assertEqual(e.text[:e.at], "one\nt")
        e.key("\x1b[A")
        e.key("\x15")  # ctrl+u: to the start of this line only
        self.assertEqual(e.text, "ne\ntwo\nx")
        e.key("\x1b[B")
        e.key("\x1b[B")
        e.key("\x05")
        e.key("\x1b\r")  # alt+⏎ too
        self.assertEqual(e.text, "ne\ntwo\nx\n")

    def test_line_edit_views_keep_the_cursor_in_sight(self):
        e = line_edit.LineEdit("abcdefghij")
        self.assertEqual(e.view(5), ("ghij", 4))
        for _ in range(3):
            e.key("\x1b[D")
        self.assertEqual(e.view(5), ("ghij", 1))  # the view stays until the cursor reaches its edge
        e.key("\x1b[D")
        e.key("\x1b[D")
        self.assertEqual(e.view(5), ("fghij", 0))
        e.key("\x01")
        self.assertEqual(e.view(5), ("abcde", 0))
        e = line_edit.LineEdit("abcdefgh\nij", lines=True)
        self.assertEqual(e.rows(5, 9), (["abcd", "efgh", "ij"], (2, 2)))
        self.assertEqual(e.rows(5, 2), (["efgh", "ij"], (1, 2)))  # the rows around the cursor
        e.key("\x1b[A")
        e.key("\x01")
        self.assertEqual(e.rows(5, 2), (["abcd", "efgh"], (0, 0)))

    def test_a_comment_can_have_lines_and_a_tell_is_one(self):
        st = self.stepper()
        st.key("c")
        st.key("\x1b[200~Use the helper:\n\n  helper(x)  \n\x1b[201~")
        st.key("\n")
        for k in "Then test it ":
            st.key(k)
        plain = lambda lines: [re.sub(r"\x1b\[[0-9;]*m", "", line).rstrip() for line in lines]
        out = plain(st.screen(42, 16))
        self.assertEqual(len(out), 16)
        self.assertEqual(out[9:], [" Comment on step 1:", " Use the helper:", "", "   helper(x)", "",
                                   " Then test it", " ⏎ saves · ctrl+j new line · esc cancels"])
        self.assertEqual(st.cursor(42, 16), (15, 15))
        self.assertEqual(st.key("\r"), ("comment", "Use the helper:\n\n  helper(x)\n\nThen test it"))
        st.key("t")
        st.key("\x1b[200~one\ntwo\x1b[201~")  # a pasted line break doesn't send it
        self.assertEqual(st.mode, "tell")
        st.key("\x1bb")  # alt+b: a word back
        st.key("\x17")
        self.assertEqual(st.key("\r"), ("tell", "two"))

    def test_split_keys_keeps_a_paste_and_alt_keys_whole(self):
        self.assertEqual(line_edit.split_keys("a\x1b[200~x\ny\x1b[201~\x1bb\x1b\r\x1b\x1b"),
                         ["a", "\x1b[200~x\ny\x1b[201~", "\x1bb", "\x1b\r", "\x1b", "\x1b"])

    def test_split_keys_keeps_escape_sequences_whole(self):
        self.assertEqual(line_edit.split_keys("ab\x1b[Bc\x1bOA\x1b"), ["a", "b", "\x1b[B", "c", "\x1bOA", "\x1b"])

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
