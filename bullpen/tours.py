"""Guided review tours: writing a tour of a ready task's change, and the person's
comments on its steps. The stepper (stepper.py) reads them on the board.
"""

import json
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

from .core import (
    Fail, PROMPTS, Repo, SELF, clip, command, git, head, is_claude, is_status, merge_base,
    model_in, now, read_json, repo_root, resolve, with_model, write_json,
)
from .jobs import background
from .modes import task_base_ref
from .news import tell_coordinator
from .stops import stop
from .start import hand_back
from .review import close_reviewer, patch_id


# On request (`bullpen tour BP-3`, the board's g), a cheap headless agent writes a
# tour of a ready or in-review task's change: its steps in reading order, each a
# title, a note and the hunks it covers (file and line ranges). The agent only reads:
# it prints the tour as JSON and bullpen writes the file (briefs/<ID>.tour.json),
# adding a last step, "Other changes", for any hunk no step covers. Code is always
# shown from git, never from the agent's text. A tour is tied to the commit it was
# written on: it stays current while the patch is the same (as approvals do), and is
# stale otherwise. The person's comments on its steps are drafts
# (briefs/<ID>.tour-comments.json) until `tour --send` sends them to the worker as a
# findings-style file; its next ready gets a review of them that doesn't count toward
# review_rounds. t["tour"] says whether a tour is being written ("writing", with the
# pid writing it), was written, or failed (with why).

TOUR_TOOLS = ["Read", "Grep", "Glob", "Bash(git diff:*)", "Bash(git log:*)", "Bash(git show:*)",
              "Bash(git status:*)", "Bash(git blame:*)", "Bash(git ls-files:*)"]
TOUR_TIMEOUT = 20 * 60  # seconds before a tour agent counts as failed
MAX_TOUR_DIFF_LINES = 3000  # longer, and the agent reads the diff with git itself
TOUR_ATTEMPTS = 2  # an answer that isn't a tour is asked for once more
# the answer's shape, for claude's structured output (--json-schema); tour_steps checks the rest
TOUR_SCHEMA = {"type": "object", "required": ["steps"], "properties": {"steps": {
    "type": "array", "minItems": 1, "items": {
        "type": "object", "required": ["title", "note", "hunks"], "properties": {
            "title": {"type": "string"}, "note": {"type": "string"}, "hunks": {
                "type": "array", "items": {
                    "type": "object", "required": ["file"], "properties": {
                        "file": {"type": "string"},
                        "lines": {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2}}}}}}}}}
OTHER_CHANGES = "Other changes"


def tour_path(repo, t):
    return repo.dir / "briefs" / f"{t['id']}.tour.json"


def tour_log(repo, t):
    return repo.dir / "briefs" / f"{t['id']}.tour.log"


def comments_path(repo, t):
    return repo.dir / "briefs" / f"{t['id']}.tour-comments.json"


