"""bullpen rebuilding its command line and the board from its own checkout, with a fake cargo."""

import json
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
    RUST_BIN, RUST_OWNED = "{rust_bin}", "{rust_owned}"
    d = Path(os.environ["FAKE_DIR"])
    if (d / "cargo_mise").exists():
        sys.exit("mise ERROR No version is set for shim: cargo")
    if sys.argv[1:] == ["--version"]:  # bin/bullpen-cargo's check: not a build
        sys.exit(print("cargo 1.0.0 (fake)"))
    with open(d / "cargo.log", "a") as f:
        f.write(json.dumps(sys.argv[1:]) + "\\n")
    if (d / "cargo_slow").exists():
        import time
        time.sleep(float((d / "cargo_slow").read_text()))
    if (d / "cargo_fails").exists():
        sys.exit("error[E0425]: cannot find value `x` in this scope")
    home = Path(sys.argv[sys.argv.index("--manifest-path") + 1]).parent
    packages = [sys.argv[i + 1] for i, a in enumerate(sys.argv) if a == "-p"]
    for package, crate, name in (("bullpen-cli", "cli", "bullpen"), ("bullpen-board", "board", "bullpen-board")):
        if package not in packages:
            continue
        out = home / "target" / "release" / name
        out.parent.mkdir(parents=True, exist_ok=True)
        source = " ".join((home / crate / "src" / "main.rs").read_text().split())
        # the command line it builds runs the commands Rust owns with a copy of the real
        # build in this checkout's target (so this checkout is its home: `board` runs this
        # checkout's board), and hands the rest to this copy's Python
        real = out.parent / "bullpen-real"
        if name == "bullpen":  # a copy renamed into place: the old one may be running
            import shutil
            shutil.copy2(RUST_BIN, f"{real}.tmp")
            os.replace(f"{real}.tmp", real)
        rust = f'case $1 in {RUST_OWNED}) exec "{real}" "$@" ;; esac\\n'
        # a command a port moved: "// moved: NAME" in its main.rs, which it answers itself
        for line in (home / crate / "src" / "main.rs").read_text().splitlines():
            if line.startswith("// moved: "):
                rust += f'[ "$1" = {line.split()[-1]} ] && echo "$1 runs in the new build" && exit 0\\n'
        run = rust + f'exec python3 -S "{home}/bin/bullpen-py" "$@"' if name == "bullpen" else ""
        out.write_text(f"#!/bin/sh\\n# built from: {source}\\n{run}\\n")
        out.chmod(0o755)
