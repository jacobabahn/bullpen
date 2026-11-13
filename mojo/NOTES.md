# BP-92 Mojo port: working notes (paused)

Toolchain: Mojo 1.2.0 (2016d0cd), installed with `uv venv .venv && uv pip install --python .venv mojo`
in this worktree only (`.venv/`, untracked). hyperfine isn't installed yet.

## What I found out about Mojo 1.2 so far
- `fn` is gone in 1.2 ("'fn' has been removed; use 'def' instead"). Typed `def` replaces it.
- `len()` on a String is an error: you pick `byte_length()`, `codepoints()` or `graphemes()`.
- A struct can't hold a `List` of itself ("non-'Deinitable' type"), so a JSON value has to
  live in an arena (nodes in one List, children by index).
- The stdlib has `std.sys.argv`, `std.os.getenv/listdir`, `std.os.path.exists`, `std.pathlib.Path`,
  `open()` and `Dict`/`List`. A hello-world build takes about 5 s and makes an 18 KB binary.
- `std.subprocess.run(cmd)` takes one shell string and returns only stdout: no exit code, and
  stderr goes straight to the terminal. bullpen needs both (`check=False`, error messages),
  so I'm writing a runner over libc (`posix_spawnp` with memfd stdout/stderr, then `waitpid`).
- The pointer API is moving: `UnsafePointer` is deprecated in favor of `Pointer`, `alloc` without
  a Layout is deprecated, and positional pointer indexing is deprecated. That's where I paused.

## Plan from here
1. Shared base in `mojo/src/`: process runner, JSON (parse, and a `dumps(indent=2)` matching Python's,
   ensure_ascii included), time (ISO stamps with offsets, local time), config and task loading, entry point.
2. `ls`, `ls --json`, `show`, `news` (not --follow), then `peek`, `diff --stat`, `check`. Command groups
   go to sub-agents, each checking byte-for-byte against `bin/bullpen` on a copy of the state.
3. A compare script, then interleaved timings against main's `bin/bullpen`, and `mojo/REPORT.md`.
