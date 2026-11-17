"""doctor --json, and doctor --all-repos: the setup's checks once, then each repo's."""

import json
import os
import re
import shutil
import unittest

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module

SHARED = ["rex", "bullpen", "board", "cargo", "keys", "watcher", "state", "temp", "global"]
EACH = ["gh", "agents", "config", "setup", "teardown", "coordinators", "copy", "risk", "news", "push"]


class DoctorJsonTest(BullpenTest):
    def test_doctor_json_has_each_check_and_the_problems(self):
        self.healthy()
        p = self.bp("doctor", "--json")
        d = json.loads(p.stdout)
        self.assertEqual(d["repo"], str(self.repo))
        self.assertEqual(d["problems"], [])
        names = [c["name"] for c in d["checks"]]
        self.assertEqual(names, ["rex", "gh", "agents", "bullpen", "board", "cargo", "keys", "watcher", "state",
                                 "temp", "global", "config", "setup", "teardown", "coordinators", "copy", "risk", "news", "push"])
        _, lines = self.doctor()  # the same checks as the lines say them
        self.assertEqual({c["name"]: c["state"] for c in d["checks"]}["rex"], "ok")
        self.assertEqual([c["text"] for c in d["checks"]], [lines[n][1] for n in names])
        (self.fake / "actions.json").write_text(json.dumps({"actions": []}))
        p = self.bp("doctor", "--json", ok=False)
        self.assertEqual(p.returncode, 1)
        self.assertEqual(json.loads(p.stdout)["problems"], ["keys"])

    def test_doctor_json_outside_a_repository(self):
        self.healthy()
        d = json.loads(self.bp("doctor", "--json", ok=False, cwd=self.tmp).stdout)
        self.assertIsNone(d["repo"])
        self.assertNotIn("config", [c["name"] for c in d["checks"]])


class DoctorTempTest(BullpenTest):
    """doctor's `temp` line: how full the temp folder is, and the Claude scratch folders
    closed tasks left in it, which `doctor --fix` removes."""

    def scratch(self, wt):
        d = self.tmp / "ctmp" / f"claude-{os.getuid()}" / re.sub("[^A-Za-z0-9]", "-", str(wt))
        (d / "session").mkdir(parents=True)
        (d / "session" / "f").write_text("x" * 5000)
        return d

    def test_the_line_counts_closed_tasks_folders_and_fix_leaves_open_ones(self):
        self.healthy()
        (self.tmp / "ctmp").mkdir()
        _, lines = self.doctor()
        self.assertEqual(lines["temp"][0], "ok")
        self.assertRegex(lines["temp"][1], r"^.*ctmp is \d+% full \(.+ free\); no Claude scratch folders left by "
                         r"closed tasks$")
        done = self.start_task()
        wt_done = self.show(done)["worktree"]
        self.bp("drop", done)
        left = self.scratch(wt_done)  # one from before scratch folders went with their worktree
        open_ = self.bp("add", "Still going", "--brief", "x").stdout.strip()
        self.bp("start", open_)
        kept = self.scratch(self.show(open_)["worktree"])
        other = self.scratch(self.tmp / "elsewhere")  # no task's
        _, lines = self.doctor()
        self.assertIn("; 1 Claude scratch folder of closed tasks hold", lines["temp"][1])
        self.assertTrue(lines["temp"][1].endswith(": `bullpen doctor --fix` removes them"))
        p = self.bp("doctor", "--fix", ok=False)
        self.assertIn("fixed  temp: removed 1 Claude scratch folder of closed tasks (", p.stdout)
        self.assertFalse(left.exists())
        self.assertTrue((kept / "session" / "f").exists())
        self.assertTrue(other.exists())
        _, lines = self.doctor()
        self.assertIn("no Claude scratch folders left by closed tasks", lines["temp"][1])
        self.assertNotIn("fixed", self.bp("doctor", "--fix", ok=False).stdout)

    def test_fix_works_outside_a_repository_and_never_takes_a_worktree_still_there(self):
        self.healthy()
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        self.bp("drop", tid)
        sh("git", "-C", str(self.repo), "worktree", "add", "-q", "--detach", wt)  # its path in use again
        d = self.scratch(wt)
        _, lines = self.doctor()
        self.assertIn("no Claude scratch folders left by closed tasks", lines["temp"][1])
        self.bp("doctor", "--fix", ok=False, cwd=self.tmp)
        self.assertTrue(d.exists())
        sh("git", "-C", str(self.repo), "worktree", "remove", "--force", wt)
        p = self.bp("doctor", "--fix", ok=False, cwd=self.tmp)
        self.assertIn("fixed  temp: removed 1", p.stdout)
        self.assertFalse(d.exists())

    def test_without_its_temp_folder_it_cant_tell(self):
        self.healthy()
        _, lines = self.doctor()
        self.assertEqual(lines["temp"][0], "--")