""")


def fake_cargo():
    """FAKE_CARGO, with the commands the real build (target/bullpen) runs itself."""
    rust_bin = ROOT / "target" / "bullpen"
    table = json.loads(subprocess.run([str(rust_bin), "_commands"], capture_output=True, text=True).stdout)
    owned = "|".join(c["name"] for c in table if c["owner"] == "rust")
    return FAKE_CARGO.replace("{rust_bin}", str(rust_bin)).replace("{rust_owned}", owned)


class BoardBuildTest(unittest.TestCase):
    """bullpen rebuilding its programs from its own checkout: a copy of bullpen with a
    workspace of its own (core/, cli/ and board/), which is also the repository its tasks
    are in (local mode), and a fake cargo."""

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
        (home / "Cargo.toml").write_text('[workspace]\nmembers = ["core", "cli", "board"]\n')
        (home / "Cargo.lock").write_text("version = 4\n")
        for crate, main in (("core", "lib.rs"), ("cli", "main.rs"), ("board", "main.rs")):
            (home / crate / "src").mkdir(parents=True)
            (home / crate / "Cargo.toml").write_text(f'[package]\nname = "bullpen-{crate}"\n')
            (home / crate / "src" / main).write_text("fn main() {}\n")
        (home / ".gitignore").write_text("/target/\n")
        sh("git", "init", "-q", "-b", "main", str(home))
        sh("git", "-C", str(home), "add", ".")
        sh("git", "-C", str(home), "-c", "user.name=T", "-c", "user.email=t@example.com",
           "commit", "-qm", "bullpen")
        self.home = self.repo = home
        self.bin = home / "bin" / "bullpen"
        cargo = self.fake / "cargo"
        cargo.write_text(fake_cargo())
        cargo.chmod(0o755)
        del self.env["BULLPEN_BOARD"], self.env["BULLPEN_NO_BUILD"]  # about building: this copy builds
        (self.tmp / "home").mkdir()
        self.env.update(HOME=str(self.tmp / "home"), PATH=f"{self.fake}:{self.env['PATH']}",
                        # bin/bullpen-cargo's account home and mise: the test's, never the person's
                        BULLPEN_ACCOUNT_HOME=str(self.tmp / "home"), BULLPEN_MISE=str(self.fake / "no-mise"))
        for name in ("RUSTUP_HOME", "CARGO_HOME", "RUSTUP_TOOLCHAIN"):
            self.env.pop(name, None)
        self.target = home / "target"
        # as install.sh leaves it before its first `bullpen build`: the command line, with no stamp
        self.target.mkdir()
        shutil.copy2(ROOT / "target" / "bullpen", self.target / "bullpen")

    def bp(self, *args, cwd=None, ok=True):
        p = subprocess.run([str(self.bin), *args], cwd=cwd or self.repo, env=self.env,
                           text=True, capture_output=True)
        if ok and p.returncode != 0:
            self.fail(f"bullpen {' '.join(args)} failed: {p.stderr}")
        return p

    def settle(self):
        """Wait for a background build to be done: it holds the lock until then."""
        import fcntl
        lock = self.target / "build.lock"
        deadline = time.time() + 15
        while lock.exists():
            with lock.open("a") as f:
                try:
                    fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    return
                except BlockingIOError:
                    pass
            if time.time() > deadline:
                self.fail("bullpen's build didn't finish")
            time.sleep(0.05)

    def builds(self):
        log = self.fake / "cargo.log"
        return len(log.read_text().splitlines()) if log.exists() else 0

    def board(self, name="bullpen-board"):
        b = self.target / name
        return b.read_text() if b.exists() else None

    def edit_board(self, text, commit=True, crate="board"):
        (self.home / crate / "src" / "main.rs").write_text(text)
        if commit:
            sh("git", "-C", str(self.home), "-c", "user.name=T", "-c", "user.email=t@example.com",
               "commit", "-qam", "Change the board")

    def board_lines(self):
        return [l for l in self.news_log().splitlines() if " board board: " in l]

    def doctor_board(self):
        line = next(l for l in self.bp("doctor", ok=False).stdout.splitlines() if " board: " in l)
        return line.split(None, 1)[0], line.split(": ", 1)[1]

    def test_up_builds_when_the_source_changed_and_not_otherwise(self):
        out = self.bp("up").stdout  # nothing built yet: both programs, in one cargo run
        self.assertIn("rebuilding bullpen's command line and the board in the background (their source changed); "
                      "running boards reload when it's done", out)
        self.settle()
        self.assertEqual(self.builds(), 1)
        self.assertIn("fn main() {}", self.board("bullpen"))
        self.assertIn("fn main() {}", self.board())
        self.assertEqual(self.doctor_board()[0], "ok")
        self.assertNotIn("rebuilding", self.bp("up").stdout)  # unchanged: no build
        self.settle()
        self.assertEqual(self.builds(), 1)
        self.edit_board("fn main() { v2() }\n")
        self.assertEqual(self.doctor_board()[0], "FAIL")  # stale until it's built
        self.assertIn("rebuilding the board in the background (its source changed)", self.bp("up").stdout)
        self.settle()
        self.assertEqual(self.builds(), 2)
        self.assertIn("v2()", self.board())
        self.assertEqual(self.board_lines(), [])  # a build that works isn't news

    def test_up_leaves_the_build_running_in_the_background(self):
        (self.fake / "cargo_slow").write_text("3")
        start = time.time()
        self.assertIn("rebuilding", self.bp("up").stdout)
        self.assertLess(time.time() - start, 2.5)  # it didn't wait for cargo
        self.settle()
        self.assertIn("fn main() {}", self.board())

    def test_a_build_renames_a_copy_of_each_program_into_place(self):
        out = self.bp("build").stdout
        self.assertEqual(out.splitlines(), [f"built bullpen's command line: {self.target / 'bullpen'}",
                                            f"built the board: {self.target / 'bullpen-board'}"])
        for name in ("bullpen", "bullpen-board"):
            cargo_out = self.target / "release" / name
            built = self.target / name
            self.assertEqual(built.read_text(), cargo_out.read_text())
            self.assertNotEqual(os.stat(built).st_ino, os.stat(cargo_out).st_ino)  # not cargo's hard link
        board = self.target / "bullpen-board"
        ino = os.stat(board).st_ino
        self.edit_board("fn main() { v2() }\n")
        self.bp("build")
        self.assertNotEqual(os.stat(board).st_ino, ino)  # a new file: running boards see it replaced
        self.assertEqual([p.name for p in self.target.iterdir() if p.name.startswith(".bullpen")], [])
        self.assertEqual(json.loads((self.target / "bullpen.json").read_text()).keys(), {"source"})

    def test_accept_in_bullpens_own_repo_rebuilds_the_board(self):
        self.bp("build")
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

    def test_accept_in_bullpens_own_repo_rebuilds_the_command_line(self):
        self.bp("build")
        self.bp("config", "--review", "off")
        tid = self.start_task()
        wt = Path(self.show(tid)["worktree"])
        (wt / "cli" / "src" / "main.rs").write_text("fn main() { accepted() }\n")
        sh("git", "-C", str(wt), "commit", "-qam", "Command line change")
        self.bp("ready", tid, "--summary", "Changed the command line.", cwd=wt)
        out = self.bp("accept", tid).stdout
        self.assertIn("rebuilding bullpen's command line in the background (its source changed)", out)
        self.assertNotIn("running boards reload", out)  # the board's source didn't change
        self.settle()
        self.assertEqual(self.builds(), 2)
        self.assertIn("accepted()", self.board("bullpen"))
        self.assertNotIn("accepted()", self.board())
        self.assertEqual(self.show(tid)["status"], "done")  # the new build runs the next call

    def test_an_accept_that_changes_the_keys_says_to_reload_once(self):
        self.bp("build")
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
        self.bp("build")
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
        self.bp("build")
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
        self.bp("build")
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
        log = self.target / "build.log"
        self.assertIn(f"Log: {log}", lines[0])
        self.assertIn("error[E0425]", lines[0])
        self.assertIn("error[E0425]", log.read_text())
        state, text = self.doctor_board()
        self.assertEqual(state, "FAIL")
        self.assertIn("its build failed", text)
        (self.fake / "cargo_fails").unlink()
        self.bp("build")  # the fix doctor names
        self.assertEqual(self.doctor_board()[0], "ok")

    def install(self):
        shutil.copy2(ROOT / "install.sh", self.home / "install.sh")
        return subprocess.run([str(self.home / "install.sh")], cwd=self.home, text=True, capture_output=True,
                              env={**self.env, "BIN_DIR": str(self.tmp / "bin")})

    def test_board_runs_the_built_copy(self):
        self.bp("build")
        board = self.target / "bullpen-board"
        board.write_text("#!/bin/sh\necho \"$0\"\n")
        self.assertEqual(self.bp("board").stdout.strip(), str(board))
