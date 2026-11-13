"""Answering a task's prompt from the board, and replying to what the coordinator asked."""

import hashlib
import json
import re

from .core import Fail, REX, Repo, clip, command, ok, repo_root, run, set_status
from .rex import rex_alive, screen, screen_end
from .agents import pane, running, waits_on
from .coordinators import sees_to
from .news import news


# A task whose agent is at a prompt can be answered from the board: `prompt` reads the
# question and its options off the pane's screen, and `answer` presses one of them,
# once it has read the screen again and found the same question there. A task the
# coordinator asked the person about (`ask`) is answered with `reply`: the answer goes
# to the coordinator, never to the worker.

# Claude Code's menus: "❯ 1. Yes", "  2. No" (the ❯ marks the chosen one). Its old
# boxed layout puts a │ at each edge.
OPTION_RE = re.compile(r"^(\s*)(❯|›|>)?\s*(\d{1,2})\.\s+(\S.*)$")
RULE_RE = re.compile(r"^\s*[╭╰]?[─━╌┄-]{3,}[╮╯]?\s*$")
YES_NO_RE = re.compile(r"[(\[]\s*y\s*/\s*n\s*[)\]]", re.I)
HINT_LINES = 3  # lines a menu can have below it: "Esc to cancel · Tab to amend", a rule


def unboxed(line):
    return re.sub(r"^\s*│|│\s*$", "", line).rstrip()


def prompt_id(lines):
    """A short ID for a prompt from its lines: the same prompt on screen again, not
    just the same question ("Do you want to proceed?" asks about every command)."""
    text = "\n".join(l.strip() for l in lines if l.strip() and not RULE_RE.match(l))
    return hashlib.sha1(text.encode()).hexdigest()[:12]


PROMPT_CONTEXT = 8  # at most this many lines above a question are its prompt's: the command it asks about
TOP_RULE_RE = re.compile(r"^\s*╭?─{3,}╮?\s*$")  # a prompt's top edge (a dashed ╌ rule is inside it)


def prompt_top(lines, i):
    """Where the prompt whose question is line i starts: below its top edge."""
    top = max(0, i - PROMPT_CONTEXT)
    for j in range(i - 1, top - 1, -1):
        if TOP_RULE_RE.match(lines[j]):
            return j + 1
    return top


def read_prompt(text):
    """The question Claude Code's screen ends with, as {"question", "options": [[key,
    label], ...], "id"}, or None when it doesn't end with one. A numbered menu with its
    ❯ on an option is answered by its number; a "(y/n)" question by y or n. A menu's
    "Type something." is left out: it wants typing in the pane. id: prompt_id of the
    question, what's above it and its options."""
    lines = [unboxed(l) for l in text.splitlines()]
    while lines and not lines[-1].strip():
        lines.pop()
    filled = [i for i, l in enumerate(lines) if l.strip() and not RULE_RE.match(l)]
    for i in reversed(filled[-HINT_LINES:]):
        if YES_NO_RE.search(lines[i]):
            return {"question": lines[i].strip(), "options": [["y", "Yes"], ["n", "No"]],
                    "id": prompt_id(lines[prompt_top(lines, i):i + 1])}
    # the last option, with at most a few hint lines under it, and no input box
    last = None
    for i in reversed(filled):
        if OPTION_RE.match(lines[i]):
            last = i
            break
        if lines[i].lstrip().startswith(("❯", "›")):
            return None  # the agent's own input box: it isn't asking
        if len(filled) - filled.index(i) > HINT_LINES:
            return None
    if last is None:
        return None
    options, marked, question, top = [], False, None, 0
    indent = len(OPTION_RE.match(lines[last]).group(1))
    for i in range(last, -1, -1):
        line = lines[i]
        m = OPTION_RE.match(line)
        if m:
            options.append((m.group(3), m.group(4).strip()))
            marked = marked or bool(m.group(2))
            indent = min(indent, len(m.group(1)))
        elif not line.strip() or RULE_RE.match(line) or len(line) - len(line.lstrip()) > indent + 1:
            continue  # a gap, a rule, or the option above's description or wrapped label
        else:
            question, top = line.strip(), prompt_top(lines, i)
            break
    options.reverse()
    if not marked or [k for k, _ in options] != [str(n) for n in range(1, len(options) + 1)]:
        return None  # a numbered list in what the agent wrote, not a menu
    options = [[k, label] for k, label in options if not label.startswith("Type something")]
    return {"question": question or "", "options": options, "id": prompt_id(lines[top:last + 1])}


