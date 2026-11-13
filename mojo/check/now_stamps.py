#!/usr/bin/env python3
"""now_stamps.py START END FILE...: write each ISO stamp in the files that falls between
START and END (seconds since the epoch, give or take 2 s) as <now>, in place. A command's
own now() differs between two runs; the fixture's stamps are all well outside."""

import datetime as dt
import re
import sys

STAMP = re.compile(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?(?:[+-]\d\d:\d\d|Z)?")


def main(start, end, *files):
    lo, hi = float(start) - 2, float(end) + 2

    def sub(m):
        try:
            t = dt.datetime.fromisoformat(m.group(0))
            t = t if t.tzinfo else t.astimezone()
        except ValueError:
            return m.group(0)
        return "<now>" if lo <= t.timestamp() <= hi else m.group(0)

    for f in files:
        with open(f, encoding="utf-8", errors="surrogateescape") as fh:
            text = fh.read()
        with open(f, "w", encoding="utf-8", errors="surrogateescape") as fh:
            fh.write(STAMP.sub(sub, text))


if __name__ == "__main__":
    main(*sys.argv[1:])
