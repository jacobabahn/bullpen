"""Guided review tours: the tour agent (a fake), the tour file, comments sent back."""

import json
import os
import re
import select
import subprocess
import textwrap
import time
import unittest
from pathlib import Path

from support import setUpModule, tearDownModule, RUN, sh, BullpenTest  # a temp folder for each module


FAKE_TOUR = textwrap.dedent("""\
    #!/usr/bin/env python3
    import os, sys
    from pathlib import Path
    d = Path(os.environ["FAKE_DIR"])
    (d / "tour_prompt.md").write_text(sys.stdin.read())  # the prompt comes on stdin
    with open(d / "tour.log", "a") as f:
        f.write(" ".join(sys.argv[1:]) + "\\n")
    out = d / "tour_out"  # what the tests make the agent answer
    sys.stdout.write(out.read_text() if out.exists() else "")
    if (d / "tour_out_next").exists():  # what it answers when asked again
        (d / "tour_out_next").rename(out)
    sys.exit(int((d / "tour_exit").read_text()) if (d / "tour_exit").exists() else 0)
""")


class TourTest(unittest.TestCase):
    """Guided review tours: the tour agent (a fake), the tour file, comments sent back."""

    setUp_bullpen = BullpenTest.setUp
    bp = BullpenTest.bp
    rex_calls = BullpenTest.rex_calls
    api = BullpenTest.api
    typed = BullpenTest.typed
    show = BullpenTest.show
    start_task = BullpenTest.start_task
    block = BullpenTest.block
    commit_and_ready = BullpenTest.commit_and_ready
    verdict = BullpenTest.verdict
    news = BullpenTest.news
    commit_file = BullpenTest.commit_file
    ready = BullpenTest.ready

    def setUp(self):
        self.setUp_bullpen()
        agent = self.fake / "tour-agent"
        agent.write_text(FAKE_TOUR)
        agent.chmod(0o755)
        (self.tmp / "xdg" / "bullpen").mkdir(parents=True)
        (self.tmp / "xdg" / "bullpen" / "config.json").write_text(json.dumps({"tour": [str(agent)]}))

    def answer(self, steps):
        (self.fake / "tour_out").write_text(json.dumps({"steps": steps}))

    def agent_runs(self):
        log = self.fake / "tour.log"
        return len(log.read_text().splitlines()) if log.exists() else 0

    def two_file_task(self):
        """An approved task changing two files: a.txt in two hunks, b.txt in one."""
        lines = [f"line {i}" for i in range(1, 31)]
        (self.repo / "a.txt").write_text("\n".join(lines) + "\n")
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "a.txt")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")
        tid = self.start_task()
        lines[1], lines[25] = "line 2, changed", "line 26, changed"
        self.commit_file(tid, "a.txt", "\n".join(lines) + "\n")
        self.commit_and_ready(tid)  # adds fix1.txt
        self.verdict(tid, "approve")
        return tid

    def tour_file(self, tid):
        return json.loads(next((self.tmp / "state").rglob(f"{tid}.tour.json")).read_text())

    def test_a_tour_of_a_ready_task_covers_every_hunk(self):
        tid = self.two_file_task()
        self.answer([{"title": "The fix", "note": "Why it changed.", "hunks": [{"file": "fix1.txt", "lines": [1, 1]}]},
                     {"title": "The top of a.txt", "note": "Line 2.", "hunks": [{"file": "a.txt", "lines": [2, 2]},
                                                                               {"file": "nope.txt", "lines": [1, 2]}]}])
        path = self.bp("tour", tid, "--wait").stdout.strip()
        self.assertTrue(path.endswith(f"{tid}.tour.json"))
        tour = self.tour_file(tid)
        wt = self.show(tid)["worktree"]
        self.assertEqual(tour["commit"], sh("git", "-C", wt, "rev-parse", "HEAD").strip())
        self.assertTrue(tour["patch"])
        self.assertEqual(tour["model"], None)  # not claude: no --model
        self.assertEqual([s["title"] for s in tour["steps"]], ["The fix", "The top of a.txt", "Other changes"])
        self.assertEqual(tour["steps"][1]["hunks"], [{"file": "a.txt", "lines": [2, 2]}])  # nope.txt left out
        self.assertEqual(tour["steps"][2]["hunks"], [{"file": "a.txt", "lines": [23, 29]}])  # the hunk at line 26
        prompt = (self.fake / "tour_prompt.md").read_text()
        for given in ("Make it work.", "Did it.", "Looks right.", "+line 26, changed", "- a.txt: 1-5, 23-29"):
            self.assertIn(given, prompt)
        t = self.show(tid)
        self.assertEqual((t["status"], t["tour"]["state"]), ("ready", "written"))
        # a current tour: its path, and no new agent
        self.assertEqual(self.bp("tour", tid).stdout.strip(), path)
        self.assertEqual(self.agent_runs(), 1)
        shown = json.loads(self.bp("tour", tid, "--show", "--json").stdout)
        self.assertEqual((shown["state"], len(shown["steps"])), ("current", 3))
        self.assertEqual(shown["steps"][2]["files"], "a.txt 23-29")
        step = self.bp("tour", tid, "--step", "3").stdout
        self.assertIn("diff --git a/a.txt b/a.txt", step)
        self.assertIn("+line 26, changed", step)
        self.assertNotIn("line 2, changed", step)  # step 2's hunk, from git
        step = json.loads(self.bp("tour", tid, "--step", "2", "--json").stdout)
        self.assertEqual((step["title"], step["note"]), ("The top of a.txt", "Line 2."))
        self.assertIn("+line 2, changed", step["diff"])
        self.assertEqual(self.news()[-1], "bullpen · BP-1 is ready: Fix the thing (2 commits, PR #7, approved by "
                         "light review: 5 lines in 2 files)")  # a tour is no news

    def test_a_tour_is_refused_while_the_task_is_working(self):
        tid = self.start_task()
        p = self.bp("tour", tid, ok=False)
        self.assertIn("is working: a tour is only for a task that is ready or in review", p.stderr)
        self.assertEqual(self.agent_runs(), 0)
        self.assertNotIn("tour", self.show(tid))

    def test_a_tour_of_a_task_in_review_runs_in_the_background(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.assertEqual(self.show(tid)["status"], "review")
        self.answer([{"title": "All of it", "note": "n", "hunks": [{"file": "fix1.txt"}]}])
        self.assertIn("writing a tour of BP-1 in the background", self.bp("tour", tid).stdout)
        for _ in range(100):
            if self.show(tid)["tour"]["state"] != "writing":
                break
            time.sleep(0.1)
        self.assertEqual(self.show(tid)["tour"]["state"], "written")
        self.assertEqual([s["title"] for s in self.tour_file(tid)["steps"]], ["All of it"])
        self.assertEqual(self.show(tid)["status"], "review")

    def test_a_tour_stays_current_after_a_clean_rebase_and_goes_stale_when_the_code_changes(self):
        tid = self.two_file_task()
        self.answer([{"title": "All", "note": "n", "hunks": [{"file": "a.txt"}, {"file": "fix1.txt"}]}])
        path = self.bp("tour", tid, "--wait").stdout.strip()
        wt = self.show(tid)["worktree"]
        (self.repo / "other.txt").write_text("other\n")
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "Other work")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")
        sh("git", "-C", wt, "fetch", "-q", "origin")
        sh("git", "-C", wt, "rebase", "-q", "origin/main")
        self.assertNotEqual(sh("git", "-C", wt, "rev-parse", "HEAD").strip(), self.tour_file(tid)["commit"])
        shown = json.loads(self.bp("tour", tid, "--show", "--json").stdout)
        self.assertEqual(shown["state"], "current")  # the same patch
        self.assertIn("+line 26, changed", self.bp("tour", tid, "--step", "1").stdout)
        self.commit_file(tid, "fix1.txt", "fixed better\n")
        shown = json.loads(self.bp("tour", tid, "--show", "--json").stdout)
        self.assertEqual(shown["state"], "stale")
        self.assertIn("the code changed since the tour was written", shown["why"])
        p = self.bp("tour", tid)
        self.assertEqual(p.stdout.strip(), path)  # the old one, said to be stale
        self.assertIn("is stale", p.stderr)
        self.assertIn(f"`bullpen tour {tid} --new`", p.stderr)
        self.assertEqual(self.agent_runs(), 1)
        self.bp("tour", tid, "--new", "--wait")
        self.assertEqual(self.agent_runs(), 2)
        self.assertEqual(json.loads(self.bp("tour", tid, "--show", "--json").stdout)["state"], "current")

    def test_a_failed_tour_is_a_line_on_the_board_and_g_retries(self):
        tid = self.two_file_task()
        before = len(self.typed())
        (self.fake / "tour_out").write_text("Here is your tour! It's great.")
        p = self.bp("tour", tid, "--wait", ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("doesn't parse", p.stderr)
        t = self.show(tid)
        self.assertEqual((t["status"], t["tour"]["state"]), ("ready", "failed"))
        self.assertTrue(t["note"].startswith("tour failed: its tour doesn't parse"))
        self.assertIn("Here is your tour!", next((self.tmp / "state").rglob(f"{tid}.tour.log")).read_text())
        self.assertEqual(len(self.typed()), before)  # no news
        (self.fake / "tour_out").write_text('{"steps": [{"title": "x", "note": "y", "hunks": "a.txt"}]}')
        self.assertIn("isn't in the format: step 1's note must be text and its hunks a list",
                      self.bp("tour", tid, "--wait", ok=False).stderr)
        (self.fake / "tour_exit").write_text("3")
        self.assertIn("the agent exited with 3", self.bp("tour", tid, "--wait", ok=False).stderr)
        (self.fake / "tour_exit").unlink()
        self.answer([{"title": "x", "note": "y", "hunks": []}])
        self.bp("tour", tid, "--wait")
        t = self.show(tid)
        self.assertEqual((t["tour"]["state"], t["note"]), ("written", None))
        self.assertEqual([s["title"] for s in self.tour_file(tid)["steps"]], ["x", "Other changes"])
        # a writer that died is a failure too, not a tour forever being written
        state = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
        t = json.loads(state.read_text())
        state.write_text(json.dumps({**t, "tour": {"state": "writing", "pid": 999999999}}))
        shown = json.loads(self.bp("tour", tid, "--show", "--json").stdout)
        self.assertEqual(shown["state"], "failed")
        self.assertIn("stopped before the tour was written", shown["why"])

    def tour_log(self, tid):
        return next((self.tmp / "state").rglob(f"{tid}.tour.log")).read_text()

    def test_a_reply_with_a_corrected_tour_uses_the_last_tour_in_it(self):
        tid = self.two_file_task()
        first = json.dumps({"steps": [{"title": "PLACEHOLDER", "note": "", "hunks": []}]})
        second = json.dumps({"steps": [{"title": "The fix", "note": "Why.",
                                        "hunks": [{"file": "a.txt", "lines": [2, 2]}]}]})
        (self.fake / "tour_out").write_text(
            f"{first}\n\nWait, correcting that: here is the full tour.\n\n{second}\n")
        self.bp("tour", tid, "--wait")
        self.assertEqual([s["title"] for s in self.tour_file(tid)["steps"]], ["The fix", "Other changes"])
        self.assertEqual(self.agent_runs(), 1)  # no need to ask again
        self.assertIn("its answer had text around the tour (2 JSON objects)", self.tour_log(tid))

    def test_a_tour_in_a_code_fence_is_read(self):
        tid = self.two_file_task()
        tour = {"steps": [{"title": "All of it", "note": "", "hunks": []}]}
        (self.fake / "tour_out").write_text(f"```json\n{json.dumps(tour, indent=2)}\n```\n")
        self.bp("tour", tid, "--wait")
        self.assertEqual([s["title"] for s in self.tour_file(tid)["steps"]], ["All of it", "Other changes"])
        self.assertEqual(self.agent_runs(), 1)

    def test_an_answer_that_isnt_a_tour_is_asked_for_once_more(self):
        tid = self.two_file_task()
        (self.fake / "tour_out").write_text('{"steps": [{"title": "x", "hunks": [{"file": "a.txt"')  # cut off
        (self.fake / "tour_out_next").write_text(json.dumps({"steps": [{"title": "x", "note": "", "hunks": []}]}))
        self.bp("tour", tid, "--wait")
        self.assertEqual(self.agent_runs(), 2)
        self.assertEqual(self.show(tid)["tour"]["state"], "written")
        log = self.tour_log(tid)
        self.assertIn("attempt 1: its tour doesn't parse", log)
        self.assertIn("attempt 2: ", log)
        self.assertEqual(log.count("--- stdout"), 2)  # both answers are in the log
        # twice not a tour: the tour fails, after the second attempt
        (self.fake / "tour.log").unlink()
        (self.fake / "tour_out").write_text("no tour here")
        self.assertIn("doesn't parse", self.bp("tour", tid, "--new", "--wait", ok=False).stderr)
        self.assertEqual(self.agent_runs(), 2)
        # an agent that fails isn't asked again
        (self.fake / "tour.log").unlink()
        (self.fake / "tour_exit").write_text("3")
        self.bp("tour", tid, "--wait", ok=False)
        self.assertEqual(self.agent_runs(), 1)

    def test_claude_answers_with_structured_output(self):
        tid = self.two_file_task()
        claude = self.fake / "bin-claude" / "claude"
        claude.parent.mkdir()
        claude.write_text(FAKE_TOUR)
        claude.chmod(0o755)
        (self.tmp / "xdg" / "bullpen" / "config.json").write_text(json.dumps({"tour": [str(claude)]}))
        (self.fake / "tour_out").write_text(json.dumps({
            "type": "result", "is_error": False, "result": "{not quite json",
            "structured_output": {"steps": [{"title": "Structured", "note": "", "hunks": []}]}}))
        self.bp("tour", tid, "--wait")
        self.assertEqual([s["title"] for s in self.tour_file(tid)["steps"]], ["Structured", "Other changes"])
        args = (self.fake / "tour.log").read_text().split()
        schema = json.loads(args[args.index("--json-schema") + 1])
        self.assertEqual(schema["required"], ["steps"])
        self.assertIn("-p", args)

    def toured(self):
        tid = self.two_file_task()
        self.answer([{"title": "The fix", "note": "n", "hunks": [{"file": "fix1.txt", "lines": [1, 1]}]},
                     {"title": "a.txt", "note": "n", "hunks": [{"file": "a.txt", "lines": [2, 2]},
                                                              {"file": "a.txt", "lines": [26, 26]}]}])
        self.bp("tour", tid, "--wait")
        return tid

    def test_comments_are_drafts_until_sent_back_to_the_worker(self):
        tid = self.toured()
        worker = self.block(tid)
        self.assertEqual(self.bp("tour-comment", tid, "1", "Say", "why", "fixed.").stdout.strip(), "1")
        self.bp("tour-comment", tid, "2", "Line 26 should stay as it was.")
        self.bp("tour-comment", tid, "2", "A typo.")
        self.bp("tour-comment", tid, "--remove", "3")
        listed = json.loads(self.bp("tour-comment", tid, "--json").stdout)
        self.assertEqual([(c["id"], c["step"], c["text"]) for c in listed],
                         [(1, 1, "Say why fixed."), (2, 2, "Line 26 should stay as it was.")])
        self.assertIn("steps 1 to 2", self.bp("tour-comment", tid, "5", "x", ok=False).stderr)
        shown = json.loads(self.bp("tour", tid, "--show", "--json").stdout)
        self.assertEqual((shown["comments"], [s["comments"] for s in shown["steps"]]), (2, [1, 1]))
        self.assertIn("sent 2 comments", self.bp("tour", tid, "--send").stdout)
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        sent = Path(t["tour_sent"]["file"])
        self.assertEqual(sent.name, "BP-1.tour-1.comments.md")
        text = sent.read_text()
        self.assertIn("## Comment 1: step 1, The fix\n\nFiles: fix1.txt 1-1", text)
        self.assertIn("## Comment 2: step 2, a.txt\n\nFiles: a.txt 2-2, a.txt 26-26", text)
        self.assertIn("Line 26 should stay as it was.", text)
        block, line = self.typed()[-2]
        self.assertEqual(block, worker)
        self.assertIn(f"Read {sent}, address each comment", line)
        self.assertEqual(self.news()[-1], "bullpen · BP-1 sent back by the person after the tour: 2 comments")
        self.assertEqual(json.loads(self.bp("tour-comment", tid, "--json").stdout), [])  # no drafts left
        self.assertIn("is working", self.bp("tour", tid, "--send", ok=False).stderr)

    def test_the_review_of_the_persons_comments_doesnt_count_toward_the_rounds(self):
        tid = self.toured()  # round 1 approved
        self.bp("config", "--review-rounds", "1")
        self.bp("tour-comment", tid, "2", "Keep line 26.")
        self.bp("tour", tid, "--send")
        self.commit_file(tid, "a.txt", "".join(f"line {i}\n" for i in range(1, 31)).replace("line 2\n", "line 2, changed\n"))
        self.assertIn("checks that the person's comments are addressed", self.ready(tid).stdout)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["kind"], r["tour"]), (1, "tour", 1))  # still round 1
        prompt = next((self.tmp / "state").rglob(f"{tid}.tour-1.review.md")).read_text()
        self.assertIn("Keep line 26.", prompt)
        self.assertIn("-line 26, changed", prompt)  # what changed since they were sent
        self.assertIn("confirm that each of the person's comments is addressed", prompt)
        # round 1 of 1 is used, yet its changes go back to the worker
        self.verdict(tid, "changes", "Line 26 still differs.")
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        self.assertNotIn("tour_sent", t)
        self.assertIn("BP-1.tour-1.review.findings.md", self.typed()[-1][1])
        self.assertIn("the person's comments aren't all addressed", self.typed()[-1][1])
        # the next ready is a follow-up of that review, and counts
        self.commit_file(tid, "fix1.txt", "fixed again\n")
        self.ready(tid)
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["kind"]), (2, "followup"))
        self.verdict(tid, "approve")
        self.assertIn("approved by follow-up review", self.news()[-1])

    def test_an_approved_review_of_the_comments_makes_it_ready(self):
        tid = self.toured()
        self.bp("tour-comment", tid, "1", "Fine, but say why.")
        self.bp("tour", tid, "--send")
        self.ready(tid)  # the worker disagreed and changed nothing: still reviewed
        r = self.show(tid)["review"]
        self.assertEqual((r["round"], r["kind"]), (1, "tour"))
        self.verdict(tid, "approve")
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertIn("approved by review of the person's comments", self.news()[-1])

    # ---------- the stepper, for the board's g ----------

    def test_g_opens_a_current_tour_in_a_large_pane_over_the_board(self):
        tid = self.toured()
        out = self.bp("tour", tid, "--pane", "--from", "blk1", "--session", "session:1").stdout
        self.assertIn(f"opened {tid}'s tour", out)
        layer = self.api("session.new_layer")[-1]
        block = layer["layout"]["block"]
        self.assertEqual(block["label"], f"bullpen: {tid} tour")
        self.assertEqual(block["options"]["command"][-4:], ["tour", tid, "--view", "--back"])
        self.assertEqual(layer["bounds"], [0.05, 0.04, 0.9, 0.9])
        self.assertEqual(layer["window_id"], "w1")  # over the pane it was asked from
        self.assertEqual(self.api("session.set_layer_state")[-1]["state"], "normal")
        self.assertTrue(self.api("session.set_layer_state")[-1]["focus"])
        before = len(self.api("session.new_layer"))
        closes = lambda: [a for a in self.rex_calls() if a[:2] == ["block", "close"]]
        closed = len(closes())
        self.bp("tour", tid, "--pane", "--from", "blk1", "--session", "session:1")  # one stepper per task
        self.assertEqual(len(self.api("session.new_layer")), before + 1)
        self.assertEqual(len(closes()), closed + 1)
        self.assertEqual(self.agent_runs(), 1)  # no new tour

    def test_g_with_no_tour_writes_one_and_opens_it_once_written(self):
        tid = self.two_file_task()
        self.answer([{"title": "All", "note": "n", "hunks": [{"file": "a.txt"}, {"file": "fix1.txt"}]}])
        out = self.bp("tour", tid, "--pane", "--from", "blk1", "--session", "session:1").stdout
        self.assertIn(f"writing a tour of {tid}: it opens when it's done", out)
        for _ in range(100):
            if self.show(tid)["tour"]["state"] != "writing":
                break
            time.sleep(0.1)
        self.assertEqual(self.show(tid)["tour"]["state"], "written")
        self.assertNotIn("open", self.show(tid)["tour"])
        layer = self.api("session.new_layer")[-1]
        self.assertEqual(layer["layout"]["block"]["label"], f"bullpen: {tid} tour")
        self.assertEqual(layer["window_id"], "w1")
        # a stale one opens as it is (the board asked first), and --new writes another
        self.commit_file(tid, "fix1.txt", "fixed better\n")
        self.assertIn("(stale: the code changed since it was written)",
                      self.bp("tour", tid, "--pane", "--from", "blk1").stdout)
        self.assertEqual(self.agent_runs(), 1)
        self.assertIn("writing a tour", self.bp("tour", tid, "--pane", "--new", "--from", "blk1").stdout)
        self.assertIn("is working", self.bp("tour", self.start_task("Other"), "--pane", ok=False).stderr)

    def stepper_on_a_terminal(self, tid, keys, how=("--view",)):
        """Run the stepper on a terminal of its own, type keys, and return what it drew
        after each (the first: before any key) and how it exited."""
        import pty
        master, slave = pty.openpty()
        p = subprocess.Popen([str(RUN), "tour", tid, *how], cwd=self.repo, env=self.env,
                             stdin=slave, stdout=slave, stderr=slave)
        os.close(slave)
        screens = []
        for k in keys + [None]:
            seen = b""
            while not (seen.endswith((b"\x1b[?25l", b"H")) and (b"close" in seen or b"cancels" in seen)):
                r, _, _ = select.select([master], [], [], 10)
                self.assertTrue(r, f"the stepper drew nothing: {seen!r}")
                try:
                    seen += os.read(master, 65536)
                except OSError:
                    break
            screens.append(re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", seen.decode(errors="replace")))
            if k is None:
                break
            os.write(master, k.encode())
            time.sleep(0.1)
        try:
            while select.select([master], [], [], 10)[0] and os.read(master, 65536):
                pass
        except OSError:
            pass
        os.close(master)
        return screens, p.wait(10)

    def test_tour_on_a_terminal_opens_the_stepper_and_elsewhere_prints_the_path(self):
        tid = self.toured()
        path = self.bp("tour", tid).stdout.strip()  # not a terminal (an agent): its path
        self.assertTrue(path.endswith(f"{tid}.tour.json"))
        screens, code = self.stepper_on_a_terminal(tid, ["q"], how=())
        self.assertEqual(code, 0)
        self.assertIn("step 1 of 2", screens[0])
        self.assertEqual(self.bp("tour", tid, "--path").stdout.strip(), path)

    def test_the_stepper_steps_takes_comments_resumes_and_sends(self):
        tid = self.toured()  # steps: The fix, a.txt
        screens, code = self.stepper_on_a_terminal(tid, ["n", "c", *"Keep line 26.", "\r", "q"])
        self.assertEqual(code, 0)
        self.assertIn("step 1 of 2", screens[0])
        self.assertIn("The fix", screens[0])
        self.assertIn("step 2 of 2", screens[1])
        self.assertIn("+line 2, changed", screens[1])  # its hunks, from git
        self.assertIn("Comment on step 2:", screens[2])
        self.assertIn("Saved comment 1 on step 2.", screens[-2])
        self.assertIn("Keep line 26.", screens[-2])  # under the step's note
        drafts = json.loads(self.bp("tour-comment", tid, "--json").stdout)
        self.assertEqual([(c["step"], c["text"]) for c in drafts], [(2, "Keep line 26.")])
        self.assertEqual(self.show(tid)["tour"]["opened"], self.tour_file(tid)["commit"])  # toured
        # opened again: it resumes at step 2; past the last step, the summary
        screens, code = self.stepper_on_a_terminal(tid, ["n", "s"])
        self.assertIn("step 2 of 2", screens[0])
        self.assertIn("End of the tour", screens[1])
        self.assertIn("Your 1 comment, not sent yet:", screens[1])
        self.assertIn("step 2, a.txt", screens[1])
        self.assertEqual(code, 0)
        t = self.show(tid)
        self.assertEqual(t["status"], "working")  # s sent them back
        self.assertIn("Keep line 26.", Path(t["tour_sent"]["file"]).read_text())
        self.assertEqual(self.news()[-1], "bullpen · BP-1 sent back by the person after the tour: 1 comment")

    def test_the_stepper_warns_before_accepting_with_comments_unsent(self):
        tid = self.toured()
        self.bp("tour-comment", tid, "1", "Hmm.")
        screens, code = self.stepper_on_a_terminal(tid, ["n", "n", "a", "a"])
        self.assertIn("1 comment not sent: a again accepts anyway", screens[3])
        self.assertEqual(code, 0)
        self.assertEqual(self.show(tid)["status"], "done")

    def test_the_stepper_tells_the_worker_and_v_closes_it_for_the_diff(self):
        tid = self.toured()
        screens, code = self.stepper_on_a_terminal(tid, ["t", *"Rename it", "\r", "v"])
        self.assertEqual(code, 0)
        self.assertIn("Tell BP-1:", screens[1])
        self.assertIn("Sent to BP-1.", screens[-2])
        self.assertIn((self.block(tid), "Rename it"), self.typed())
        self.assertEqual(self.show(tid)["status"], "working")  # told: back to work, as from the board
        self.assertEqual(self.api("session.new_layer")[-1]["layout"]["block"]["label"], f"bullpen: {tid} diff")

    def test_a_new_ready_is_not_toured_yet(self):
        tid = self.toured()
        self.stepper_on_a_terminal(tid, ["q"])
        self.assertIn("opened", self.show(tid)["tour"])
        self.commit_file(tid, "fix1.txt", "fixed better\n")
        self.ready(tid)
        self.assertNotIn("opened", self.show(tid)["tour"])
        self.assertTrue(self.show(tid)["tour"]["stale"])  # the board says it's not toured
        self.stepper_on_a_terminal(tid, ["q"])  # the old tour, opened anyway (the board's o)
        self.assertNotIn("opened", self.show(tid)["tour"])  # is no tour of this code

    def test_tour_model_is_a_layered_setting(self):
        self.assertEqual(json.loads(self.bp("config", "--json").stdout)["tour_model"], "sonnet")
        self.bp("config", "--global", "--tour-model", "haiku")
        cfg = json.loads(self.bp("config", "--json").stdout)
        self.assertEqual((cfg["tour_model"], cfg["sources"]["tour_model"]), ("haiku", "global"))
        self.bp("config", "--tour-model", "opus")
        cfg = json.loads(self.bp("config", "--json").stdout)
        self.assertEqual((cfg["tour_model"], cfg["sources"]["tour_model"]), ("opus", "repo"))
