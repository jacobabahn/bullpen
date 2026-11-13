"""The coordinator's guide, and telling running coordinators when it changed."""

import difflib
import hashlib
import os
import sys

from .core import Fail, PROMPTS, STATE, command, known_repos
from .coordinators import alive
from .news import follow_cmd, news


def guide_text():
    """The coordinator's guide, with this bullpen's own path in its commands."""
    text = (PROMPTS / "coordinator.md").read_text()
    return text.replace("{{follow}}", follow_cmd())


def guide_file(repo):
    """The guide as a file, for the coordinator's system prompt."""
    path = repo.dir / "coordinator.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(guide_text())
    return path


# ---------- telling running coordinators that their guide changed ----------
#
# A coordinator gets its guide once, in its system prompt or by reading `bullpen guide`
# (`up --here`), and can't load it again. Each records the hash of the guide it was
# given (c["guide"]), and the text itself is kept in <state>/guides/<hash>.md: one
# folder for every repo, since they all run this bullpen's guide. `up` and `accept`
# (a merge or a pull may have changed prompts/coordinator.md) compare each running
# coordinator's hash, in every repo, with the guide now. A coordinator whose guide is
# out of date gets one news line, only it, with the diff after the line when it is
# short, and its hash moves on, so each change is told once. One from before hashes,
# or whose old text is gone, is told to read the guide again, with no diff.

GUIDES = STATE / "guides"
GUIDE_NEWS = {"id": "guide"}  # the news log's task column for these lines
GUIDE_DIFF_LINES = 40  # a longer diff is summed up by its sections instead


def guide_hash(text):
    return hashlib.sha256(text.encode()).hexdigest()


def give_guide(c, text):
    """Record that coordinator c has this guide: its hash, and the text, for the diff
    when it changes."""
    h = guide_hash(text)
    path = GUIDES / f"{h}.md"
    if not path.exists():
        GUIDES.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
        tmp.write_text(text)
        os.replace(tmp, path)
    c["guide"] = h


def old_guide(h):
    """The guide text with hash h, if it was kept."""
    try:
        return (GUIDES / f"{h}.md").read_text() if h else None
    except OSError:
        return None


def guide_sections(old, new):
    """The headings of the guide's sections that changed, in order: those around changed
    lines in the new guide, or in the old one for a section that's gone."""
    a, b = old.splitlines(), new.splitlines()

    def heading(lines, i):
        for l in reversed(lines[:i + 1]):
            if l.startswith("#"):
                return l.lstrip("#").strip()
        return "the top"

    out = []
    for op, i1, i2, j1, j2 in difflib.SequenceMatcher(None, a, b, autojunk=False).get_opcodes():
        if op == "equal":
            continue
        hs = [heading(b, j) for j in range(j1, max(j2, j1 + 1))] if b else []
        hs += [heading(a, i) for i in range(i1, i2) if a[i].startswith("#")]  # removed sections
        for h in hs:
            if h not in out:
                out.append(h)
    return out


def guide_news(old, new):
    """(line, lines after it) telling a coordinator given the guide old (None: not
    known) that it is now new."""
    follow = "follow it from now on"
    if old is None:
        return (f"Your coordinator guide may have changed since you were given it: run "
                f"`bullpen guide` to read it all, and {follow}.", [])
    diff = list(difflib.unified_diff(old.splitlines(), new.splitlines(),
                                     "guide (yours)", "guide (now)", lineterm=""))
    where = guide_sections(old, new)
    where = f" ({'section' if len(where) == 1 else 'sections'}: {', '.join(where)})" if where else ""
    if len(diff) <= GUIDE_DIFF_LINES:
        return f"Your coordinator guide changed{where}. Read the change below and {follow}:", diff
    return (f"Your coordinator guide changed{where}, {len(diff)} lines of diff: run `bullpen guide` "
            f"to read it all, and {follow}.", [])


def tell_guide_changes(repo):
    """Tell each running coordinator, in this repo and every other, whose guide isn't
    the current one what changed. Never fails the command that called it."""
    try:
        text = guide_text()
        h = guide_hash(text)
        for r in [repo] + [o for o in known_repos() if o.dir != repo.dir]:
            told = [c for c in r.cfg["coordinators"] if c.get("guide") != h and alive(c)]
            for c in told:
                line, more = guide_news(old_guide(c.get("guide")), text)
                news(r, GUIDE_NEWS, "guide", line, to=[c["id"]], more=more)
                give_guide(c, text)
                print(f"coordinator {c['id']}{'' if r is repo else ' of ' + str(r.root)}: "
                      f"told that its guide changed")
            if told:
                r.save()
    except (Fail, OSError, ValueError) as e:
        print(f"bullpen: couldn't tell the coordinators about their guide: {e}", file=sys.stderr)


def cmd_guide(args):
    print(guide_text())


def commands(sub):
    """This module's commands, for bullpen's parser."""
    command(sub, "guide", cmd_guide, "Coordinator", help="print the coordinator's guide")
