"""Time: bullpen's ISO stamps ("2026-11-12T20:49:27-08:00"), now, and local time.

Mojo's standard library has a clock (std.time) but no calendar, time zones or
datetime, so stamps are parsed here, and local time comes from libc's localtime_r."""

from std.ffi import external_call, c_int


def days_from_civil(y0: Int, m: Int, d: Int) -> Int:
    """Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's)."""
    var y = y0 - 1 if m <= 2 else y0
    var era = (y if y >= 0 else y - 399) // 400
    var yoe = y - era * 400
    var mp = m - 3 if m > 2 else m + 9
    var doy = (153 * mp + 2) // 5 + d - 1
    var doe = yoe * 365 + yoe // 4 - yoe // 100 + doy
    return era * 146097 + doe - 719468


def _digits(s: String, a: Int, b: Int) raises -> Int:
    return Int(String(s[byte=a:b]))


def parse_iso(s: String) raises -> Float64:
    """Seconds since the epoch of an ISO stamp, as Python's datetime.fromisoformat reads
    bullpen's: a date, a time to the second with optional fraction, and an offset (none:
    local time)."""
    if s.byte_length() < 19:
        raise Error("Invalid isoformat string: '" + s + "'")
    var days = days_from_civil(_digits(s, 0, 4), _digits(s, 5, 7), _digits(s, 8, 10))
    var secs = Float64(days * 86400 + _digits(s, 11, 13) * 3600 + _digits(s, 14, 16) * 60 + _digits(s, 17, 19))
    var i = 19
    var n = s.byte_length()
    if i < n and s[byte=i] == ".":
        var j = i + 1
        while j < n and s[byte=j] >= "0" and s[byte=j] <= "9":
            j += 1
        secs += Float64(String("0") + String(s[byte=i:j]))
        i = j
    if i >= n:
        return secs - Float64(local_offset(Int(secs)))
    if s[byte=i] == "Z":
        return secs
    var sign = -1 if s[byte=i] == "-" else 1
    var off = _digits(s, i + 1, i + 3) * 3600 + _digits(s, i + 4, i + 6) * 60
    return secs - Float64(sign * off)


def now() -> Float64:
    """Seconds since the epoch, to the microsecond."""
    var ts = List[Int](length=2, fill=0)
    _ = external_call["clock_gettime", c_int](c_int(0), Int(ts.unsafe_ptr()))
    return Float64(ts[0]) + Float64(ts[1]) / 1e9


struct Local(Copyable, Movable):
    """A moment in local time, from libc's localtime_r."""

    var year: Int
    var month: Int  # 1-12
    var day: Int
    var hour: Int
    var minute: Int
    var second: Int
    var weekday: Int  # 0 Sunday
    var offset: Int  # seconds east of UTC

    def __init__(out self, epoch: Int):
        var t = List[Int](length=1, fill=epoch)
        var tm = List[Int32](length=16, fill=0)  # struct tm: 9 ints, then long tm_gmtoff at byte 40
        _ = external_call["localtime_r", Int](Int(t.unsafe_ptr()), Int(tm.unsafe_ptr()))
        _ = t^  # alive until here: a value can be destroyed right after its last use
        self.second = Int(tm[0])
        self.minute = Int(tm[1])
        self.hour = Int(tm[2])
        self.day = Int(tm[3])
        self.month = Int(tm[4]) + 1
        self.year = Int(tm[5]) + 1900
        self.weekday = Int(tm[6])
        self.offset = Int(tm[10])  # the low half of tm_gmtoff: an offset fits

    def days(self) -> Int:
        return days_from_civil(self.year, self.month, self.day)


def local_offset(epoch: Int) -> Int:
    return Local(epoch).offset


def _two(n: Int) -> String:
    return ("0" if n < 10 else "") + String(n)


def iso_now(micro: Bool = False) -> String:
    """Now as Python's datetime.now().astimezone().isoformat(timespec="seconds"),
    or "microseconds" with micro."""
    var t = now()
    var sec = Int(t)
    var l = Local(sec)
    var out = String(l.year) + "-" + _two(l.month) + "-" + _two(l.day) + "T" + _two(l.hour) + ":" + _two(l.minute) + ":" + _two(l.second)
    if micro:
        var us = String(Int((t - Float64(sec)) * 1e6))
        out += "." + "0" * (6 - us.byte_length()) + us
    var off = l.offset
    var sign = "+" if off >= 0 else "-"
    off = off if off >= 0 else -off
    return out + sign + _two(off // 3600) + ":" + _two((off % 3600) // 60)


comptime DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
comptime MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]


def clock(when: Float64, at: Float64) -> String:
    """ "3:00 PM", with the day when it isn't today: "Mon 12:00 AM", "Oct 9 3:00 PM"
    (limit.clock)."""
    var w = Local(Int(when))
    var a = Local(Int(at))
    var h = w.hour % 12
    var text = String(h if h else 12) + ":" + _two(w.minute) + (" AM" if w.hour < 12 else " PM")
    var days = w.days() - a.days()
    if days == 0:
        return text
    if days > 0 and days < 7:
        return String(materialize[DAYS]()[w.weekday]) + " " + text
    return String(materialize[MONTHS]()[w.month - 1]) + " " + String(w.day) + " " + text


def seconds_since(stamp: String) raises -> Float64:
    """Seconds from an ISO stamp to now (core.seconds_since)."""
    return now() - parse_iso(stamp)
