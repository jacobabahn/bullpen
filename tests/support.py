"""What the tests share: fake `rex` and `gh` (tests/fakes, in Rust), and a project with a
local bare repo as origin.

Run the tests: python3 tests/run.py (or python3 -m unittest discover -s tests).
"""

import datetime as dt
import hashlib
import json
import os
import re
import select
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BULLPEN = ROOT / "bin" / "bullpen"
sys.path.insert(0, str(ROOT))  # the bullpen package, for the tests that call it directly

def sh(*cmd, cwd=None):
    return subprocess.run(cmd, cwd=cwd, check=True, text=True, capture_output=True).stdout


def cli_source_hash():
    """The hash of the command line's source that its build stamp (target/bullpen.json)
    records: cli/src/build.rs's source_hash, for bullpen-cli."""
    return source_hash(("Cargo.toml", "Cargo.lock", "core/Cargo.toml", "core/src", "cli/Cargo.toml", "cli/src"))


def source_hash(rels):
    files = []
    for rel in rels:
        p = ROOT / rel
        files += sorted(f for f in p.rglob("*") if f.is_file()) if p.is_dir() else [p]
    parts = {str(f.relative_to(ROOT)): hashlib.sha256(f.read_bytes()).hexdigest() for f in files if f.is_file()}
    return hashlib.sha256(json.dumps(parts, sort_keys=True).encode()).hexdigest()


def fakes_source_hash():
    """The hash of the fakes' source (tests/fakes) that their stamp (target/fakes.json) records."""
    return source_hash(("Cargo.toml", "Cargo.lock", "tests/fakes/Cargo.toml", "tests/fakes/src"))


def odd_name(stem):
    """A file name git quotes in its output, from stem: a non-ASCII letter, a space, a
    quote and a tab and a newline, those last two only where the file system takes them."""
    name = f'{stem} café "q"'
    with tempfile.TemporaryDirectory() as d:
        try:
            (Path(d) / f"{name}\tx\ny").write_text("")
            return f"{name}\tx\ny"
        except OSError:
            return name


FAKES = ("fake-rex", "fake-gh")  # in target/, where tests/run.py puts them


def stamped(name, source, programs):
    """Are target/<programs> built from the source as it is (target/<name>.json says so)?"""
    try:
        stamp = json.loads((ROOT / "target" / f"{name}.json").read_text())
    except (OSError, ValueError):
        return False
    return all((ROOT / "target" / p).exists() for p in programs) and stamp.get("source") == source


def cli_built():
    """Is target/bullpen built from the command line's source as it is?"""
    return stamped("bullpen", cli_source_hash(), ["bullpen"])


def fakes_built():
    """Are the fake rex and gh (target/fake-rex, target/fake-gh) built from their source as it is?"""
    return stamped("fakes", fakes_source_hash(), FAKES)


def check_build():
    """The tests run bin/bullpen, which runs the Rust build, and the fake rex and gh: they
    must be built from the source as it is (tests/run.py builds them first)."""
    if os.environ.get("BULLPEN_TESTS_BUILT"):
        return
    if not cli_built():
        raise RuntimeError("bullpen's command line isn't built from its source as it is: "
                           "run the tests with tests/run.py (it builds first), or run `bin/bullpen build`")
    if not fakes_built():
        raise RuntimeError("the tests' fake rex and gh (tests/fakes) aren't built from their source as it is: "
                           "run the tests with tests/run.py (it builds them first)")


def setUpModule():
    """Run the suite in a temp folder of its own, so a test that leaves files behind fails it."""
    global SUITE_TMP, OUTER_TMPDIR
    check_build()
    OUTER_TMPDIR = os.environ.get("TMPDIR")
    SUITE_TMP = tempfile.mkdtemp(prefix="bullpen-tests-")
    os.environ["TMPDIR"] = tempfile.tempdir = SUITE_TMP


def tearDownModule():
    left = sorted(os.listdir(SUITE_TMP))
    shutil.rmtree(SUITE_TMP, ignore_errors=True)
    tempfile.tempdir = None
    if OUTER_TMPDIR is None:
        os.environ.pop("TMPDIR", None)
    else:
        os.environ["TMPDIR"] = OUTER_TMPDIR
    if left:
        raise AssertionError(f"tests left {len(left)} files in TMPDIR: {', '.join(left[:5])}")


