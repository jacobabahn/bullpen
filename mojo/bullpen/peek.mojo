"""`peek`: the end of a worker's screen (bullpen/start.py cmd_peek, rex.screen_end)."""

from bullpen.args import Args
from bullpen.core import Repo, repo_root, rex_program, run_cmd
from bullpen.rex import rex_alive
from bullpen.textutil import py_strip


def _digits_at(s: String, i: Int) -> Int:
    """The end of the run of ASCII digits starting at byte i."""
    var b = s.as_bytes()
    var j = i
    while j < len(b) and Int(b[j]) >= ord("0") and Int(b[j]) <= ord("9"):
        j += 1
    return j


def cursor_forward_as_spaces(s: String) raises -> String:
    """re.sub(r"\\x1b\\[(\\d*)C", <that many spaces, 1 if none>, s)."""
    var out = String()
    var b = s.as_bytes()
    var i = 0
    var start = 0
    while i < len(b):
        if Int(b[i]) == 0x1B and i + 1 < len(b) and Int(b[i + 1]) == ord("["):
            var j = _digits_at(s, i + 2)
            if j < len(b) and Int(b[j]) == ord("C"):
                out += s[byte=start:i]
                var n = Int(String(s[byte=i + 2:j])) if j > i + 2 else 1
                out += " " * n
                i = j + 1
                start = i
                continue
        i += 1
    out += s[byte=start:]
    return out^


def without_sgr(s: String) -> String:
    """re.sub(r"\\x1b\\[[0-9;:]*m", "", s)."""
    var out = String()
    var b = s.as_bytes()
    var i = 0
    var start = 0
    while i < len(b):
        if Int(b[i]) == 0x1B and i + 1 < len(b) and Int(b[i + 1]) == ord("["):
            var j = i + 2
            while j < len(b) and ((Int(b[j]) >= ord("0") and Int(b[j]) <= ord("9")) or Int(b[j]) == ord(";") or Int(b[j]) == ord(":")):
                j += 1
            if j < len(b) and Int(b[j]) == ord("m"):
                out += s[byte=start:i]
                i = j + 1
                start = i
                continue
        i += 1
    out += s[byte=start:]
    return out^


def screen_end(session: String, block: String, n: Int, vt: Bool = False) raises -> List[String]:
    """The last n lines of a pane's screen, without the blank ones at the end. vt keeps
    its colours, as escape codes, with a cursor-forward gap as spaces."""
    var cmd: List[String] = [rex_program(), "capture"]
    if vt:
        cmd.append("--format=vt")
    if session:
        cmd += ["-s", session]
    cmd += ["-b", block]
    var out = run_cmd(cmd).out
    if vt:
        out = cursor_forward_as_spaces(out)
    var lines = List[String]()
    for l in out.split("\n"):
        var s = String(l)
        while s.endswith("\r"):
            var cut = String(s[byte=:s.byte_length() - 1])
            s = cut^
        lines.append(s^)
    while len(lines) > 0 and py_strip(without_sgr(lines[len(lines) - 1])).byte_length() == 0:
        _ = lines.pop()
    # lines[-n:], as Python slices it (n 0: all of them; n < 0: from -n on)
    var start = len(lines) - n if n > 0 else -n
    start = max(0, min(start, len(lines)))
    var end = List[String]()
    for i in range(start, len(lines)):
        end.append(lines[i])
    return end^


def cmd_peek(args: Args) raises:
    var n: Int
    try:
        n = Int(args.opt("--lines", "40"))
    except:
        raise Error("__python__")  # not a number: argparse's own error, from Python
    var repo = Repo(repo_root())
    var t = repo.task(args.pos[0])
    if not rex_alive(t.s("session"), t.s("block")):
        raise Error(t.id() + "'s agent isn't running")
    print(String("\n").join(screen_end(t.s("session"), t.s("block"), n, args.flag("--vt"))))
