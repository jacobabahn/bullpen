"""Triage, and notifications for the person."""

import json
import textwrap
from pathlib import Path

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module


class TriageTest(BullpenTest):
    def test_worker_stops_go_to_the_coordinator_and_prompts_to_the_person(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "error", "", "API error")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Failed: API error"))
        self.assertTrue(self.news()[-1].startswith("bullpen · For the coordinator: BP-1's turn failed: API error"))
        self.assertIn("goes to the person", self.news()[-1])
        self.bp("_event", blk, "working", "", "")  # clears by itself
        self.assertEqual(self.show(tid)["status"], "working")
        self.bp("_event", blk, "idle", "", "")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Ended its turn without saying ready"))
        self.assertTrue(self.news()[-1].startswith("bullpen · For the coordinator: BP-1 ended its turn without `ready`"))
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Needs you (permission): Allow rm?"))
        self.assertEqual(self.news()[-1], "bullpen · BP-1 is at a permission prompt: Allow rm?")

    def test_reviewer_stops_go_to_the_coordinator_and_prompts_to_the_person(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp("_event", reviewer, "blocked", "permission", "Allow curl?")
        self.assertEqual(self.show(tid)["status"], "needs")
        self.assertEqual(self.news()[-1], "bullpen · BP-1's reviewer is at a permission prompt: Allow curl?")
        self.bp("_event", reviewer, "working", "", "")
        self.assertEqual(self.show(tid)["status"], "review")
        self.bp("_event", reviewer, "idle", "", "")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Reviewer stopped without a verdict"))
        self.assertIn("For the coordinator: BP-1's reviewer stopped without a verdict", self.news()[-1])
        self.assertIn("`bullpen review BP-1`", self.news()[-1])
        self.bp("_gone", reviewer)
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Reviewer exited without a verdict"))
        self.assertIn("For the coordinator: BP-1's reviewer exited without a verdict", self.news()[-1])

    def test_ask_puts_the_question_to_the_person(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        p = self.bp("ask", tid, "Use", "SQLite or Postgres?")
        self.assertIn("needs the person", p.stdout)
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Use SQLite or Postgres?"))
        self.assertEqual(self.news()[-1], "bullpen · BP-1 needs the person: Use SQLite or Postgres?")
        self.assertEqual(json.loads(self.bp("go", "--next", "--json").stdout)["block"], blk)
        # its agent's own state doesn't undo it: the question is the person's
        self.agent(blk, "working", msg="Reading")
        self.assertEqual(self.check(), [])
        # the coordinator asking from its own pane hears nothing of it
        before = len(self.news())
        self.bp_in("blk1", "ask", tid, "Still SQLite?")
        self.assertEqual(len(self.news()), before)
        self.assertEqual(self.show(tid)["why"], "Still SQLite?")
        # the person answers in its pane: it works again
        self.bp("_event", blk, "working", "", "")
        t = self.show(tid)
        self.assertEqual((t["status"], t.get("ask")), ("working", None))
        self.assertNotEqual(self.bp("ask", tid, ok=False).returncode, 0)  # no question

    # Claude Code's permission prompt, as `rex capture` shows it (a real one, cut short)
    PERMISSION = textwrap.dedent("""\
        ● Creating empty hello.txt
          ⎿  $ touch hello.txt

        ────────────────────────────────────────
         Bash command

         touch hello.txt
         Create empty hello.txt in current directory
        ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
         Do you want to proceed?
         ❯ 1. Yes
           2. Yes, and always allow access to /tmp/x
              from this project
           3. No

         Esc to cancel · Tab to amend

        """)

    def at_prompt(self, blk, screen=None, who="worker"):
        (self.fake / f"screen_{blk}").write_text(screen or self.PERMISSION)
        self.bp("_event", blk, "blocked", "permission", "Bash command")

    def test_prompt_reads_the_question_off_the_agents_screen(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.assertIn("isn't waiting at a prompt", self.bp("prompt", tid, ok=False).stderr)
        self.at_prompt(blk)
        p = json.loads(self.bp("prompt", tid).stdout)
        self.assertEqual((p["who"], p["question"]), ("worker", "Do you want to proceed?"))
        self.assertEqual(p["options"], [["1", "Yes"], ["2", "Yes, and always allow access to /tmp/x"],
                                        ["3", "No"]])
        self.assertIn(" touch hello.txt", p["screen"])
        self.assertIn("--format=vt", [a for a in self.rex_calls() if a[0] == "capture"][-1])
        # answered in its pane meanwhile: the screen shows no prompt now
        (self.fake / f"screen_{blk}").write_text("● Done.\n\n❯ \n")
        p = json.loads(self.bp("prompt", tid).stdout)
        self.assertEqual((p["question"], p["options"], p["id"]), (None, [], None))

    def test_answer_presses_the_option_while_the_prompt_is_still_there(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.at_prompt(blk)
        pid = json.loads(self.bp("prompt", tid).stdout)["id"]
        p = self.bp("answer", tid, "3", "--prompt", pid)
        self.assertEqual(p.stdout.strip(), "answered BP-1's worker: 3 (No)")
        self.assertEqual(self.typed()[-1], (blk, "3"))
        self.assertEqual(self.rex_calls()[-1][0], "send")  # the number alone answers it: no enter
        # the watcher sees it work again, as when the person answers in the pane
        self.bp("_event", blk, "working", "", "")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_answer_refuses_a_prompt_that_is_gone_or_changed(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.at_prompt(blk)
        pid = json.loads(self.bp("prompt", tid).stdout)["id"]
        sent = len(self.typed())
        p = self.bp("answer", tid, "4", "--prompt", pid, ok=False)
        self.assertIn("4 isn't one of the prompt's options (1, 2, 3)", p.stderr)
        # another command asks the same question: not the prompt the person read
        (self.fake / f"screen_{blk}").write_text(self.PERMISSION.replace("touch hello.txt", "rm -rf build"))
        p = self.bp("answer", tid, "1", "--prompt", pid, ok=False)
        self.assertIn("isn't at that prompt any more: nothing was sent", p.stderr)
        (self.fake / f"screen_{blk}").write_text("● Done.\n\n❯ \n")
        self.assertNotEqual(self.bp("answer", tid, "1", ok=False).returncode, 0)
        self.assertEqual(len(self.typed()), sent)

    def test_answer_goes_to_the_reviewer_when_it_is_at_the_prompt(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        (self.fake / f"screen_{reviewer}").write_text("Overwrite notes.md? (y/n)\n")
        self.bp("_event", reviewer, "blocked", "permission", "Write")
        p = json.loads(self.bp("prompt", tid).stdout)
        self.assertEqual((p["who"], p["question"], p["options"]),
                         ("reviewer", "Overwrite notes.md? (y/n)", [["y", "Yes"], ["n", "No"]]))
        self.bp("answer", tid, "Y", "--prompt", p["id"])
        self.assertEqual(self.typed()[-1], (reviewer, "y"))
        (self.fake / "alive" / reviewer).unlink()
        self.assertIn("reviewer isn't running", self.bp("prompt", tid, ok=False).stderr)

    def asked(self, question="Use SQLite or Postgres?"):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "idle", "", "")
        self.bp_in("blk1", "ask", tid, question)
        return tid, blk

    def test_reply_goes_to_the_coordinator_tagged_with_the_task(self):
        tid, blk = self.asked()
        sent = len(self.typed())
        self.bp("reply", tid, "Postgres,", "it's", "on", "the", "server")
        self.assertEqual(self.typed()[sent:], [("blk1", "BP-1: Postgres, it's on the server")])  # not the worker
        t = self.show(tid)
        self.assertEqual((t["status"], t.get("ask"), t.get("ok")), ("working", None, True))
        log = next((self.tmp / "state").rglob("news.log")).read_text().splitlines()[-1]
        self.assertIn('bullpen · BP-1: the person answers "Use SQLite or Postgres?": Postgres, it\'s on the server', log)
        p = self.bp("reply", tid, "again", ok=False)
        self.assertIn("no question from its coordinator to answer", p.stderr)

    def test_reply_puts_a_task_asked_about_in_review_back_in_review(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp_in("blk1", "ask", tid, "Ship it today?")
        self.bp_in("blk1", "ask", tid, "Ship it today, or wait?")  # asked again: still from review
        self.bp("reply", tid, "Today")
        t = self.show(tid)
        self.assertEqual((t["status"], t.get("ask"), t.get("asked_from"), t.get("ok")), ("review", None, None, None))
        self.verdict(tid, "approve")  # its reviewer, still running, gives its verdict as usual
        self.assertEqual(self.show(tid)["status"], "ready")

    def test_reply_puts_a_ready_task_back_in_the_ready_queue(self):
        tid = self.start_task()
        self.bp("config", "--review", "off")
        wt = self.show(tid)["worktree"]
        (Path(wt) / "fix.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix")
        self.bp("ready", tid, "--summary", "Did it.", cwd=wt)
        self.assertEqual(self.show(tid)["status"], "ready")
        self.bp_in("blk1", "ask", tid, "Merge before the release?")
        self.bp("reply", tid, "Yes")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"], t.get("ok")), ("ready", None, None))
        # the answer still reaches its coordinator
        self.assertEqual(self.typed()[-1], ("blk1", "BP-1: Yes"))

    def test_reply_is_news_when_the_coordinator_follows_it(self):
        tid, blk = self.asked()
        self.bp("config", "--news", "monitor")
        sent = len(self.typed())
        self.bp("reply", tid, "Postgres")
        self.assertEqual(len(self.typed()), sent)  # nothing typed: its monitor reads it
        out = self.bp_in("blk1", "news").stdout
        self.assertIn('bullpen · BP-1: the person answers "Use SQLite or Postgres?": Postgres', out)

    def test_reply_to_a_task_whose_agent_is_gone_leaves_it_to_the_coordinator(self):
        tid, blk = self.asked()
        (self.fake / "alive" / blk).unlink()
        self.bp("reply", tid, "Postgres")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"], t["stop"]["kind"]),
                         ("triage", "The person answered: Postgres", "ended"))
        # no coordinator to take it: refused, and the question stays
        tid2 = self.bp("add", "Other", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        self.bp("_event", self.block(tid2), "idle", "", "")
        self.bp_in("blk1", "ask", tid2, "Which?")
        (self.fake / "alive" / "blk1").unlink()
        p = self.bp("reply", tid2, "That one", ok=False)
        self.assertIn("no coordinator is running to take the answer", p.stderr)
        self.assertEqual(self.show(tid2)["status"], "needs")

    def test_ok_clears_triage_without_typing(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.assertNotEqual(self.bp("ok", tid, ok=False).returncode, 0)  # working: nothing to clear
        self.bp("_event", blk, "idle", "", "")
        typed = len(self.typed())
        self.bp("ok", tid)
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"], t["ok"]), ("working", None, True))
        self.assertEqual(len(self.typed()), typed)  # nothing typed anywhere
        self.agent(blk, "idle")
        self.assertEqual(self.check(), [])  # the coordinator said it's fine
        self.agent(blk, "blocked", "permission", "Allow?")
        self.assertEqual(len(self.check()), 1)  # a prompt still counts
        self.bp("_event", blk, "working", "", "")
        self.assertNotIn("ok", self.show(tid))  # its next turn is reported as usual
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        (self.fake / "alive" / blk).unlink()
        p = self.bp("ok", tid, ok=False)
        self.assertIn("`bullpen resume BP-1`", p.stderr)

    def test_ok_puts_a_task_whose_reviewer_stopped_back_in_review(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp("_event", reviewer, "idle", "", "")
        self.bp("ok", tid)
        self.assertEqual(self.show(tid)["status"], "review")
        self.agent(reviewer, "idle")
        self.assertEqual(self.check(), [])
        self.bp("_event", reviewer, "working", "", "")
        self.bp("_event", reviewer, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        (self.fake / "alive" / reviewer).unlink()
        p = self.bp("ok", tid, ok=False)
        self.assertIn("`bullpen review BP-1`", p.stderr)

    def test_triage_goes_to_the_person_after_triage_minutes(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "error", "", "boom")
        self.ago(tid, 5)
        self.assertEqual(self.bp("check").stdout, "")
        self.bp("check", "--fix", "--quiet")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.ago(tid, 11)
        out = self.bp("check", ok=False)
        self.assertEqual(out.returncode, 1)
        self.assertEqual(out.stdout.strip(), "BP-1 waits on its coordinator, but its coordinator didn't "
                                             "resolve it in 10 min: it needs the person")
        self.assertEqual(self.show(tid)["status"], "triage")  # no --fix: nothing changes
        before = len(self.news())
        self.bp("check", "--fix", "--quiet")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Failed: boom (coordinator didn't resolve it)"))
        self.assertEqual(self.news()[before:], ["bullpen · BP-1 needs the person: Failed: boom "
                                                "(coordinator didn't resolve it)"])
        self.bp("check", "--fix", "--quiet")
        self.assertEqual(len(self.news()), before + 1)  # once
        # the repo's own setting
        tid2 = self.bp("add", "Two", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        self.bp("config", "--triage-minutes", "30")
        self.bp("_event", self.block(tid2), "idle", "", "")
        self.ago(tid2, 20)
        self.bp("check", "--fix", "--quiet", tid2)
        self.assertEqual(self.show(tid2)["status"], "triage")
        self.assertIn("in 30 min", [l for l in self.news() if "BP-2" in l][-1])

    def test_triage_needs_the_person_when_no_coordinator_runs(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "error", "", "boom")
        self.assertEqual(self.show(tid)["status"], "triage")
        (self.fake / "alive" / "blk1").unlink()  # the coordinator's pane is gone
        self.bp("check", "--fix", "--quiet")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Failed: boom (no coordinator is running)"))
        self.bp("_event", blk, "working", "", "")
        self.bp("_event", blk, "idle", "", "")  # straight to the person
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Ended its turn without saying ready"))
        line = [l for l in self.outbox_lines() if "ended its turn" in l][-1]
        self.assertIn("No coordinator is running to see to it, so it needs the person", line)
        self.assertNotIn("For the coordinator", line)

    def outbox_lines(self):
        return [l for f in (self.tmp / "state").rglob("outbox*.txt") for l in f.read_text().splitlines()]

    def test_triage_follows_its_own_coordinator(self):
        other = self.two_coordinators()
        tid = self.bp_in(other, "add", "Theirs", "--brief", "x").stdout.strip()
        self.bp("start", tid)
        (self.fake / "alive" / other).unlink()  # its owner is gone; c1 still runs
        self.bp("_event", self.block(tid), "error", "", "boom")
        self.assertEqual(self.show(tid)["status"], "needs")

    def test_review_starts_a_new_reviewer_within_the_rounds(self):
        tid = self.start_task()
        self.assertIn("no review", self.bp("review", tid, ok=False).stderr)
        reviewer = self.commit_and_ready(tid)
        self.assertEqual(self.show(tid)["review"]["round"], 1)
        self.bp("_event", reviewer, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.assertIn("round 2 of 2", self.bp("review", tid).stdout)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["round"], t["review"]["verdict"]), ("review", 2, None))
        self.assertNotEqual(t["review"]["block"], reviewer)
        self.assertFalse((self.fake / "alive" / reviewer).exists())  # the old one is closed
        self.bp("_gone", t["review"]["block"])
        self.assertEqual(self.show(tid)["status"], "triage")
        p = self.bp("review", tid, ok=False)
        self.assertIn("used its 2 review rounds: `bullpen ask BP-1`", p.stderr)
        self.assertEqual(self.show(tid)["status"], "triage")

    def test_review_is_refused_while_the_worker_is_on_it(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.verdict(tid, "changes", "Fix it.")
        self.assertEqual(self.show(tid)["status"], "working")
        self.assertIn(", not waiting on a reviewer", self.bp("review", tid, ok=False).stderr)

    def test_review_is_refused_for_a_worker_stopped_after_ready_was_undone(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.bp("_event", blk, "idle", "", "")
        self.bp("_event", blk, "working", "", "More")  # back to working, its approval on record
        self.assertEqual(self.show(tid)["review"]["verdict"], "approved")
        self.bp("_event", blk, "error", "", "boom")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.assertIn(", not waiting on a reviewer", self.bp("review", tid, ok=False).stderr)
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        self.assertIn(", not waiting on a reviewer", self.bp("review", tid, ok=False).stderr)
        self.assertEqual(self.show(tid)["status"], "needs")

    def test_a_ready_task_whose_worker_works_again_is_working_again(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Fixing")
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        self.assertEqual(self.show(tid)["status"], "ready")
        self.bp("_event", blk, "working", "", "Still in the turn that ran ready")
        self.assertEqual(self.show(tid)["status"], "ready")  # the same turn: nothing new
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "ready")
        self.bp("_event", blk, "working", "", "The person asked for more")
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["verdict"]), ("working", "approved"))  # kept on record
        self.assertIn("BP-1 is working again (was ready)", self.news()[-1])
        self.assertIn("run `ready` again; its approval holds if the code doesn't change", self.news()[-1])
        self.assertNotEqual(self.bp("accept", tid, ok=False).returncode, 0)

    def test_old_needs_tasks_become_triage_once(self):
        tid = self.start_task()
        d = next((self.tmp / "state").rglob("config.json")).parent
        cfg = json.loads((d / "config.json").read_text())
        cfg.pop("triage_migrated", None)
        (d / "config.json").write_text(json.dumps(cfg))
        f = d / "tasks" / f"{tid}.json"
        base = json.loads(f.read_text())
        whys = {"BP-1": "Ended its turn without saying ready", "BP-2": "Needs you (permission): Allow?",
                "BP-3": "Reviewer stopped without a verdict", "BP-4": "Reviewer needs you (input)",
                "BP-5": "PR checks failed: test", "BP-6": "Failed: boom"}
        for i, why in whys.items():
            (d / "tasks" / f"{i}.json").write_text(json.dumps({**base, "id": i, "status": "needs", "why": why,
                                                               "block": None}))
        self.bp("ls")
        got = {i: self.show(i)["status"] for i in whys}
        self.assertEqual(got, {"BP-1": "triage", "BP-2": "needs", "BP-3": "triage", "BP-4": "needs",
                               "BP-5": "triage", "BP-6": "triage"})
        # once: a task needing the person for such a reason later stays so
        t = json.loads((d / "tasks" / "BP-1.json").read_text())
        (d / "tasks" / "BP-1.json").write_text(json.dumps({**t, "status": "needs"}))
        self.bp("ls")
        self.assertEqual(self.show("BP-1")["status"], "needs")

    def test_old_stopped_tasks_get_their_stop_once(self):
        tid = self.start_task()
        d = next((self.tmp / "state").rglob("config.json")).parent
        cfg = json.loads((d / "config.json").read_text())
        cfg.pop("stops_migrated", None)
        (d / "config.json").write_text(json.dumps(cfg))
        base = json.loads((d / "tasks" / f"{tid}.json").read_text())
        old = {"BP-1": ("triage", "Reviewer stopped without a verdict", {}),
               "BP-2": ("needs", "Should it go?", {"ask": True, "ask_of": "Needs you (permission): Allow?"}),
               "BP-3": ("stopped", "Agent exited before saying ready", {}),
               "BP-4": ("triage", "PR checks failed: test", {}),
               "BP-5": ("working", None, {})}
        for i, (status, why, more) in old.items():
            (d / "tasks" / f"{i}.json").write_text(json.dumps({**base, "id": i, "status": status, "why": why,
                                                               "block": None, **more}))
        self.bp("ls")
        got = {i: self.show(i).get("stop") for i in old}
        self.assertEqual(got, {"BP-1": {"by": "reviewer", "kind": "ended"},
                               "BP-2": {"by": "worker", "kind": "prompt"},
                               "BP-3": {"by": "worker", "kind": "exited"},
                               "BP-4": {"by": "worker", "kind": "not_sent"}, "BP-5": None})
        self.assertNotIn("ask_of", self.show("BP-2"))
        # once: a stop recorded later is left as it is
        t = json.loads((d / "tasks" / "BP-1.json").read_text())
        (d / "tasks" / "BP-1.json").write_text(json.dumps({**t, "stop": {"by": "worker", "kind": "failed"}}))
        self.bp("ls")
        self.assertEqual(self.show("BP-1")["stop"], {"by": "worker", "kind": "failed"})

    def test_go_next_skips_triage(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")
        p = self.bp("go", "--next", "--json", ok=False)
        self.assertIn("nothing needs you", p.stderr)

    def notified(self):
        log = next((self.tmp / "state").rglob("notify.log"), None)
        return [l.split(" ", 1)[1] for l in log.read_text().splitlines()] if log else []

    def test_a_task_that_needs_you_sends_a_notification_from_its_pane(self):
        self.assertEqual(json.loads(self.bp("config", "--json").stdout)["notify"], "needs")  # the default
        tid = self.start_task()
        blk = self.block(tid)
        read = self.terminal(blk)
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        self.assertEqual(read(), "\x1b]777;notify;bullpen · proj;BP-1 needs you (permission): Allow rm?\x07")
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")  # the same prompt: once
        self.assertEqual(read(), "")
        self.bp("_event", blk, "working", "", "")
        self.bp("_event", blk, "idle", "", "")  # triage: the coordinator's, not the person's
        self.assertEqual(read(), "")
        self.bp("ask", tid, "Use SQLite?")
        self.assertEqual(read(), "\x1b]777;notify;bullpen · proj;BP-1 needs you: Use SQLite?\x07")
        self.assertEqual(self.notified(), [f"BP-1 sent to {blk}: BP-1 needs you (permission): Allow rm?",
                                           f"BP-1 sent to {blk}: BP-1 needs you: Use SQLite?"])

    def test_notify_needs_leaves_ready_out_and_all_sends_it(self):
        self.bp("config", "--review", "off")
        tid = self.start_task()
        read = self.terminal(self.block(tid))
        self.commit_and_ready_no_review(tid)
        self.assertEqual((self.show(tid)["status"], read()), ("ready", ""))
        tid2 = self.bp("add", "Second thing", "--brief", "x").stdout.strip()
        self.bp("start", tid2)
        read2 = self.terminal(self.block(tid2))
        self.bp("config", "--notify", "all")
        wt = self.show(tid2)["worktree"]
        (Path(wt) / "two.txt").write_text("two\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Two")
        self.bp("ready", tid2, "--summary", "Did it.", cwd=wt)
        self.assertEqual(read2(), "\x1b]777;notify;bullpen · proj;BP-2 is ready: Second thing\x07")

    def test_notify_off_sends_nothing(self):
        tid = self.start_task()
        read = self.terminal(self.block(tid))
        self.bp("config", "--notify", "off")
        self.bp("_event", self.block(tid), "blocked", "permission", "Allow rm?")
        self.assertEqual((read(), self.notified()), ("", []))
        self.assertNotEqual(self.bp("config", "--notify", "loud", ok=False).returncode, 0)

    def test_no_notification_while_the_person_looks_at_the_task_or_its_board(self):
        tid = self.start_task()
        blk = self.block(tid)
        read = self.terminal(blk)
        (self.fake / "focused").write_text("blk2")  # the board
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        self.assertEqual(read(), "")
        self.assertEqual(self.notified(), ["BP-1 not sent: the person is looking: "
                                           "BP-1 needs you (permission): Allow rm?"])
        self.bp("_event", blk, "working", "", "")
        (self.fake / "focused").write_text("blk1")  # the coordinator: it doesn't show the task
        self.bp("_event", blk, "blocked", "permission", "Allow rm?")
        self.assertIn("needs you (permission)", read())

    def notify_state(self, **change):
        path = next((self.tmp / "state").rglob("notify.json"))
        st = json.loads(path.read_text())
        path.write_text(json.dumps({**st, **{k: st[k] + v for k, v in change.items()}}))

    def test_several_tasks_that_need_you_at_once_get_one_notification_more(self):
        tids = [self.start_task(t) for t in ("One", "Two", "Three")]
        reads = [self.terminal(self.block(tid)) for tid in tids]
        for tid in tids:
            self.bp("_event", self.block(tid), "blocked", "permission", f"Allow {tid}?")
        self.assertEqual([r() for r in reads],
                         ["\x1b]777;notify;bullpen · proj;BP-1 needs you (permission): Allow BP-1?\x07", "", ""])
        self.assertTrue((self.tmp / "state" / "outbox").exists())  # the watcher's round comes back
        self.bp("_flush")
        self.assertEqual([r() for r in reads], ["", "", ""])  # not yet: they might keep coming
        self.notify_state(last=-5)
        self.bp("_flush")
        self.assertEqual([r() for r in reads],
                         ["", "", "\x1b]777;notify;bullpen · proj;3 tasks need you: BP-1, BP-2, BP-3\x07"])
        self.assertFalse((self.tmp / "state" / "outbox").exists())
        self.bp("_flush")
        self.assertEqual([r() for r in reads], ["", "", ""])  # once
        self.assertEqual(self.notified()[1:3], ["BP-2 held: another task needs you too: BP-2 needs you (permission): Allow BP-2?",
                                                "BP-3 held: another task needs you too: BP-3 needs you (permission): Allow BP-3?"])

    def test_one_held_notification_goes_out_as_it_was(self):
        tids = [self.start_task(t) for t in ("One", "Two")]
        reads = [self.terminal(self.block(tid)) for tid in tids]
        for tid in tids:
            self.bp("_event", self.block(tid), "blocked", "permission", f"Allow {tid}?")
        self.notify_state(last=-5)
        self.bp("_flush")
        self.assertEqual(reads[1](), "\x1b]777;notify;bullpen · proj;BP-2 needs you (permission): Allow BP-2?\x07")

    def test_a_task_that_needs_you_later_is_its_own_notification(self):
        tids = [self.start_task(t) for t in ("One", "Two")]
        reads = [self.terminal(self.block(tid)) for tid in tids]
        self.bp("_event", self.block(tids[0]), "blocked", "permission", "Allow one?")
        self.notify_state(at=-11)
        self.bp("_event", self.block(tids[1]), "blocked", "permission", "Allow two?")
        self.assertEqual([r() for r in reads],
                         ["\x1b]777;notify;bullpen · proj;BP-1 needs you (permission): Allow one?\x07",
                          "\x1b]777;notify;bullpen · proj;BP-2 needs you (permission): Allow two?\x07"])

    def test_a_bare_permission_prompt_isnt_said_twice(self):
        tid = self.start_task()
        read = self.terminal(self.block(tid))
        self.bp("_event", self.block(tid), "blocked", "permission", "permission prompt")
        self.assertEqual(self.news()[-1], "bullpen · BP-1 is at a permission prompt")
        self.assertEqual(read(), "\x1b]777;notify;bullpen · proj;BP-1 needs you (permission)\x07")

    def test_a_task_without_a_terminal_sends_from_its_board(self):
        tid = self.start_task()
        read = self.terminal("blk2")  # the worker's pane has none Rex can tell
        self.bp("_event", self.block(tid), "blocked", "permission", "Allow rm?")
        self.assertIn("BP-1 needs you (permission)", read())
