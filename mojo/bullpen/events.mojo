"""What an agent's screen says (bullpen/events.py): a background shell still running,
and compacting. Mojo has no regex module, so SHELL_RE and COMPACT_RE are matched by hand."""

from bullpen.rex import screen
from bullpen.textutil import is_word, is_space, cps, py_splitlines, stripped_lines


def _lower_ascii(c: Int) -> Int:
    return c + 32 if c >= ord("A") and c <= ord("Z") else c


def _at(c: List[Int], i: Int, word: String) -> Bool:
    """Does c have word (ASCII) at i, ignoring case?"""
    var w = word.as_bytes()
    if i + len(w) > len(c):
        return False
    for k in range(len(w)):
        if _lower_ascii(c[i + k]) != Int(w[k]):
            return False
    return True


def _word_end(c: List[Int], i: Int) -> Bool:
    """\\b after the word ending before i."""
    return i >= len(c) or not is_word(c[i])


def shell_re_search(line: String) -> Bool:
    """SHELL_RE.search: `\\b\\d+ (?:background )?shells?\\b|shells? still running`, ignoring case."""
    var c = cps(line)
    var n = len(c)
    for i in range(n):
        if _at(c, i, "shell still running") or _at(c, i, "shells still running"):
            return True
        var d = c[i]
        if d < ord("0") or d > ord("9") or (i > 0 and is_word(c[i - 1])):
            continue
        var j = i
        while j < n and c[j] >= ord("0") and c[j] <= ord("9"):
            j += 1
        if j >= n or c[j] != ord(" "):
            continue
        j += 1
        if _at(c, j, "background "):
            if _shells_at(c, j + 11):
                return True
        if _shells_at(c, j):
            return True
    return False


def _shells_at(c: List[Int], j: Int) -> Bool:
    """`shells?\\b` at j."""
    if not _at(c, j, "shell"):
        return False
    var k = j + 5
    if k < len(c) and _lower_ascii(c[k]) == ord("s") and _word_end(c, k + 1):
        return True
    return _word_end(c, k)


def shell_running(text: String) -> Bool:
    """Does Claude Code's footer say a background shell of its own still runs?"""
    var lines = py_splitlines(text)
    for i in range(max(0, len(lines) - 6), len(lines)):
        if shell_re_search(lines[i]):
            return True
    return False


def _compact_word(c: List[Int], j: Int) -> Bool:
    """(?:Compacting conversation|Running Pre-?Compact hooks) at j, case and all."""
    for w in ["Compacting conversation", "Running PreCompact hooks", "Running Pre-Compact hooks"]:
        var wc = cps(w)
        if j + len(wc) > len(c):
            continue
        var ok = True
        for k in range(len(wc)):
            if c[j + k] != wc[k]:
                ok = False
                break
        if ok:
            return True
    return False


def compact_re_match(line: String) -> Bool:
    """COMPACT_RE.match: `^\\W{0,2}\\s*(?:Compacting conversation|Running Pre-?Compact hooks)`."""
    var c = cps(line)
    for k in range(3):
        if k > len(c):
            break
        if k > 0 and is_word(c[k - 1]):
            break
        var j = k
        while j < len(c) and is_space(c[j]):
            j += 1
        if _compact_word(c, j):
            return True
    return False


def compacting(session: String, block: String, msg: String = "") raises -> Bool:
    """Is Claude Code compacting its conversation (auto-compaction)? By its status
    message, else its spinner line."""
    if _compact_word(cps(msg), 0):
        return True
    for l in stripped_lines(screen(session, block), 12):
        if compact_re_match(l):
            return True
    return False
