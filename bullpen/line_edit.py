"""Typing on a terminal: the keys a read gives, and the line editor the picker and
the stepper's comment and tell boxes share.
"""

import os
import re
import select
import unicodedata


PASTE_START, PASTE_END = "\x1b[200~", "\x1b[201~"


def char_width(c):
    """The columns a character takes on screen."""
    return 0 if unicodedata.combining(c) else 2 if unicodedata.east_asian_width(c) in "WF" else 1


class LineEdit:
    """What's typed in the picker and in the tour stepper's comment and tell boxes, and
    the cursor in it (a character index). The board's tell and answer line
    (board/src/edit.rs) takes the same keys. On one line, a pasted line break is a space;
    with lines, a paste keeps its breaks and ctrl+j or alt+⏎ starts a new line."""

    LEFT, RIGHT = ("\x1b[D", "\x1bOD", "\x02"), ("\x1b[C", "\x1bOC", "\x06")  # ←, ctrl+b; →, ctrl+f
    WORD_LEFT = ("\x1bb", "\x1b[1;3D", "\x1b[1;5D")  # alt+b, alt+←, ctrl+←
    WORD_RIGHT = ("\x1bf", "\x1b[1;3C", "\x1b[1;5C")
    HOME = ("\x1b[H", "\x1bOH", "\x1b[1~", "\x1b[7~", "\x01")  # Home, ctrl+a
    END = ("\x1b[F", "\x1bOF", "\x1b[4~", "\x1b[8~", "\x05")  # End, ctrl+e
    UP, DOWN = ("\x1b[A", "\x1bOA"), ("\x1b[B", "\x1bOB")  # with lines
    NEW_LINE = ("\n", "\x1b\r")  # ctrl+j, alt+⏎ (with lines)

    def __init__(self, text="", lines=False):
        self.lines, self.text, self.at, self.left, self.top = lines, "", 0, 0, 0
        self.paste(text)

    def key(self, k):
        """Apply one editing key; False when it isn't one (⏎, esc), for the caller."""
        t, at = self.text, self.at
        if k.startswith(PASTE_START):
            self.paste(k[len(PASTE_START):].removesuffix(PASTE_END))
        elif k in self.LEFT:
            self.at = max(at - 1, 0)
        elif k in self.RIGHT:
            self.at = min(at + 1, len(t))
        elif k in self.WORD_LEFT:
            self.at = self.word_start()
        elif k in self.WORD_RIGHT:
            self.at = self.word_end()
        elif k in self.HOME:
            self.at = self.line_start()
        elif k in self.END:
            self.at = self.line_end()
        elif k in ("\x7f", "\x08"):
            self.cut(max(at - 1, 0), at)
        elif k == "\x1b[3~":  # delete
            self.cut(at, min(at + 1, len(t)))
        elif k in ("\x17", "\x1b\x7f", "\x1b\x08"):  # ctrl+w, alt+backspace
            self.cut(self.word_start(), at)
        elif k == "\x1bd":  # alt+d
            self.cut(at, self.word_end())
        elif k == "\x15":  # ctrl+u: to the start of the line
            self.cut(self.line_start(), at)
        elif k == "\x0b":  # ctrl+k: to its end
            self.cut(at, self.line_end())
        elif self.lines and k in self.NEW_LINE:
            self.insert("\n")
        elif self.lines and k in self.UP + self.DOWN:
            self.move_line(-1 if k in self.UP else 1)
        elif k.isprintable() and not k.startswith("\x1b"):
            self.insert(k)
        else:
            return False
        return True

    def paste(self, text):
        """Pasted text at the cursor: a tab is a space, a line break is one unless this
        has lines, and other control characters go."""
        text = text.replace("\r\n", "\n").replace("\r", "\n").replace("\t", " ")
        if not self.lines:
            text = text.replace("\n", " ")
        self.insert("".join(c for c in text if c == "\n" or c.isprintable()))

    def insert(self, text):
        self.text = self.text[:self.at] + text + self.text[self.at:]
        self.at += len(text)

    def cut(self, a, b):
        self.text, self.at = self.text[:a] + self.text[b:], a

    def word_start(self):
        """Where the word before the cursor starts (the spaces after it go with it)."""
        return len(re.sub(r"\S*\s*$", "", self.text[:self.at]))

    def word_end(self):
        return self.at + re.match(r"\s*\S*", self.text[self.at:]).end()

    def line_start(self):
        return self.text.rfind("\n", 0, self.at) + 1

    def line_end(self):
        n = self.text.find("\n", self.at)
        return len(self.text) if n < 0 else n

    def move_line(self, by):
        """Up or down a line, to the same column or the line's end."""
        start, col = self.line_start(), self.at - self.line_start()
        if by < 0 and start:
            prev = self.text.rfind("\n", 0, start - 1) + 1
            self.at = min(prev + col, start - 1)
        elif by > 0 and self.line_end() < len(self.text):
            nxt = self.line_end() + 1
            end = self.text.find("\n", nxt)
            self.at = min(nxt + col, len(self.text) if end < 0 else end)

    def view(self, room):
        """On one line: what's shown in room columns, and the cursor's column in it. The
        shown part moves only when the cursor would leave it, as a shell's line does."""
        w = lambda a, b: sum(char_width(c) for c in self.text[a:b])
        room = max(room, 1)  # the cursor takes a column of its own at the end
        self.left = min(self.left, self.at)
        while self.left < self.at and w(self.left, self.at) >= room:
            self.left += 1
        to = self.left
        while to < len(self.text) and w(self.left, to + 1) <= room:
            to += 1
        return self.text[self.left:to], w(self.left, self.at)

    def rows(self, width, most):
        """With lines: the text wrapped to width, at most `most` rows of it around the
        cursor, and the cursor's (row, column) in them."""
        width, out, cur, i = max(width, 2), [], (0, 0), 0
        for line in self.text.split("\n"):
            row, used = "", 0
            for c in line:
                if used + char_width(c) > width - 1:  # a column for the cursor at the end
                    out.append(row)
                    row, used = "", 0
                if i == self.at:
                    cur = (len(out), used)
                row, used, i = row + c, used + char_width(c), i + 1
            if i == self.at:
                cur = (len(out), used)
            out.append(row)
            i += 1  # the line break
        most = max(most, 1)
        self.top = min(max(self.top, cur[0] - most + 1), cur[0])
        return out[self.top:self.top + most], (cur[0] - self.top, cur[1])


