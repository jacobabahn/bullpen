"""What the tests share: fake `rex` and `gh`, and a project with a local bare repo as
origin.

Run the tests: python3 tests/run.py (or python3 -m unittest discover -s tests).
"""

import datetime as dt
import json
import os
import select
import shutil
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BULLPEN = ROOT / "bin" / "bullpen"
sys.path.insert(0, str(ROOT))  # the bullpen package, for the tests that call it directly

FAKE_REX = textwrap.dedent("""\
    #!/usr/bin/env python3
    import json, os, sys
    from pathlib import Path
    d = Path(os.environ["FAKE_DIR"])
    a = sys.argv[1:]
    with open(d / "rex.log", "a") as f:
        f.write(json.dumps(a) + "\\n")
    alive = d / "alive"
    alive.mkdir(exist_ok=True)
    reg = d / "reg.json"  # blocks: {id: {label, layer, window}}; layers: {id: state}; windows
    R = json.loads(reg.read_text()) if reg.exists() else {"blocks": {}, "layers": {}, "windows": []}

    def new_block(label, layer=None, window=None):
        n = len(R["blocks"]) + 1
        (alive / f"blk{n}").write_text("")
        R["blocks"][f"blk{n}"] = {"label": label, "layer": layer, "window": window}
        return f"blk{n}"

    def new_window():
        w = f"w{len(R['windows']) + 1}"
        R["windows"].append(w)
        return w

    if a[0] in ("send", "send-key") and (d / f"send_fails_{a[a.index('-b') + 1]}").exists():
        sys.exit("rex: couldn't send to the block")  # what the tests make a pane refuse
    if a[0] == "run":
        b = new_block(a[a.index("--label") + 1] if "--label" in a else None, window=new_window())
        print(json.dumps({"window_id": R["blocks"][b]["window"], "block_ids": [b]}))
    elif a[0] == "split":
        anchor = R["blocks"].get(a[a.index("-b") + 1]) if "-b" in a else None
        b = new_block(a[a.index("--label") + 1] if "--label" in a else None,
                      window=anchor["window"] if anchor else new_window())
        print(json.dumps({"block_ids": [b]}))
    elif a[:2] == ["block", "inspect"]:
        sys.exit(0 if (alive / a[2]).exists() else 1)
    elif a[:2] == ["block", "close"]:
        (alive / a[2]).unlink(missing_ok=True)
        if (d / f"kills_tree_{a[2]}").exists():
            # its program is Claude Code, which kills the tree of the command it ran (the one
            # closing it, here) as it ends: every descendant by parent, new sessions too
            import signal, subprocess
            kids = {}
            for row in subprocess.run(["ps", "-A", "-o", "pid=,ppid="], capture_output=True, text=True).stdout.splitlines():
                pid, ppid = map(int, row.split())
                kids.setdefault(ppid, []).append(pid)
            tree = [os.getppid()]
            for pid in tree:
                tree += kids.get(pid, [])
            reg.write_text(json.dumps(R))
            for pid in tree:
                if pid != os.getpid():
                    try:
                        os.kill(pid, signal.SIGKILL)
                    except OSError:
                        pass
            sys.exit(0)
    elif a[0] == "capture":
        own = d / f"screen_{a[a.index('-b') + 1]}"  # one pane's screen, else every pane's
        screen = own if own.exists() else d / "screen"  # what the tests put on screen, else two lines
        print(screen.read_text() if screen.exists() else "line one\\nline two\\n\\n")
    elif a[:2] == ["block", "call"] and a[-1] == "process":
        pid = d / f"pid_{a[a.index('-b') + 1]}"  # a process the tests run on a terminal of their own
        prog = d / f"prog_{a[a.index('-b') + 1]}"  # the program the tests say a pane runs
        if not pid.exists() and not prog.exists():
            sys.exit("rex: no such block")
        child = {"pid": int(pid.read_text())} if pid.exists() else {}
        if prog.exists():
            child.update(path=prog.read_text(), invoked_path=prog.read_text(), argv0=prog.read_text())
        print(json.dumps({"child": child}))
    elif a[:2] == ["block", "call"] and a[-1] == "program_status":
        rec = d / f"status_{a[a.index('-b') + 1]}.json"  # the record the tests give a pane
        if not rec.exists():
            sys.exit("rex: no such method")  # an older Rex: bullpen goes by the screen
        print(json.dumps({"records": [json.loads(rec.read_text())]}))
    elif a[:2] == ["server", "status"]:
        if (d / "server_down").exists():
            sys.exit("rex: no server")
        version = d / "server_version"  # what the tests say the server is; else Rex says nothing
        if version.exists():
            print(json.dumps({"rex_version": version.read_text()}))
    elif a[:2] == ["config", "reload"] and (d / "reload_fails").exists():
        sys.exit("rex: no server")
    elif a[0] == "actions":
        acts = d / "actions.json"  # the actions the tests say the Rex config defines
        if acts.exists():
            print(acts.read_text())
    elif a[:2] == ["api", "call"]:
        rest = a[2:]
        sid = R.get("session", "session:1")
        if rest[0] == "-s":
            if rest[1] not in (sid, "main"):  # a session that no longer exists
                sys.exit(1)
            rest = rest[2:]
        method, args = rest[0], json.loads(rest[1]) if len(rest) > 1 else {}
        if method == "session.new_layer":
            lid = f"lyr{len(R['layers']) + 1}"
            R["layers"][lid] = "normal"
            b = new_block(args["layout"]["block"]["label"], lid,
                          args.get("window_id") or (R["windows"] or [new_window()])[0])
            print(json.dumps({"layer_id": lid, "block_ids": [b]}))
        elif method == "session.new_window":
            w = new_window()
            R.setdefault("labels", {})[w] = args.get("window_label")
            print(json.dumps({"window_id": w, "block_ids": [new_block(args["layout"]["block"]["label"], window=w)]}))
        elif method == "session.new_block":
            print(json.dumps({"block_id": new_block(args["label"], "detached")}))
        elif method == "session.move_block":
            blk, anchor = R["blocks"][args["block_id"]], R["blocks"][args["anchor_block_id"]]
            # beside a floating block, as in Rex, it joins that block's layer
            blk.update(layer=anchor["layer"] if anchor["layer"] != "detached" else None, window=anchor["window"])
            print("{}")
        elif method == "session.float_block":
            lid = f"lyr{len(R['layers']) + 1}"
            R["layers"][lid] = "normal"
            R["blocks"][args["block_id"]]["layer"] = lid
            print(json.dumps({"layer_id": lid}))
        elif method == "session.detach_block":
            R["blocks"][args["block_id"]].update(layer="detached", window=None)
            print("{}")
        elif method == "session.list_blocks":
            print(json.dumps({"blocks": [
                {"block_id": b, "label": info["label"],
                 **({"placement": "detached"} if info["layer"] == "detached" else
                    {"placement": "placed", "window_id": info["window"]})}
                for b, info in R["blocks"].items() if (alive / b).exists()]}))
        elif method == "session.close_window":
            R["windows"].remove(args["window_id"])
            print("{}")
        elif method == "session.set_layer_state":
            w = next(i["window"] for i in R["blocks"].values() if i["layer"] == args["layer_id"])
            up = [b for b, i in R["blocks"].items() if i["window"] == w and (alive / b).exists()
                  and i["layer"] != args["layer_id"] and (not i["layer"] or R["layers"][i["layer"]] == "normal")]
            if args["state"] == "minimized" and not up:
                sys.exit("rex: cannot minimize layer: it has the window's last visible panes")
            R["layers"][args["layer_id"]] = args["state"]
            print("{}")
        elif method == "session.view":
            windows = []
            for w in R["windows"]:
                tiled = {"layer_id": f"tiled-{w}", "kind": "tiled", "blocks": []}
                layers = [tiled]
                for b, info in R["blocks"].items():
                    if not (alive / b).exists() or info["layer"] == "detached" or info["window"] != w:
                        continue
                    entry = {"block_id": b, "label": info["label"]}
                    if info["layer"]:
                        layer = next((l for l in layers if l["layer_id"] == info["layer"]), None)
                        if not layer:
                            layer = {"layer_id": info["layer"], "kind": "floating",
                                     "state": R["layers"][info["layer"]], "blocks": []}
                            layers.append(layer)
                        layer["blocks"].append(entry)
                    else:
                        tiled["blocks"].append(entry)
                windows.append({"window_id": w, "layers": layers,
                                **({"label": R["labels"][w]} if R.get("labels", {}).get(w) else {})})
            view = {"session_id": sid, "label": "main", "windows": windows}
            focused = d / "focused"  # the block the tests say has focus, in the active window
            if focused.exists():
                b = focused.read_text()
                view["active_window_id"] = R["blocks"][b]["window"]
                for w in windows:
                    if w["window_id"] == view["active_window_id"]:
                        w["focused_block_id"] = b
            print(json.dumps(view))
    reg.write_text(json.dumps(R))
""")

