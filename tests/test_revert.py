"""`bullpen revert BP-N`: an accepted task taken back by a task that reverts its commits."""

import json
import subprocess
import unittest
from pathlib import Path

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module


class RevertHelpers:
    def git(self, *args, cwd=None):
        return sh("git", "-C", str(cwd or self.repo), *args).strip()

    def commit(self, where, name, text, msg):
        (Path(where) / name).write_text(text)
        self.git("add", ".", cwd=where)
        self.git("commit", "-qm", msg, cwd=where)

    def task_file(self, tid):
        return next((self.tmp / "state").rglob(f"tasks/{tid}.json"))

    def change(self, tid, **fields):
        """Write fields onto a task's record (None takes a key off)."""
        f = self.task_file(tid)
        t = {**json.loads(f.read_text()), **fields}
        f.write_text(json.dumps({k: v for k, v in t.items() if v is not None}))

    def brief(self, tid):
        return next((self.tmp / "state").rglob(f"briefs/{tid}.md")).read_text()

    def work(self, n=1, title="Fix the thing"):
        """A started task with n commits (fix1.txt ...), ready: its ID and worktree."""
        self.bp("config", "--review", "off")
        tid = self.start_task(title)
        wt = self.show(tid)["worktree"]
        for i in range(1, n + 1):
            self.commit(wt, f"fix{i}.txt", "fixed\n", f"Fix {i}")
        self.bp("ready", tid, "--summary", "Fixed the thing.", cwd=wt)
        return tid, wt

    def do_the_revert(self, tid):
        """What the revert task's worker does: the brief's two git commands, then ready."""
        wt = self.show(tid)["worktree"]
        cmds = [l.strip() for l in self.brief(tid).splitlines() if l.startswith("       git ")]
        self.assertEqual([c.split()[1] for c in cmds], ["revert", "commit"])
        for c in cmds:
            subprocess.run(c, shell=True, cwd=wt, check=True, env=self.env, capture_output=True)
        self.bp("ready", tid, "--summary", "Took it back.", cwd=wt)
        return wt


