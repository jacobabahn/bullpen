"""The risk label on a ready task: bullpen's rules and the reviewer's judgement, and
where it shows (news, show, ls, stats, --json)."""

import json

from support import setUpModule, tearDownModule, sh, odd_name, BullpenTest  # a temp folder for each module

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
        self.assertEqual((r["level"], r["why"]), ("high", "changes how accept merges (1 line)"))
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

    def test_files_whose_names_git_quotes_count_where_they_belong(self):
        self.bp("config", "--review", "off")
        tid = self.start_task()
        self.commit_file(tid, f"docs/{odd_name('usage')}.md", "How to.\n")
        self.commit_file(tid, f"tests/{odd_name('test_x')}.py", "".join(f"x{i}\n" for i in range(90)))
        self.ready(tid)
        r = self.show(tid)["risk"]
        self.assertEqual((r["level"], r["why"]), ("low", "docs and tests only"))
        self.bp("tell", tid, "More.")
        self.commit_file(tid, f"prompts/{odd_name('worker')}.md", "Be careful.\n")
        self.ready(tid)
        r = self.show(tid)["risk"]
        self.assertEqual((r["level"], r["why"]), ("medium", "changes a prompt"))

    def test_a_sensitive_path_whose_name_git_quotes_is_counted(self):
        self.bp("config", "--review", "off")
        self.config(risk_paths={"secret/": "changes the secrets"})
        tid = self.start_task()
        self.commit_file(tid, f"secret/{odd_name('key')}.txt", "k\n")
        self.ready(tid)
        r = self.show(tid)["risk"]
        self.assertEqual((r["level"], r["why"]), ("high", "changes the secrets (1 line)"))

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

    def rebased_after_an_accept(self, before=lambda: None):
        """BP-1 and BP-2 are approved (BP-2 rated high by its reviewer); before() runs, then
        BP-1 is accepted, which rebases BP-2 keeping its approval. Returns BP-2's worktree."""
        tid = self.approved_task()
        self.bp("add", "Second", "--brief", "x")
        self.bp("start", "BP-2")
        wt = self.commit_file("BP-2", "two.txt", "two\n")
        self.ready("BP-2")
        self.verdict("BP-2", "approve", APPROVE_HIGH)
        before()
        self.merge_on_github(tid)
        self.bp("accept", tid)
        self.assertIn("BP-2 rebased; its approval carried over (the same patch after a clean rebase onto "
                      "origin/main)", self.news()[-1])
        return wt

    def test_a_task_rebased_after_an_accept_keeps_its_label(self):
        wt = self.rebased_after_an_accept()  # the same patch on a new base: the rules say the same
        t = self.show("BP-2")
        r = t["risk"]
        self.assertEqual(t["status"], "ready")
        self.assertEqual((r["level"], r["why"]), ("high", "couldn't run the GitHub path"))
        self.assertEqual(r["rules"], {"level": "low", "why": "a small change (1 line in 1 file)"})
        self.assertEqual(r["commit"], sh("git", "-C", wt, "rev-parse", "HEAD").strip())  # it follows the code

    def test_a_task_rebased_after_an_accept_is_labelled_by_the_rules_as_they_are_now(self):
        def sensitive():  # after BP-2 was labelled
            self.bp("config", "--risk-path", "two.txt", "--level", "high", "--why", "changes two")
        self.rebased_after_an_accept(sensitive)
        r = self.show("BP-2")["risk"]
        self.assertEqual(r["rules"], {"level": "high", "why": "changes two (1 line)"})
        self.assertEqual((r["level"], r["why"]), ("high", "couldn't run the GitHub path"))  # the reviewer's kept

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

    def test_size_alone_is_medium_until_far_past_the_line(self):
        self.bp("config", "--review", "off", "--risk-huge-lines", "500")
        tid = self.start_task()
        self.commit_file(tid, "big.txt", "".join(f"line {i}\n" for i in range(450)))
        self.ready(tid)
        r = self.show(tid)["risk"]
        self.assertEqual((r["level"], r["why"]), ("medium", "450 lines in 1 file"))  # high, before
        self.bp("tell", tid, "More.")
        self.commit_file(tid, "more.txt", "".join(f"line {i}\n" for i in range(51)))
        self.ready(tid)
        r = self.show(tid)["risk"]
        self.assertEqual((r["level"], r["why"]), ("high", "a very large change (501 lines in 2 files)"))

    def test_a_medium_area_by_how_much_of_it_is_touched(self):
        self.bp("config", "--review", "off", "--risk-large-lines", "30")
        self.bp("config", "--risk-path", "queue/", "--level", "medium", "--why", "changes when the queue starts a task")
        tid = self.start_task()
        self.commit_file(tid, "queue/start.txt", "".join(f"line {i}\n" for i in range(10)))
        self.ready(tid)
        r = self.show(tid)["risk"]  # a tiny touch doesn't count
        self.assertEqual((r["level"], r["why"]), ("low", "a small change (10 lines in 1 file)"))
        for lines, label in ((11, ("medium", "changes when the queue starts a task (11 lines)")),
                             (31, ("high", "changes when the queue starts a task, and a lot of it (31 lines)"))):
            self.bp("tell", tid, "More.")
            self.commit_file(tid, "queue/start.txt", "".join(f"line {i}\n" for i in range(lines)))
            self.ready(tid)
            r = self.show(tid)["risk"]
            self.assertEqual((r["level"], r["why"]), label)
        self.assertTrue(self.news()[-1].endswith(
            "; risk: high · changes when the queue starts a task, and a lot of it (31 lines)"))

    def test_the_older_forms_of_risk_paths_are_high(self):
        self.bp("config", "--review", "off")
        for form, why in (({"fix.txt": "changes how accept merges"}, "changes how accept merges (1 line)"),
                          (["fix.txt"], "changes fix.txt (1 line)")):
            self.config(risk_paths=form)
            tid = self.start_task()
            self.commit_and_ready_no_review(tid)
            r = self.show(tid)["risk"]
            self.assertEqual((r["level"], r["why"]), ("high", why))

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