# What a line whose kind has no row in the guide's table (prompts/coordinator.md, "News")
# has, since it says what to do itself: a command to run, who to tell, or that there is
# nothing to do. Only the instruction is held to wording: a line's row goes by its kind.
NEWS_OWN = r"`bullpen |`rex |[Tt]ell the person|[Aa]sk the person|[Nn]othing to do|^Your coordinator guide "

_news_kinds = None


def news_kinds():
    """The table of news kinds (`bullpen _news_kinds`, from cli/src/news/kind.rs): the
    guide's rows in order (`rows`), and each kind's row by its event (`kinds`; None for
    a kind whose lines say what to do themselves)."""
    global _news_kinds
    if _news_kinds is None:
        table = json.loads(subprocess.run([str(BULLPEN), "_news_kinds"], env={**os.environ, "BULLPEN_NO_BUILD": "1"},
                                          text=True, capture_output=True, check=True).stdout)
        _news_kinds = {"rows": table["rows"], "kinds": {k["event"]: k["row"] for k in table["kinds"]}}
    return _news_kinds


def news_row(event, line):
    """The guide's row for a news line of this event (the line without its `bullpen · `):
    its kind's row, "own" for a kind whose lines say what to do themselves and this one
    does, or None: a kind the table doesn't have, or a line that leaves the coordinator to
    guess."""
    kinds = news_kinds()["kinds"]
    if event not in kinds:
        return None
    return kinds[event] or ("own" if re.search(NEWS_OWN, line) else None)


def check_news(test):
    """Every news line bullpen sent in a test is of a kind the table has, and one whose
    kind has no row of the guide's table says what to do itself. The logs are read
    through bullpen (`_news_log`, the reader news.log has), not parsed here. A line a
    test wrote into the log by hand, with an event its class names in `own_news_events`,
    isn't bullpen's."""
    own = getattr(test, "own_news_events", ())
    logs = [str(p) for p in (test.tmp / "state").rglob("news.log")]
    if not logs:
        return
    read = subprocess.run([str(BULLPEN), "_news_log", *logs], env=test.env, text=True, capture_output=True,
                          check=True)
    for log in json.loads(read.stdout):
        for r in log["records"]:
            if r["said"].startswith("bullpen · ") and r["event"] not in own and news_row(r["event"], r["text"]) is None:
                test.fail(f"a news line of a kind the table doesn't have, or that neither says what to do "
                          f"nor has a row of the guide's table ({r['event']}): {r['text']}")