class RevertLocalTest(RevertHelpers, BullpenTest):
    """Local mode: accept merged the task's branch into the local base branch."""

    def setUp(self):
        super().setUp()
        sh("git", "-C", str(self.repo), "remote", "remove", "origin")

    def tearDown(self):
        self.assertFalse((self.fake / "gh.log").exists(), "gh was called in local mode")

    def accepted(self, n=1, merge="rebase"):
        """A task with n commits, accepted with config --merge merge, then later work on main."""
        self.bp("config", "--merge", merge)
        tid, _ = self.work(n)
        before = self.git("rev-parse", "main")
        self.bp("accept", tid)
        after = self.git("rev-parse", "main")
        self.commit(self.repo, "later.txt", "later\n", "Later work on main")
        return tid, before, after

    def test_a_one_commit_task_gets_a_revert_task_with_its_commit(self):
        tid, before, after = self.accepted()
        t = self.show(tid)
        self.assertEqual((t["merged"], t["merged_from"], t["merged_how"]), (after, before, "rebase"))
        out = self.bp("revert", tid, "--why", "It broke login.").stdout
        self.assertEqual(out.splitlines()[0], "BP-2 added: Revert BP-1: Fix the thing (1 commit to revert)")
        self.assertIn("BP-2 started in ", out)
        new = self.show("BP-2")
        self.assertEqual((new["title"], new["reverts"], new["status"], new["base"]),
                         ("Revert BP-1: Fix the thing", "BP-1", "working", "main"))
        brief = self.brief("BP-2")
        self.assertIn("Why it's taken back: It broke login.", brief)
        self.assertIn("BP-1 was accepted (merged into main, rebase). It left 1 commit on `main`", brief)
        self.assertIn(f"- `{after}` Fix 1\n", brief)
        self.assertIn(f"       git revert --no-commit {after}\n", brief)
        self.assertIn("       git commit -m 'Revert BP-1: Fix the thing'\n", brief)
        self.assertIn("One commit, so one revert.", brief)
        for told in ("keep the later work", "Run the full suite", "couldn't be cleanly taken back",
                     "a later task that built on BP-1", "state file"):
            self.assertIn(told, brief)
        self.assertIn(brief.strip(), self.full_brief("BP-2"))  # what its worker reads

    def test_a_several_commit_task_has_its_commits_newest_first(self):
        tid, before, after = self.accepted(3)
        self.assertIn("(3 commits to revert)", self.bp("revert", tid).stdout)
        shas = self.git("rev-list", f"{before}..{after}").split()
        self.assertEqual(len(shas), 3)
        brief = self.brief("BP-2")
        self.assertIn("It left 3 commits on `main`, listed newest first", brief)
        self.assertIn("\n".join(f"- `{s}` Fix {n}" for s, n in zip(shas, (3, 2, 1))), brief)
        self.assertIn(f"       git revert --no-commit {' '.join(shas)}\n", brief)
        self.assertIn("One commit for the lot, not one per commit", brief)
        self.assertNotIn("Why it's taken back", brief)

    def test_a_squash_leaves_one_commit_and_a_merge_commit_is_reverted_against_the_base(self):
        tid, _, after = self.accepted(3, merge="squash")
        self.bp("revert", tid)
        brief = self.brief("BP-2")
        self.assertIn("(merged into main, squash). It left 1 commit on `main`", brief)
        self.assertIn(f"       git revert --no-commit {after}\n", brief)
        self.bp("drop", "BP-2")
        self.bp("config", "--merge", "merge")
        self.bp("add", "More", "--brief", "x")
        self.bp("start", "BP-3")
        for n in (1, 2):
            self.commit(self.show("BP-3")["worktree"], f"more{n}.txt", "more\n", f"More {n}")
        self.bp("ready", "BP-3", "--summary", "More.", cwd=self.show("BP-3")["worktree"])
        self.bp("accept", "BP-3")
        merge = self.git("rev-parse", "main")
        self.assertEqual(len(self.git("rev-list", "--parents", "-n", "1", merge).split()), 3)
        self.bp("revert", "BP-3")
        brief = self.brief("BP-4")
        self.assertIn("(merged into main, by a merge commit). It left 1 commit on `main`", brief)
        self.assertIn(f"       git revert --no-commit -m 1 {merge}\n", brief)
        self.assertIn("`-m 1` reverts it against its first parent", brief)
        # and it does take the whole branch back
        self.do_the_revert("BP-4")
        self.bp("accept", "BP-4")
        self.assertFalse((self.repo / "more1.txt").exists() or (self.repo / "more2.txt").exists())
        self.assertTrue((self.repo / "later.txt").exists())

    def test_an_older_record_gives_its_commits_only_when_they_are_certain(self):
        # one commit: a rebase and a squash leave the same commit
        tid, _, after = self.accepted()
        self.change(tid, merged_how=None, merged_from=None)
        self.bp("revert", tid)
        self.assertIn(f"       git revert --no-commit {after}\n", self.brief("BP-2"))
        self.assertIn("BP-1 was accepted (merged into main). It left", self.brief("BP-2"))

    def test_an_older_record_of_several_commits_is_refused(self):
        tid, _, _ = self.accepted(3)
        self.change(tid, merged_how=None, merged_from=None)
        p = self.bp("revert", tid, ok=False)
        self.assertEqual(p.returncode, 1)
        self.assertIn("BP-1's record doesn't say how it was merged", p.stderr)
        self.assertIn("its branch had 3 commits: a rebase and a squash leave different commits", p.stderr)
        self.assertEqual(len(json.loads(self.bp("ls", "--all", "--json").stdout)), 1)  # no task was added

    def test_an_older_record_of_a_merge_commit_is_told_by_its_two_parents(self):
        tid, _, after = self.accepted(2, merge="merge")
        self.change(tid, merged_how=None, merged_from=None)
        self.bp("revert", tid)
        self.assertIn(f"       git revert --no-commit -m 1 {after}\n", self.brief("BP-2"))

    def refused(self, tid):
        p = self.bp("revert", tid, ok=False)
        self.assertEqual((p.returncode, p.stdout), (1, ""))
        return p.stderr.strip()

    def test_a_task_that_wasnt_accepted_is_refused(self):
        tid, wt = self.work()
        self.assertEqual(self.refused(tid), "bullpen: BP-1 is ready to accept: only an accepted task can be reverted")
        self.bp("drop", tid)
        self.assertEqual(self.refused(tid),
                         "bullpen: BP-1 was dropped: nothing of it was merged, so there is nothing to revert")
        # finished with nothing to merge, and closed
        self.bp("add", "A report", "--brief", "x")
        self.bp("start", "BP-2")
        self.bp("finish", "BP-2", "--summary", "Here it is.", cwd=self.show("BP-2")["worktree"])
        self.assertIn("only an accepted task can be reverted", self.refused("BP-2"))
        self.bp("close", "BP-2")
        self.assertEqual(self.refused("BP-2"),
                         "bullpen: BP-2 finished with nothing to merge: there is nothing to revert")
        # closed with its branch kept, not merged
        self.bp("add", "An experiment", "--brief", "x")
        self.bp("start", "BP-3")
        self.commit(self.show("BP-3")["worktree"], "try.txt", "try\n", "Try it")
        self.bp("close", "BP-3", "--keep-branch")
        self.assertEqual(self.refused("BP-3"),
                         "bullpen: BP-3 was closed with its branch kept, not merged: there is nothing to revert")
        # ready with no commits, accepted: nothing was merged
        self.bp("add", "Nothing", "--brief", "x")
        self.bp("start", "BP-4")
        self.bp("ready", "BP-4", "--summary", "Nothing to do.", cwd=self.show("BP-4")["worktree"])
        self.bp("accept", "BP-4")
        self.assertEqual(self.refused("BP-4"),
                         "bullpen: BP-4 finished with nothing to merge: there is nothing to revert")
        self.assertIn("no task BP-9", self.refused("BP-9"))

    def test_commits_no_longer_in_the_bases_history_are_refused(self):
        tid, before, after = self.accepted()
        self.git("reset", "-q", "--hard", before)
        self.assertEqual(self.refused(tid),
                         f"bullpen: BP-1's commits are no longer in main's history ({after[:12]} isn't in main): "
                         "there is nothing to revert")

    def test_show_says_it_on_both_tasks_and_a_second_revert_is_refused(self):
        tid, _, _ = self.accepted(2)
        self.assertNotIn("revert", self.bp("show", tid).stdout)
        self.bp("revert", tid)
        self.assertIn("\n reverted: a revert is open: BP-2\n", self.bp("show", tid).stdout)
        self.assertIn("\n  reverts: BP-1\n", self.bp("show", "BP-2").stdout)
        self.assertEqual(self.refused(tid), "bullpen: BP-1 already has an open revert task: BP-2 (working)")
        self.do_the_revert("BP-2")
        self.assertIn("a revert is open: BP-2", self.bp("show", tid).stdout)
        self.assertEqual(self.refused(tid), "bullpen: BP-1 already has an open revert task: BP-2 (ready to accept)")
        self.bp("accept", "BP-2")
        # one commit took both back, and the later work stays
        self.assertEqual(self.git("log", "-1", "--format=%s"), "Revert BP-1: Fix the thing")
        self.assertFalse((self.repo / "fix1.txt").exists() or (self.repo / "fix2.txt").exists())
        self.assertTrue((self.repo / "later.txt").exists())
        self.assertIn("\n reverted: reverted by BP-2\n", self.bp("show", tid).stdout)
        self.assertIn("\n  reverts: BP-1\n", self.bp("show", "BP-2").stdout)
        self.assertEqual(self.refused(tid),
                         "bullpen: BP-1 was reverted by BP-2 already: `bullpen revert BP-2` brings it back")
        # reverting the revert is just a revert, and then BP-1 can be taken back again
        self.assertIn("BP-3 added: Revert BP-2: Revert BP-1: Fix the thing", self.bp("revert", "BP-2").stdout)
        self.do_the_revert("BP-3")
        self.bp("accept", "BP-3")
        self.assertTrue((self.repo / "fix1.txt").exists() and (self.repo / "fix2.txt").exists())
        self.assertIn("\n reverted: reverted by BP-2 (which BP-3 reverted)\n", self.bp("show", tid).stdout)
        self.assertIn("BP-4 added: Revert BP-1: Fix the thing", self.bp("revert", tid).stdout)

    def test_a_dropped_revert_task_leaves_the_task_free_to_revert(self):
        tid, _, _ = self.accepted()
        self.bp("revert", tid)
        self.bp("drop", "BP-2")
        self.assertNotIn("revert", self.bp("show", tid).stdout)
        self.assertIn("BP-3 added", self.bp("revert", tid).stdout)

    def test_it_stays_todo_when_max_running_tasks_work(self):
        tid, _, _ = self.accepted()
        self.bp("config", "--max-running", "1")
        self.bp("add", "Busy", "--brief", "x")
        self.bp("start", "BP-2")
        out = self.bp("revert", tid).stdout.splitlines()
        self.assertEqual(out, ["BP-3 added: Revert BP-1: Fix the thing (1 commit to revert)",
                               "BP-3 stays todo (1 task is working, and max_running is 1): "
                               "`bullpen start BP-3` starts it"])
        self.assertEqual(self.show("BP-3")["status"], "todo")
        self.assertIn("a revert is open: BP-3", self.bp("show", tid).stdout)

    def test_the_coordinator_hears_of_a_revert_the_person_ran(self):
        tid, _, _ = self.accepted()
        self.bp("revert", tid)
        self.assertEqual(self.news()[-1], "bullpen · BP-2 started (it reverts BP-1)")

    def test_stats_and_the_timeline_take_the_new_key(self):
        tid, _, _ = self.accepted()
        self.bp("revert", tid)
        self.do_the_revert("BP-2")
        self.bp("accept", "BP-2")
        self.assertIn("BP-2", self.bp("stats", "BP-2").stdout)
        one = json.loads(self.bp("stats", "BP-2", "--json").stdout)
        self.assertEqual(one["id"], "BP-2")
        json.loads(self.bp("stats", "--json").stdout)
        self.bp("stats")
        timeline = next((self.tmp / "state").rglob("timeline/BP-2.jsonl")).read_text().splitlines()
        self.assertEqual(json.loads(timeline[0])["event"], "added")
        self.assertEqual(json.loads(timeline[-1])["outcome"], "accepted")


