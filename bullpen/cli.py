"""bullpen's command line: the parser, built from each module's commands, and main."""

import argparse
import importlib
import sys

from . import __doc__ as ABOUT
from . import core
from .core import Fail, run_later

# The modules with commands, in the order `bullpen --help` lists a group's commands.
MODULES = ("go", "board", "show", "answer", "accept", "close", "stepper", "tours", "pick", "start",
           "triage", "check", "news", "guide", "ready", "auto_accept", "up", "config", "rules",
           "push_remote", "board_build", "keys", "doctor", "coordinators", "notifications", "prs",
           "events")

# Each command and the module that declares it, in --help's order. A call imports only
# its command's module (and what that imports), not all of them: start-up is most of a
# call's time. A test checks this against what the modules declare.
COMMANDS = {
    "go": "go", "hide": "go", "adopt": "go",
    "board": "board",
    "ls": "show", "show": "show", "diff": "show",
    "answer": "answer", "reply": "answer", "prompt": "answer",
    "accept": "accept",
    "close": "close", "drop": "close",
    "tour": "stepper", "_tour": "stepper",
    "tour-comment": "tours",
    "pick": "pick",
    "add": "start", "after": "start", "model": "start", "review-mode": "start", "start": "start",
    "resume": "start", "tell": "start", "peek": "start",
    "ask": "triage", "ok": "triage", "review": "triage", "note": "triage",
    "check": "check",
    "news": "news",
    "guide": "guide",
    "ready": "ready", "finish": "ready",
    "verdict": "auto_accept", "_auto_accept": "auto_accept",
    "up": "up",
    "config": "config",
    "rules": "rules",
    "_push": "push_remote",
    "build-board": "board_build", "_build_board": "board_build",
    "keys": "keys",
    "doctor": "doctor",
    "_coordinator_gone": "coordinators",
    "_flush": "notifications",
    "_prs": "prs",
    "_event": "events", "_stuck": "events", "_limit": "events", "_gone": "events",
}

# The modules that register what saving a task sets off (core.on_save), in the order
# their hooks run: the run queue, the person's notification, the PR to follow.
SAVERS = ("start", "notifications", "prs")

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


class Formatter(argparse.HelpFormatter):
    """argparse's formatter, but its colours (Python 3.14's _colorize, which takes about
    13 ms to import) load when it first formats something, not whenever a parser is
    built: most calls never print help or usage."""

    def _set_color(self, color):
        self.__dict__.pop("_theme", None)
        self.__dict__.pop("_decolor", None)
        self._wants_color = color

    def __getattr__(self, name):
        if name in ("_theme", "_decolor") and "_wants_color" in self.__dict__:
            super()._set_color(self._wants_color)
            return self.__dict__[name]
        raise AttributeError(name)


class CommandParser(argparse.ArgumentParser):
    """A command's parser, with the formatter above."""

    def __init__(self, *a, formatter_class=Formatter, **kw):
        super().__init__(*a, formatter_class=formatter_class, **kw)


class Parser(CommandParser):
    """The top-level parser: a short usage line and the commands in groups."""

    def format_usage(self):
        return USAGE

    def format_help(self):
        import shutil
        import textwrap
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
    import difflib
    close = difflib.get_close_matches(name, [n for n in choices if not n.startswith("_")], n=1)
    hint = f" Did you mean `{close[0]}`?" if close else ""
    return f"bullpen: no command `{name}`.{hint} `bullpen --help` lists them."


def module(name):
    return importlib.import_module(f"{__package__}.{name}")


def load_savers():
    """Import the modules that act on a save, and run their hooks in SAVERS' order,
    whichever of them the command imported first."""
    order = [module(m).__name__ for m in SAVERS]
    core.SAVED.sort(key=lambda fn: order.index(fn.__module__) if fn.__module__ in order else len(order))


def parser(modules=MODULES):
    """The parser with the commands of these modules (all of them for --help)."""
    ap = Parser(prog="bullpen", description=ABOUT.splitlines()[0])
    # prog as argparse would work it out, which would format a usage line to do it
    sub = ap.add_subparsers(dest="cmd", required=True, parser_class=CommandParser, prog="bullpen")
    for m in modules:
        module(m).commands(sub)
    return ap


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if not argv:
        print(parser().format_help(), end="")
        return 2
    if err := unknown_command(argv, COMMANDS):
        print(err, file=sys.stderr)
        return 2
    core.LOAD_SAVERS[:] = [load_savers]
    args = parser([COMMANDS[argv[0]]] if argv[0] in COMMANDS else MODULES).parse_args(argv)
    try:
        code = args.fn(args)
    except Fail as e:
        print(f"bullpen: {e}", file=sys.stderr)
        code = 1
    finally:
        run_later()  # what the command's saves set off: the run queue
    return code if type(code) is int else 0
