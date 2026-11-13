# The code

bullpen's Python is the `bullpen/` package, beside `bin/`. `bin/bullpen` is only the
entry script: it finds the package from its own resolved path (so `install.sh`'s link
to it works) and runs `bullpen.cli.main`. Python caches a package's bytecode, but
compiles a script it runs on every call, so keeping the script small makes every call
(the board's, the watcher's, the keys') about twice as fast. Lua, the key bindings
and bullpen's own background jobs still run `bin/bullpen`.

The modules:

- `cli.py`: the parser and `main`. It keeps only the `--help` groups' titles and
  their order (`GROUPS`), and the modules with commands (`MODULES`).
- `core.py`: everything else for now: repo, config and tasks, the task lifecycle, Rex,
  news, review, accept and the watcher's events. Its settings table, `SETTINGS`, is at
  the top.
- `tours.py`: guided review tours and the person's comments on them
- `stepper.py`: reading a tour on the board
- `line_edit.py`: typing on a terminal, for the picker and the stepper
- `pick.py`: the jump picker
- `push_remote.py`: the push remote, in local mode
- `board_build.py`: rebuilding the board when its source changes
- `keys.py`: bullpen's key bindings in the Rex config
- `doctor.py`: `bullpen doctor`

Each module with commands declares them in its own `commands(sub)`, with
`core.command(sub, name, fn, group, help=...)`; the group is a title in `cli.GROUPS`,
and internal commands (the watcher's and bullpen's own, named `_...`) have none.
`bullpen --help` lists a group's commands in `MODULES` order, then in the order each
module declares them. A new command touches only its own module.

The feature modules import what they need from `core.py`. Where `core.py` needs a
feature module (`up` and `accept` start a push or a board build), it imports it
inside the function, because the feature modules import `core.py` at the top. The
same goes for `tours.py` opening the stepper.

A file stays under 1,000 lines unless there's a good reason. `core.py` is the one
exception until its split: a test (`tests/test_cli.py`) holds the others to it.
