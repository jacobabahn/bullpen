#!/usr/bin/env python3
"""Run the tests across CPUs.

    python3 tests/run.py                 # the whole suite
    python3 tests/run.py board tour      # only tests whose module, class or name has one of these
    python3 tests/run.py -j 4            # 4 processes (default: one per CPU)

Each process runs a few tests at a time, in a temp folder of its own (setUpModule), so
tests that share nothing but the machine run side by side. Exits 1 if any test fails.
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


def run_chunk(names):
    try:
        suite = unittest.defaultTestLoader.loadTestsFromNames(names)
        result = Result()
        suite.run(result)
        return names, result.testsRun, len(result.skipped), result.bad
    except Exception:
        return names, 0, 0, [("ERROR", " ".join(names), traceback.format_exc())]


def main():
    ap = argparse.ArgumentParser(description="Run the tests across CPUs.")
    ap.add_argument("only", nargs="*", help="run only tests whose ID has one of these words")
    ap.add_argument("-j", "--jobs", type=int, default=os.cpu_count() or 1)
    args = ap.parse_args()
    loader = unittest.TestLoader()
    suite = loader.discover(str(HERE), top_level_dir=str(HERE))
    if loader.errors:  # a module that doesn't import: say why, run nothing
        sys.exit("\n".join(loader.errors))
    names = [n for n in ids(suite) if not args.only or any(w in n for w in args.only)]
    if not names:
        sys.exit(f"no tests match {' '.join(args.only)}")
    start = time.monotonic()
    ran = skipped = 0
    bad = []
    with multiprocessing.get_context("fork" if hasattr(os, "fork") else "spawn").Pool(
            min(args.jobs, len(names))) as pool:
        for _, n, s, b in pool.imap_unordered(run_chunk, chunks(names)):
            ran, skipped = ran + n, skipped + s
            bad += b
            marks = "".join("F" if k == "FAIL" else "E" for k, _, _ in b)
            print("." * max(0, n - len(b)) + marks, end="", flush=True)
    print()
    for kind, name, tb in bad:
        print("=" * 70, f"{kind}: {name}", "-" * 70, tb, sep="\n")
    print("-" * 70)
    print(f"Ran {ran} tests in {time.monotonic() - start:.1f}s ({min(args.jobs, len(names))} processes)")
    print(f"\nFAILED ({len(bad)} failing)" if bad else "\nOK" + (f" (skipped={skipped})" if skipped else ""))
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