def split_keys(text):
    """The keys in what the terminal sent: an escape sequence (an arrow), an alt+key, or
    a whole paste is one key."""
    out, i = [], 0
    while i < len(text):
        m = re.match(r"\x1b\[200~.*?\x1b\[201~|\x1b(\[[0-?]*[ -/]*[@-~]|O.|[^\x1b\[O])", text[i:], re.S)
        n = m.end() if m else 1
        out.append(text[i:i + n])
        i += n
    return out


def read_keys(fd, decoder):
    data = os.read(fd, 1024)
    if not data:
        return ["\x03"]  # the terminal went away
    if data == b"\x1b" and select.select([fd], [], [], 0.03)[0]:
        data += os.read(fd, 1024)  # the rest of an arrow, not esc
    text = decoder.decode(data)
    # a paste is one key however many reads it takes (unended after a second: its keys)
    while PASTE_START in text and PASTE_END not in text[text.rindex(PASTE_START):]:
        more = os.read(fd, 65536) if select.select([fd], [], [], 1)[0] else b""
        if not more:
            break
        text += decoder.decode(more)
    return split_keys(text)


# bracketed paste on and off: a paste arrives marked, so its line breaks don't press ⏎
PASTE_ON, PASTE_OFF = "\x1b[?2004h", "\x1b[?2004l"
