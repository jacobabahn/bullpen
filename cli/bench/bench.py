#!/usr/bin/env python3
"""Time the Python bullpen (main's bin/bullpen) against the Rust build, command by command
(BP-92's mojo/bench/bench.py, for Rust).

    bench.py [--runs N] [--warm N] [--only NAME...] [--json OUT]

Each case runs both programs N times, interleaved (Python then Rust, then Rust then
Python, and so on), after a few warm-up runs, with output to /dev/null. Other agents
share the machine, so interleaving makes noise hit both alike. It prints a Markdown
table: the median wall time of each, the spread (25th to 75th percentile), and the ratio.

Cases run in one of two states, both copies so nothing real changes:
- "fixture": check/fixture.py's (41 tasks, fake rex: a Python script, so commands that
  call rex pay its start-up too)
- "real": a copy of ~/.local/state/bullpen (this repo's 90-odd tasks), in this
  checkout, with the real rex, for the commands that read panes
"""

import argparse
import json
import os
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
TOP = HERE.parent.parent
PY = Path(os.environ.get("BENCH_PY", "/home/jbahn/dev/bullpen/bin/bullpen"))
RUST = Path(os.environ.get("BENCH_RUST", TOP / "cli" / "target" / "release" / "bullpen"))

# (name, argv, state). "_nop" is the Rust build's empty command: its start-up alone.
# Python's start-up is an unknown command: it imports every module and builds the
# parser, then exits 2, as every command does before its work.
CASES = [
    ("start-up", {"py": ["_no_such_command"], "rust": ["_nop"]}, "fixture"),
    ("ls --json", ["ls", "--json"], "fixture"),
    ("ls", ["ls"], "fixture"),
    ("ls --all", ["ls", "--all"], "fixture"),
    ("ls --all (real, 92 tasks)", ["ls", "--all"], "real"),
    ("ls --all-repos", ["ls", "--all-repos"], "fixture"),
    ("show BP-17", ["show", "BP-17"], "fixture"),
    ("show BP-17 --json", ["show", "BP-17", "--json"], "fixture"),
    ("news", ["news"], "fixture"),
    ("peek BP-3", ["peek", "BP-3"], "fixture"),
    ("diff BP-3 --stat", ["diff", "BP-3", "--stat"], "fixture"),
    ("check", ["check"], "fixture"),
    ("check (real rex)", ["check"], "real"),
    ("note BP-3 (writes the task)", ["note", "BP-3", "Waiting on CI"], "fixture"),
    ("model BP-1 opus (writes the task)", ["model", "BP-1", "opus"], "fixture"),
]


def fixture_env(work, now):
    out = subprocess.run([sys.executable, str(TOP / "cli/check/fixture.py"), str(work / "fx"), str(now)],
                         check=True, capture_output=True, text=True).stdout
    env, cwd = dict(os.environ), None
    for line in out.splitlines():
        if line.startswith("export "):
            k, v = line[len("export "):].split("=", 1)
            env[k] = v.strip("'")
        elif line.startswith("unset "):
            for k in line.split()[1:]:
                env.pop(k, None)
    cwd = env["FIXTURE_ROOT"]
    return env, cwd


def real_env(work):
    state = work / "real-state"
    if not state.exists():
        shutil.copytree(Path.home() / ".local/state/bullpen", state, symlinks=True)
    env = dict(os.environ, BULLPEN_STATE=str(state))
    env.pop("REX_BLOCK", None)
    return env, str(TOP)


def time_once(argv, env, cwd):
    t = time.perf_counter_ns()
    p = subprocess.run(argv, env=env, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       stdin=subprocess.DEVNULL)
    return (time.perf_counter_ns() - t) / 1e6, p.returncode


def summary(xs):
    q = statistics.quantiles(xs, n=4)
    return {"median": statistics.median(xs), "p25": q[0], "p75": q[2], "min": min(xs), "n": len(xs)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=40)
    ap.add_argument("--warm", type=int, default=3)
    ap.add_argument("--only", nargs="*")
    ap.add_argument("--json")
    ap.add_argument("--work", default=os.environ.get("BENCH_DIR", "/tmp/bullpen-bench"))
    a = ap.parse_args()
    work = Path(a.work)
    work.mkdir(parents=True, exist_ok=True)
    now = time.time()
    fx = fixture_env(work, now)
    real = real_env(work)
    results = []
    for name, argv, state in CASES:
        if a.only and name not in a.only:
            continue
        env, cwd = fx if state == "fixture" else real
        progs = {k: [str(PY if k == "py" else RUST), *(argv[k] if isinstance(argv, dict) else argv)]
                 for k in ("py", "rust")}
        times, codes = {"py": [], "rust": []}, {"py": set(), "rust": set()}
        for i in range(a.warm + a.runs):
            order = ("py", "rust") if i % 2 == 0 else ("rust", "py")
            for k in order:
                ms, code = time_once(progs[k], env, cwd)
                if i >= a.warm:
                    times[k].append(ms)
                    codes[k].add(code)
        r = {"case": name, "state": state, "py": summary(times["py"]), "rust": summary(times["rust"]),
             "codes": {k: sorted(v) for k, v in codes.items()}}
        r["ratio"] = r["py"]["median"] / r["rust"]["median"]
        results.append(r)
        print(f"{name}: py {r['py']['median']:.1f} ms, rust {r['rust']['median']:.1f} ms, "
              f"{r['ratio']:.1f}x  (exit {r['codes']})", file=sys.stderr, flush=True)
    print("| command | Python median (p25–p75) | Rust median (p25–p75) | Python / Rust |")
    print("|---|---|---|---|")
    for r in results:
        p, m = r["py"], r["rust"]
        print(f"| `{r['case']}` | {p['median']:.1f} ms ({p['p25']:.1f}–{p['p75']:.1f}) | "
              f"{m['median']:.1f} ms ({m['p25']:.1f}–{m['p75']:.1f}) | {r['ratio']:.1f}× |")
    if a.json:
        Path(a.json).write_text(json.dumps(results, indent=2) + "\n")


if __name__ == "__main__":
    main()
