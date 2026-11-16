"""The coordinator's guide: the sections a repository's settings bring, the news that one
came, and the table every news line has to match (or say what to do itself)."""

import hashlib
import json
import re
import subprocess

from support import setUpModule, tearDownModule, BullpenTest, BULLPEN, NEWS_ROWS, news_row  # a temp folder for each module

SECTIONS = ["News by wake", "News by monitor", "Pull requests", "Auto-accept", "Pickup", "Other coordinators"]
CORE = ["Rules", "Plan", "Commands", "News", "Triage", "Reporting"]


class GuideSectionsTest(BullpenTest):
    def guide(self, *args, cwd=None):
        return self.bp("guide", *args, cwd=cwd).stdout

    def headings(self, *args, cwd=None):
        return re.findall(r"^## (.*)$", self.guide(*args, cwd=cwd), re.M)

    def guide_typed(self, block="blk1"):
        return [l for b, l in self.typed(guide=True) if b == block and l.startswith("bullpen · Your coordinator guide")]

    def has_guide_now(self, block="blk1"):
        """Is the guide that coordinator was given the repo's guide now?"""
        c = next(c for c in self.coordinators() if c["block"] == block)
        return c["guide"] == hashlib.sha256(self.guide()[:-1].encode()).hexdigest()

    def test_a_guide_has_only_the_sections_that_apply_to_its_repository(self):
        # a remote, typed news, one coordinator, nothing turned on
        self.assertEqual(self.headings(), ["Rules", "Plan", "Commands", "News", "Triage", "Pull requests", "Reporting"])
        subprocess.run(["git", "-C", str(self.repo), "remote", "remove", "origin"], check=True)
        self.assertEqual(self.headings(), CORE)  # local mode: no pull requests
        for flags, section in ((["--auto-accept", "on"], "Auto-accept"), (["--pickup", "on"], "Pickup"),
                               (["--news", "monitor"], "News by monitor")):
            self.bp("config", *flags)
            self.assertEqual(self.headings(), CORE[:5] + [section, "Reporting"], flags)
            self.bp("config", *[f if f != "on" else "off" for f in flags[:1]] + ["off" if flags[1] == "on" else "type"])
        self.assertEqual(self.headings(), CORE)
        self.bp("config", "--pickup", "on", "--pickup-auto-accept", "on")  # what pickup starts accepts by itself
        self.assertEqual(self.headings(), CORE[:5] + ["Auto-accept", "Pickup", "Reporting"])

    def test_all_and_outside_a_repository_print_every_section(self):
        want = CORE[:5] + SECTIONS + ["Reporting"]
        self.assertEqual(self.headings("--all"), want)
        self.assertEqual(self.headings(cwd=self.tmp), want)
        self.assertNotIn("{{", self.guide("--all"))

    def test_a_second_coordinator_brings_the_first_its_section(self):
        other = self.two_coordinators()
        self.assertIn("Other coordinators", self.headings())
        [told] = self.guide_typed()
        lines = told.split("\n")
        self.assertEqual(lines[0], "bullpen · Your coordinator guide changed with this repo's settings: it has the "
                                   "section Other coordinators now. Read it below and follow it from now on:")
        self.assertEqual(lines[1], "## Other coordinators")
        self.assertIn("Leave them alone", told)
        self.assertEqual(self.guide_typed(other), [])  # it read the guide with the section in it
        self.assertTrue(self.has_guide_now() and self.has_guide_now(other))
        (self.fake / "alive" / other).unlink()
        self.bp("up")  # the second is dropped: its section no longer applies, which is no news
        self.assertNotIn("Other coordinators", self.headings())
        self.assertEqual(len(self.guide_typed()), 1)
        self.assertTrue(self.has_guide_now())

    def test_a_setting_turned_on_tells_a_running_coordinator_its_section_once(self):
        self.bp("up")
        p = self.bp("config", "--auto-accept", "on", "--json")
        self.assertTrue(json.loads(p.stdout)["auto_accept"])  # nothing else printed
        [told] = self.guide_typed()
        self.assertTrue(told.startswith("bullpen · Your coordinator guide changed with this repo's settings: it has "
                                        "the section Auto-accept now. Read it below and follow it from now on:\n"
                                        "## Auto-accept\n"))
        self.assertIn("run the full suite again and run `ready` again", " ".join(told.split()))
        self.assertTrue(self.has_guide_now())
        self.bp("config", "--max-running", "5")  # no section's setting
        self.bp("up")
        self.assertEqual(len(self.guide_typed()), 1)
        self.bp("config", "--auto-accept", "off")  # gone: not said, and not said again later
        self.bp("up")
        self.assertEqual(len(self.guide_typed()), 1)
        self.assertTrue(self.has_guide_now())
        self.bp("config", "--pickup", "on", "--pickup-auto-accept", "on")
        self.assertIn("it has the sections Auto-accept and Pickup now. Read them below", self.guide_typed()[-1])

    def test_a_global_setting_tells_every_repositorys_coordinator(self):
        self.bp("up")
        self.bp("config", "--global", "--auto-accept", "on")
        self.assertEqual(len(self.guide_typed()), 1)
        self.assertIn("the section Auto-accept now", self.guide_typed()[0])

    def test_a_news_switch_carries_its_section_in_the_switch_line(self):
        tid = self.start_task()
        for mode, heading in (("monitor", "## News by monitor"), ("wake", "## News by wake")):
            self.bp("config", "--news", mode)
            told = self.typed()[-1][1].split("\n")
            self.assertTrue(told[0].startswith(f"bullpen · news now {'comes by monitor' if mode == 'monitor' else 'wakes you'}"))
            self.assertEqual(told[1], heading)
            self.assertTrue(self.has_guide_now(), mode)  # told with the switch: no news of its guide
            self.bp("config", "--news", "type")
            self.assertTrue(self.has_guide_now(), mode)
        [monitor] = [l for b, l in self.typed() if "## News by monitor" in l]
        self.assertRegex(monitor, rf"\n- command: `env BULLPEN_STATE=\S+ .*{BULLPEN} news --follow`\n")
        self.assertEqual(monitor.count("news --follow"), 1)  # in its section, not in the line too
        self.ask(tid, "typed again")
        self.assertEqual(self.guide_typed(), [])


