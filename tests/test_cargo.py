"""Finding the cargo to build bullpen with (bin/bullpen-cargo), wherever the build starts:
a mise shim with no version, rustup's proxy with no default toolchain, a job whose HOME
or RUSTUP_HOME points elsewhere, and none at all."""

import os
import textwrap
import unittest

import test_board_build  # its setup, not its tests: a bare import keeps them out of this module
from support import setUpModule, tearDownModule  # a temp folder for each module

# rustup's proxy: the toolchain from RUSTUP_TOOLCHAIN, else the default in RUSTUP_HOME
# (by default $HOME/.rustup), which must be installed; then the fake cargo.
PROXY = textwrap.dedent("""\
    #!/usr/bin/env python3
    import os, sys
    from pathlib import Path
    with open(Path(os.environ["FAKE_DIR"]) / "version.log", "a") as f:
        if sys.argv[1:] == ["--version"]:
            f.write(sys.argv[0] + "\\n")
    home = Path(os.environ.get("RUSTUP_HOME") or Path(os.environ["HOME"]) / ".rustup")
    tc = os.environ.get("RUSTUP_TOOLCHAIN")
    settings = home / "settings.toml"
    for line in settings.read_text().splitlines() if settings.exists() and not tc else []:
        if line.startswith("default_toolchain"):
            tc = line.split("=", 1)[1].strip().strip('"')
    if not tc:
        sys.exit("error: rustup could not choose a version of cargo to run, because one wasn't specified "
                 "explicitly, and no default is configured.")
    if not (home / "toolchains" / tc).is_dir():
        sys.exit(f"error: toolchain '{tc}' is not installed")
    os.execv("{real}", ["{real}", *sys.argv[1:]])
""")


