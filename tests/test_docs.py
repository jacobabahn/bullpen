"""The docs: every relative link in README.md, GLOSSARY.md and docs/ leads somewhere."""

import re
import unittest

from support import setUpModule, tearDownModule, ROOT  # a temp folder for each module


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


class DocsTest(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
