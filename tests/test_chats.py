"""Each agent's conversation: the session ID bullpen starts Claude Code with, records as
`chat`, resumes with, and the Stop hook moves for the agent whose turn ended."""

import json
import re
import subprocess
from pathlib import Path

from support import setUpModule, tearDownModule, BullpenTest  # a temp folder for each module

UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")


class ChatTest(BullpenTest):
    def command(self):
        return self.made()[-1]["options"]["command"]

    def flag(self, cmd, name):
        """The value after flag name in a command line, or None."""
        return cmd[cmd.index(name) + 1] if name in cmd else None

    def gone(self, tid):
        """The worker's pane goes."""
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)

    def stop(self, tid, session, transcript="", said="Done."):
        """A Stop of the hook in the task's worktree, as Claude Code runs it."""
        wt = Path(self.show(tid)["worktree"])
        stop = json.loads((wt / ".claude/settings.local.json").read_text())["hooks"]["Stop"]
        [hook] = [h["command"] for g in stop for h in g["hooks"] if "_worker_stop" in h["command"]]
        text = json.dumps({"session_id": session, "transcript_path": transcript, "hook_event_name": "Stop",
                           "last_assistant_message": said, "background_tasks": []})
        p = subprocess.run(["sh", "-c", hook], input=text, env=self.env, text=True, capture_output=True, cwd=wt)
        self.assertEqual((p.returncode, p.stderr), (0, ""))

    def turns(self, tid):
        path = next((self.tmp / "state").rglob(f"turns/{tid}.json"), None)
        return json.loads(path.read_text()) if path else None

    def transcript(self, name, title):
        """A transcript that starts as Claude Code's do, with its -n name."""
        f = self.tmp / f"{name}.jsonl"
        f.write_text(json.dumps({"type": "custom-title", "customTitle": title, "sessionId": name}) + "\n"
                     + json.dumps({"type": "mode", "mode": "normal"}) + "\n")
        return str(f)

    def test_the_worker_and_its_reviewer_each_start_with_their_own_recorded_id(self):
        tid = self.start_task()
        chat = self.show(tid)["chat"]
        self.assertRegex(chat, UUID)
        self.assertEqual(self.flag(self.command(), "--session-id"), chat)
        self.assertNotIn("--continue", self.command())
        self.commit_and_ready(tid)
        review = self.show(tid)["review"]["chat"]
        self.assertRegex(review, UUID)
        self.assertNotEqual(review, chat)
        self.assertEqual(self.flag(self.command(), "--session-id"), review)
        self.assertEqual(self.show(tid)["chat"], chat)  # the worker's is kept

    def test_the_coordinator_starts_with_its_recorded_id(self):
        self.bp("up")
        cmd = next(a for a in self.rex_calls() if a[0] == "run")  # the coordinator's pane
        cfg = json.loads(next((self.tmp / "state").rglob("config.json")).read_text())
        [c] = cfg["coordinators"]
        self.assertRegex(c["chat"], UUID)
        self.assertEqual(self.flag(cmd, "--session-id"), c["chat"])

    def test_resume_and_a_hand_back_take_the_workers_conversation_not_the_reviewers(self):
        tid = self.start_task()
        chat = self.show(tid)["chat"]
        self.commit_and_ready(tid)  # the reviewer's conversation is the newest in the worktree now
        self.gone(tid)
        self.verdict(tid, "changes", "Fix it.")  # handed back to a worker whose pane is gone
        cmd = self.command()
        self.assertEqual(self.flag(cmd, "--resume"), chat)
        self.assertNotIn("--session-id", cmd)
        self.assertNotIn("--continue", cmd)
        self.gone(tid)
        self.bp("resume", tid)
        self.assertEqual(self.flag(self.command(), "--resume"), chat)
        self.assertEqual(self.show(tid)["chat"], chat)

    def test_a_worker_that_isnt_claude_gets_neither_flag(self):
        self.bp("up")
        self.set_commands(worker=["codex"])
        tid = self.bp("add", "Fix it", "--brief", "Make it work.").stdout.strip()
        self.bp("start", tid)
        cmd = self.command()
        self.assertEqual(cmd[-3:-1], ["-n", f"{tid}: Fix it"])
        self.assertFalse({"--session-id", "--resume"} & set(cmd))
        self.assertIsNone(self.show(tid)["chat"])
        self.gone(tid)
        self.bp("resume", tid)
        self.assertFalse({"--session-id", "--resume"} & set(self.command()))

    def test_the_workers_turn_record_keeps_its_transcript(self):
        tid = self.start_task()
        chat = self.show(tid)["chat"]
        self.stop(tid, chat, "/p/w.jsonl", said="Working on it.")
        rec = self.turns(tid)
        self.assertEqual((rec["session"], rec["transcript"], rec["said"]), (chat, "/p/w.jsonl", "Working on it."))

    def test_a_reviewers_stop_moves_only_the_reviews_chat(self):
        tid = self.start_task()
        chat = self.show(tid)["chat"]
        self.stop(tid, chat, "/p/w.jsonl", said="Mine.")
        self.commit_and_ready(tid)
        review = self.show(tid)["review"]["chat"]
        # its own session: told by ID, nothing moves and the worker's record stays
        self.stop(tid, review, "/p/r.jsonl", said="Reviewing.")
        # a /clear: an unknown session, told by its title
        self.stop(tid, "r2", self.transcript("r2", f"{tid} review"), said="Reviewing again.")
        t = self.show(tid)
        self.assertEqual((t["chat"], t["review"]["chat"]), (chat, "r2"))
        rec = self.turns(tid)
        self.assertEqual((rec["session"], rec["said"]), (chat, "Mine."))
        # the worker's own /clear moves its chat, and its turn is recorded
        self.stop(tid, "w2", self.transcript("w2", f"{tid}: Fix the thing"), said="Cleared.")
        t = self.show(tid)
        self.assertEqual((t["chat"], t["review"]["chat"]), ("w2", "r2"))
        self.assertEqual(self.turns(tid)["said"], "Cleared.")
        # another agent's title: nobody's here
        self.stop(tid, "x", self.transcript("x", "BP-30: Another"), said="Not ours.")
        self.assertEqual(self.show(tid)["chat"], "w2")
        self.assertEqual(self.turns(tid)["said"], "Cleared.")