FAKE_GH = textwrap.dedent("""\
    #!/usr/bin/env python3
    import json, os, sys
    from pathlib import Path
    d = Path(os.environ["FAKE_DIR"])
    body = sys.stdin.read()  # like a gh that waits for input: it must get none
    with open(d / "gh.log", "a") as f:
        f.write(json.dumps({"argv": sys.argv[1:], "body": body}) + "\\n")
    if sys.argv[1:3] == ["auth", "status"] and (d / "gh_signed_out").exists():
        sys.exit("You are not logged into any GitHub hosts.")
    if sys.argv[1:3] == ["pr", "create"]:
        print("https://github.com/me/repo/pull/7")
    elif sys.argv[1:3] == ["pr", "view"]:
        pr = d / "pr.json"  # what the tests say GitHub has on the PR
        print(pr.read_text() if pr.exists() else json.dumps({"state": "OPEN"}))
""")


def sh(*cmd, cwd=None):
    return subprocess.run(cmd, cwd=cwd, check=True, text=True, capture_output=True).stdout


def check_build():
    """The tests run bin/bullpen, which runs the Rust build: it must be built from the
    source as it is (tests/run.py builds it first)."""
    if os.environ.get("BULLPEN_TESTS_BUILT"):
        return
    from bullpen import board_build as b
    if not b.CLI.installed.exists() or b.read_json(b.CLI.stamp, {}).get("source") != b.CLI.source_hash():
        raise RuntimeError("bullpen's command line isn't built from its source as it is: "
                           "run the tests with tests/run.py (it builds first), or run `bin/bullpen build`")


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


class BullpenTest(unittest.TestCase):
    """A project with a local origin, fake rex and gh, and its own state folder.
    The test classes of each area build on it."""
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        fake = self.tmp / "fake"
        fake.mkdir()
        for name, src in (("rex", FAKE_REX), ("gh", FAKE_GH)):
            p = fake / name
            p.write_text(src)
            p.chmod(0o755)
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

    def typed(self):
        """Lines typed into panes with `rex send`, as (block, text)."""
        out = []
        for a in self.rex_calls():
            if a[0] == "send":
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
            for n in ("bullpen_next", "bullpen_board", "bullpen_coordinator", "bullpen_jump")]}))
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
