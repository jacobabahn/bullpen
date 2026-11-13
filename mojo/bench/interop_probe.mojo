"""What one use of Python interop costs a Mojo program's start-up (bench/bench.py has none:
the port uses no interop). Run with an argument to import Python's json and use it once."""

from std.sys import argv
from std.python import Python


def main() raises:
    if len(argv()) > 1:  # PYTHON INTEROP (this probe only)
        var json = Python.import_module("json")
        _ = json.dumps(Python.list(1, 2))
