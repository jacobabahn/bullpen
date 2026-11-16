"""doctor --json, and doctor --all-repos: the setup's checks once, then each repo's."""

import json
import shutil
import unittest

from support import setUpModule, tearDownModule, BullpenTest  # a temp folder for each module

SHARED = ["rex", "bullpen", "board", "cargo", "keys", "watcher", "state", "global"]
EACH = ["gh", "agents", "config", "setup", "teardown", "coordinators", "copy", "push"]


class DoctorJsonTest(BullpenTest):
    def test_doctor_json_has_each_check_and_the_problems(self):
        self.healthy()
        p = self.bp("doctor", "--json")
        d = json.loads(p.stdout)
        self.assertEqual(d["repo"], str(self.repo))
        self.assertEqual(d["problems"], [])
        names = [c["name"] for c in d["checks"]]
        self.assertEqual(names, ["rex", "gh", "agents", "bullpen", "board", "cargo", "keys", "watcher", "state",
                                 "global", "config", "setup", "teardown", "coordinators", "copy", "push"])
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
