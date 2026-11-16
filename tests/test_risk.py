"""The risk label on a ready task: bullpen's rules and the reviewer's judgement, and
where it shows (news, show, ls, stats, --json)."""

import json

from support import setUpModule, tearDownModule, sh, BullpenTest  # a temp folder for each module

APPROVE_HIGH = "## Findings\n\nChecked it.\n\n## Risk\n\nhigh: couldn't run the GitHub path\n"
APPROVE_LOW = "## Findings\n\nChecked it.\n\n## Risk\n\nlow: a rename\n"


class RiskTest(BullpenTest):
    def config(self, **settings):
        path = next((self.tmp / "state" / "repos").iterdir()) / "config.json"
        cfg = json.loads(path.read_text())
        cfg.update(settings)
        path.write_text(json.dumps(cfg))

    def ls(self):
        return self.bp("ls").stdout

    def test_a_reviewer_rates_above_the_rules_and_it_shows_everywhere(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.verdict(tid, "approve", APPROVE_HIGH)
        t = self.show(tid)  # show --json: the record has it
        self.assertEqual((t["risk"]["level"], t["risk"]["why"]), ("high", "couldn't run the GitHub path"))
        self.assertEqual(t["risk"]["rules"], {"level": "low", "why": "a small change (1 line in 1 file)"})
        self.assertEqual(t["risk"]["review"], {"level": "high", "why": "couldn't run the GitHub path", "round": 1})
        self.assertTrue(self.news()[-1].endswith(
            "approved by light review: 1 line in 1 file); risk: high · couldn't run the GitHub path"))
        self.assertIn("     risk: high · couldn't run the GitHub path (rules: low · a small change (1 line in 1 file); "
                      "review: high · couldn't run the GitHub path)\n", self.bp("show", tid).stdout)
        line = next(l for l in self.ls().splitlines() if tid in l)
        self.assertIn("Fix the thing  high  ", line)  # the word before the age
        self.assertIn("High risk: couldn't run the GitHub path.", line)
        self.assertEqual(json.loads(self.bp("ls", "--json").stdout)[0]["risk"]["level"], "high")

    def test_the_reviewer_cant_lower_a_sensitive_path(self):
        tid = self.start_task()
        self.config(risk_paths={"fix1.txt": "changes how accept merges"})
        self.commit_and_ready(tid)
        self.verdict(tid, "approve", APPROVE_LOW)
        r = self.show(tid)["risk"]
        self.assertEqual((r["level"], r["why"]), ("high", "changes how accept merges"))
        self.assertEqual(r["review"]["level"], "low")

    def test_review_off_labels_by_the_rules_alone(self):
        tid = self.ready_with_a_commit()
        r = self.show(tid)["risk"]
        self.assertEqual((r["level"], r["why"], r["review"]), ("low", "a small change (1 line in 1 file)", None))
        self.assertTrue(self.news()[-1].endswith("; risk: low · a small change (1 line in 1 file)"))

    def test_docs_only_is_low(self):
        tid = self.start_task()
        self.commit_file(tid, "docs/usage.md", "How to.\n")
        self.ready(tid)
        self.verdict(tid, "approve")  # no Risk part: the rules alone
        r = self.show(tid)["risk"]
        self.assertEqual((r["level"], r["why"], r["review"]), ("low", "docs only", None))

    def test_a_clean_rebase_keeps_the_label(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.verdict(tid, "approve", APPROVE_HIGH)
        wt = self.show(tid)["worktree"]
        (self.repo / "other.txt").write_text("other\n")
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "Other work")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")
        sh("git", "-C", wt, "fetch", "-q", "origin")
        sh("git", "-C", wt, "rebase", "-q", "origin/main")
        self.bp("tell", tid, "Rebase onto main.")
        self.assertIn("approval carries over", self.ready(tid).stdout)
        r = self.show(tid)["risk"]
        self.assertEqual((r["level"], r["why"]), ("high", "couldn't run the GitHub path"))
        self.assertEqual(r["commit"], sh("git", "-C", wt, "rev-parse", "HEAD").strip())  # it follows the code

    def test_a_change_after_approval_works_the_rules_out_again(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.verdict(tid, "approve", APPROVE_LOW)
        self.bp("tell", tid, "Do more.")
        self.commit_file(tid, "prompts/worker.md", "More.\n")
        self.ready(tid)
        self.verdict(tid, "approve")  # round 2 gives no Risk part: round 1's stays
        r = self.show(tid)["risk"]
        self.assertEqual(r["rules"], {"level": "medium", "why": "changes a prompt"})
        self.assertEqual(r["review"], {"level": "low", "why": "a rename", "round": 1})
        self.assertEqual((r["level"], r["why"]), ("medium", "changes a prompt"))

    def test_a_light_review_set_on_a_big_change_is_at_least_medium(self):
        tid = self.start_task()
        self.bp("review-mode", tid, "light")
        self.commit_file(tid, "big.txt", "".join(f"line {i}\n" for i in range(60)))
        self.ready(tid)
        self.verdict(tid, "approve", APPROVE_LOW)
        r = self.show(tid)["risk"]
        self.assertEqual((r["level"], r["why"]), ("medium", "a light review where its size calls for a full one"))

    def test_stats_timeline_says_the_label(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.verdict(tid, "approve", APPROVE_HIGH)
        out = self.bp("stats", tid).stdout
        self.assertIn("ready to accept (risk: high · couldn't run the GitHub path)", out)
        events = json.loads(self.bp("stats", tid, "--json").stdout)["events"]
        ready = [e for e in events if e.get("to") == "ready"][-1]
        self.assertEqual(ready["risk"], {"level": "high", "why": "couldn't run the GitHub path"})

    def test_the_reviewer_is_asked_for_its_level(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        prompt = next((self.tmp / "state").rglob(f"{tid}.review-1.md")).read_text()
        self.assertIn("- `## Risk` (with an approval): one line, `low`, `medium` or `high`", prompt)


class LateLabelTest(RiskTest):
    """A task made ready by a copy of bullpen from before the label has no `risk` in its
    record: `check --fix` (the watcher's) labels it and says so, once."""
    def unlabel(self, tid):
        """The task as an older copy leaves it: no `risk` key at all. Returns the label it had."""
        path = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
        t = json.loads(path.read_text())
        risk = t.pop("risk")
        path.write_text(json.dumps(t))
        return risk

    def test_a_ready_task_without_a_label_gets_it_and_one_news_line(self):
        tid = self.ready_with_a_commit()
        risk, told = self.unlabel(tid), len(self.news())
        self.assertNotIn("risk", self.show(tid))
        self.assertEqual(self.check(settle=False), [f"{tid} is ready without a risk label"])
        self.assertEqual(self.bp("check", ok=False).returncode, 1)
        self.assertNotIn("risk", self.show(tid))  # check alone changes nothing
        self.assertEqual(self.check("--fix", settle=False), [f"{tid} is ready without a risk label (fixed)"])
        self.assertEqual(self.show(tid)["risk"], risk)
        self.assertEqual(self.news()[told:], [
            f"bullpen · {tid} became ready without a risk label, so here it is: "
            "risk: low · a small change (1 line in 1 file). Tell the person"])
        self.assertIn("risk: low · a small change (1 line in 1 file)", self.bp("show", tid).stdout)
        self.assertEqual(self.check("--fix", settle=False), [])  # once
        self.assertEqual(len(self.news()), told + 1)

    def test_the_late_label_has_the_reviewers_part_from_the_approvals_findings(self):
        tid = self.start_task()
        self.commit_and_ready(tid)
        self.verdict(tid, "approve", APPROVE_HIGH)
        risk = self.unlabel(tid)
        self.assertEqual(risk["review"], {"level": "high", "why": "couldn't run the GitHub path", "round": 1})
        self.bp("check", "--fix", "--quiet")
        self.assertEqual(self.show(tid)["risk"], risk)
        self.assertEqual(self.news()[-1], f"bullpen · {tid} became ready without a risk label, so here it is: "
                                          "risk: high · couldn't run the GitHub path. Tell the person")

    def test_a_task_with_a_label_or_with_nothing_to_merge_is_left_alone(self):
        tid = self.ready_with_a_commit()
        before, told = self.show(tid), len(self.news())
        self.assertEqual(self.check("--fix", settle=False), [])
        self.assertEqual(self.show(tid), before)
        empty = self.start_task("Look into it")
        self.bp("ready", empty, "--summary", "Nothing to change.")
        self.assertIsNone(self.unlabel(empty))
        told = len(self.news())
        self.assertEqual(self.check("--fix", settle=False), [])
        self.assertNotIn("risk", self.show(empty))
        self.assertEqual(len(self.news()), told)
