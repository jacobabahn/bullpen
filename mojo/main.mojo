"""The entry point of bullpen in Mojo. It runs the commands ported so far and hands any
other one to the Python bullpen (bin/bullpen beside it), so it can stand in for it."""

from std.sys import argv, stderr, exit
from std.ffi import external_call, c_int
from std.os import getenv

from bullpen.args import Args, parse
from bullpen.core import python_self
from bullpen import show, rex, coordinators


def dispatch(av: List[String]) raises -> Int:
    """Run a ported command; -1 when it isn't one (or isn't ported with these arguments)."""
    var cmd = av[0]
    if cmd == "_nop":  # does nothing: bench/bench.py times start-up with it
        return 0
    if cmd == "ls":
        show.cmd_ls(parse(av, "--all --all-repos --json", "", 0, 0))
    elif cmd == "show":
        show.cmd_show(parse(av, "--json", "", 1, 1))
    else:
        return -1
    return 0


def hand_to_python(av: List[String]) raises -> Int:
    """Exec the Python bullpen with the same arguments; returns only if that fails.
    BULLPEN_MOJO_STRICT=1 (check/compare.sh) says so and fails instead."""
    if getenv("BULLPEN_MOJO_STRICT"):
        print("bullpen-mojo: not ported: " + String(" ").join(av), file=stderr)
        return 3
    var path = python_self()
    var all: List[String] = [path]
    all += av.copy()
    var cstrs = List[String]()
    for a in all:
        cstrs.append(a + "\0")
    var ptrs = List[Int](length=len(cstrs) + 1, fill=0)
    for i in range(len(cstrs)):
        ptrs[i] = Int(cstrs[i].unsafe_ptr())
    _ = external_call["execv", c_int](Int(cstrs[0].unsafe_ptr()), Int(ptrs.unsafe_ptr()))
    print("bullpen: couldn't run " + path, file=stderr)
    return 1


def main() raises:
    var raw = argv()
    var av = List[String]()
    for i in range(1, len(raw)):
        av.append(String(raw[i]))
    if len(av) == 0 or av[0].startswith("-"):
        exit(hand_to_python(av))
    var code = 0
    try:
        code = dispatch(av)
    except e:
        var msg = String(e)
        if msg == "__usage__":
            exit(2)
        if msg == "__python__":
            exit(hand_to_python(av))
        print("bullpen: " + msg, file=stderr)
        exit(1)
    if code < 0:
        exit(hand_to_python(av))
    exit(code)