class RiskPathsTest(RiskTest):
    """`config --risk-path`: the repository's sensitive paths, set without editing config.json."""
    def paths(self):
        return json.loads(self.bp("config", "--json").stdout)["risk_paths"]

    def test_config_adds_changes_and_removes_a_risk_path(self):
        self.assertIsNone(self.paths())
        self.bp("config", "--risk-path", "cli/src/accept/", "--why", "changes how accept merges")
        self.bp("config", "--risk-path", "cli/src/review.rs", "--level", "medium", "--why", "changes how review runs")
        self.bp("config", "--risk-path", "bin/")
        self.assertEqual(self.paths(), {
            "cli/src/accept/": {"level": "high", "why": "changes how accept merges"},  # high unless it says
            "cli/src/review.rs": {"level": "medium", "why": "changes how review runs"},
            "bin/": {"level": "high", "why": "changes bin/"}})
        self.bp("config", "--risk-path", "cli/src/accept/", "--level", "medium", "--why", "changes accept")
        self.assertEqual(list(self.paths()), ["cli/src/accept/", "cli/src/review.rs", "bin/"])  # in its place
        self.assertEqual(self.paths()["cli/src/accept/"], {"level": "medium", "why": "changes accept"})
        out = self.bp("config", "--remove-risk-path", "bin/", "--remove-risk-path", "cli/src/accept/").stdout
        self.assertEqual(self.paths(), {"cli/src/review.rs": {"level": "medium", "why": "changes how review runs"}})
        self.assertIn('{"cli/src/review.rs": {"level": "medium", "why": "changes how review runs"}}', out)  # listed
        self.bp("config", "--remove-risk-path", "cli/src/review.rs")
        self.assertEqual(self.paths(), {})

    def test_a_list_or_whys_written_by_hand_get_their_levels_when_one_is_added(self):
        self.bp("config", "--review", "off")  # its config.json is there
        self.config(risk_paths={"install.sh": "changes the install"})
        self.bp("config", "--risk-path", "lua/", "--level", "medium")
        self.assertEqual(self.paths(), {"install.sh": {"level": "high", "why": "changes the install"},
                                        "lua/": {"level": "medium", "why": "changes lua/"}})
        self.config(risk_paths=["install.sh", "bin/"])
        self.bp("config", "--remove-risk-path", "bin/")
        self.assertEqual(self.paths(), {"install.sh": {"level": "high", "why": "changes install.sh"}})

    def test_what_it_refuses(self):
        for flags, says in (
                (["--level", "medium"], "--level and --why go with --risk-path PATTERN"),
                (["--why", "x"], "--level and --why go with --risk-path PATTERN"),
                (["--risk-path", " "], "--risk-path takes a path in the repository"),
                (["--risk-path", "/abs/bin"], "--risk-path takes a path in the repository, from its top"),
                (["--risk-path", "../bin"], "--risk-path takes a path in the repository, from its top"),
                (["--risk-path", "./"], "--risk-path takes a path in the repository, from its top"),
                (["--risk-path", "a", "--risk-path", "b"], "--risk-path takes one pattern a run"),
                (["--remove-risk-path", "bin/"], "no risk path bin/: this repository has none"),
                (["--global", "--risk-path", "bin/", "--level", "high"],
                 "--risk-path, --level are per repository, so not for the global config")):
            self.assertIn(says, self.bp("config", *flags, ok=False).stderr)
        self.assertIn("invalid choice: 'low'", self.bp("config", "--risk-path", "a", "--level", "low", ok=False).stderr)
        self.bp("config", "--risk-path", "./lua/")  # as git names it, without the ./
        p = self.bp("config", "--remove-risk-path", "bin/", ok=False)
        self.assertIn("no risk path bin/: this repository has lua/", p.stderr)
        self.assertEqual(list(self.paths()), ["lua/"])

    def test_doctor_says_once_when_none_are_set_and_it_is_no_problem(self):
        self.healthy()
        p, lines = self.doctor()
        self.assertEqual(lines["risk"], ("ok", "no risk paths set, so a ready task's risk goes by its size alone: "
                                               "name the sensitive ones with "
                                               '`bullpen config --risk-path PATTERN --level high|medium --why "..."`'))
        self.assertEqual(p.returncode, 0)
        self.assertEqual(p.stdout.count("risk path"), 1)
        self.bp("config", "--risk-path", "bin/")
        self.bp("config", "--risk-path", "lua/", "--level", "medium")
        self.assertEqual(self.doctor()[1]["risk"], ("ok", "2 risk paths (1 high, 1 medium)"))