class RevertRemoteTest(RevertHelpers, BullpenTest):
    """Remote mode: accept merged the task's PR on GitHub."""

    def gh_calls(self):
        log = self.fake / "gh.log"
        return [json.loads(l)["argv"] for l in log.read_text().splitlines()] if log.exists() else []

    def accepted(self, n=1):
        """A task with n commits whose PR GitHub merged (by rebase) at accept: its ID and
        the commit gh says the merge left."""
        tid, wt = self.work(n)
        head = self.git("rev-parse", "HEAD", cwd=wt)
        self.merge_on_github(tid)
        self.github(state="OPEN", mergeCommit={"oid": head})
        self.bp("accept", tid)
        return tid, head

    def test_accept_records_the_commit_gh_says_the_merge_left(self):
        tid, head = self.accepted()
        t = self.show(tid)
        self.assertEqual((t["merged"], t["merged_how"]), (head, "rebase"))
        self.assertNotIn("merged_from", t)
        self.assertIn(["pr", "view", t["pr"], "--json", "mergeCommit"], self.gh_calls())

    def test_the_revert_task_names_the_prs_commits_and_opens_a_pr_like_any_other(self):
        tid, head = self.accepted(2)
        # someone else's work lands on origin's main after it
        self.commit(self.repo, "later.txt", "later\n", "Later work")
        self.git("pull", "-q", "--rebase", "origin", "main")
        self.git("push", "-q", "origin", "main")
        out = self.bp("revert", tid, "--why", "Too slow.").stdout
        self.assertIn("BP-2 added: Revert BP-1: Fix the thing (2 commits to revert)", out)
        shas = self.git("rev-list", "-n", "2", head).split()
        brief = self.brief("BP-2")
        self.assertIn("BP-1 was accepted (PR #7 merged, rebase). It left 2 commits on `main`", brief)
        self.assertIn(f"       git revert --no-commit {' '.join(shas)}\n", brief)
        self.assertIn("Why it's taken back: Too slow.", brief)
        self.assertIn("a revert is open: BP-2", self.bp("show", tid).stdout)
        creates = len([a for a in self.gh_calls() if a[:2] == ["pr", "create"]])
        wt = self.do_the_revert("BP-2")
        new = self.show("BP-2")
        self.assertEqual((new["status"], new["pr"]), ("ready", "https://github.com/me/repo/pull/7"))
        self.assertEqual(len([a for a in self.gh_calls() if a[:2] == ["pr", "create"]]), creates + 1)
        self.assertFalse((Path(wt) / "fix1.txt").exists() or (Path(wt) / "fix2.txt").exists())
        self.assertTrue((Path(wt) / "later.txt").exists())
        self.merge_on_github("BP-2")
        self.github(state="OPEN", mergeCommit={"oid": self.git("rev-parse", "HEAD", cwd=wt)})
        self.bp("accept", "BP-2")
        self.assertIn("reverted by BP-2", self.bp("show", tid).stdout)
        self.assertIn("reverts: BP-1", self.bp("show", "BP-2").stdout)

    def test_a_pr_merged_outside_bullpen_has_no_method_on_record(self):
        tid, wt = self.work(2)
        head = self.git("rev-parse", "HEAD", cwd=wt)
        self.merge_on_github(tid)
        self.github(state="MERGED", mergeCommit={"oid": head})
        self.bp("_prs")
        self.bp("accept", tid)
        t = self.show(tid)
        self.assertEqual(t["merged"], head)
        self.assertNotIn("merged_how", t)
        p = self.bp("revert", tid, ok=False)
        self.assertIn("its PR was merged outside bullpen", p.stderr)

    def test_an_older_record_asks_gh_for_the_merge_commit(self):
        tid, head = self.accepted()
        self.change(tid, merged=None, merged_how=None)
        self.bp("revert", tid)
        self.assertIn(f"       git revert --no-commit {head}\n", self.brief("BP-2"))

    def test_a_record_without_the_merge_commit_is_refused_when_gh_doesnt_say_it(self):
        tid, _ = self.accepted()
        self.change(tid, merged=None)
        self.github(state="MERGED")
        p = self.bp("revert", tid, ok=False)
        self.assertEqual(p.stderr.strip(),
                         "bullpen: BP-1's record doesn't name the commit its merge left on main, and gh doesn't "
                         "say it now: its commits can't be told from it. Revert it by hand")

    def test_commits_gone_from_origins_base_are_refused(self):
        tid, head = self.accepted()
        self.git("push", "-q", "--force", "origin", f"{head}~1:main")
        p = self.bp("revert", tid, ok=False)
        self.assertIn(f"BP-1's commits are no longer in main's history ({head[:12]} isn't in origin/main)", p.stderr)


if __name__ == "__main__":
    unittest.main()