class DoctorAllReposTest(BullpenTest):
    def test_the_setup_once_then_each_repo(self):
        self.healthy()
        shop = self.other_repo("shop")
        self.bp("up", cwd=shop)
        gone = self.other_repo("gone")
        self.bp("add", "Lost", "--brief", "x", cwd=gone)
        shutil.rmtree(gone)  # its checkout is gone: left out
        p, sections = self.doctor_all(cwd=self.tmp)  # works outside a repository
        self.assertEqual(p.returncode, 0, p.stdout)
        self.assertEqual(list(sections), ["Setup", "proj", "shop"])
        self.assertEqual(list(sections["Setup"]), SHARED)
        for repo in ("proj", "shop"):
            self.assertEqual(list(sections[repo]), EACH)
        self.assertEqual(p.stdout.count(" rex: "), 1)  # the setup's checks run once
        self.assertIn(f"\nshop  ({shop})\n", p.stdout)
        self.assertEqual(sections["proj"]["coordinators"], ("ok", "c1 (blk1)"))
        self.assertEqual(sections["shop"]["gh"][0], "--")  # local mode: no gh
        self.assertEqual(p.stdout.splitlines()[-1], "All good.")

    def test_a_board_on_an_older_build_and_a_broken_config_show_up(self):
        self.healthy()
        shop = self.other_repo("shop")
        self.bp("up", cwd=shop)
        board = json.loads(self.bp("config", "--json", cwd=shop).stdout)["coordinators"][0]["board"]["block"]
        (self.fake / f"prog_{board}").write_text(str(self.tmp / "old" / "release" / "bullpen-board"))
        broken = self.tmp / "state" / "repos" / "broken"
        broken.mkdir(parents=True)
        (broken / "config.json").write_text("{")
        (self.fake / "actions.json").write_text(json.dumps({"actions": []}))
        p, sections = self.doctor_all()
        self.assertEqual(p.returncode, 1)
        self.assertEqual(sections["shop"]["coordinators"],
                         ("FAIL", "c1's board is from an older build: `bullpen up` restarts it"))
        self.assertIn("\nbroken  (its config.json doesn't parse)\n", p.stdout)
        self.assertEqual(sections["broken"]["config"][0], "FAIL")
        self.assertEqual(sections["broken"]["setup"], ("--", "can't tell: config.json doesn't parse"))
        self.assertEqual(p.stdout.splitlines()[-1],
                         "3 problems: keys, broken/config, shop/coordinators.")
        d = json.loads(self.bp("doctor", "--all-repos", "--json", ok=False).stdout)
        self.assertEqual([p["name"] for p in d["problems"]], ["keys", "broken/config", "shop/coordinators"])
        # each says which folder its fix is run in (none for the setup's, or an unreadable config)
        self.assertEqual(d["problems"][2], {"name": "shop/coordinators", "check": "coordinators",
                                            "repo": str(shop), "text": sections["shop"]["coordinators"][1]})
        self.assertEqual([p["repo"] for p in d["problems"][:2]], [None, None])
        self.assertEqual([c["name"] for c in d["shared"]], SHARED)
        self.assertEqual([(r["name"], r["repo"]) for r in d["repos"]],
                         [("broken", None), ("proj", str(self.repo)), ("shop", str(shop))])
        self.assertEqual([c["name"] for c in d["repos"][2]["checks"]], EACH)

    def test_two_repos_of_the_same_name_are_told_apart(self):
        self.healthy()
        a, b = self.other_repo("a/shop"), self.other_repo("b/shop")
        for r in (a, b):
            self.bp("up", cwd=r)
        for r in (a, b):  # each its own problem: a board on an older build
            board = json.loads(self.bp("config", "--json", cwd=r).stdout)["coordinators"][0]["board"]["block"]
            (self.fake / f"prog_{board}").write_text(str(self.tmp / "old" / "release" / "bullpen-board"))
        p, _ = self.doctor_all()
        self.assertEqual(p.stdout.splitlines()[-1], "2 problems: a/shop/coordinators, b/shop/coordinators.")
        d = json.loads(self.bp("doctor", "--all-repos", "--json", ok=False).stdout)
        self.assertEqual([(p["name"], p["repo"]) for p in d["problems"]],
                         [("a/shop/coordinators", str(a)), ("b/shop/coordinators", str(b))])
        self.assertEqual([r["name"] for r in d["repos"]].count("shop"), 2)

    def test_a_broken_global_config_still_lists_each_repo(self):
        self.healthy()
        (self.tmp / "xdg" / "bullpen" / "config.json").parent.mkdir(parents=True, exist_ok=True)
        (self.tmp / "xdg" / "bullpen" / "config.json").write_text("{oops")
        p, sections = self.doctor_all()
        self.assertEqual(sections["Setup"]["global"][0], "FAIL")
        self.assertIn(f"\nproj  ({self.repo})\n", p.stdout)
        self.assertEqual(sections["proj"]["setup"], ("--", "can't tell: the global config.json doesn't parse"))


if __name__ == "__main__":
    unittest.main()
