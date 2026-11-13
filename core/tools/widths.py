#!/usr/bin/env python3
"""Write core/src/widths.rs (src/ beside this tools/ folder): the code points Python's
unicodedata calls East Asian Wide or Full (two columns in task_line.text_width) and those
with a combining class (none), as sorted inclusive ranges. Run with the Python bullpen
runs on: `python3 core/tools/widths.py`."""

import sys
import unicodedata
from pathlib import Path


def ranges(pred):
    out, start = [], None
    for c in range(sys.maxunicode + 2):
        hit = c <= sys.maxunicode and pred(chr(c))
        if hit and start is None:
            start = c
        elif not hit and start is not None:
            out.append((start, c - 1))
            start = None
    return out


def table(name, rs):
    body = "".join(f"    (0x{a:x}, 0x{b:x}),\n" for a, b in rs)
    return f"pub const {name}: [(u32, u32); {len(rs)}] = [\n{body}];\n"


wide = ranges(lambda c: unicodedata.east_asian_width(c) in "WF")
comb = ranges(lambda c: unicodedata.combining(c) != 0)
out = Path(__file__).resolve().parent.parent / "src" / "widths.rs"
out.write_text(
    f"//! Made by tools/widths.py from Python {sys.version.split()[0]}'s unicodedata "
    f"(Unicode {unicodedata.unidata_version}): don't edit.\n\n"
    "/// East Asian Wide or Full: two columns.\n" + table("WIDE", wide) +
    "\n/// A combining class other than 0: no column.\n" + table("COMBINING", comb))
