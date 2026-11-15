#!/usr/bin/env python3
"""Build bullpen's command line, then run the tests across CPUs.

    python3 tests/run.py                 # the whole suite
    python3 tests/run.py board tour      # only tests whose module, class or name has one of these
    python3 tests/run.py -j 4            # 4 processes (default: one per CPU)

The tests run bin/bullpen, which runs the Rust build (target/bullpen), and the fake rex
and gh in tests/fakes (target/fake-rex, target/fake-gh): they're built first (at low
priority), and the run stops with cargo's error if that fails. Each process
runs a few tests at a time, in a temp folder of its own (setUpModule), so
tests that share nothing but the machine run side by side. The processes run at low
priority (nice 10), and so does everything they start, so several agents running the
suite at once leave the machine responsive. Exits 1 if any test fails.
"""

import argparse
import multiprocessing
import os
import sys
import time
import traceback
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))  # the test modules import by name, here and in each process
CHUNK = 3  # tests of one class run together: few enough to keep every CPU busy at the end
JOBS = os.cpu_count() or 1  # every CPU: nice 10 already gives way to everything else
NICE = 10


def ids(suite):
    for t in suite:
        if isinstance(t, unittest.TestSuite):
            yield from ids(t)
        else:
            yield t.id()


def chunks(names):
    """Consecutive tests of one class, CHUNK at a time."""
    out = []
    for name in names:
        cls = name.rsplit(".", 1)[0]
        if out and len(out[-1]) < CHUNK and out[-1][0].rsplit(".", 1)[0] == cls:
            out[-1].append(name)
        else:
            out.append([name])
    return out


class Result(unittest.TestResult):
    def __init__(self):
        super().__init__()
        self.buffer = True  # a passing test's prints stay quiet
        self.bad = []
        self.times = {}

    def startTest(self, test):
        self.started = time.monotonic()
        super().startTest(test)

    def stopTest(self, test):
        super().stopTest(test)
        self.times[test.id()] = time.monotonic() - self.started

    def addError(self, test, err):
        super().addError(test, err)
        self.bad.append(("ERROR", str(test), self._exc_info_to_string(err, test)))

    def addFailure(self, test, err):
        super().addFailure(test, err)
        self.bad.append(("FAIL", str(test), self._exc_info_to_string(err, test)))

    def addSubTest(self, test, subtest, err):
        super().addSubTest(test, subtest, err)
        if err is not None:
            kind = "FAIL" if issubclass(err[0], test.failureException) else "ERROR"
            self.bad.append((kind, str(subtest), self._exc_info_to_string(err, test)))

    def addUnexpectedSuccess(self, test):
        super().addUnexpectedSuccess(test)
        self.bad.append(("UNEXPECTED SUCCESS", str(test), ""))


def lower_priority():
    """Each pool process starts here: it, and what it starts (bullpen, git, rex), only
    take spare CPU."""
    try:
        os.nice(NICE)
    except (AttributeError, OSError):
        pass  # no nice here (Windows): run at normal priority


def run_chunk(names):
    try:
        suite = unittest.defaultTestLoader.loadTestsFromNames(names)
        result = Result()
        suite.run(result)
        return result.testsRun, len(result.skipped), result.bad, result.times
    except Exception:
        return 0, 0, [("ERROR", " ".join(names), traceback.format_exc())], {}


def build():
    """Build the command line the tests run and their fake rex and gh (tests/fakes),
    unless they're built from their source as it is, and put them in place with their
    stamps, as `bullpen build` does (holding the build lock). Exits with cargo's error
    when the build fails."""
    import fcntl
    import json
    import shutil
    import subprocess
    import support
    if support.cli_built() and support.fakes_built():
        return
    root = HERE.parent
    target = root / "target"
    target.mkdir(exist_ok=True)
    argv = [str(root / "bin" / "bullpen-cargo"), "build", "--release", "--manifest-path", str(root / "Cargo.toml"),
            "-p", "bullpen-cli", "-p", "bullpen-fakes"]
    with (target / "build.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        source, fakes = support.cli_source_hash(), support.fakes_source_hash()
        p = subprocess.run(["nice", *argv], cwd=root, stdin=subprocess.DEVNULL, text=True, capture_output=True)
        if p.returncode != 0:
            sys.exit(f"bullpen's command line, or the tests' fakes, didn't build:\n\n{p.stdout}{p.stderr}")
        for name in ("bullpen", *support.FAKES):
            tmp = target / f".{name}.{os.getpid()}"  # a copy, renamed into place
            shutil.copy2(target / "release" / name, tmp)
            os.replace(tmp, target / name)
        (target / "bullpen.json").write_text(json.dumps({"source": source}, indent=2) + "\n")
        (target / "fakes.json").write_text(json.dumps({"source": fakes}, indent=2) + "\n")


def main():
    ap = argparse.ArgumentParser(description="Run the tests across CPUs.")
    ap.add_argument("only", nargs="*", help="run only tests whose ID has one of these words")
    ap.add_argument("-j", "--jobs", type=int, default=JOBS,
                    help=f"processes to run (default: one per CPU, {JOBS} here)")
    ap.add_argument("--slowest", type=int, default=0, metavar="N", help="list the N slowest tests")
    args = ap.parse_args()
    build()
    os.environ["BULLPEN_TESTS_BUILT"] = "1"  # support.py checks the build only without run.py
    loader = unittest.TestLoader()
    suite = loader.discover(str(HERE), top_level_dir=str(HERE))
    if loader.errors:  # a module that doesn't import: say why, run nothing
        sys.exit("\n".join(loader.errors))
    names = [n for n in ids(suite) if not args.only or any(w in n for w in args.only)]
    if not names:
        sys.exit(f"no tests match {' '.join(args.only)}")
    start = time.monotonic()
    ran = skipped = 0
    bad, times = [], {}
    with multiprocessing.get_context("fork" if hasattr(os, "fork") else "spawn").Pool(
            min(args.jobs, len(names)), initializer=lower_priority) as pool:
        for n, s, b, t in pool.imap_unordered(run_chunk, chunks(names)):
            ran, skipped = ran + n, skipped + s
            bad += b
            times.update(t)
            marks = "".join("F" if k == "FAIL" else "E" for k, _, _ in b)
            print("." * max(0, n - len(b)) + marks, end="", flush=True)
    print()
    for kind, name, tb in bad:
        print("=" * 70, f"{kind}: {name}", "-" * 70, tb, sep="\n")
    for name, secs in sorted(times.items(), key=lambda x: -x[1])[:args.slowest]:
        print(f"{secs:6.2f}s  {name}")
    print("-" * 70)
    print(f"Ran {ran} tests in {time.monotonic() - start:.1f}s ({min(args.jobs, len(names))} processes)")
    print(f"\nFAILED ({len(bad)} failing)" if bad else "\nOK" + (f" (skipped={skipped})" if skipped else ""))
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
