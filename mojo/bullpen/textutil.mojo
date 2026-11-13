"""Python's str methods where Mojo's differ: strip() and split() by Python's whitespace
(str.isspace, Unicode spaces included; Mojo's strip doesn't take \\xa0 or \\u3000),
and \\w for the regexes the screen readers port by hand. Whitespace, strip and
splitlines are text.mojo's; these names stay for the modules that use them."""

from bullpen.text import is_space, strip, splitlines


def py_strip(s: String) -> String:
    return strip(s)


def py_rstrip(s: String) -> String:
    return strip(s, lead=False)


def py_splitlines(s: String) -> List[String]:
    return splitlines(s)


def is_word(c: Int) -> Bool:
    """Python's \\w for one code point, for ASCII; other code points count as letters
    unless they're spaces or in the symbol and punctuation blocks Claude Code draws with
    (Python asks Unicode's categories, which Mojo has no table of)."""
    if c < 128:
        return (c >= ord("a") and c <= ord("z")) or (c >= ord("A") and c <= ord("Z")) or (
            c >= ord("0") and c <= ord("9")) or c == ord("_")
    if is_space(c):
        return False
    if c <= 0xBF or c == 0xD7 or c == 0xF7:  # Latin-1 punctuation and signs
        return False
    if c >= 0x2000 and c <= 0x2BFF:  # punctuation, arrows, maths, boxes, shapes, dingbats
        return False
    if c >= 0x3000 and c <= 0x303F:  # CJK punctuation
        return False
    if c >= 0x1F000:  # emoji and pictographs
        return False
    return True


def cps(s: String) -> List[Int]:
    var out = List[Int]()
    for cp in s.codepoints():
        out.append(Int(cp))
    return out^


def from_cps(c: List[Int], a: Int, b: Int) -> String:
    var out = String()
    for i in range(a, b):
        out += String(Codepoint(unsafe_unchecked_codepoint=UInt32(c[i])))
    return out^


def stripped_lines(text: String, last: Int) -> List[String]:
    """[l.strip() for l in text.splitlines() if l.strip()][-last:]"""
    var all = List[String]()
    for l in py_splitlines(text):
        var s = py_strip(l)
        if s.byte_length() > 0:
            all.append(s)
    var out = List[String]()
    for i in range(max(0, len(all) - last), len(all)):
        out.append(all[i])
    return out^
