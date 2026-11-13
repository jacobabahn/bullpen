"""A command's arguments. Mojo has no argparse, so each command says what it takes
(its flags, its options with a value, and how many positionals) and this reads argv
against that. Usage errors don't copy argparse's messages: they say what was wrong
and exit 2, as argparse does."""

from std.sys import stderr


struct Args(Movable):
    var cmd: String
    var pos: List[String]
    var flags: List[String]  # the flags given, like "--json"
    var keys: List[String]  # options given with a value, like "--lines"
    var vals: List[String]

    def __init__(out self, cmd: String):
        self.cmd = cmd
        self.pos = List[String]()
        self.flags = List[String]()
        self.keys = List[String]()
        self.vals = List[String]()

    def flag(self, name: String) -> Bool:
        for f in self.flags:
            if f == name:
                return True
        return False

    def opt(self, name: String, default: String = "") -> String:
        for i in range(len(self.keys)):
            if self.keys[i] == name:
                return self.vals[i]
        return default

    def has_opt(self, name: String) -> Bool:
        for k in self.keys:
            if k == name:
                return True
        return False


def _listed(name: String, words: String) -> Bool:
    return (" " + name + " ") in (" " + words + " ")


def parse(argv: List[String], flags: String, options: String, least: Int, most: Int) raises -> Args:
    """argv[0] is the command; flags and options are space-separated names ("--json --all").
    least..most positionals (most -1: any number). `--opt=value` works too."""
    var a = Args(argv[0])
    var i = 1
    var only_pos = False
    while i < len(argv):
        var w = argv[i]
        if only_pos or not w.startswith("-") or w == "-":
            a.pos.append(w)
        elif w == "--":
            only_pos = True
        elif _listed(w, flags):
            a.flags.append(w)
        elif "=" in w and _listed(String(w[byte=:w.find("=")]), options):
            a.keys.append(String(w[byte=:w.find("=")]))
            a.vals.append(String(w[byte=w.find("=") + 1:]))
        elif _listed(w, options):
            if i + 1 >= len(argv):
                usage_error(argv[0], "argument " + w + ": expected one argument")
            a.keys.append(w)
            a.vals.append(argv[i + 1])
            i += 1
        else:
            usage_error(argv[0], "unrecognized arguments: " + w)
        i += 1
    if len(a.pos) < least:
        usage_error(argv[0], "the following arguments are required")
    if most >= 0 and len(a.pos) > most:
        usage_error(argv[0], "unrecognized arguments: " + a.pos[most])
    return a^


def usage_error(cmd: String, msg: String) raises:
    print("usage: bullpen " + cmd + " ...", file=stderr)
    print("bullpen " + cmd + ": error: " + msg, file=stderr)
    raise Error("__usage__")