HUNK_RE = re.compile(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def git_path(line, prefix):
    """The path in a `--- a/x` or `+++ b/x` line, or None for /dev/null."""
    p = line[4:].rstrip("\t")
    return p[len(prefix):] if p.startswith(prefix) else None


def diff_files(wt, base, commit):
    """The diff from base to commit as a list of files, each {"file", "head": its header
    lines, "hunks": [{"old": (start, count), "new": (start, count), "text"}], "deleted"}.
    A binary file has a header and no hunks."""
    text = git(wt, "-c", "core.quotePath=false", "diff", "--no-color", "--no-ext-diff",
               "--src-prefix=a/", "--dst-prefix=b/", base, commit).stdout
    files = []
    for line in text.split("\n"):
        if line.startswith("diff --git "):
            s = line[len("diff --git a/"):]
            half = (len(s) - 3) // 2
            files.append({"file": s[:half] if s[half:half + 3] == " b/" else s, "head": [line],
                          "hunks": [], "deleted": False})
            continue
        if not files:
            continue
        f = files[-1]
        m = HUNK_RE.match(line)
        if m:
            o, oc, n, nc = m.groups()
            f["hunks"].append({"old": (int(o), int(1 if oc is None else oc)),
                               "new": (int(n), int(1 if nc is None else nc)), "text": [line]})
        elif f["hunks"]:
            f["hunks"][-1]["text"].append(line)
        else:
            f["head"].append(line)
            if line.startswith("+++ "):
                f["file"] = git_path(line, "b/") or f["file"]
            elif line.startswith("rename to "):
                f["file"] = line[len("rename to "):]
            elif line.startswith("deleted file"):
                f["deleted"] = True
    for f in files:
        for h in f["hunks"]:
            while h["text"] and h["text"][-1] == "":
                h["text"].pop()  # the diff's last newline
            h["text"] = "\n".join(h["text"])
    return files


def span(start_count):
    start, count = start_count
    start = max(start, 1)
    return start, start + max(count, 1) - 1


def lines_of(f, h):
    """The lines a hunk covers, as the tour names them: in the new file, or in the old
    one for a deleted file."""
    return list(span(h["old"] if f["deleted"] else h["new"]))


def covers(ref, f, h):
    """Does a tour's hunk reference {"file", "lines"} cover hunk h of file f?"""
    if ref["file"] != f["file"]:
        return False
    if not ref.get("lines"):
        return True  # the whole file
    a, b = ref["lines"]
    sides = [h["new"]] if not f["deleted"] else []
    if f["deleted"] or h["new"][1] == 0:
        sides.append(h["old"])  # lines it only removes are named by their old numbers
    return any(a <= hi and lo <= b for lo, hi in map(span, sides))


def ref_lines(v):
    """A hunk reference's lines, as [start, end]: from [a, b], [a], a or "a-b". None for
    the whole file. Raises ValueError for anything else."""
    if v is None:
        return None
    if isinstance(v, str):
        v = [int(x) for x in v.split("-", 1)]
    if isinstance(v, int) and not isinstance(v, bool):
        v = [v]
    if not isinstance(v, list) or not 1 <= len(v) <= 2 or not all(type(x) is int for x in v):
        raise ValueError(f"lines must be [start, end], not {json.dumps(v)}")
    a, b = v[0], v[-1]
    return [min(a, b), max(a, b)]


def json_objects(text):
    """Every JSON object in text, in order, outermost ones only: an agent may wrap its
    answer in a code fence or a sentence, or follow it with a corrected one."""
    dec, out, i = json.JSONDecoder(), [], text.find("{")
    while i >= 0:
        try:
            obj, end = dec.raw_decode(text, i)
        except ValueError:
            i = text.find("{", i + 1)
            continue
        out.append(obj)
        i = text.find("{", end)
    return out


def parse_tour(answer, files):
    """The checked steps of an agent's answer (claude's structured output, or text):
    the text as JSON, else the last JSON object in it that is a tour. Raises ValueError
    for an answer with no tour. Returns (steps, references dropped, a note on any text
    around the tour, else None)."""
    if isinstance(answer, dict):
        return (*tour_format(answer, files), None)
    text = answer.strip()
    try:
        data = json.loads(text)
    except ValueError:
        pass
    else:
        return (*tour_format(data, files), None)
    objs = json_objects(text)
    if not objs:
        raise ValueError("its tour doesn't parse (no JSON object in its output)")
    first = None
    for obj in reversed(objs):
        try:
            steps, dropped = tour_format(obj, files)
        except ValueError as e:
            first = first or e
            continue
        return steps, dropped, (f"its answer had text around the tour ({len(objs)} JSON objects): "
                                "the last one that is a tour was used")
    raise first


def tour_format(data, files):
    """tour_steps, with its error saying the tour isn't in the format."""
    try:
        return tour_steps(data, files)
    except ValueError as e:
        raise ValueError(f"its tour isn't in the format: {e}")


def tour_steps(data, files):
    """The steps of an agent's tour, checked, with only the hunk references that match
    the diff, then "Other changes" for each hunk no step covers. Raises ValueError for
    a tour that isn't the format. Returns (steps, references dropped)."""
    steps = data.get("steps") if isinstance(data, dict) else None
    if not isinstance(steps, list) or not steps:
        raise ValueError("it has no list of steps")
    out, dropped, covered = [], [], set()
    for i, s in enumerate(steps, 1):
        if not isinstance(s, dict) or not isinstance(s.get("title"), str) or not s["title"].strip():
            raise ValueError(f"step {i} has no title")
        if not isinstance(s.get("note", ""), str) or not isinstance(s.get("hunks", []), list):
            raise ValueError(f"step {i}'s note must be text and its hunks a list")
        refs = []
        for r in s.get("hunks", []):
            if not isinstance(r, dict) or not isinstance(r.get("file"), str):
                raise ValueError(f"step {i} has a hunk with no file")
            try:
                ref = {"file": r["file"], "lines": ref_lines(r.get("lines"))}
            except ValueError as e:
                raise ValueError(f"step {i}: {e}")
            hit = {(fi, hi) for fi, f in enumerate(files) for hi, h in enumerate(f["hunks"] or [None])
                   if (covers(ref, f, h) if h else ref["file"] == f["file"])}
            if not hit:
                dropped.append(f"step {i}: {r['file']} {ref['lines'] or ''}".rstrip())
                continue
            covered |= hit
            refs.append(ref if ref["lines"] else {"file": ref["file"]})
        out.append({"title": " ".join(s["title"].split()), "note": s.get("note", "").strip(), "hunks": refs})
    rest = []
    for fi, f in enumerate(files):
        if not f["hunks"]:
            if (fi, 0) not in covered:
                rest.append({"file": f["file"]})
            continue
        rest += [{"file": f["file"], "lines": lines_of(f, h)} for hi, h in enumerate(f["hunks"])
                 if (fi, hi) not in covered]
    if rest:
        out.append({"title": OTHER_CHANGES, "note": "The changes no step above covers.", "hunks": rest})
    return out, dropped


def tour_inputs(repo, t, base_ref, files):
    """What the tour agent is given (PRD 7): the brief, the worker's summary, the review
    findings, the diff (or how to get it) and its hunks."""
    briefs = repo.dir / "briefs"
    brief = briefs / f"{t['id']}.md"
    findings = sorted(briefs.glob(f"{t['id']}.review-*.findings.md"),
                      key=lambda p: int(re.search(r"review-(\d+)", p.name).group(1)))
    found = "\n\n".join(f"### {p.name}\n\n{p.read_text().strip()}" for p in findings) or "(none)"
    wt = t["worktree"]
    diff = git(wt, "diff", "--no-color", f"{base_ref}...HEAD").stdout.rstrip("\n")
    if len(diff.split("\n")) > MAX_TOUR_DIFF_LINES:
        diff = f"({len(diff.splitlines())} lines, too long to include: run the command)"
    hunks = "\n".join(f"- {f['file']}" + (" (binary)" if not f["hunks"] else
                                          ": " + ", ".join("{}-{}".format(*lines_of(f, h)) for h in f["hunks"]))
                      for f in files)
    return dict(id=t["id"], title=t["title"], worktree=wt, base_ref=base_ref,
                brief=brief.read_text().strip() if brief.exists() else "(none)",
                summary=t.get("summary") or "(none)", findings=found, diff=diff, hunks=hunks)


def tour_command(repo):
    """The tour agent's argv: headless, on tour_model, with read-only tools."""
    cmd = with_model(repo.cfg["tour"], repo.cfg["tour_model"])
    if not is_claude(cmd):
        return resolve(cmd)
    return resolve(cmd) + ["-p", "--output-format", "json", "--no-session-persistence",
                           "--permission-mode", "dontAsk", "--tools", "Read,Grep,Glob,Bash",
                           "--allowedTools", *TOUR_TOOLS,
                           "--json-schema", json.dumps(TOUR_SCHEMA, separators=(",", ":"))]


def tour_answer(out):
    """A tour agent's answer: claude's structured output (a dict) when it has one, else
    its text. Raises ValueError for an agent error."""
    try:
        r = json.loads(out)
    except ValueError:
        r = None
    if isinstance(r, dict) and r.get("type") == "result" and isinstance(r.get("structured_output"), dict):
        agent_result(out)  # an error is still an error
        return r["structured_output"]
    return agent_result(out)


def agent_result(out):
    """The text of a headless claude's answer: its --output-format json result, or the
    output as it is from another agent. Raises ValueError for an agent error."""
    try:
        r = json.loads(out)
    except ValueError:
        return out
    if isinstance(r, dict) and r.get("type") == "result":
        if r.get("is_error"):
            raise ValueError(f"the agent failed: {clip(str(r.get('result') or r.get('subtype')), 160)}")
        return r.get("result") or ""
    return out


def write_tour(repo, tid, on_written=None):
    """Run the tour agent on the task's code now and write its tour. Returns the tour
    file, or raises Fail with why it failed (the task records that). If the person
    asked on the board to open it once written, on_written(repo, t, opener) does."""
    t = repo.task(tid)
    wt = t.get("worktree")
    log = tour_log(repo, t)
    try:
        if not wt or not Path(wt).is_dir():
            raise ValueError("the task has no worktree")
        base_ref = task_base_ref(repo, t)
        commit, base = head(wt), merge_base(wt, base_ref)
        files = diff_files(wt, base, commit)
        if not files:
            raise ValueError(f"it has no changes against {base_ref}")
        prompt = (PROMPTS / "tour.md").read_text().format(**tour_inputs(repo, t, base_ref, files))
        argv = tour_command(repo)
        for attempt in range(1, TOUR_ATTEMPTS + 1):
            answer = tour_answer(run_tour_agent(argv, prompt, wt, log, attempt))
            try:
                steps, dropped, extra = parse_tour(answer, files)
                break
            except ValueError as e:
                if attempt == TOUR_ATTEMPTS:
                    raise
                with log.open("a") as f:
                    f.write(f"{now()} attempt {attempt}: {e}; asking again\n\n")
        with log.open("a") as f:
            if extra:
                f.write(extra + "\n")
            if dropped:
                f.write("hunks that match nothing in the diff, left out: " + "; ".join(dropped) + "\n")
    except ValueError as e:
        tour_failed(repo, tid, str(e))
        raise Fail(f"{tid}'s tour failed: {e} (log: {log})")
    path = tour_path(repo, t)
    write_json(path, {"task": t["id"], "commit": commit, "base": base, "base_ref": base_ref,
                      "patch": patch_id(wt, base_ref), "written": now(),
                      "model": model_in(tour_command(repo)), "steps": steps})
    t = repo.task(tid)  # read again: it may have changed while the agent ran
    opener = (t.get("tour") or {}).get("open")
    t["tour"] = {"state": "written", "at": now(), "commit": commit}
    if (t.get("note") or "").startswith("tour failed"):
        t["note"] = None
    write_json(repo.task_path(tid), t)  # not save_task: the status didn't change
    if opener and on_written:
        on_written(repo, t, opener)
    return path


def run_tour_agent(argv, prompt, wt, log, attempt):
    """Run the tour agent once, its output in the log (a new log on the first attempt).
    Returns its stdout, or raises ValueError if it didn't finish."""
    with log.open("w" if attempt == 1 else "a") as f:
        f.write(f"{now()} attempt {attempt}: {shlex.join(argv)}\n(the prompt on stdin: {len(prompt)} characters)\n\n")
    try:
        p = subprocess.run(argv, cwd=wt, input=prompt, text=True, capture_output=True,
                           timeout=TOUR_TIMEOUT)
    except FileNotFoundError:
        raise ValueError(f"{argv[0]} isn't installed")
    except subprocess.TimeoutExpired:
        raise ValueError(f"the agent took over {TOUR_TIMEOUT // 60} minutes")
    with log.open("a") as f:
        f.write(f"exit {p.returncode}\n--- stdout\n{p.stdout}\n--- stderr\n{p.stderr}\n")
    if p.returncode != 0:
        last = (p.stderr or p.stdout).strip().splitlines()
        raise ValueError(f"the agent exited with {p.returncode}: {clip(last[-1] if last else 'no output', 120)}")
    return p.stdout


def tour_failed(repo, tid, why):
    """The tour wasn't written: a line on the board and in the log. No news, no triage."""
    t = repo.task(tid)
    t["tour"] = {"state": "failed", "at": now(), "why": why}
    t["note"] = f"tour failed: {why}"
    write_json(repo.task_path(tid), t)
    with tour_log(repo, t).open("a") as f:
        f.write(f"{now()} failed: {why}\n")


def pid_alive(pid):
    try:
        os.kill(pid, 0)
    except (OSError, TypeError):
        return False
    return True


def tour_state(repo, t):
    """(state, tour, why): "writing", "failed" (why), "current", "stale" (why) or "none".
    A tour being written whose writer is gone has failed."""
    run_ = t.get("tour") or {}
    if run_.get("state") == "writing":
        if pid_alive(run_.get("pid")):
            return "writing", None, None
        tour_failed(repo, t["id"], "its writer stopped before the tour was written")
        t.update(repo.task(t["id"]))
        run_ = t["tour"]
    if run_.get("state") == "failed":
        return "failed", read_json(tour_path(repo, t)), run_.get("why")
    tour = read_json(tour_path(repo, t))
    if not tour:
        return "none", None, None
    stale = tour_stale(repo, t, tour)
    return ("stale", tour, stale) if stale else ("current", tour, None)


def tour_stale(repo, t, tour):
    """Why the tour no longer shows the task's code, or None while it does: the same
    commit, or the same patch (after a clean rebase, as approvals carry over)."""
    wt = t.get("worktree")
    if not wt or not Path(wt).is_dir():
        return None  # nothing to compare with: the tour is all there is
    if head(wt) == tour["commit"] or patch_id(wt, task_base_ref(repo, t)) == tour.get("patch"):
        return None
    return f"the code changed since the tour was written (on {tour['commit'][:12]})"


def has_commit(wt, c):
    return bool(c) and git(wt, "cat-file", "-e", f"{c}^{{commit}}", check=False).returncode == 0


def tour_diff(repo, t, tour):
    """The diff the tour was written on, as diff_files: from its own commits, else (they're
    gone after a rebase) from the task's code now."""
    wt = t["worktree"]
    if has_commit(wt, tour["commit"]) and has_commit(wt, tour.get("base")):
        return diff_files(wt, tour["base"], tour["commit"])
    base_ref = task_base_ref(repo, t)
    return diff_files(wt, merge_base(wt, base_ref), head(wt))


def step_diff(files, step):
    """A step's hunks from git, as a patch: each file the step names, in the step's order,
    with its header and the hunks the step covers."""
    out = []
    names = list(dict.fromkeys(r["file"] for r in step["hunks"]))
    for name in names:
        for f in files:
            if f["file"] != name:
                continue
            refs = [r for r in step["hunks"] if r["file"] == name]
            hunks = [h["text"] for h in f["hunks"] if any(covers(r, f, h) for r in refs)]
            if hunks or not f["hunks"]:
                out.append("\n".join(f["head"] + hunks))
    return "\n".join(out) + ("\n" if out else "")


def step_files(step):
    """ "bin/bullpen 120-140, README.md": a step's files and lines, for people."""
    return ", ".join(r["file"] + (" {}-{}".format(*r["lines"]) if r.get("lines") else "")
                     for r in step["hunks"]) or "(no code)"


def tour_status(t):
    if t["status"] not in ("ready", "review"):
        raise Fail(f"{is_status(t)}: a tour is only for a task that is "
                   f"ready or in review, whose code isn't changing")


def start_tour(repo, t, wait, opener=None, on_written=None):
    """Write a tour of the task: now (wait), or by a process of its own in the background.
    t["tour"] says it's being written, with that process's pid, so the board can show it.
    With opener ({"session", "from"}), the stepper opens there once it's written."""
    if not t.get("worktree") or not Path(t["worktree"]).is_dir():
        raise Fail(f"{t['id']} has no worktree, so there's nothing to tour")
    t["tour"] = {"state": "writing", "at": now(), "pid": os.getpid(), **({"open": opener} if opener else {})}
    if (t.get("note") or "").startswith("tour failed"):
        t["note"] = None
    write_json(repo.task_path(t["id"]), t)
    if wait:
        return write_tour(repo, t["id"], on_written)
    t["tour"]["pid"] = background([SELF, "_tour", t["id"]], repo.root, repo.dir / "jobs.log",
                                  f"Writing {t['id']}'s tour", t["id"])
    write_json(repo.task_path(t["id"]), t)
    return None


def show_tour(repo, t, args):
    """The tour as it is, for the board: its state and steps (--json), or one step's
    hunks from git (--step N), as a patch for delta or a pager."""
    state, tour, why = tour_state(repo, t)
    if args.step is None:
        drafts = read_comments(repo, t)["comments"]
        out = {"id": t["id"], "state": state, "why": why, "path": str(tour_path(repo, t)) if tour else None,
               "commit": tour and tour["commit"], "written": tour and tour.get("written"),
               "comments": len(drafts),
               "steps": [{"n": i, "title": s["title"], "note": s["note"], "hunks": s["hunks"],
                          "files": step_files(s), "comments": sum(c["step"] == i for c in drafts)}
                         for i, s in enumerate(tour["steps"], 1)] if tour else []}
        if args.json:
            print(json.dumps(out, indent=2))
            return
        print(f"{t['id']}'s tour: {state}" + (f" ({why})" if why else ""))
        for s in out["steps"]:
            print(f"{s['n']:>3}. {s['title']}  [{s['files']}]" +
                  (f"  ({s['comments']} comment{'s' if s['comments'] != 1 else ''})" if s["comments"] else ""))
        return
    if not tour:
        raise Fail(f"{t['id']} has no tour: `bullpen tour {t['id']}` writes one")
    if not 1 <= args.step <= len(tour["steps"]):
        raise Fail(f"{t['id']}'s tour has steps 1 to {len(tour['steps'])}")
    step = tour["steps"][args.step - 1]
    diff = step_diff(tour_diff(repo, t, tour), step) if t.get("worktree") and Path(t["worktree"]).is_dir() else ""
    if args.json:
        print(json.dumps({"n": args.step, "of": len(tour["steps"]), "title": step["title"],
                          "note": step["note"], "hunks": step["hunks"], "files": step_files(step),
                          "diff": diff}, indent=2))
        return
    sys.stdout.write(diff)


def read_comments(repo, t):
    return read_json(comments_path(repo, t)) or {"next": 1, "comments": []}


def cmd_tour_comment(args):
    """Draft comments on a tour's steps: add one, list them, or remove one."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    c = read_comments(repo, t)
    if args.remove is not None:
        left = [x for x in c["comments"] if x["id"] != args.remove]
        if len(left) == len(c["comments"]):
            raise Fail(f"{t['id']} has no draft comment {args.remove}")
        c["comments"] = left
        write_json(comments_path(repo, t), c)
        print(f"removed comment {args.remove} from {t['id']}")
        return
    if args.step is None:
        if args.json:
            print(json.dumps(c["comments"], indent=2))
            return
        for x in c["comments"]:
            print(f"{x['id']:>3}. step {x['step']} ({x['title']}): {x['text']}")
        if not c["comments"]:
            print(f"{t['id']} has no draft comments")
        return
    text = " ".join(args.text or []).strip()
    if not text:
        raise Fail("say what the comment is")
    print(add_comment(repo, t, args.step, text))


def add_comment(repo, t, step, text):
    """Add a draft comment on step N of the task's tour; returns its number."""
    tour = read_json(tour_path(repo, t))
    if not tour:
        raise Fail(f"{t['id']} has no tour to comment on: `bullpen tour {t['id']}` writes one")
    if not 1 <= step <= len(tour["steps"]):
        raise Fail(f"{t['id']}'s tour has steps 1 to {len(tour['steps'])}")
    c = read_comments(repo, t)
    s = tour["steps"][step - 1]
    # the step as it was: a new tour may number its steps differently
    c["comments"].append({"id": c["next"], "step": step, "title": s["title"], "hunks": s["hunks"],
                          "commit": tour["commit"], "text": text, "at": now()})
    c["next"] += 1
    write_json(comments_path(repo, t), c)
    return c["next"] - 1


def comments_text(t, comments, k):
    """The findings-style file of the person's comments: each one with its step, files and lines."""
    parts = [f"# The person's comments on {t['id']}, after a guided tour of the change\n\n"
             f"Each comment is on one step of the tour: the step's title, the files and lines it covers "
             f"(in the code the tour was written on), then the comment. A line the person points at "
             f"is in the comment's text."]
    for i, c in enumerate(comments, 1):
        parts.append(f"## Comment {i}: step {c['step']}, {c['title']}\n\n"
                     f"Files: {step_files(c)} (commit {c['commit'][:12]})\n\n{c['text']}")
    return "\n\n".join(parts) + "\n"


def send_comments(repo, t):
    """Send the person's draft comments to the worker, as a findings-style file: the task
    goes back to working, and its next ready gets a review of them."""
    tour_status(t)
    drafts = read_comments(repo, t)["comments"]
    if not drafts:
        raise Fail(f"{t['id']} has no draft comments to send: `bullpen tour-comment {t['id']} STEP TEXT` adds one")
    k = (t.get("tours_sent") or 0) + 1
    path = repo.dir / "briefs" / f"{t['id']}.tour-{k}.comments.md"
    path.write_text(comments_text(t, drafts, k))
    wt = t["worktree"]
    base_ref = task_base_ref(repo, t)
    n = len(drafts)
    said = f"{n} comment{'s' if n != 1 else ''}"
    t["tours_sent"] = k
    t["tour_sent"] = {"n": k, "file": str(path), "count": n, "at": now(),
                      "commit": head(wt), "base": merge_base(wt, base_ref)}
    if t["status"] == "review":
        close_reviewer(t)  # it reviews code the person has just sent back
    msg = (f"The person went through your change with a guided tour and sent back {said}. Read {path}, "
           f"address each comment, commit, and run your ready command again. If you disagree with a "
           f"comment, say why in your ready summary.")
    try:
        failed = hand_back(repo, t, msg)
    except Fail as e:
        failed = clip(str(e), 160)
    comments_path(repo, t).unlink(missing_ok=True)  # sent: in the file now, no longer drafts
    if failed:
        stop(repo, t, "worker", "not_sent", "The person's tour comments didn't reach its worker",
             f"{t['id']} was sent back by the person after the tour ({said}), but the message didn't "
             f"reach its worker ({failed}). The comments are in {path}: `bullpen tell {t['id']}` it "
             f"to read them", "not-sent")
        print(f"{said} written to {path}, but they didn't reach {t['id']}'s worker: its coordinator is told")
        return
    tell_coordinator(repo, t, "tour-sent", f"{t['id']} sent back by the person after the tour: {said}")
    print(f"sent {said} to {t['id']}'s worker ({path})")


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "tour-comment", cmd_tour_comment, "You",
                help="draft comments on a tour's steps: add, list (no STEP) or remove")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("step", nargs="?", type=int, help="the step the comment is on")
    p.add_argument("text", nargs="*", help="the comment (mention a line in it to point at one)")
    p.add_argument("--remove", type=int, metavar="N", help="remove draft comment N")
    p.add_argument("--json", action="store_true", help="list them as JSON")
