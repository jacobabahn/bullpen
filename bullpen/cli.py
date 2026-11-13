"""bullpen's command line in Python: the parser, built from each module's commands, and
main. The Rust entry (cli/src/main.rs, run by bin/bullpen) prints the top-level help
and the unknown-command line, and hands over the commands this side still owns."""

import argparse
import importlib
import sys

from . import __doc__ as ABOUT
from . import core
from .core import Fail, run_later

# The modules with commands (cli/src/commands.rs lists their commands in `bullpen --help`'s order).
MODULES = ("stepper",)

# Each command Python owns and the module that declares it, in --help's order. A call
# imports only its command's module (and what that imports), not all of them: start-up
# is most of a call's time. A test checks this against what the modules declare, and
# another against the Rust entry's table (cli/src/commands.rs): a command is one side's.
COMMANDS = {
    "tour": "stepper",
}

# The modules that register what saving a task sets off (core.on_save), in the order
# their hooks run: the run queue and the PR to follow, then the person's notification.
SAVERS = ("saves", "notifications")

# The groups a command's help may name (core.command). The Rust entry lists them in
# `bullpen --help`, from its own table (cli/src/help.rs).
GROUPS = [
    ("You", "(the person's call) "),
    ("Coordinator", "(coordinator) "),
    ("Worker and reviewer", ""),
    ("Setup", ""),
]
USAGE = "usage: bullpen COMMAND [ARGS]\n"


class Formatter(argparse.HelpFormatter):
    """argparse's formatter, but its colours (Python 3.14's _colorize, which takes about
    13 ms to import) and the terminal's width (shutil, which imports the compression
    modules) load when it first formats something, not whenever a parser is built:
    most calls never print help or usage."""

    def __init__(self, prog, indent_increment=2, max_help_position=24, width=None, **kw):
        super().__init__(prog, indent_increment, max_help_position, width or 0, **kw)
        if width is None:
            del self._width, self._max_help_position
            self._wants_width = (indent_increment, max_help_position)

    def _set_color(self, color):
        self.__dict__.pop("_theme", None)
        self.__dict__.pop("_decolor", None)
        self._wants_color = color

    def __getattr__(self, name):
        if name in ("_theme", "_decolor") and "_wants_color" in self.__dict__:
            super()._set_color(self._wants_color)
            return self.__dict__[name]
        if name in ("_width", "_max_help_position") and "_wants_width" in self.__dict__:
            import shutil
            indent_increment, max_help_position = self._wants_width
            self._width = shutil.get_terminal_size().columns - 2
            self._max_help_position = min(max_help_position, max(self._width - 20, indent_increment * 2))
            return self.__dict__[name]
        raise AttributeError(name)


class CommandParser(argparse.ArgumentParser):
    """A command's parser, with the formatter above."""

    def __init__(self, *a, formatter_class=Formatter, **kw):
        super().__init__(*a, formatter_class=formatter_class, **kw)


class Parser(CommandParser):
    """The top-level parser: a short usage line."""

    def format_usage(self):
        return USAGE


def module(name):
    return importlib.import_module(f"{__package__}.{name}")


def load_savers():
    """Import the modules that act on a save, and run their hooks in SAVERS' order (a
    module's own in the order it defines them), whichever of them the command imported first."""
    order = [module(m).__name__ for m in SAVERS]
    core.SAVED.sort(key=lambda fn: (order.index(fn.__module__) if fn.__module__ in order else len(order),
                                    fn.__code__.co_firstlineno))


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
    if not argv or argv[0] not in COMMANDS:
        build = module("board_build")
        if argv and argv[0] in build.BUILD_ENTRIES:
            return build.build_entry(argv)  # never the catch-up: it may be the build itself
        if argv:
            build.catch_up(argv)  # returns only when the new build can't run it
        print(build.not_in_build(argv[0] if argv else None), file=sys.stderr)
        return 2
    core.LOAD_SAVERS[:] = [load_savers]
    args = parser([COMMANDS[argv[0]]]).parse_args(argv)
    try:
        code = args.fn(args)
    except Fail as e:
        print(f"bullpen: {e}", file=sys.stderr)
        code = 1
    finally:
        run_later()  # what the command's saves set off: the run queue
    return code if type(code) is int else 0
