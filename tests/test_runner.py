"""tests/run.py itself: how many processes, and at what priority."""

import os
import subprocess
import sys
import unittest

from support import setUpModule, tearDownModule, ROOT  # a temp folder for each module

import run


class RunnerTest(unittest.TestCase):
    def test_one_process_per_cpu_by_default(self):
        self.assertEqual(run.JOBS, os.cpu_count() or 1)

    @unittest.skipUnless(hasattr(os, "nice"), "no nice here")
    def test_a_pool_process_lowers_its_priority(self):
        code = ("import os, sys; sys.path.insert(0, sys.argv[1]); import run; "
                "before = os.nice(0); run.lower_priority(); print(os.nice(0) - before)")
        p = subprocess.run([sys.executable, "-c", code, str(ROOT / "tests")],
                           text=True, capture_output=True, check=True)
        self.assertEqual(int(p.stdout), min(run.NICE, 19 - os.nice(0)))
