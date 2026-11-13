"""Up, start, ready, review and accept: the main path of a task."""

import json
from pathlib import Path

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module


class FlowTest(BullpenTest):
    def test_up_spawns_coordinator_and_one_watcher(self):
        self.bp("up")
        self.bp("up")
        runs = [a for a in self.rex_calls() if a[0] == "run"]
        labels = [a[a.index("--label") + 1] for a in runs]
        self.assertEqual(labels, ["bullpen: proj"])
        watchers = self.api("session.new_block")
        self.assertEqual(len(watchers), 1)  # detached: no tab
        self.assertEqual(watchers[0]["label"], "bullpen: watcher")
        splits = [a for a in self.rex_calls() if a[0] == "split"]
        self.assertEqual(len(splits), 1)  # one board, beside the coordinator
        self.assertEqual(splits[0][splits[0].index("-b") + 1], "blk1")
        self.assertEqual(splits[0][-3:], ["board", "--coordinator", "c1"])
        watcher = watchers[0]["options"]["command"]
        self.assertIn("--all", watcher)
        self.assertTrue(any(x.startswith("bin=") for x in watcher))

    def test_up_here_registers_this_pane(self):
        self.bp("up", "--here")
        self.assertFalse(any(a[0] == "run" and "bullpen: proj" in a for a in self.rex_calls()))
        cfg = json.loads(self.bp("config", "--json").stdout)
        self.assertEqual([(c["id"], c["block"], c["session"]) for c in cfg["coordinators"]],
                         [("c1", "me", "session:1")])

    def test_start_makes_worktree_branch_and_pane(self):
        tid = self.start_task()
        self.assertEqual(tid, "BP-1")
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        self.assertEqual(t["branch"], "bullpen/bp-1-fix-the-thing")
        self.assertTrue(Path(t["worktree"]).is_dir())
        self.assertEqual(t["block"], "blk4")
        layer = self.api("session.new_layer")[-1]
        self.assertEqual(layer["layout"]["block"]["options"]["cwd"], t["worktree"])
        self.assertEqual(layer["layout"]["block"]["label"], "bullpen: BP-1 · ctrl+esc hides")
        brief = (self.tmp / "state").rglob("BP-1.full.md").__next__().read_text()
        self.assertIn("Make it work.", brief)
        self.assertIn("ready BP-1 --summary-file", brief)
        self.assertIn("Never add a Co-Authored-By", brief)

    def test_ready_pushes_and_opens_pr_then_news(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "fix.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        summary = self.tmp / "s.md"
        summary.write_text("Fixed the thing. Checked by hand.\n")
        self.bp("config", "--review", "off")
        self.bp("ready", tid, "--summary-file", str(summary), cwd=wt)
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertEqual(t["pr"], "https://github.com/me/repo/pull/7")
        sh("git", "--git-dir", str(self.tmp / "origin.git"), "rev-parse", "refs/heads/" + t["branch"])
        gh = [json.loads(l) for l in (self.fake / "gh.log").read_text().splitlines()]
        self.assertEqual(gh[0]["argv"][:2], ["pr", "create"])
        self.assertIn("Fix the thing (BP-1)", gh[0]["argv"])
        self.assertIn("Fixed the thing.", gh[0]["body"])
        self.assertNotIn("Claude", gh[0]["body"])
        block, line = self.typed()[-1]
        self.assertEqual(block, "blk1")
        self.assertEqual(line, "bullpen · BP-1 is ready: Fix the thing (1 commit, PR #7)")

    def test_ready_refuses_uncommitted_changes(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "dirty.txt").write_text("x\n")
        p = self.bp("ready", tid, cwd=wt, ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("uncommitted", p.stderr)

    def test_events_become_news_once(self):
        tid = self.start_task()
        self.bp("_event", self.block(tid), "working", "", "Reading files")
        self.bp("_event", self.block(tid), "blocked", "permission", "Allow rm?")
        self.bp("_event", self.block(tid), "blocked", "permission", "Allow rm?")  # repeat: no news
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertEqual(len(news), 1)
        self.assertTrue(news[0].startswith("bullpen · BP-1 is at a permission prompt"))
        self.assertIn("Allow rm?", news[0])
        self.assertEqual(self.show(tid)["status"], "needs")
        self.bp("_event", self.block(tid), "working", "", "")
        self.assertEqual(self.show(tid)["status"], "working")
        self.bp("_event", self.block(tid), "done", "", "")
        news = [l for b, l in self.typed() if b == "blk1"]
        self.assertIn("ended its turn without `ready`", news[-1])

    def test_every_end_of_turn_is_news(self):
        # BP-12: it ended its turn, a background shell finished, it took another turn
        # and ended it the same way. Each end is news; a repeat within a turn isn't.
        tid = self.start_task()
        blk = self.block(tid)
        for turn in (1, 2):
            self.bp("_event", blk, "working", "", "Running the tests")
            self.assertEqual(self.show(tid)["status"], "working")
            self.bp("_event", blk, "idle", "", "Waiting for the tests")
            self.bp("_event", blk, "idle", "", "Waiting for the tests")  # repeat: no news
            t = self.show(tid)
            self.assertEqual((t["status"], t["why"]), ("triage", "Ended its turn without saying ready"))
            ended = [l for l in self.news() if "ended its turn" in l]
            self.assertEqual(len(ended), turn)

    def test_every_stop_of_a_reviewer_is_news(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        for turn in (1, 2):
            self.bp("_event", reviewer, "working", "", "Reading the diff")
            self.assertEqual(self.show(tid)["status"], "review")
            self.bp("_event", reviewer, "idle", "", "Waiting")
            self.bp("_event", reviewer, "idle", "", "Waiting")  # repeat: no news
            t = self.show(tid)
            self.assertEqual((t["status"], t["why"]), ("triage", "Reviewer stopped without a verdict"))
            stopped = [l for l in self.news() if "stopped without a verdict" in l]
            self.assertEqual(len(stopped), turn)

    def test_done_after_ready_is_silent(self):
        tid = self.start_task()
        self.bp("ready", tid)  # no commits: ready with nothing to merge
        before = len(self.typed())
        self.bp("_event", self.block(tid), "done", "", "")
        self.assertEqual(len(self.typed()), before)

    def test_exit_marks_stopped_and_resume_relaunches(self):
        tid = self.start_task()
        blk = self.block(tid)
        (self.fake / "alive" / blk).unlink()
        self.bp("_gone", blk)
        t = self.show(tid)
        self.assertEqual(t["status"], "stopped")
        self.assertIn("stopped before saying ready", self.typed()[-1][1])
        self.bp("resume", tid)
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertIn("--continue", cmd)
        self.assertEqual(self.show(tid)["status"], "working")

    def test_events_from_panes_that_arent_a_tasks_are_ignored(self):
        self.start_task()
        before = len(self.typed())
        self.bp("_event", "stranger", "blocked", "permission", "x")
        self.bp("_event", "blk2", "done", "", "")  # the board's
        self.bp("_gone", "blk2")
        self.assertEqual(len(self.typed()), before)

    def test_accept_merges_and_cleans_up(self):
        tid = self.start_task()
        t = self.show(tid)
        wt, blk = t["worktree"], t["block"]
        (Path(wt) / "fix.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        self.bp("config", "--review", "off")
        self.bp("ready", tid, "--summary", "done")
        self.bp("accept", tid)
        t = self.show(tid)
        self.assertEqual(t["status"], "done")
        self.assertFalse(Path(wt).exists())
        gh = [json.loads(l)["argv"] for l in (self.fake / "gh.log").read_text().splitlines()]
        self.assertEqual(gh[-1][:2], ["pr", "merge"])
        self.assertIn("--rebase", gh[-1])
        self.assertFalse((self.fake / "alive" / blk).exists())
        branches = sh("git", "-C", str(self.repo), "branch")
        self.assertNotIn("bullpen/", branches)

    def test_a_task_published_with_an_origin_finishes_on_github(self):
        tid = self.ready_with_a_commit()
        self.assertIs(self.show(tid)["local"], False)
        sh("git", "-C", str(self.repo), "remote", "remove", "origin")
        self.bp("accept", tid)  # merged by gh, as it started: not locally
        gh = [json.loads(l)["argv"] for l in (self.fake / "gh.log").read_text().splitlines()]
        self.assertEqual(gh[-1][:2], ["pr", "merge"])
        self.assertNotIn("fix.txt", sh("git", "-C", str(self.repo), "ls-tree", "--name-only", "main"))

    def test_the_close_step_closes_the_workers_pane_last(self):
        for verb in ("drop", "accept"):
            with self.subTest(verb=verb):
                self.setUp()
                tid = self.start_task()
                blk = self.block(tid)
                review = self.commit_and_ready(tid)
                if verb == "accept":
                    self.verdict(tid, "approve")
                    review = None  # its verdict closed it
                self.bp(verb, tid)
                closes = [a[2] for a in self.rex_calls() if a[:2] == ["block", "close"]]
                self.assertEqual(closes[-1], blk)
                if review:
                    self.assertIn(review, closes[:-1])

    def test_accept_refuses_unready(self):
        tid = self.start_task()
        p = self.bp("accept", tid, ok=False)
        self.assertNotEqual(p.returncode, 0)

    def test_drop_refuses_dirty_without_force(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "dirty.txt").write_text("x\n")
        self.assertNotEqual(self.bp("drop", tid, ok=False).returncode, 0)
        self.bp("drop", tid, "--force")
        self.assertEqual(self.show(tid)["status"], "dropped")
        self.assertFalse(Path(wt).exists())

    def test_accept_from_elsewhere_is_news(self):
        tid = self.ready_with_a_commit()
        self.bp("accept", tid)  # REX_BLOCK is "me": the board, say, not the coordinator
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-1 accepted (merged PR #7, rebase)"))

    def test_accept_from_the_coordinators_pane_is_no_news(self):
        tid = self.ready_with_a_commit()
        before = self.typed()
        self.env["REX_BLOCK"] = "blk1"
        self.bp("accept", tid)
        self.assertEqual(self.show(tid)["status"], "done")
        self.assertEqual(self.typed(), before)

    def test_accept_without_a_pane_of_its_own_is_news(self):
        tid = self.ready_with_a_commit()
        del self.env["REX_BLOCK"]  # can't tell where it ran: post
        self.bp("accept", tid)
        self.assertIn("BP-1 accepted", self.typed()[-1][1])

    def test_accept_of_a_pr_merged_outside_says_so(self):
        tid = self.ready_with_a_commit()
        (self.fake / "pr.json").write_text(json.dumps({"state": "MERGED"}))
        self.bp("_prs")
        self.bp("accept", tid)
        self.assertEqual(self.typed()[-1][1], "bullpen · BP-1 accepted (PR #7 was merged outside bullpen)")

    def test_drop_is_news_from_elsewhere_only(self):
        self.start_task("First")
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        self.bp("drop", "BP-1")
        self.assertEqual(self.typed()[-1], ("blk1", "bullpen · BP-1 dropped"))
        before = self.typed()
        self.env["REX_BLOCK"] = "blk1"
        self.bp("drop", "BP-2")
        self.assertEqual(self.show("BP-2")["status"], "dropped")
        self.assertEqual(self.typed(), before)

    def watcher(self):
        return json.loads((self.tmp / "state" / "watcher.json").read_text())

    def make_watcher_stale(self, **change):
        w = self.watcher()
        w["stamp"] = {**w["stamp"], **change}
        (self.tmp / "state" / "watcher.json").write_text(json.dumps(w))

    def test_up_restarts_a_stale_watcher(self):
        self.bp("up")
        old = self.watcher()
        self.assertEqual(sorted(old["stamp"]), ["bullpen", "watch"])
        self.make_watcher_stale(watch="an older watch.lua")
        out = self.bp("up").stdout
        new = self.watcher()
        self.assertEqual(out, "Coordinator already running. Watcher restarted (watch.lua or bullpen changed).\n")
        self.assertNotEqual(new["block"], old["block"])
        self.assertEqual(new["stamp"], old["stamp"])  # what runs now
        self.assertFalse((self.fake / "alive" / old["block"]).exists())  # the old one is closed
        self.assertTrue((self.fake / "alive" / new["block"]).exists())
        self.assertEqual(len(self.api("session.new_block")), 2)
        self.bp("up")  # current now: left alone
        self.assertEqual(self.watcher(), new)
        self.assertEqual(len(self.api("session.new_block")), 2)

    def test_a_watcher_from_before_stamps_is_restarted(self):
        self.bp("up")
        old = self.watcher()
        (self.tmp / "state" / "watcher.json").write_text(
            json.dumps({"block": old["block"], "session": old["session"]}))
        self.bp("up")
        self.assertNotEqual(self.watcher()["block"], old["block"])
        self.assertFalse((self.fake / "alive" / old["block"]).exists())

    def test_accept_restarts_a_stale_watcher_and_leaves_a_current_one(self):
        tid = self.ready_with_a_commit()
        old = self.watcher()
        self.bp("drop", tid)  # nothing merged: the watcher isn't looked at
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        self.bp("ready", "BP-2")
        self.bp("accept", "BP-2")
        self.assertEqual(self.watcher(), old)  # current: left alone
        self.assertTrue((self.fake / "alive" / old["block"]).exists())
        self.bp("add", "Third", "--brief", "x")
        self.bp("start", "BP-3")
        self.bp("ready", "BP-3")
        self.make_watcher_stale(bullpen="an older bullpen")
        self.bp("accept", "BP-3")
        new = self.watcher()
        self.assertNotEqual(new["block"], old["block"])
        self.assertFalse((self.fake / "alive" / old["block"]).exists())
        self.assertEqual(len(self.api("session.new_block")), 2)

    def test_ready_starts_a_reviewer(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        t = self.show(tid)
        self.assertEqual(t["status"], "review")
        self.assertEqual(t["pr"], "https://github.com/me/repo/pull/7")  # pushed and opened first
        made = self.api("session.new_block")[-1]  # in the background, by default
        self.assertEqual(made["label"], "bullpen: BP-1 review · ctrl+esc hides")
        self.assertEqual((t["review"]["block"], t["review"]["pane"]), (reviewer, "background"))
        self.assertEqual(json.loads((self.fake / "reg.json").read_text())["blocks"][reviewer]["layer"], "detached")
        prompt = next((self.tmp / "state").rglob("BP-1.review-1.md")).read_text()
        self.assertIn("verdict BP-1 approve", prompt)
        self.assertIn("Did it.", prompt)
        self.assertFalse(any("is ready" in l for _, l in self.typed()))  # not ready yet
        self.assertTrue((self.fake / "alive" / reviewer).exists())

    def test_approval_makes_it_ready_and_closes_the_reviewer(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertEqual(t["review"]["verdict"], "approved")
        self.assertFalse((self.fake / "alive" / reviewer).exists())
        self.assertEqual(self.typed()[-1], ("blk1",
            "bullpen · BP-1 is ready: Fix the thing (1 commit, PR #7, approved by light review: 1 line in 1 file)"))

    def test_changes_go_back_to_the_worker_then_ready_after_the_last_round(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.commit_and_ready(tid)
        self.verdict(tid, "changes", "The fix misses the empty case.")
        t = self.show(tid)
        self.assertEqual(t["status"], "working")
        block, line = self.typed()[-1]
        self.assertEqual(block, worker)  # the findings go to the worker, not the coordinator
        self.assertIn("BP-1.review-1.findings.md", line)
        self.commit_and_ready(tid, 2)
        self.assertEqual(self.show(tid)["review"]["round"], 2)
        self.verdict(tid, "changes", "Still misses it.")
        t = self.show(tid)
        self.assertEqual(t["status"], "ready")
        self.assertIn("after 2 rounds", t["why"])
        block, line = self.typed()[-1]
        self.assertEqual(block, "blk1")
        self.assertIn("still asks for changes", line)

    def test_changes_that_dont_reach_the_worker_go_to_the_coordinator(self):
        tid = self.start_task()
        worker = self.block(tid)
        self.commit_and_ready(tid)
        (self.fake / f"send_fails_{worker}").write_text("")
        self.verdict(tid, "changes", "The fix misses the empty case.")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("triage", "Review findings didn't reach its worker"))
        self.assertEqual(t["review"]["verdict"], "changes")
        line = self.news()[-1]
        self.assertIn("BP-1's reviewer asked for changes, but the message didn't reach its worker", line)
        self.assertIn("BP-1.review-1.findings.md", line)
        self.assertIn("`bullpen tell BP-1`", line)
        (self.fake / f"send_fails_{worker}").unlink()
        self.bp("tell", tid, "Read the findings.")
        self.assertEqual(self.show(tid)["status"], "working")

    def test_an_empty_findings_file_is_refused(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        empty = self.tmp / "findings.md"
        empty.write_text("\n")
        p = self.bp("verdict", tid, "changes", "--summary-file", str(empty), ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("is empty", p.stderr)
        t = self.show(tid)
        self.assertEqual((t["status"], t["review"]["verdict"]), ("review", None))
        self.assertEqual(list((self.tmp / "state").rglob("BP-1.review-1.findings.md")), [])
        p = self.bp("verdict", tid, "changes", "--summary-file", str(self.tmp / "missing.md"), ok=False)
        self.assertIn("can't read", p.stderr)

    def test_an_empty_ready_summary_file_is_refused(self):
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        empty = self.tmp / "summary.md"
        empty.write_text("")
        p = self.bp("ready", tid, "--summary-file", str(empty), cwd=wt, ok=False)
        self.assertNotEqual(p.returncode, 0)
        self.assertIn("is empty", p.stderr)
        self.assertEqual(self.show(tid)["status"], "working")

    def test_changes_resume_a_worker_that_exited(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        (self.fake / "alive" / self.block(tid)).unlink()
        self.verdict(tid, "changes", "Fix it.")
        cmd = self.api("session.new_layer")[-1]["layout"]["block"]["options"]["command"]
        self.assertIn("--continue", cmd)
        self.assertTrue(any("findings" in x for x in cmd))

    def test_a_stuck_reviewer_needs_you(self):
        tid = self.start_task()
        reviewer = self.commit_and_ready(tid)
        self.bp("_event", reviewer, "blocked", "permission", "Allow curl?")
        t = self.show(tid)
        self.assertEqual((t["status"], t["why"]), ("needs", "Reviewer needs you (permission): Allow curl?"))
        self.assertIn("reviewer is at a permission prompt", self.typed()[-1][1])
        self.bp("_event", reviewer, "working", "", "")
        self.assertEqual(self.show(tid)["status"], "review")
        self.bp("_event", reviewer, "done", "", "")
        self.assertEqual(self.show(tid)["why"], "Reviewer stopped without a verdict")
        self.bp("_gone", reviewer)
        self.assertEqual(self.show(tid)["status"], "triage")
