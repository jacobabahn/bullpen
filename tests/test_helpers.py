"""bullpen's functions, called directly from the package's modules."""

import argparse
import datetime as dt
import re
import textwrap
import unittest

from support import setUpModule, tearDownModule, ROOT  # a temp folder for each module
from bullpen import (
    answer, check, cli, core, doctor, limit, line_edit, notifications, pick, show, stepper,
    task_line,
)


class HelpersTest(unittest.TestCase):
    # ---- one task line ----

    def repo(self, *tasks, **cfg):
        class R:
            pass
        r = R()
        r.cfg, r.tasks = {"review_rounds": 2, **cfg}, lambda: list(tasks)
        return r

    def ago(self, minutes):
        import datetime as dt
        return (dt.datetime.now().astimezone() - dt.timedelta(minutes=minutes)).isoformat()

    def test_board_order_puts_what_you_do_next_first(self):
        ts = [{"id": f"BP-{i}", "status": s} for i, s in
              enumerate(["todo", "working", "ready", "stopped", "needs", "triage", "done", "review", "finished"], 1)]
        self.assertEqual([t["id"] for t in task_line.board_order(ts)],
                         ["BP-4", "BP-5", "BP-6", "BP-3", "BP-9", "BP-8", "BP-2", "BP-1", "BP-7"])

    def test_the_boards_sentence(self):
        d = lambda t, *more, **cfg: task_line.board_detail(self.repo(t, *more, **cfg), t)
        self.assertEqual(d({"id": "BP-1", "status": "stopped"}), "Its agent exited. r resumes it.")
        self.assertEqual(d({"id": "BP-1", "status": "finished", "summary": "# Recorded the demo\n\nmore"}),
                         "Finished, nothing to merge: Recorded the demo. a closes it.")
        self.assertEqual(d({"id": "BP-1", "status": "ready", "pr": "https://github.com/a/b/pull/41",
                            "review": {"verdict": "approved", "kind": "full"}, "gh": {"checks": "passing"}}),
                         "PR #41, approved by full review. Checks passing.")
        self.assertEqual(d({"id": "BP-1", "status": "review", "review": {"kind": "followup", "round": 2}}),
                         "Follow-up review on round 2 of 2.")
        self.assertEqual(d({"id": "BP-2", "status": "todo", "after": ["BP-1"]}, {"id": "BP-1", "status": "ready"}),
                         "Waits on BP-1.")
        self.assertEqual(d({"id": "BP-1", "status": "triage", "why": "Ended its turn without saying ready"}),
                         "Ended its turn without ready.")
        self.assertEqual(d({"id": "BP-1", "status": "todo", "worker_model": "opus"}, worker_model="sonnet"),
                         "Not started. s starts it. Model: opus.")
        self.assertEqual(d({"id": "BP-1", "status": "needs", "why": "Asks: x?", "note": "Looking"}), "Looking")

    def test_a_task_line_fits_the_width_and_keeps_its_age(self):
        t = {"id": "BP-3", "status": "needs", "title": "Fix the login redirect after sign-in",
             "why": "Asks: python3 or python? The Makefile uses both.", "since": self.ago(12)}
        r = task_line.task_line(self.repo(t), t)
        self.assertEqual(r, "● BP-3  Fix the login redirect after sign-in  12m  Asks: python3 or python? The Makefile uses both.")
        cut = task_line.task_line(self.repo(t), t, width=42)
        self.assertEqual(cut, "● BP-3  Fix the login redirect after…  12m")  # no room for the sentence
        self.assertEqual(task_line.task_line(self.repo(t), t, width=70),
                         "● BP-3  Fix the login redirect after sign-in  12m  Asks: python3 or p…")  # 70 wide
        colour = task_line.task_line(self.repo(t), t, color=True)
        self.assertTrue(colour.startswith("\x1b[1;33m●\x1b[0m "))  # the mark amber, as on the board
        self.assertIn("\x1b[1;31m12m", colour)  # needs you for over 5 minutes: the age turns red

    def test_the_marks_are_the_boards(self):
        # one mark per status everywhere: the board's mark() in board/src/model.rs
        src = (ROOT / "board" / "src" / "model.rs").read_text()
        body = src[src.index("pub fn mark(status: &str)"):]
        body = body[:body.index("\n}\n")]
        board = dict(re.findall(r'"(\w+)" => "(.)"', body))
        self.assertEqual(task_line.MARKS, board)
        # ready and closed are told apart
        self.assertEqual((task_line.MARKS["ready"], task_line.MARKS["done"]), ("✔", "·"))

    def test_the_docs_tables_are_the_boards(self):
        # docs/board.md's marks and keys tables say what the board's `?` list says
        use = (ROOT / "docs" / "board.md").read_text()
        rows = [r.split(" | ") for r in re.findall(r"^\| (.+) \|$", use, re.M)]
        cells = {r[1]: r[0] for r in rows if len(r) == 2}
        # every still mark, each in docs/board.md beside its words
        for st, mark in task_line.MARKS.items():
            self.assertTrue(any(f"`{mark}`" in c for c in cells.values()), (st, mark))
        for words in ("needs you", "ready to accept", "dropped", "in review", "working"):
            self.assertIn(words, cells)
        self.assertIn("`◐`", cells["in review"])
        self.assertIn("`○`", cells["working"])
        # the turning marks: the first frame of each animation in board/src/ui.rs
        ui = (ROOT / "board" / "src" / "ui.rs").read_text()
        for name, words in (("REVIEW", "in review"), ("WORKING", "working")):
            first = re.search(name + r': &\[&str\] = &\["(.)"', ui).group(1)
            self.assertIn(f"`{first}`", cells[words])
        spin = re.search(r'SPINNER: &\[&str\] = &\["(.)"', ui).group(1)
        self.assertTrue(any(c.startswith(f"`{spin}`") for c in cells.values()))
        # every key of the board (BOARD_KEYS in board/src/main.rs, the tour's aside), in its order
        main = (ROOT / "board" / "src" / "main.rs").read_text()
        keys = main[main.index("pub const BOARD_KEYS"):main.index('("In a tour"')]
        board = [k.split()[0] for k in re.findall(r'\("([^"]+)", "', keys)]
        table = [re.match(r"`([^`]+)`", c).group(1) for c in cells.values() if re.match(r"`[^`]+`", c)]
        listed = [k for k in table if k in board]
        self.assertEqual(listed, board)

    def test_a_narrow_terminal_gets_the_boards_rows(self):
        a = {"id": "BP-3", "status": "needs", "title": "Fix the login redirect after sign-in",
             "why": "Asks: python3 or python? The Makefile uses both and the CI image only has python3.",
             "since": self.ago(12)}
        b = {"id": "BP-16", "status": "todo", "title": "Emails: digest", "since": self.ago(1)}
        r = self.repo(a, b)
        self.assertEqual(list(show.ls_lines(r, [b, a], 42)), [
            "● BP-3   Fix the login redirect aft…  12m",
            "         Asks: python3 or python? The",
            "         Makefile uses both and the CI …",
            "◌ BP-16  Emails: digest                1m",
            "         Not started. s starts it."])
        self.assertTrue(all(len(l) <= 41 for l in show.ls_lines(r, [b, a], 42)))
        self.assertEqual(len(list(show.ls_lines(r, [b, a], 200))), 2)  # wide: one line each
        self.assertEqual(len(list(show.ls_lines(r, [b, a]))), 2)  # piped: whole
        wide = list(show.ls_lines(r, [b, a], 80))  # room for the sentence: one cut line each, not rows
        self.assertEqual(wide, ["● BP-3   Fix the login redirect after sign-in  12m  Asks: python3 or python? T…",
                                "◌ BP-16  Emails: digest  1m  Not started. s starts it."])
        self.assertTrue(all(len(l) <= 79 for l in wide))
        c = {"id": "BP-5", "status": "todo", "title": "Later", "note": "One two three four five six seven eight"}
        self.assertEqual(len(task_line.task_rows(self.repo(c), c, width=20)), 2)  # the board's one line
        self.assertEqual(len(task_line.task_rows(self.repo(c), c, width=20, lines=99)), 5)  # show's: all of it

    def test_next_says_what_you_can_do_now(self):
        n = lambda **t: show.next_steps(self.repo(), {"id": "BP-4", **t})
        self.assertEqual(n(status="needs", stop={"kind": "prompt"}),
                         "`bullpen answer BP-4 <option>` (`bullpen peek BP-4` shows the prompt), or i on the board")
        self.assertEqual(n(status="needs", ask=True), "`bullpen reply BP-4 <answer>`, or i on the board")
        self.assertIn("`bullpen accept BP-4`", n(status="ready"))
        self.assertIn("`bullpen close BP-4`", n(status="finished"))
        self.assertIn("`bullpen resume BP-4`", n(status="stopped"))
        self.assertEqual(n(status="done"), "nothing: it's closed")

    def test_the_picker_shows_marks_in_colour(self):
        p = pick.Picker(["● BP-1  Fix login", "◌ BP-2  Cover logout"], ["1;33", "2"])
        out = p.screen(80, 5)
        self.assertEqual(out[2], "\x1b[7m> \x1b[1;33m●\x1b[22;39m BP-1  Fix login\x1b[0m")  # still reversed after
        self.assertEqual(out[3], "  \x1b[2m◌\x1b[22;39m BP-2  Cover logout")
        self.assertEqual(p.lines, ["● BP-1  Fix login", "◌ BP-2  Cover logout"])  # what's searched has no colour

    # ---- pick ----

    def test_fuzzy_matches_each_word_in_order_ignoring_case(self):
        f = pick.fuzzy_score
        self.assertIsNotNone(f("lgn", "Fix the LOGIN redirect"))
        self.assertIsNone(f("ngl", "Fix the login redirect"))  # out of order
        self.assertIsNotNone(f("redirect fix", "Fix the login redirect"))  # words in any order
        self.assertIsNone(f("fix logout", "Fix the login redirect"))  # every word must match
        self.assertIsNone(f("digest", "Migrate sessions table"))  # each word in one word
        self.assertIsNotNone(f("rl", "Rate-limit the endpoint"))
        self.assertEqual(f("", "anything"), 0)

    def test_fuzzy_ranks_letters_together_and_word_starts_first(self):
        pf = pick.pick_filter
        self.assertEqual(pf("log", ["lxoxg", "a login"]), [1, 0])  # together
        self.assertEqual(pf("log", ["l o g", "a login"]), [1])  # not across words
        self.assertEqual(pf("re", ["more", "redirect"]), [1, 0])  # the start of a word
        self.assertEqual(pf("42", ["BP-142", "BP-42"]), [1, 0])
        self.assertEqual(pf("bp", ["BP-1", "BP-2"]), [0, 1])  # ties keep their order
        self.assertEqual(pf("x", ["a", "b"]), [])

    def test_picker_keys(self):
        p = pick.Picker(["BP-1  working  Fix login", "BP-2  queued  Cover logout", "BP-3  ready  Docs"])
        self.assertEqual(p.shown, [0, 1, 2])
        for k in "lo":
            p.key(k)
        self.assertEqual((p.query, p.shown, p.sel), ("lo", [0, 1], 0))
        p.key("\x1b[B")
        p.key("\x1b[B")  # can't go past the last
        self.assertEqual(p.sel, 1)
        self.assertEqual(p.key("\r"), "choose")
        self.assertEqual(p.chosen(), 1)
        p.key("\x7f")
        self.assertEqual((p.query, p.sel), ("l", 0))  # a new search starts at the top
        p.key("\x10")  # ctrl+p: can't go above the first
        self.assertEqual(p.sel, 0)
        for k in " zz":
            p.key(k)
        self.assertEqual(p.shown, [])
        self.assertIsNone(p.key("\r"))  # nothing to choose
        p.key("\x17")  # ctrl+w: the last word
        self.assertEqual(p.query, "l ")
        p.key("\x15")  # ctrl+u: all of it
        self.assertEqual((p.query, p.shown), ("", [0, 1, 2]))
        self.assertEqual(p.key("\x04"), "cancel")  # ctrl+d with nothing typed
        self.assertEqual(p.key("\x1b"), "cancel")
        self.assertEqual(p.key("\x03"), "cancel")
        p.key("\x1b[C")  # another escape sequence does nothing
        self.assertEqual(p.query, "")

    def test_picker_screen_keeps_the_selected_task_in_view(self):
        p = pick.Picker([f"BP-{n}  queued  Task number {n}" for n in range(1, 11)])
        for _ in range(6):
            p.key("\x0e")  # ctrl+n
        out = p.screen(13, 5)  # the query, a hint and three tasks
        self.assertEqual(out[0], "jump to> ")
        self.assertIn("10/10", out[1])
        self.assertEqual(out[2:], ["  BP-5  queu…", "  BP-6  queu…", "\x1b[7m> BP-7  queu…\x1b[0m"])
        self.assertIn("no open tasks in any repository", pick.Picker([]).screen(80, 5)[1])

    def test_picker_says_a_failure_in_the_normal_weight_after_the_count(self):
        p = pick.Picker(["◌ BP-1  Fix login", "◌ BP-2  Cover logout"])
        self.assertEqual(p.screen(42, 5)[1], "\x1b[2m2/2  ⏎ go · esc close · ↑↓ move\x1b[0m")
        p.msg = "no board: run `bullpen up`"
        self.assertEqual(p.screen(42, 5)[1], "\x1b[2m2/2\x1b[0m  no board: run `bullpen up`")
        self.assertEqual(p.screen(20, 5)[1], "\x1b[2m2/2\x1b[0m  no board: run …")  # cut to the width

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

    def test_the_picker_edits_its_query_at_the_cursor(self):
        p = pick.Picker(["BP-1  working  Fix login", "BP-2  queued  Cover logout"])
        for k in "lgn":
            p.key(k)
        p.key("\x1b[D")
        p.key("\x1b[B")  # down moves the selection, not the cursor
        self.assertEqual(p.sel, 1 if len(p.shown) > 1 else 0)
        p.key("o")
        self.assertEqual((p.query, p.sel), ("lgon", 0))  # a new search starts at the top
        self.assertEqual(p.typed(42), ("lgon", 13))  # the cursor before n
        p.key("\x1b[D")
        self.assertEqual(p.sel, 0)  # moving the cursor isn't a new search
        self.assertEqual(p.screen(42, 5)[0], "jump to> lgon")

    def test_split_keys_keeps_a_paste_and_alt_keys_whole(self):
        self.assertEqual(line_edit.split_keys("a\x1b[200~x\ny\x1b[201~\x1bb\x1b\r\x1b\x1b"),
                         ["a", "\x1b[200~x\ny\x1b[201~", "\x1bb", "\x1b\r", "\x1b", "\x1b"])

    def test_split_keys_keeps_escape_sequences_whole(self):
        self.assertEqual(line_edit.split_keys("ab\x1b[Bc\x1bOA\x1b"), ["a", "b", "\x1b[B", "c", "\x1bOA", "\x1b"])

    def test_rex_versions_compare_previews_before_releases(self):
        key = doctor.rex_version_key
        self.assertLess(key("0.1.0-preview.12"), doctor.REX_MIN)
        self.assertEqual(key("0.1.0-preview.13"), doctor.REX_MIN)
        self.assertGreater(key("0.1.0"), key("0.1.0-preview.99"))
        self.assertGreater(key("v0.2.0-preview.1"), doctor.REX_MIN)
        self.assertIsNone(key("dev"))

    def test_a_shebangs_program_skips_envs_options(self):
        prog = doctor.shebang_program
        self.assertEqual(prog(["/bin/bash", "-e"]), "/bin/bash")
        self.assertEqual(prog(["/usr/bin/env", "bash"]), "bash")
        self.assertEqual(prog(["/usr/bin/env", "-S", "bash", "-e"]), "bash")
        self.assertEqual(prog(["/usr/bin/env", "-Sbash", "-e"]), "bash")
        self.assertEqual(prog(["/usr/bin/env", "-i", "FOO=1", "-u", "BAR", "python3"]), "python3")
        self.assertIsNone(prog(["/usr/bin/env", "-S"]))

    # ---------- reading prompts ----------

    # Claude Code's AskUserQuestion menu, as `rex capture` shows it (a real one)
    CHOICE = textwrap.dedent("""\
        ────────────────────────────────────────
         ☐ Drink

        Do you prefer tea or coffee?

        ❯ 1. Tea
             You prefer tea.
          2. Coffee
             You prefer coffee.
          3. Type something.
        ────────────────────────────────────────
          4. Chat about this

        Enter to select · ↑/↓ to navigate · Esc to cancel
        """)

    # the boxed layout of older Claude Code versions
    BOXED = textwrap.dedent("""\
        ╭──────────────────────────────────────╮
        │ Bash command                         │
        │                                      │
        │   npm test                           │
        │                                      │
        │ Do you want to proceed?              │
        │ ❯ 1. Yes                             │
        │   2. No, and tell Claude what to do  │
        │      differently (esc)               │
        ╰──────────────────────────────────────╯
        """)

    def test_a_choice_menu_is_read_without_type_something(self):
        p = answer.read_prompt(self.CHOICE)
        self.assertEqual(p["question"], "Do you prefer tea or coffee?")
        self.assertEqual(p["options"], [["1", "Tea"], ["2", "Coffee"], ["4", "Chat about this"]])

    def test_the_boxed_layout_is_read_too(self):
        p = answer.read_prompt(self.BOXED)
        self.assertEqual((p["question"], p["options"]),
                         ("Do you want to proceed?", [["1", "Yes"], ["2", "No, and tell Claude what to do"]]))

    def test_a_yes_no_question_is_answered_by_y_or_n(self):
        p = answer.read_prompt("Some output\nOverwrite it? [y/N]\n\n")
        self.assertEqual((p["question"], p["options"]), ("Overwrite it? [y/N]", [["y", "Yes"], ["n", "No"]]))

    def test_what_the_agent_wrote_is_no_prompt(self):
        listed = "● Two ways:\n  1. Patch it\n  2. Rewrite it\n\n────\n❯ \n────\n  ⏸ manual mode on\n"
        self.assertIsNone(answer.read_prompt(listed))  # its input box is under it
        self.assertIsNone(answer.read_prompt("● Two ways:\n  1. Patch it\n  2. Rewrite it\n"))  # no ❯ on one
        self.assertIsNone(answer.read_prompt("Do you want to proceed?\n❯ 1. Yes\n  2. No\n" + "more\n" * 4))
        self.assertIsNone(answer.read_prompt("❯ 2. Yes\n  3. No\n"))  # not counted from 1
        self.assertIsNone(answer.read_prompt(""))

    def test_a_prompt_id_tells_one_command_from_another(self):
        a = answer.read_prompt(self.BOXED)
        self.assertEqual(a["id"], answer.read_prompt("old output\n" * 20 + self.BOXED)["id"])
        b = answer.read_prompt(self.BOXED.replace("npm test", "rm -rf /"))
        self.assertNotEqual(a["id"], b["id"])

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
        on = core.on_reviewer
        self.assertTrue(on(self.stopped("needs", "reviewer", "prompt")))
        self.assertTrue(on(self.stopped("triage", "reviewer", "exited")))
        self.assertTrue(on(self.stopped("needs", "reviewer", "ended", why="Delete it?", ask=True)))
        self.assertFalse(on(self.stopped("triage", "reviewer", "limit")))  # seen to as its worker's
        self.assertFalse(on(self.stopped("triage", "worker", "ended")))
        self.assertFalse(on({"status": "review"}))
        t = self.stopped("needs", "reviewer", "ended", review={"block": "blk2", "session": "s"}, block="blk1")
        self.assertEqual(check.waited_on(t), ("reviewer", "s", "blk2"))
        t["stop"] = {"by": "worker", "kind": "failed"}
        self.assertEqual(check.waited_on(t), ("worker", None, "blk1"))

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

    def test_a_working_task_only_shows_what_github_sent_it_back_for(self):
        gh = {"state": "open", "checks": "passing", "new": 0}
        self.assertEqual(task_line.pr_phrase({"status": "working", "gh": gh}), "")
        self.assertEqual(task_line.pr_phrase({"status": "working", "gh": {**gh, "sent": True}}), "checks passing")
        self.assertEqual(task_line.pr_phrase({"status": "ready", "gh": gh}), "checks passing")

    def test_only_the_agent_a_task_waits_on_is_stuck_or_limited(self):
        import datetime as dt
        ago = (dt.datetime.now().astimezone() - dt.timedelta(minutes=25)).isoformat()
        t = {"status": "working", "stuck": {"who": "worker", "since": ago}}
        self.assertTrue(task_line.is_stuck(t))
        self.assertEqual(task_line.stuck_phrase(t), "no progress for 25 min")
        self.assertFalse(task_line.is_stuck({**t, "status": "review"}))
        self.assertFalse(task_line.is_stuck({"status": "working"}))
        at = dt.datetime(2026, 11, 11, 13, 0).astimezone()
        lim = {"who": "worker", "resets": at.replace(hour=15).isoformat(), "text": "x"}
        self.assertTrue(limit.is_limited({"status": "working", "limit": lim}))
        self.assertFalse(limit.is_limited({"status": "ready", "limit": lim}))
        self.assertEqual(limit.limit_why(lim, at), "Usage limit, resets 3:00 PM")

    def test_reset_times_are_read(self):
        import datetime as dt
        at = dt.datetime(2026, 11, 11, 13, 0).astimezone()  # a Wednesday, 1 PM
        when = lambda text: limit.limit_reset(text, at)
        day = lambda d, h, m=0: dt.datetime(2026, 11, d, h, m).astimezone()
        self.assertEqual(when("You've hit your 5-hour limit · resets 3pm"), day(11, 15))
        self.assertEqual(when("You've hit your session limit · resets 3:45pm"), day(11, 15, 45))
        self.assertEqual(when("Resets at 3:10 PM"), day(11, 15, 10))
        self.assertEqual(when("resets 15:00"), day(11, 15))
        self.assertEqual(when("You've hit your Opus limit · resets 9am"), day(12, 9))  # tomorrow
        self.assertEqual(when("resets 12:58pm"), day(11, 12, 58))  # just gone: now, not tomorrow
        self.assertEqual(when("You've hit your weekly limit · resets Mon 12:00am"), day(16, 0))
        self.assertEqual(when("weekly limit · resets Nov 14, 3pm · progress saved"), day(14, 15))
        self.assertEqual(when("Claude AI usage limit reached|1794430800"),
                         dt.datetime.fromtimestamp(1794430800).astimezone())
        ny = when("Claude usage limit reached. Your limit will reset at 3pm (America/New_York).")
        self.assertEqual((ny.utcoffset(), ny.astimezone(dt.timezone.utc).hour),
                         (at.utcoffset(), 20))  # 3 PM in New York, told in local time
        for text in ("Usage limit reached", "spend limit reached · resets Aug 8", "resets in 8 minutes",
                     "resets 3", "resets 13pm", "resets 3pm (Nowhere/Land)"):
            self.assertIsNone(when(text), text)
        self.assertEqual(limit.clock(day(11, 15), at), "3:00 PM")
        self.assertEqual(limit.clock(day(16, 0), at), "Mon 12:00 AM")
        self.assertEqual(limit.clock(day(30, 9, 5), at), "Nov 30 9:05 AM")
