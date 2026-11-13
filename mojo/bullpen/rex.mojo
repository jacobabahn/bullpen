"""Rex, as the read-only commands need it (bullpen/rex.py): is a pane alive, what's on
its screen, and its program's status record."""

from bullpen.core import rex_program, run_cmd
from bullpen.json import Doc, OBJ, ARR, copy_into


def _s(session: String) -> List[String]:
    if session:
        return ["-s", session]
    return List[String]()


def rex_alive(session: String, block: String) raises -> Bool:
    if not block:
        return False
    var cmd: List[String] = [rex_program(), "block", "inspect", block]
    cmd += _s(session)
    return run_cmd(cmd, False).code == 0


def screen(session: String, block: String) raises -> String:
    """The text on a pane's screen, or "" if Rex can't say."""
    var cmd: List[String] = [rex_program(), "capture"]
    cmd += _s(session)
    cmd += ["-b", block]
    var p = run_cmd(cmd, False)
    return p.out if p.code == 0 else ""


def program_status(session: String, block: String) raises -> Doc:
    """The program's own (root) status record in the pane: a Doc whose node 0 is the
    record, or an empty Doc (no nodes) when there is none."""
    var cmd: List[String] = [rex_program(), "block", "call"]
    cmd += _s(session)
    cmd += ["-b", block, "program_status"]
    var p = run_cmd(cmd, False)
    var none = Doc()
    if p.code != 0:
        return none^
    var d: Doc
    try:
        d = Doc.parse(p.out)
    except:
        return none^
    var recs = d.get(0, "records")
    if d.kind(recs) != ARR:
        return none^
    for i in range(d.len(recs)):
        var r = d.item(recs, i)
        if d.kind(r) == OBJ and not d.truthy(r, "id"):
            var out = Doc()
            _ = copy_into(out, d, r)
            return out^
    return none^

