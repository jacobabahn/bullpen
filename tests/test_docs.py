"""The docs: every relative link in README.md, GLOSSARY.md and docs/ leads somewhere, the
board's tables say what the board does, and every command and setting is named in its page."""

import json
import re
import subprocess
import unittest

from support import setUpModule, tearDownModule, ROOT, BullpenTest  # a temp folder for each module


def slug(heading):
    """GitHub's anchor for a heading: lower case, punctuation gone, spaces to hyphens."""
    return re.sub(r"[^\w\- ]", "", heading.strip().lower()).replace(" ", "-")


def anchors(path):
    text = re.sub(r"^```.*?^```", "", path.read_text(), flags=re.M | re.S)
    return {slug(h) for h in re.findall(r"^#+ (.+)$", text, re.M)}


def links(path):
    """(line, target) of each markdown link outside code."""
    text = re.sub(r"^```.*?^```", lambda m: "\n" * m.group().count("\n"),
                  path.read_text(), flags=re.M | re.S)
    text = re.sub(r"``.*?``|`[^`\n]*`", "", text, flags=re.S)
    for m in re.finditer(r"\]\(([^)\s]+)\)", text):
        yield text.count("\n", 0, m.start()) + 1, m.group(1)


def rules_paths(text):
    """Each path named in backticks: a word with a slash or a file's extension, not a
    placeholder (`<name>`), nor outside the repo (`~/...`)."""
    for span in re.findall(r"`([^`\n]+)`", text):
        for word in span.split():
            if "<" in word or word.startswith(("~", "-")):
                continue
            if "/" in word or re.fullmatch(r"[\w.-]+\.(md|rs|py|lua|toml|sh|json)", word):
                yield word


def code(text):
    """The text of a page's code: its blocks and its spans (``a `b` c`` too), a span's line
    breaks as spaces."""
    blocks = re.findall(r"^```.*?^```", text, flags=re.M | re.S)
    spans = re.findall(r"(?s:``.+?``)|`[^`]+`", re.sub(r"^```.*?^```", "", text, flags=re.M | re.S))
    return re.sub(r"\s+", " ", " ".join(blocks + spans))


def unnamed(page, names, forms):
    """The names the page doesn't name in its code (a span or a block) in any of
    forms(name): `bullpen accept`, or `merge` (a span of its own) / `--merge`."""
    text = code((ROOT / page).read_text())
    named = lambda n: any(re.search(r"(?<![\w-])" + re.escape(f) + r"(?![\w-])", text) for f in forms(n))
    return [n for n in names if not named(n)]