class CargoTest(unittest.TestCase):
    """test_board_build's copy of bullpen, its fake cargo on PATH, and HOME (the account's
    home too) in the test's folder."""

    B = test_board_build.BoardBuildTest
    setUp_with_origin, setUp, bp, settle, board, news_log, install, edit_board = (
        B.setUp_with_origin, B.setUp, B.bp, B.settle, B.board, B.news_log, B.install, B.edit_board)

    def path_without_cargo(self):
        """PATH: the fake folder, then every other program but cargo, rustup and mise."""
        tools = self.tmp / "tools"
        if not tools.exists():
            tools.mkdir()
            for d in os.environ["PATH"].split(os.pathsep):
                if not os.path.isdir(d):
                    continue
                for e in os.scandir(d):
                    if e.name not in ("cargo", "rustup", "mise") and not os.path.lexists(tools / e.name):
                        os.symlink(e.path, tools / e.name)
        self.env["PATH"] = f"{self.fake}:{tools}"

    def rustups_cargo(self):
        """A cargo of rustup's, in ~/.cargo/bin (one with a default toolchain)."""
        rustup = self.tmp / "home" / ".cargo" / "bin" / "cargo"
        rustup.parent.mkdir(parents=True)
        rustup.write_text(test_board_build.fake_cargo().replace('(d / "cargo_mise").exists()', "False"))
        rustup.chmod(0o755)

    def rustup_proxy(self, where):
        """rustup's proxy at where, which runs the fake cargo (moved out of PATH)."""
        real = self.tmp / "real-cargo"
        if not real.exists():
            (self.fake / "cargo").rename(real)
        where.parent.mkdir(parents=True, exist_ok=True)
        where.write_text(PROXY.replace("{real}", str(real)))
        where.chmod(0o755)
        self.path_without_cargo()

    def rustup_home(self, where, default=None, installed=()):
        for tc in installed:
            (where / "toolchains" / tc).mkdir(parents=True)
        if default:
            (where / "settings.toml").write_text(f'default_toolchain = "{default}"\n')
        return where

    def remembered(self):
        found = self.home / "target" / "cargo.env"
        return dict(l.split("=", 1) for l in found.read_text().splitlines()) if found.exists() else {}

    def checks(self):
        log = self.fake / "version.log"
        return len(log.read_text().splitlines()) if log.exists() else 0

    def build_lines(self):
        return [l for l in self.news_log().splitlines() if " bullpen build: " in l]

    def doctor_cargo(self):
        line = next(l for l in self.bp("doctor", ok=False).stdout.splitlines() if " cargo: " in l)
        return line.split(None, 1)[0], line.split(": ", 1)[1]

    def test_cargo_on_path_is_used_and_remembered(self):
        self.bp("build")
        self.assertIn("fn main() {}", self.board())
        self.assertEqual(self.remembered(), {"cargo": str(self.fake / "cargo")})
        self.assertEqual(self.doctor_cargo(), ("ok", f"{self.fake / 'cargo'} (cargo 1.0.0 (fake))"))

    def test_a_mise_shim_with_no_other_cargo_says_what_it_tried(self):
        (self.fake / "cargo_mise").write_text("")
        self.path_without_cargo()
        p = self.bp("build", ok=False)
        tried = (f"no cargo works here: tried cargo on PATH ({self.fake / 'cargo'}): a mise shim with no Rust "
                 f"version set for {self.home}")
        self.assertIn(tried, p.stderr)
        self.assertFalse((self.target / "bullpen-board").exists())
        state, text = self.doctor_cargo()
        self.assertEqual(state, "FAIL")
        self.assertIn(tried, text)
        # the background rebuild's news line says the same
        (self.target / "bullpen.json").unlink()
        self.bp("up")
        self.settle()
        lines = self.build_lines()
        self.assertEqual(len(lines), 1, lines)
        self.assertIn(tried, lines[0])

    def test_no_cargo_at_all(self):
        (self.fake / "cargo").unlink()
        self.path_without_cargo()
        p = self.bp("build", ok=False)
        self.assertIn("no cargo works here: tried cargo on PATH: there's none. Install Rust (https://rustup.rs)",
                      p.stderr)

    def test_a_mise_shim_falls_back_to_rustups_cargo(self):
        (self.fake / "cargo_mise").write_text("")
        self.rustups_cargo()
        self.bp("build")
        self.assertIn("fn main() {}", self.board())
        self.assertEqual(self.remembered()["cargo"], str(self.tmp / "home" / ".cargo" / "bin" / "cargo"))

    def test_a_job_whose_home_is_elsewhere_uses_the_accounts_rustup(self):
        # the incident: a rebuild started where HOME isn't the account's, so rustup's proxy
        # reads an empty ~/.rustup: "no default is configured"
        account = self.rustup_home(self.tmp / "home" / ".rustup", default="1.99", installed=["1.99"])
        self.rustup_proxy(self.fake / "cargo")
        (self.tmp / "elsewhere").mkdir()
        self.env["HOME"] = str(self.tmp / "elsewhere")
        self.assertIn("rebuilding", self.bp("up").stdout)
        self.settle()
        self.assertIn("fn main() {}", self.board())
        self.assertEqual(self.build_lines(), [])
        self.assertEqual(self.remembered(), {"cargo": str(self.fake / "cargo"), "RUSTUP_HOME": str(account),
                                             "CARGO_HOME": str(self.tmp / "home" / ".cargo")})
        # the next build tries the one that worked first: one check, not two
        before = self.checks()
        self.edit_board("fn main() { v2() }\n")
        self.bp("build")
        self.assertIn("v2()", self.board())
        self.assertEqual(self.checks() - before, 1)

    def test_rustup_home_pointing_elsewhere_uses_the_accounts_rustup(self):
        account = self.rustup_home(self.tmp / "home" / ".rustup", default="1.99", installed=["1.99"])
        self.rustup_proxy(self.fake / "cargo")
        self.env["RUSTUP_HOME"] = str(self.tmp / "nowhere")
        self.bp("build")
        self.assertIn("fn main() {}", self.board())
        self.assertEqual(self.remembered()["RUSTUP_HOME"], str(account))
        state, text = self.doctor_cargo()
        self.assertEqual(state, "ok")
        self.assertIn("with RUSTUP_HOME=~/.rustup", text)  # doctor says the home as ~

    def test_rustup_with_no_default_uses_an_installed_toolchain_and_says_so(self):
        self.rustup_home(self.tmp / "home" / ".rustup", installed=["1.80.0-x", "1.99.1-x"])
        self.rustup_proxy(self.tmp / "home" / ".cargo" / "bin" / "cargo")
        self.bp("build")
        self.assertIn("fn main() {}", self.board())
        self.assertEqual(self.remembered()["RUSTUP_TOOLCHAIN"], "1.99.1-x")
        self.assertIn("rustup has no default toolchain: using 1.99.1-x (set one with `rustup default 1.99.1-x`)",
                      (self.target / "build.log").read_text())

    def test_rustup_with_no_toolchain_says_so(self):
        self.rustup_proxy(self.fake / "cargo")
        p = self.bp("build", ok=False)
        self.assertIn(f"tried cargo on PATH ({self.fake / 'cargo'}): rustup has no default toolchain in "
                      f"{self.tmp / 'home' / '.rustup'}, and none installed", p.stderr)

    def test_the_one_remembered_is_dropped_when_it_stops_working(self):
        self.bp("build")
        found = self.home / "target" / "cargo.env"
        found.write_text(f"cargo={self.tmp / 'gone'}\n")
        self.edit_board("fn main() { v2() }\n")
        self.bp("build")
        self.assertIn("v2()", self.board())
        self.assertEqual(self.remembered(), {"cargo": str(self.fake / "cargo")})

    def test_install_with_a_mise_shim_falls_back_to_rustups_cargo(self):
        (self.fake / "cargo_mise").write_text("")
        self.rustups_cargo()
        p = self.install()
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertIn("fn main() {}", self.board())
        self.assertTrue((self.tmp / "bin" / "bullpen").is_symlink())

    def test_install_with_a_mise_shim_and_no_rustup_says_so(self):
        (self.fake / "cargo_mise").write_text("")
        self.path_without_cargo()
        p = self.install()
        self.assertEqual(p.returncode, 1)
        self.assertIn("no cargo works here: tried cargo on PATH", p.stdout)  # cargo's output, shown
        self.assertIn("a mise shim with no Rust version set for", p.stdout)
        self.assertIn("install.sh: bullpen wasn't built", p.stderr)


if __name__ == "__main__":
    unittest.main()
