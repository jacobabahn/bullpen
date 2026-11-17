"""The worker's Stop hook: what a worker that ended its turn waits on."""

import datetime as dt
import json
import subprocess
from pathlib import Path

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module

SETTINGS = ".claude/settings.local.json"


class WorkerHookTest(BullpenTest):
    def hook(self, tid):
        """The command bullpen's Stop hook runs in the task's worktree."""
        wt = Path(self.show(tid)["worktree"])
        stop = json.loads((wt / SETTINGS).read_text())["hooks"]["Stop"]
        [command] = [h["command"] for g in stop for h in g["hooks"] if "_worker_stop" in h["command"]]
        return command

    def stop(self, tid, said="", jobs=(), raw=None):
        """Claude Code ends the worker's turn: its Stop hook runs with this input."""
        bg = [{"id": f"b{i}", "type": "shell", "status": "running", "description": f"job {i}", "command": c}
              for i, c in enumerate(jobs)]
        text = raw if raw is not None else json.dumps({
            "session_id": "s1", "hook_event_name": "Stop", "stop_hook_active": False,
            "last_assistant_message": said, "background_tasks": bg, "session_crons": []})
        p = subprocess.run(["sh", "-c", self.hook(tid)], input=text, env=self.env, text=True,
                           capture_output=True, cwd=self.show(tid)["worktree"])
        self.assertEqual((p.returncode, p.stdout, p.stderr), (0, "", ""))

    def record(self, tid):
        path = next((self.tmp / "state").rglob(f"turns/{tid}.json"))
        return path, json.loads(path.read_text())

    def turn(self, tid, **said):
        """A turn: it starts working, its Stop hook runs, and the watcher says it's idle."""
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Reading")
        self.stop(tid, **said)
        self.bp("_event", blk, "idle", "", "")
        return self.show(tid)

    def ended(self):
        return [l for l in self.news() if "For the coordinator" in l]

    def test_start_writes_the_hook_and_close_removes_it(self):
        tid = self.start_task()
        t = self.show(tid)
        wt = Path(t["worktree"])
        self.assertTrue(t["stop_hook"])
        stop = json.loads((wt / SETTINGS).read_text())["hooks"]["Stop"]
        self.assertEqual(stop[0]["hooks"][0]["timeout"], 10)
        self.assertTrue(self.hook(tid).endswith("|| true"))
        self.assertEqual(sh("git", "-C", str(wt), "status", "--porcelain").strip(), "")  # git ignores it
        self.assertFalse((self.repo / SETTINGS).exists())  # the main checkout's isn't touched
        # the hook exits 0 and writes nothing on input it can't read
        self.stop(tid, raw="not json")
        self.assertFalse(list((self.tmp / "state").rglob(f"turns/{tid}.json")))
        self.stop(tid, said="Hi.")
        self.assertEqual(self.record(tid)[1]["said"], "Hi.")
        wt_ = str(wt)
        f = self.tmp / "finish.md"
        f.write_text("Answered it\n\nNothing to merge.")
        self.bp("finish", tid, "--summary-file", str(f), cwd=wt_)
        self.bp("close", tid)
        self.assertFalse(list((self.tmp / "state").rglob(f"turns/{tid}.json")))  # its turn record goes too
        self.assertFalse((wt / SETTINGS).exists())
        self.assertFalse(wt.exists())

    def test_a_worker_that_isnt_claude_gets_no_hook(self):
        self.bp("up")
        path = next((self.tmp / "state").rglob("config.json"))
        path.write_text(json.dumps({**json.loads(path.read_text()), "worker": ["codex"]}))
        tid = self.bp("add", "Fix it", "--brief", "Make it work.").stdout.strip()
        self.bp("start", tid)
        t = self.show(tid)
        self.assertNotIn("stop_hook", t)
        self.assertFalse((Path(t["worktree"]) / SETTINGS).exists())

    def test_a_turn_ended_with_a_question_puts_it_in_the_triage_line(self):
        tid = self.start_task()
        t = self.turn(tid, said="There are two.\n\nWhich `config` file?\nI'd use the first.")
        self.assertEqual(self.record(tid)[1]["jobs"], [])
        self.assertEqual((t["status"], t["why"]), ("triage", "Ended its turn without saying ready"))
        self.assertTrue(self.ended()[-1].startswith(
            "bullpen · For the coordinator: BP-1 ended its turn with a question: \"Which 'config' file?\": "
            "`bullpen peek BP-1`"), self.ended()[-1])

    def test_a_turn_ended_with_neither_says_so(self):
        tid = self.start_task()
        t = self.turn(tid, said="Stopping here.")
        self.assertEqual(t["status"], "triage")
        self.assertIn("BP-1 ended its turn without `ready`: read it with `bullpen peek BP-1`", self.ended()[-1])

    def test_a_past_turns_record_is_left_alone(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.stop(tid, said="Which config file?", jobs=["cargo test"])
        path, rec = self.record(tid)
        old = (dt.datetime.now().astimezone() - dt.timedelta(hours=1)).isoformat(timespec="seconds")
        path.write_text(json.dumps({**rec, "at": old}))
        self.bp("_event", blk, "working", "", "Reading")
        self.bp("_event", blk, "idle", "", "")  # interrupted: Claude Code runs no Stop hook then
        self.assertEqual(self.show(tid)["status"], "triage")
        self.assertIn("ended its turn without `ready`", self.ended()[-1])

    def test_a_turn_ended_with_its_job_running_waits_on_it(self):
        tid = self.start_task()
        t = self.turn(tid, said="The tests take a while.", jobs=["cargo test --workspace"])
        self.assertEqual((t["status"], t["job"]["what"]), ("working", "cargo test --workspace"))
        self.assertEqual(self.ended(), [])
        self.assertIn("Waiting on its job (cargo test --workspace)", self.bp("ls").stdout)
        # the job ends: Claude Code starts its next turn, and the wait is over
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Reading the results")
        self.assertNotIn("job", self.show(tid))
        self.stop(tid, said="They pass, but I stopped.")
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.show(tid)["status"], "triage")

    def test_a_job_that_lasts_past_stuck_after_is_triage(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.turn(tid, jobs=["npm run dev"])
        (self.fake / f"screen_{blk}").write_text("❯ \n  ⏵⏵ auto mode on · 1 shell\n")
        self.agent(blk, "idle", ago=19 * 60)  # stuck_after is 20 min
        self.assertEqual(self.check(), [])
        self.agent(blk, "idle", ago=25 * 60)
        self.check("--fix", "--quiet")
        t = self.show(tid)
        self.assertEqual(t["status"], "triage")
        self.assertNotIn("job", t)
        self.assertIn("BP-1 ended its turn without `ready` 25 min ago, and still waits on its job (npm run dev): "
                      "read it with `bullpen peek BP-1`", self.ended()[-1])

    def test_a_monitor_is_waited_on_until_stuck_after(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.turn(tid, jobs=["tail -f build.log"])  # a Monitor is a shell in the hook's input
        (self.fake / f"screen_{blk}").write_text("❯ \n  ⏵⏵ auto mode on · 1 monitor · ← 1 agent · ↓ to manage\n")
        self.agent(blk, "idle", ago=10 * 60)
        self.assertEqual(self.check(), [])
        self.check("--fix", "--quiet")
        self.assertEqual(self.show(tid)["status"], "working")
        self.assertEqual(self.ended(), [])
        self.agent(blk, "idle", ago=25 * 60)
        self.check("--fix", "--quiet")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.assertIn("still waits on its job (tail -f build.log)", self.ended()[-1])

    def test_a_background_subagent_isnt_a_job(self):
        tid = self.start_task()
        bg = [{"id": "a1", "type": "subagent", "status": "running", "description": "Look around",
               "agent_type": "general-purpose"}]
        raw = json.dumps({"session_id": "s1", "last_assistant_message": "Stopping.", "background_tasks": bg})
        blk = self.block(tid)
        self.bp("_event", blk, "working", "", "Reading")
        self.stop(tid, raw=raw)
        self.bp("_event", blk, "idle", "", "")
        self.assertEqual(self.record(tid)[1]["jobs"], [])
        self.assertEqual(self.show(tid)["status"], "triage")

    def test_a_job_gone_without_a_new_turn_is_triage(self):
        tid = self.start_task()
        blk = self.block(tid)
        self.turn(tid, jobs=["sleep 240"])
        self.assertEqual(self.show(tid)["status"], "working")
        self.agent(blk, "idle", ago=10 * 60)  # no shell on its screen now, and no turn
        self.check("--fix", "--quiet")
        self.assertEqual(self.show(tid)["status"], "triage")
        self.assertIn("BP-1 waited on its job (sleep 240), which has ended, but hasn't taken a turn since",
                      self.ended()[-1])

    def test_doctor_says_when_a_running_worker_has_no_hook(self):
        tid = self.start_task()
        workers = lambda: next(c for c in json.loads(self.bp("doctor", "--json", ok=False).stdout)["checks"]
                               if c["name"] == "workers")
        self.assertEqual(workers()["state"], "ok")
        settings = Path(self.show(tid)["worktree"]) / SETTINGS
        settings.unlink()
        w = workers()
        self.assertEqual(w["state"], "fail")
        self.assertIn("BP-1's worker has no Stop hook", w["text"])
        self.assertIn("wrote BP-1's Stop hook again", self.bp("doctor", "--fix", ok=False).stdout)
        self.assertTrue(settings.exists())
        self.assertEqual(workers()["state"], "ok")
        # a task started before workers had a hook: nothing to fix
        path = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
        path.write_text(json.dumps({k: v for k, v in json.loads(path.read_text()).items() if k != "stop_hook"}))
        settings.unlink()
        w = workers()
        self.assertEqual(w["state"], "ok")
        self.assertIn("started before workers had one", w["text"])