class DocsTest(unittest.TestCase):
    def test_every_path_in_the_rules_exists(self):
        # .bullpen/rules.md goes into every brief and review: a path it names that's gone
        # sends every agent looking for it. target/ is the build's, made by tests/run.py.
        tracked = subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True,
                                 check=True).stdout.split()
        named = list(rules_paths((ROOT / ".bullpen" / "rules.md").read_text()))
        self.assertGreater(len(named), 10)
        missing = [p for p in named if p != "target/"
                   and not any(t == p.rstrip("/") or t.startswith(p.rstrip("/") + "/") for t in tracked)]
        self.assertEqual(missing, [])

    def test_a_stale_path_in_the_rules_is_caught(self):
        text = "- run by `bin/bullpen-py`, `bin/bullpen-cargo test <name>`, `test_<module>.rs`, `~/dev/x`, `rex`/`gh`"
        self.assertEqual(list(rules_paths(text)), ["bin/bullpen-py", "bin/bullpen-cargo"])

    def test_every_link_and_anchor_resolves(self):
        pages = [ROOT / "README.md", ROOT / "GLOSSARY.md", *sorted((ROOT / "docs").rglob("*.md"))]
        broken, checked = [], 0
        for page in pages:
            for line, target in links(page):
                if re.match(r"[a-z]+:", target):
                    continue  # a web link
                checked += 1
                file, _, frag = target.partition("#")
                dest = (page.parent / file).resolve() if file else page
                where = f"{page.relative_to(ROOT)}:{line}: {target}"
                if not dest.exists():
                    broken.append(where + " (no such file)")
                elif frag and frag not in anchors(dest):
                    broken.append(where + " (no such heading)")
        self.assertGreater(checked, 30)
        self.assertEqual(broken, [])

    def test_a_broken_link_is_caught(self):
        self.assertEqual(slug("Finish with nothing to merge"), "finish-with-nothing-to-merge")
        self.assertEqual(slug("Auto-accept"), "auto-accept")
        self.assertIn("auto-accept", anchors(ROOT / "docs" / "accept.md"))
        self.assertNotIn("after-accept", anchors(ROOT / "docs" / "review.md"))

    def test_the_docs_tables_are_the_boards(self):
        # docs/board.md's marks and keys tables say what the board's `?` list says
        use = (ROOT / "docs" / "board.md").read_text()
        rows = [r.split(" | ") for r in re.findall(r"^\| (.+) \|$", use, re.M)]
        cells = {r[1]: r[0] for r in rows if len(r) == 2}
        # every still mark (core's mark() in core/src/words.rs, which the board shows), each in docs/board.md beside its words
        src = (ROOT / "core" / "src" / "words.rs").read_text()
        body = src[src.index("pub fn mark(status: &str)"):]
        marks = dict(re.findall(r'"(\w+)" => "(.)"', body[:body.index("\n}\n")]))
        self.assertGreaterEqual(len(marks), 10)
        for st, mark in marks.items():
            self.assertTrue(any(f"`{mark}`" in c for c in cells.values()), (st, mark))
        for words in ("needs you", "ready to accept", "dropped", "in review", "working"):
            self.assertIn(words, cells)
        self.assertIn("`◐`", cells["in review"])
        self.assertIn("`○`", cells["working"])
        # the turning marks: the first frame of each animation in board/src/ui/mod.rs
        ui = (ROOT / "board" / "src" / "ui" / "mod.rs").read_text()
        for name, words in (("REVIEW", "in review"), ("WORKING", "working")):
            first = re.search(name + r': &\[&str\] = &\["(.)"', ui).group(1)
            self.assertIn(f"`{first}`", cells[words])
        spin = re.search(r'SPINNER: &\[&str\] = &\["(.)"', ui).group(1)
        self.assertTrue(any(c.startswith(f"`{spin}`") for c in cells.values()))
        # every key of the board (BOARD_KEYS in board/src/ui/help.rs, the tour's aside), in its order
        help = (ROOT / "board" / "src" / "ui" / "help.rs").read_text()
        keys = help[help.index("pub const BOARD_KEYS"):help.index('("In a tour"')]
        board = [k.split()[0] for k in re.findall(r'\("([^"]+)", "', keys)]
        table = [re.match(r"`([^`]+)`", c).group(1) for c in cells.values() if re.match(r"`[^`]+`", c)]
        listed = [k for k in table if k in board]
        self.assertEqual(listed, board)

    def test_a_name_missing_from_its_page_is_caught(self):
        self.assertEqual(unnamed("docs/commands.md", ["accept", "acept", "tour", "tour-c"],
                                 lambda n: [f"bullpen {n}"]), ["acept", "tour-c"])
        self.assertEqual(code("a `bullpen config\n--notify all` b\n```sh\nbullpen ls\n```\n"),
                         "```sh bullpen ls ``` `bullpen config --notify all`")
        setting = lambda n: [f"`{n}`", f'"{n}"', "--" + n.replace("_", "-")]
        self.assertEqual(unnamed("docs/settings.md", ["merge", "worker", "review"], setting), [])


class DocsNamesTest(BullpenTest):
    """Every command and setting is named in its docs page, read from the program's own
    tables. The internal ones are exempt by a rule, not a list: a command in no group of
    the help (`_commands`'s group null), and a setting `bullpen config` doesn't list
    (bullpen's own bookkeeping, as `next`)."""

    def commands(self):
        table = json.loads(self.bp("_commands").stdout)
        return [c["name"] for c in table if c["group"] is not None]

    def settings(self):
        every = json.loads(self.bp("config", "--json").stdout)["sources"]
        listed = set(re.findall(r"(?m)^  (\w+)  ", self.bp("config").stdout))
        return [k for k in every if k in listed]

    def tables(self):
        """(page, the names it must name, how a name is written there): a table more
        (doctor's checks, say) is a row more."""
        return [
            ("docs/commands.md", self.commands(), lambda n: [f"bullpen {n}"]),
            # a bare name counts only as a span of its own or a config.json key: `review` is
            # a command too, and `bullpen review BP-3` doesn't name the setting
            ("docs/settings.md", self.settings(), lambda n: [f"`{n}`", f'"{n}"', "--" + n.replace("_", "-")]),
        ]

    def test_every_command_and_setting_is_named_in_its_page(self):
        self.bp("up")
        for page, names, forms in self.tables():
            self.assertGreater(len(names), 20, page)
            self.assertEqual(unnamed(page, names, forms), [], page)
        self.assertNotIn("_event", self.commands())
        self.assertNotIn("next", self.settings())
        self.assertIn("board_layout", self.settings())


if __name__ == "__main__":
    unittest.main()
