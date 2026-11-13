"""bullpen's command line: the parser, built from each module's commands, and main."""

import argparse
import difflib
import shutil
import sys
import textwrap

from . import __doc__ as ABOUT
from . import board_build, core, doctor, keys, pick, push_remote, tours
from .core import Fail

# The modules with commands, in the order `bullpen --help` lists a group's commands.
MODULES = (core, tours, pick, push_remote, board_build, keys, doctor)

# `bullpen --help` lists the commands in these groups, in this order: each command
# names its group (core.command). A label in a command's help that the group's title
# already says ("(coordinator) ") is left out.
GROUPS = [
    ("You", "(the person's call) "),
    ("Coordinator", "(coordinator) "),
    ("Worker and reviewer", ""),
    ("Setup", ""),
]
USAGE = "usage: bullpen COMMAND [ARGS]\n"


def grouped(sub):
    """{group title: [(name, its parser)]} in GROUPS' order; internal commands left out."""
    out = {title: [] for title, _ in GROUPS}
    for name, p in sub.choices.items():
        if p.group:
            out[p.group].append((name, p))
    return out


class Parser(argparse.ArgumentParser):
    """The top-level parser: a short usage line and the commands in groups."""

    def format_usage(self):
        return USAGE

    def format_help(self):
        sub = next(a for a in self._actions if isinstance(a, argparse._SubParsersAction))
        helps = {a.dest: a.help for a in sub._choices_actions}
        groups = grouped(sub)
        width = max(shutil.get_terminal_size().columns - 2, 40)
        name_w = max(len(n) for cmds in groups.values() for n, _ in cmds) + 2
        out = [USAGE, "\n" + textwrap.fill(self.description, width) + "\n"]
        for title, label in GROUPS:
            out.append(f"\n{title}:\n")
            for n, _ in groups[title]:
                h = helps[n].removeprefix(label)
                out.append(textwrap.fill(h, width, initial_indent=f"  {n:<{name_w}}",
                                         subsequent_indent=" " * (name_w + 2)) + "\n")
        out.append("\n" + textwrap.fill("`bullpen COMMAND --help` says more about one.", width) + "\n")
        return "".join(out)


def unknown_command(argv, choices):
    """The one-line error for a command bullpen doesn't have, else None."""
    if not argv or argv[0] in ("-h", "--help") or argv[0] in choices:
        return None
    name = argv[0]
    if name.startswith("-"):
        return f"bullpen: `{name}` goes after a command. `bullpen --help` lists them."
    close = difflib.get_close_matches(name, [n for n in choices if not n.startswith("_")], n=1)
    hint = f" Did you mean `{close[0]}`?" if close else ""
    return f"bullpen: no command `{name}`.{hint} `bullpen --help` lists them."


def parser():
    ap = Parser(prog="bullpen", description=ABOUT.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True, parser_class=argparse.ArgumentParser)
    for m in MODULES:
        m.commands(sub)
    return ap


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    ap = parser()
    if not argv:
        print(ap.format_help(), end="")
        return 2
    sub = next(a for a in ap._actions if isinstance(a, argparse._SubParsersAction))
    if err := unknown_command(argv, sub.choices):
        print(err, file=sys.stderr)
        return 2
    args = ap.parse_args(argv)
    try:
        code = args.fn(args)
    except Fail as e:
        print(f"bullpen: {e}", file=sys.stderr)
        return 1
    return code if type(code) is int else 0