def prompt_pane(t):
    """(who, session, block) of the agent the task is stopped on at a prompt. Fails
    when it isn't at one, or its pane is gone."""
    s = t.get("stop") or {}
    if t["status"] != "needs" or s.get("kind") != "prompt":
        raise Fail(f"{t['id']} isn't waiting at a prompt")
    who = waits_on(t)
    session, block = pane(t, who)
    if not rex_alive(session, block):
        raise Fail(f"{t['id']}'s {who} isn't running")
    return who, session, block


def cmd_prompt(args):
    """The prompt a task's agent is at, with the end of its screen (in colour), as JSON
    for the board. "question" is null when the screen shows none now."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    who, session, block = prompt_pane(t)
    p = read_prompt(screen(session, block)) or {"question": None, "options": [], "id": None}
    print(json.dumps({"who": who, **p, "screen": screen_end(session, block, args.lines, vt=True)}))


def cmd_answer(args):
    """Press one of the options of the prompt a task's agent is at, as the person would
    in its pane. Refuses unless its screen still shows that prompt (--prompt, its id
    from `prompt`)."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    who, session, block = prompt_pane(t)
    p = read_prompt(screen(session, block))
    if not p or (args.prompt is not None and p["id"] != args.prompt):
        raise Fail(f"{t['id']}'s {who} isn't at that prompt any more: nothing was sent")
    label = dict(map(tuple, p["options"])).get(args.key.lower())
    if label is None:
        keys = ", ".join(k for k, _ in p["options"])
        raise Fail(f"{args.key} isn't one of the prompt's options ({keys}): nothing was sent")
    run([REX, "send", *(["-s", session] if session else []), "-b", block, args.key.lower()])
    print(f"answered {t['id']}'s {who}: {args.key.lower()} ({label})")


def cmd_reply(args):
    """The person's answer to what the coordinator asked about a task (`ask`). It goes
    to the task's coordinator as "BP-3: <answer>": typed into its pane, or as news with
    config --news monitor. A task asked about while it wasn't stopped goes back to the
    status it had then. A stopped one leaves needs as `ok` would, or goes to its
    coordinator when its agent isn't running."""
    repo = Repo(repo_root())
    t = repo.task(args.id)
    if t["status"] != "needs" or not t.get("ask"):
        raise Fail(f"{t['id']} has no question from its coordinator to answer")
    text = " ".join(" ".join(args.answer).split())
    if not text:
        raise Fail("give the answer")
    if not sees_to(repo, t):
        raise Fail(f"no coordinator is running to take the answer: `bullpen tell {t['id']}` "
                   f"tells its worker instead")
    question = t.get("why") or ""
    if t.get("asked_from"):
        was = t["asked_from"]
        set_status(t, was["status"], was.get("why"))  # it wasn't stopped: back to what it was doing
    elif running(t):
        ok(t)
    else:
        set_status(t, "triage", clip(f"The person answered: {text}", 160))
    repo.save_task(t)
    news(repo, t, "answer", f"{t['id']}: the person answers \"{clip(question, 120)}\": {text}",
         typed=f"{t['id']}: {text}")
    print(f"sent {t['id']}'s answer to its coordinator")


def commands(sub):
    """This module's commands, for bullpen's parser."""
    p = command(sub, "answer", cmd_answer, "You", help="press an option of the prompt a task's agent is at")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("key", help="the option: its number, or y or n")
    p.add_argument("--prompt", metavar="ID", help="refuse unless it is still this prompt (its id from `prompt`)")

    p = command(sub, "reply", cmd_reply, "You",
                help="answer what the coordinator asked about a task: it goes to the coordinator")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("answer", nargs="+", help="the answer")

    p = command(sub, "prompt", cmd_prompt, "Coordinator",
                help="the prompt a task's agent is at, and its screen, as JSON (for the board)")
    p.add_argument("id", help="the task, like BP-3")
    p.add_argument("--lines", type=int, default=40, help="how many lines of screen (default 40)")
