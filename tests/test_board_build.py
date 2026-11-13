"""bullpen rebuilding the board from its own checkout, with a fake cargo."""

import os
import shutil
import subprocess
import textwrap
import time
import unittest
from pathlib import Path

from support import setUpModule, tearDownModule, ROOT, sh, BullpenTest  # a temp folder for each module


FAKE_CARGO = textwrap.dedent("""\
    #!/usr/bin/env python3
    import json, os, sys
    from pathlib import Path
    d = Path(os.environ["FAKE_DIR"])
    with open(d / "cargo.log", "a") as f:
        f.write(json.dumps(sys.argv[1:]) + "\\n")
    if (d / "cargo_mise").exists():
        sys.exit("mise ERROR No version is set for shim: cargo")
    if (d / "cargo_fails").exists():
        sys.exit("error[E0425]: cannot find value `x` in this scope")
    board = Path(sys.argv[sys.argv.index("--manifest-path") + 1]).parent
    out = board / "target" / "release" / "bullpen-board"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("#!/bin/sh\\n# built from: " + " ".join((board / "src" / "main.rs").read_text().split()) + "\\n")
    out.chmod(0o755)
""")


class BoardBuildTest(unittest.TestCase):
    """bullpen rebuilding the board from its own checkout: a copy of bullpen with a
    board/ of its own, which is also the repository its tasks are in (local mode), and
    a fake cargo."""

    setUp_with_origin = BullpenTest.setUp
    rex_calls = BullpenTest.rex_calls
    show = BullpenTest.show
    start_task = BullpenTest.start_task
    news_log = BullpenTest.news_log

    def setUp(self):
        self.setUp_with_origin()
        self.other = self.repo  # a repository that isn't bullpen's
        home = self.tmp / "bullpen"
        for d in ("bin", "bullpen", "lua", "prompts"):
            shutil.copytree(ROOT / d, home / d, ignore=shutil.ignore_patterns("__pycache__"))
        (home / "board" / "src").mkdir(parents=True)
        (home / "board" / "Cargo.toml").write_text('[package]\nname = "bullpen-board"\n')
        (home / "board" / "Cargo.lock").write_text("version = 4\n")
        (home / "board" / "src" / "main.rs").write_text("fn main() {}\n")
        (home / ".gitignore").write_text("board/target/\n")
        sh("git", "init", "-q", "-b", "main", str(home))
        sh("git", "-C", str(home), "add", ".")
        sh("git", "-C", str(home), "-c", "user.name=T", "-c", "user.email=t@example.com",
           "commit", "-qm", "bullpen")
        self.home = self.repo = home
        self.bin = home / "bin" / "bullpen"
        cargo = self.fake / "cargo"
        cargo.write_text(FAKE_CARGO)
        cargo.chmod(0o755)
        del self.env["BULLPEN_BOARD"]
        (self.tmp / "home").mkdir()
        self.env.update(HOME=str(self.tmp / "home"), PATH=f"{self.fake}:{self.env['PATH']}")
        self.target = home / "board" / "target"

    def bp(self, *args, cwd=None, ok=True):
        p = subprocess.run([str(self.bin), *args], cwd=cwd or self.repo, env=self.env,
                           text=True, capture_output=True)
        if ok and p.returncode != 0:
            self.fail(f"bullpen {' '.join(args)} failed: {p.stderr}")
        return p

    def settle(self):
        """Wait for a background build to be done: it holds the lock until then."""
        import fcntl
        lock = self.target / "bullpen-board.lock"
        deadline = time.time() + 15
        while lock.exists():
            with lock.open("a") as f:
                try:
                    fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    return
                except BlockingIOError:
                    pass
            if time.time() > deadline:
                self.fail("the board's build didn't finish")
            time.sleep(0.05)

    def builds(self):
        log = self.fake / "cargo.log"
        return len(log.read_text().splitlines()) if log.exists() else 0

    def board(self):
        b = self.target / "bullpen-board"
        return b.read_text() if b.exists() else None

    def edit_board(self, text, commit=True):
        (self.home / "board" / "src" / "main.rs").write_text(text)
        if commit:
            sh("git", "-C", str(self.home), "-c", "user.name=T", "-c", "user.email=t@example.com",
               "commit", "-qam", "Change the board")

    def board_lines(self):
        return [l for l in self.news_log().splitlines() if " board board: " in l]

    def doctor_board(self):
        line = next(l for l in self.bp("doctor", ok=False).stdout.splitlines() if " board: " in l)
        return line.split(None, 1)[0], line.split(": ", 1)[1]

    def test_up_builds_when_the_source_changed_and_not_otherwise(self):
        out = self.bp("up").stdout
        self.assertIn("rebuilding the board in the background", out)
        self.settle()
        self.assertEqual(self.builds(), 1)
        self.assertIn("fn main() {}", self.board())
        self.assertEqual(self.doctor_board()[0], "ok")
        self.assertNotIn("rebuilding", self.bp("up").stdout)  # unchanged: no build
        self.settle()
        self.assertEqual(self.builds(), 1)
        self.edit_board("fn main() { v2() }\n")
        self.assertEqual(self.doctor_board()[0], "FAIL")  # stale until it's built
        self.bp("up")
        self.settle()
        self.assertEqual(self.builds(), 2)
        self.assertIn("v2()", self.board())
        self.assertEqual(self.board_lines(), [])  # a build that works isn't news

    def test_a_build_renames_a_copy_into_place(self):
        self.bp("build-board")
        cargo_out = self.target / "release" / "bullpen-board"
        board = self.target / "bullpen-board"
        self.assertEqual(board.read_text(), cargo_out.read_text())
        self.assertNotEqual(os.stat(board).st_ino, os.stat(cargo_out).st_ino)  # not cargo's hard link
        ino = os.stat(board).st_ino
        self.edit_board("fn main() { v2() }\n")
        self.bp("build-board")
        self.assertNotEqual(os.stat(board).st_ino, ino)  # a new file: running boards see it replaced
        self.assertEqual([p.name for p in self.target.iterdir() if p.name.startswith(".bullpen-board")], [])

    def test_accept_in_bullpens_own_repo_rebuilds_the_board(self):
        self.bp("build-board")
        self.bp("config", "--review", "off")
        tid = self.start_task()
        wt = Path(self.show(tid)["worktree"])
        (wt / "board" / "src" / "main.rs").write_text("fn main() { accepted() }\n")
        sh("git", "-C", str(wt), "commit", "-qam", "Board change")
        self.bp("ready", tid, "--summary", "Changed the board.", cwd=wt)
        out = self.bp("accept", tid).stdout
        self.assertIn("rebuilding the board in the background", out)
        self.settle()
        self.assertEqual(self.builds(), 2)
        self.assertIn("accepted()", self.board())

    def test_an_accept_that_changes_the_keys_says_to_reload_once(self):
        self.bp("build-board")
        self.bp("config", "--review", "off")
        tid = self.start_task()
        wt = Path(self.show(tid)["worktree"])
        with (wt / "lua" / "keys.lua").open("a") as f:
            f.write("-- a new key\n")
        sh("git", "-C", str(wt), "commit", "-qam", "Key change")
        self.bp("ready", tid, "--summary", "Changed a key.", cwd=wt)
        out = self.bp("accept", tid).stdout
        self.assertEqual(out.splitlines()[-1], "Keys changed: run `rex config reload`")  # the board's line
        self.assertEqual(self.news_log().count("Keys changed: run `rex config reload`"), 1)
        # an accept that leaves keys.lua be says nothing of it
        self.bp("add", "Other", "--brief", "x")
        self.bp("start", "BP-2")
        wt = Path(self.show("BP-2")["worktree"])
        (wt / "fix.txt").write_text("fixed\n")
        sh("git", "-C", str(wt), "add", ".")
        sh("git", "-C", str(wt), "commit", "-qm", "Fix")
        self.bp("ready", "BP-2", "--summary", "Fixed.", cwd=wt)
        self.assertNotIn("Keys changed", self.bp("accept", "BP-2").stdout)
        self.assertEqual(self.news_log().count("Keys changed"), 1)

    def test_accept_in_another_repo_never_builds(self):
        self.bp("build-board")
        self.edit_board("fn main() { pulled() }\n")  # stale, as after a pull
        self.repo = self.other
        sh("git", "-C", str(self.repo), "remote", "remove", "origin")
        self.bp("config", "--review", "off")
        tid = self.start_task()  # `up` here sees the stale board and rebuilds it
        self.settle()
        self.assertEqual(self.builds(), 2)
        self.edit_board("fn main() { pulled_again() }\n")
        wt = Path(self.show(tid)["worktree"])
        (wt / "fix.txt").write_text("fixed\n")
        sh("git", "-C", str(wt), "add", ".")
        sh("git", "-C", str(wt), "commit", "-qm", "Fix")
        self.bp("ready", tid, "--summary", "Fixed.", cwd=wt)
        out = self.bp("accept", tid).stdout
        self.assertNotIn("rebuilding", out)
        self.settle()
        self.assertEqual(self.builds(), 2)

    def test_a_dirty_board_is_skipped_with_one_news_line(self):
        self.bp("build-board")
        before = self.board()
        self.edit_board("fn main() { half_done( }\n", commit=False)
        self.bp("up")
        self.bp("up")
        self.settle()
        self.assertEqual(self.builds(), 1)
        self.assertEqual(self.board(), before)
        lines = self.board_lines()
        self.assertEqual(len(lines), 1, lines)
        self.assertIn("uncommitted changes", lines[0])
        state, text = self.doctor_board()
        self.assertEqual(state, "FAIL")
        self.assertIn("uncommitted changes", text)
        sh("git", "-C", str(self.home), "-c", "user.name=T", "-c", "user.email=t@example.com",
           "commit", "-qam", "Done now")
        self.bp("up")  # the same source, committed: built
        self.settle()
        self.assertEqual(self.builds(), 2)
        self.assertIn("half_done(", self.board())

    def test_a_failed_build_is_one_news_line_and_the_old_board_stays(self):
        self.bp("build-board")
        before = self.board()
        self.edit_board("fn main() { broken }\n")
        (self.fake / "cargo_fails").write_text("")
        self.bp("up")
        self.settle()
        self.bp("up")  # the same source: not tried again
        self.settle()
        self.assertEqual(self.builds(), 2)
        self.assertEqual(self.board(), before)
        lines = self.board_lines()
        self.assertEqual(len(lines), 1, lines)
        log = self.target / "bullpen-board.log"
        self.assertIn(f"Log: {log}", lines[0])
        self.assertIn("error[E0425]", lines[0])
        self.assertIn("error[E0425]", log.read_text())
        state, text = self.doctor_board()
        self.assertEqual(state, "FAIL")
        self.assertIn("its build failed", text)
        (self.fake / "cargo_fails").unlink()
        self.bp("build-board")  # the fix doctor names
        self.assertEqual(self.doctor_board()[0], "ok")

    def test_a_mise_shim_without_a_version_says_so(self):
        (self.fake / "cargo_mise").write_text("")
        p = self.bp("build-board", ok=False)
        self.assertIn("mise shim with no Rust version set", p.stderr)
        self.assertFalse((self.target / "bullpen-board").exists())

    def test_a_mise_shim_falls_back_to_rustups_cargo(self):
        (self.fake / "cargo_mise").write_text("")
        rustup = self.tmp / "home" / ".cargo" / "bin" / "cargo"
        rustup.parent.mkdir(parents=True)
        rustup.write_text(FAKE_CARGO.replace('(d / "cargo_mise").exists()', "False"))
        rustup.chmod(0o755)
        self.bp("build-board")
        self.assertIn("fn main() {}", self.board())

    def test_board_runs_the_built_copy(self):
        self.bp("build-board")
        board = self.target / "bullpen-board"
        board.write_text("#!/bin/sh\necho \"$0\"\n")
        self.assertEqual(self.bp("board").stdout.strip(), str(board))