class BullpenTest(unittest.TestCase):
    """A project with a local origin, fake rex and gh, and its own state folder.
    The test classes of each area build on it."""
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        self.addCleanup(check_news, self)  # before the folder goes
        fake = self.tmp / "fake"
        fake.mkdir()
        for name in ("rex", "gh"):  # tests/fakes, built by tests/run.py
            (fake / name).symlink_to(ROOT / "target" / f"fake-{name}")
        self.fake = fake
        origin = self.tmp / "origin.git"
        sh("git", "init", "-q", "--bare", "-b", "main", str(origin))
        self.repo = self.tmp / "proj"
        sh("git", "init", "-q", "-b", "main", str(self.repo))
        sh("git", "-C", str(self.repo), "config", "user.email", "t@example.com")
        sh("git", "-C", str(self.repo), "config", "user.name", "T")
        (self.repo / "README").write_text("hi\n")
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", "init")
        sh("git", "-C", str(self.repo), "remote", "add", "origin", str(origin))
        sh("git", "-C", str(self.repo), "push", "-q", "-u", "origin", "main")
        # A board of the test's own: bullpen never builds the real one (BULLPEN_BOARD set),
        # and never builds, or takes the build lock in, this checkout (BULLPEN_NO_BUILD):
        # the tests that are about building (test_board_build's) build a copy, without it.
        board = fake / "bullpen-board"
        board.write_text("#!/bin/sh\n")
        board.chmod(0o755)
        self.env = {**os.environ, "BULLPEN_STATE": str(self.tmp / "state"), "BULLPEN_BOARD": str(board),
                    "BULLPEN_NO_BUILD": "1",
                    "BULLPEN_REX": str(fake / "rex"), "BULLPEN_GH": str(fake / "gh"),
                    "FAKE_DIR": str(fake), "REX_SESSION": "session:1", "REX_BLOCK": "me",
                    "XDG_CONFIG_HOME": str(self.tmp / "xdg"),
                    # Claude Code's scratch folders (cli/src/claude_tmp.rs): the test's own, never the real /tmp
                    "BULLPEN_CLAUDE_TMP": str(self.tmp / "ctmp"),
                    "GIT_AUTHOR_NAME": "T", "GIT_AUTHOR_EMAIL": "t@example.com",
                    "GIT_COMMITTER_NAME": "T", "GIT_COMMITTER_EMAIL": "t@example.com"}

    def bp(self, *args, cwd=None, ok=True):
        p = subprocess.run([str(BULLPEN), *args], cwd=cwd or self.repo, env=self.env,
                           text=True, capture_output=True)
        if ok and p.returncode != 0:
            self.fail(f"bullpen {' '.join(args)} failed: {p.stderr}")
        return p

    def rex_calls(self):
        log = self.fake / "rex.log"
        return [json.loads(l) for l in log.read_text().splitlines()] if log.exists() else []

    def api(self, method):
        """Arguments of each `rex api call` to method."""
        out = []
        for a in self.rex_calls():
            if a[:2] == ["api", "call"] and method in a:
                out.append(json.loads(a[a.index(method) + 1]))
        return out

    def layer_states(self):
        return json.loads((self.fake / "reg.json").read_text())["layers"]

    def typed(self, guide=False):
        """Lines typed into panes with `rex send`, as (block, text). Without guide: not
        the news that a coordinator's guide changed (a setting that brings a section,
        a second coordinator), which the tests of something else don't count."""
        out = []
        for a in self.rex_calls():
            if a[0] == "send" and (guide or not a[-1].startswith("bullpen · Your coordinator guide ")):
                out.append((a[a.index("-b") + 1], a[-1]))
        return out

    def show(self, tid):
        return json.loads(self.bp("show", tid, "--json").stdout)

    def start_task(self, title="Fix the thing"):
        self.bp("up")  # the coordinator (blk1), the board (blk2) and the detached watcher (blk3)
        tid = self.bp("add", title, "--brief", "Make it work.").stdout.strip()
        self.bp("start", tid)
        return tid

    def block(self, tid):
        return self.show(tid)["block"]

    def ready_with_a_commit(self):
        self.bp("config", "--review", "off")
        tid = self.start_task()
        wt = self.show(tid)["worktree"]
        (Path(wt) / "fix.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        self.bp("ready", tid, "--summary", "done")
        return tid

    def commit_and_ready(self, tid, n=1):
        wt = self.show(tid)["worktree"]
        (Path(wt) / f"fix{n}.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", f"Fix {n}")
        self.bp("ready", tid, "--summary", "Did it.", cwd=wt)
        return self.show(tid)["review"]["block"]

    def verdict(self, tid, v, text="Looks right."):
        return self.bp("verdict", tid, v, "--summary", text)

    def news(self):
        return [l for b, l in self.typed() if b == "blk1"]

    def agent(self, blk, state, kind="", msg="", ago=600):
        """What Rex says the program in pane blk is doing, as of ago seconds back."""
        at = (dt.datetime.now().astimezone() - dt.timedelta(seconds=ago)).isoformat()
        rec = {"state": state, "app": "claude-code", "msg": msg, "updated_at": at}
        if kind:
            rec["kind"] = kind
        (self.fake / f"status_{blk}.json").write_text(json.dumps(rec))

    def settle(self):
        """Every task entered its status 3 minutes ago: past check's settle (and short
        of triage_minutes)."""
        at = (dt.datetime.now().astimezone() - dt.timedelta(minutes=3)).isoformat()
        for f in (self.tmp / "state").rglob("tasks/*.json"):
            t = json.loads(f.read_text())
            f.write_text(json.dumps({**t, "since": at}))

    def check(self, *args, settle=True):
        if settle:
            self.settle()
        p = self.bp("check", *args, ok=False)
        self.assertIn(p.returncode, (0, 1), p.stderr)
        return p.stdout.splitlines()

    def ask(self, tid, msg):
        """One news line: the worker asks something."""
        self.bp("_event", self.block(tid), "blocked", "question", msg)

    def follow(self, block="blk1"):
        """Start `bullpen news --follow` in a pane (the coordinator's); returns it and a
        function reading its next n lines."""
        p = subprocess.Popen([str(BULLPEN), "news", "--follow", "--every", "0.05"], cwd=self.repo,
                             env={**self.env, "REX_BLOCK": block}, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(lambda: (p.kill(), p.wait(), p.stdout.close(), p.stderr.close()))
        buf = b""

        def lines(n, wait=10):
            nonlocal buf
            end = time.time() + wait
            while buf.count(b"\n") < n:
                ready, _, _ = select.select([p.stdout], [], [], max(0, end - time.time()))
                if not ready:
                    self.fail(f"follow printed {buf!r}, wanted {n} lines")
                buf += os.read(p.stdout.fileno(), 4096)
            out, buf = buf.split(b"\n")[:n], b"\n".join(buf.split(b"\n")[n:])
            return [l.decode() for l in out]

        def quiet(wait=0.4):
            ready, _, _ = select.select([p.stdout], [], [], wait)
            return not buf and not ready

        return p, lines, quiet

    def focused(self):
        return [a[-1] for a in self.rex_calls() if a[0] == "focus"]

    def other_repo(self, name):
        """A second repository, with no remote (local mode), bullpen's state beside proj's."""
        d = self.tmp / name
        sh("git", "init", "-q", "-b", "main", str(d))
        (d / "README").write_text("hi\n")
        sh("git", "-C", str(d), "add", ".")
        sh("git", "-C", str(d), "commit", "-qm", "init")
        return d

    def github(self, **pr):
        """What `gh pr view` says about the PR from now on."""
        (self.fake / "pr.json").write_text(json.dumps({"state": "OPEN", "headRefOid": "abc",
            "statusCheckRollup": [], "reviews": [], "comments": [], **pr}))

    def news_lines(self):
        return [l for b, l in self.typed() if b == "blk1"]

    RUNNING = [{"__typename": "CheckRun", "name": "test", "status": "IN_PROGRESS", "conclusion": ""}]

    def commit_and_ready_no_review(self, tid):
        wt = self.show(tid)["worktree"]
        (Path(wt) / "fix.txt").write_text("fixed\n")
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", "Fix it")
        self.bp("ready", tid, "--summary", "Did it.", cwd=wt)

    def merge_on_github(self, tid):
        """What `gh pr merge` would do: the task's branch lands on origin's main."""
        sh("git", "-C", str(self.repo), "push", "-q", "origin", f"{self.show(tid)['branch']}:main")

    def done_dependency(self, title="Landed"):
        """A task marked done, for queued tasks to wait on: its ID."""
        tid = self.bp("add", title, "--brief", "x").stdout.strip()
        path = next((self.tmp / "state" / "repos").iterdir()) / "tasks" / f"{tid}.json"
        path.write_text(json.dumps({**json.loads(path.read_text()), "status": "done"}))
        return tid

    def full_brief(self, tid="BP-1"):
        return next((self.tmp / "state").rglob(f"{tid}.full.md")).read_text()

    def made(self):
        """The terminal block of each pane bullpen made as a float or in the background, in order."""
        out = []
        for a in self.rex_calls():
            if a[:2] == ["api", "call"] and "session.new_layer" in a:
                out.append(json.loads(a[a.index("session.new_layer") + 1])["layout"]["block"])
            elif a[:2] == ["api", "call"] and "session.new_block" in a:
                out.append(json.loads(a[a.index("session.new_block") + 1]))
        return out

    def last_command(self):
        cmd = self.made()[-1]["options"]["command"]
        return cmd[cmd.index("--permission-mode"):]  # past env and the resolved program

    def set_commands(self, **cmds):
        """Edit the repo's config.json by hand, as a person would."""
        self.bp("config")
        path = next((self.tmp / "state").rglob("config.json"))
        cfg = json.loads(path.read_text())
        cfg.update(cmds)
        path.write_text(json.dumps(cfg))

    def models(self, cmd):
        return [cmd[i + 1] for i, a in enumerate(cmd) if a == "--model"]

    def new_pane(self, label="claude"):
        """An agent's pane the person opened in Rex, in a tab of its own."""
        reg = self.fake / "reg.json"
        R = json.loads(reg.read_text()) if reg.exists() else {"blocks": {}, "layers": {}, "windows": []}
        b, w = f"pane{len(R['blocks']) + 1}", f"w{len(R['windows']) + 1}"
        R["windows"].append(w)
        R["blocks"][b] = {"label": label, "layer": None, "window": w}
        reg.write_text(json.dumps(R))
        (self.fake / "alive").mkdir(exist_ok=True)
        (self.fake / "alive" / b).write_text("")
        return b

    def bp_in(self, block, *args, ok=True):
        """bullpen run from another pane."""
        env, self.env = self.env, {**self.env, "REX_BLOCK": block}
        try:
            return self.bp(*args, ok=ok)
        finally:
            self.env = env

    def two_coordinators(self):
        """c1 started by `up` (blk1, board blk2), c2 an agent's pane made one with
        `up --here`. Returns c2's block."""
        self.bp("up")
        other = self.new_pane()
        self.bp_in(other, "up", "--here")
        return other

    def coordinators(self):
        return json.loads(self.bp("config", "--json").stdout)["coordinators"]

    def ago(self, tid, minutes):
        """The task entered its status this many minutes ago."""
        f = next((self.tmp / "state").rglob(f"tasks/{tid}.json"))
        t = json.loads(f.read_text())
        at = dt.datetime.now().astimezone() - dt.timedelta(minutes=minutes)
        f.write_text(json.dumps({**t, "since": at.isoformat()}))

    def commit_file(self, tid, name, text, msg=None):
        wt = self.show(tid)["worktree"]
        (Path(wt) / name).parent.mkdir(parents=True, exist_ok=True)
        (Path(wt) / name).write_text(text)
        sh("git", "-C", wt, "add", ".")
        sh("git", "-C", wt, "commit", "-qm", msg or f"Change {name}")
        return wt

    def ready(self, tid):
        return self.bp("ready", tid, "--summary", "Did it.", cwd=self.show(tid)["worktree"])

    def approved_task(self):
        """A started task with one small commit, reviewed and approved, its worker
        having said it works first (so a later working event is a new turn)."""
        tid = self.start_task()
        self.bp("_event", self.block(tid), "working", "", "Fixing")
        self.commit_and_ready(tid)
        self.verdict(tid, "approve")
        return tid

    def terminal(self, blk):
        """Give pane blk a real terminal, with a process on it; returns a function that
        reads what was written to it since."""
        import pty
        pid, fd = pty.fork()
        if pid == 0:
            os.execvp("sleep", ["sleep", "60"])
        def done():
            os.kill(pid, 9)
            os.waitpid(pid, 0)
            os.close(fd)
        self.addCleanup(done)
        (self.fake / f"pid_{blk}").write_text(str(pid))

        def read():
            out = b""
            while select.select([fd], [], [], 0.2)[0]:
                try:
                    out += os.read(fd, 4096)
                except OSError:
                    break
            return out.decode()
        return read

    def healthy(self):
        """A setup doctor finds nothing wrong with: a new enough Rex, keys, a built
        board, the agent program on PATH, and `up` run."""
        (self.fake / "server_version").write_text("0.1.0-preview.13")
        (self.fake / "actions.json").write_text(json.dumps({"actions": [
            {"name": n, "source": "/x/lua/keys.lua:62"}
            for n in ("bullpen_next", "bullpen_board", "bullpen_coordinator", "bullpen_jump",
                      "bullpen_mission")]}))
        board = self.fake / "board"
        board.write_text("#!/bin/sh\n")
        board.chmod(0o755)
        claude = self.fake / "claude"
        claude.write_text("#!/bin/sh\n")
        claude.chmod(0o755)
        self.env["BULLPEN_BOARD"] = str(board)
        self.env["PATH"] = f"{self.fake}:{self.env['PATH']}"
        self.bp("up")

    def doctor(self, cwd=None):
        p = self.bp("doctor", ok=False, cwd=cwd)
        lines = {}
        for line in p.stdout.splitlines()[:-1]:
            mark, rest = line.split(None, 1)
            name, text = rest.split(": ", 1)
            lines[name] = (mark, text)
        return p, lines

    def doctor_all(self, cwd=None):
        """doctor --all-repos: (the run, {section: {check: (mark, text)}}), each section
        named by its heading's first word ("Setup", then each repo's name)."""
        p = self.bp("doctor", "--all-repos", ok=False, cwd=cwd)
        sections = {}
        for block in p.stdout.split("\n\n")[:-1]:
            head, *rest = block.splitlines()
            checks = sections.setdefault(head.split()[0].rstrip(","), {})
            for line in rest:
                mark, more = line.split(None, 1)
                name, text = more.split(": ", 1)
                checks[name] = (mark, text)
        return p, sections

    def commit_hook(self, name, text):
        """Commit .bullpen/<name>.sh on main and push it: the task's base."""
        d = self.repo / ".bullpen"
        d.mkdir(exist_ok=True)
        (d / f"{name}.sh").write_text(text)
        sh("git", "-C", str(self.repo), "add", ".")
        sh("git", "-C", str(self.repo), "commit", "-qm", f"Add {name}")
        sh("git", "-C", str(self.repo), "push", "-q", "origin", "main")

    def news_log(self):
        logs = list((self.tmp / "state").rglob("news.log"))
        return logs[0].read_text() if logs else ""