class NewsTableTest(BullpenTest):
    """Every news line bullpen sends says what to do itself or matches a row of the
    guide's table. `support.check_news` holds each test's own news to that; here, the
    table itself and a line of each kind."""

    def table(self):
        guide = self.bp("guide", "--all").stdout
        rows = re.findall(r"^\| (.*?) \| (.*?) \|$", guide, re.M)
        examples = guide.split("\n```\n")
        block = next(b for b in examples if b.startswith("bullpen · For the coordinator:"))
        return rows[1:], [l.removeprefix("bullpen · ") for l in block.splitlines()]

    def test_the_tables_rows_are_the_ones_the_tests_hold_news_to(self):
        rows, _ = self.table()
        self.assertEqual([r[0] for r in rows], [*NEWS_ROWS, "anything else"])

    def test_the_guide_has_one_example_for_each_row_and_one_that_says_it_itself(self):
        _, examples = self.table()
        self.assertEqual([news_row(l) for l in examples], [*NEWS_ROWS, "own"])

    # A line of each kind bullpen sends, and where the coordinator finds what to do.
    ROWS = dict(zip(("triage", "person", "answer", "report", "nothing", "overlap"), NEWS_ROWS))
    LINES = [
        ("triage", "For the coordinator: BP-5 ended its turn without `ready`: read it with `bullpen peek BP-5` "
                   "(still here in 10 min, it goes to the person)"),
        ("triage", "For the coordinator: BP-6's reviewer stopped without a verdict: `bullpen go BP-6 --review` shows "
                   "it, `bullpen review BP-6` starts a new one (still here in 10 min, it goes to the person)"),
        ("person", "BP-4 is at a permission prompt: Allow npm install?"),
        ("person", "BP-4's reviewer is at a prompt (question): Which config file?"),
        ("person", "BP-6 needs the person: Failed: boom (coordinator didn't resolve it)"),
        ("person", "BP-6's turn failed: read it with `bullpen peek BP-6`. No coordinator is running to see to it, "
                   "so it needs the person"),
        ("answer", "BP-3: the person answers \"Which config file?\": the one in the repo"),
        ("report", "BP-3 is ready: Fix login redirect (2 commits, PR #12, approved by full review); "
                   "risk: medium · 120 lines in 4 files"),
        ("report", "BP-3 is ready again after a rebase: Fix login redirect (2 commits, PR #12, its approval carried "
                   "over: only the base moved); risk: medium · 120 lines in 4 files"),
        ("report", "BP-5 is ready: Tidy the README (PR #13, approved by review), but the auto-accept was refused: "
                   "it still conflicts with main in README.md after 3 trips back to rebase. Left for you"),
        ("report", "BP-11 finished (nothing to merge): Recorded the demo video, saved to ~/Movies/demo.mov"),
        ("report", "BP-11 finished (branch to be kept, not merged): Tried the port"),
        ("report", "BP-11 finished (nothing to merge) and was closed by auto-accept: Recorded the demo video"),
        ("report", "BP-3 approved by review with 2 notes and auto-accepted (merged PR #12, rebase)"),
        ("nothing", "BP-3 accepted (merged PR #12, rebase)"),
        ("nothing", "BP-11 closed (finished, nothing to merge)"),
        ("nothing", "BP-11 closed without merging. Its branch is kept: try-a-port"),
        ("nothing", "BP-9 dropped"),
        ("nothing", "BP-4 started (BP-3 was accepted)"),
        ("nothing", "BP-9 started by itself from the backlog (L7: Tidy the errors)"),
        ("nothing", "BP-5 is working again (was ready): it must run `ready` again; its approval holds if the code "
                    "doesn't change"),
        ("nothing", "BP-3 sent back by the person after the tour: 2 comments"),
        ("nothing", "BP-3's PR checks failed: test. Sent back to its worker."),
        ("nothing", "BP-5 approved by review, but the auto-accept was refused: it conflicts with main in README.md. "
                    "Sent back to its worker to rebase (trip 1 of 3)"),
        ("nothing", "After BP-8: BP-5 told to rebase (conflicts in bin/bullpen); BP-7 left alone (no overlap)."),
        ("overlap", "BP-4 and BP-6 both changed cli/src/accept/mod.rs (and 2 more)"),
        ("own", "BP-7 stopped before saying ready: `bullpen resume BP-7` continues it"),
        ("own", "BP-9 has shown no progress for 20 min: `bullpen peek BP-9`"),
        ("own", "BP-9's reviewer has shown no progress for 20 min: `bullpen go BP-9 --review` shows it"),
        ("own", "BP-4 added L8 to the backlog: Split watch.lua's timers (`bullpen later L8 --keep` puts it on the "
                "list, `--drop` says no)"),
        ("own", "Room for one from the backlog: L7 Tidy the errors. Write its brief and give it with `bullpen later "
                "L7 --brief-file <f>` (pickup then starts it), or `bullpen later L7 --skip \"why\"`"),
        ("own", "Pickup paused: BP-9 and BP-10 wait for a look. Tell the person: it goes on once they have looked"),
        ("own", "Pickup started its 4 for today (pickup_per_day): the backlog waits for tomorrow, nothing to do"),
        ("own", "BP-4 stopped at the usage limit, resets 3:00 PM: nothing to do, it is continued then. `bullpen "
                "start` refuses meanwhile (`--force` overrides, only if the person asks)"),
        ("own", "the usage limit has reset: continued BP-4, BP-6's reviewer: nothing to do"),
        ("own", "BP-4 stopped at the usage limit, and its reset time couldn't be read, so it won't be continued by "
                "itself: tell the person, and `bullpen tell BP-4 continue` when they say the limit has reset"),
        ("own", "BP-3's PR has 1 new comment: ann Please cover logout too. Not sent to its worker, which is on a "
                "prompt: `bullpen tell BP-3` it when it's free"),
        ("own", "BP-3's PR #12 was merged outside bullpen: tell the person; `bullpen accept BP-3` cleans up when "
                "they say so"),
        ("own", "BP-1 not auto-accepted: its PR's checks failed (nothing to do: the news about that follows)"),
        ("own", "After BP-8, main wasn't pushed to github: no route. The accept still counts; `bullpen doctor` says "
                "how far behind github is. Nothing to do the first time; if it keeps failing, tell the person."),
        ("own", "BP-10 waits on BP-9, which was dropped, so it stays queued. Ask the person what to do: `bullpen "
                "after BP-10 --none` (or other tasks) to go ahead without it, then `bullpen start BP-10`; or drop it"),
        ("own", "BP-4 couldn't start (BP-3 was accepted): no Rex session. `bullpen start BP-4` tries again"),
        ("own", "The board wasn't rebuilt: cargo failed. Boards keep running the old one. Log: /x/build.log. Tell "
                "the person: once it is fixed, `bullpen build` builds it."),
        ("own", "Keys changed: tell the person to run `rex config reload` (the cmd+; keys run the old ones until "
                "then)"),
        ("own", "BP-3 was accepted, but rebasing the other tasks failed: boom. The accept still counts: tell the "
                "person"),
        ("own", "Starting BP-3's review failed: it was killed (signal 15). Log: /x/jobs.log. Tell the person if it "
                "isn't yours to settle"),
        ("own", "Your coordinator guide changed (section: News). Read the change below and follow it from now on:"),
    ]

    def test_a_line_of_each_kind_has_its_row_or_says_what_to_do(self):
        for want, line in self.LINES:
            self.assertEqual(news_row(line), self.ROWS.get(want, want), line)
        for line in ("BP-3 did something new", "Pickup did something new", "BP-3 failed: boom. Log: /x/jobs.log"):
            self.assertIsNone(news_row(line), line)  # a line like these fails the test that sent it

    def test_a_line_the_table_doesnt_cover_fails_the_test_that_sent_it(self):
        self.bp("up")
        log = self.repo_state() / "news.log"
        log.write_text("2026-01-01T00:00:00+00:00 [c1] BP-1 odd: bullpen · BP-1 did something new\n")
        with self.assertRaisesRegex(AssertionError, r"neither says what to do nor matches a row .*\(odd\): BP-1 did"):
            from support import check_news
            check_news(self)
        log.unlink()

    def repo_state(self):
        return next((self.tmp / "state" / "repos").iterdir())
