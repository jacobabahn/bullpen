# bullpen in Mojo (experiment, BP-92)

A port of part of bullpen's Python CLI to Mojo, to compare the two. See REPORT.md for
how far it got and what it measured.

- Build: `mojo/build.sh` (Mojo 1.2.0 from the worktree's `.venv`: `uv venv .venv && uv pip
  install --python .venv mojo`). The binary is `mojo/build/bullpen` (`BUILD_OUT` to put it elsewhere).
- Run: `mojo/build/bullpen ls`, in a repo, with the same `BULLPEN_STATE` as `bin/bullpen`. A
  command it hasn't ported is handed to `bin/bullpen` (exec), so it can stand in for it.
- Check: `mojo/check/compare.sh [CASE...]` runs each case of `check/cases.txt` with Python and
  Mojo in a fresh fixture state (`check/fixture.py`) and compares stdout, stderr, exit code and
  the state folder after, byte for byte.

## The code

`main.mojo` is the entry point: it dispatches a command to its module, by argv[0].
`bullpen/` mirrors the Python package, a module per Python module, with the same
function names where it can:

- `proc.mojo`: running a program (posix_spawnp over FFI; Mojo's subprocess has no exit code)
- `json.mojo`: JSON. A document is an arena (`Doc`, nodes by index; node 0 the top); `dumps`
  writes what Python's `json.dumps(indent=2)` does
- `timeutil.mojo`: ISO stamps, now, local time (localtime_r over FFI), `clock`
- `core.mojo`: paths, config layering, `Repo`, `Task` (a Doc: `t.s(key)`, `t.truthy(key)`,
  `t.get(key)` for a node), status words
- `args.mojo`: each command's flags, options and positionals (no argparse)
- `rex.mojo`, `coordinators.mojo`: the parts of those the read-only commands need

Conventions: typed `def` (Mojo 1.2 removed `fn`), structs for records, a command's error is
`raise Error(msg)` (main prints `bullpen: msg`, exit 1). A Python `None` is "" for a string,
-1 for a node index. No Python interop anywhere unless marked `# PYTHON INTEROP` (there is none so far).
Keep each file under 1,000 lines.
