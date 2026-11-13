"""`bullpen news`, one-off (bullpen/news.py): print the news logged for this
coordinator since the last time, once each, moving its cursor. `--follow` keeps
running and stays with Python."""

from std.os import getenv, makedirs
from std.os.path import exists
from std.ffi import external_call, c_int

from bullpen.args import Args
from bullpen.core import Repo, repo_root, safe, load_config
from bullpen.coordinators import calling_coordinator, primary, coordinator_at
from bullpen.textutil import py_strip

comptime NEWS_MARK = "bullpen · "
comptime NOT_A_COORDINATOR = (
    "this pane isn't one of this repo's coordinators, so it doesn't read "
    + "their news: don't start the monitor again (`bullpen up --here` here makes it one)"
)


def cursor_path(repo: Repo, cid: String) -> String:
    return repo.dir + "/" + ("news-" + safe(cid) + ".cursor" if cid else "news.cursor")


def _read_int(path: String) -> Int:
    """int(path.read_text()), or -1 when there's no file or no number in it."""
    try:
        with open(path, "r") as f:
            var text = py_strip(f.read())
            return Int(text) if text.byte_length() > 0 else -1
    except:
        return -1


def news_cursor(repo: Repo, cid: String) -> Int:
    """How far into news.log (in bytes) coordinator cid has read, or -1: never.
    (news.cursor: the one cursor from before several coordinators; c1 carries it on.)"""
    var n = _read_int(cursor_path(repo, cid))
    if n < 0 and cid == "c1":
        n = _read_int(cursor_path(repo, ""))
    return n


def set_news_cursor(repo: Repo, offset: Int, cid: String) raises:
    makedirs(repo.dir, exist_ok=True)
    var name = "news-" + safe(cid) + ".cursor" if cid else "news.cursor"
    var path = repo.dir + "/" + name
    var tmp = repo.dir + "/." + name + ".tmp"
    with open(tmp, "w") as f:
        f.write(String(offset) + "\n")
    var a = tmp + "\0"
    var b = path + "\0"
    var rc = external_call["rename", c_int](Int(a.unsafe_ptr()), Int(b.unsafe_ptr()))
    _ = a^  # keep the C strings alive past the call (Mojo frees a value after its last use)
    _ = b^
    if rc != 0:
        raise Error("couldn't write " + path)


def news_size(repo: Repo) -> Int:
    var path = repo.dir + "/news.log"
    try:
        with open(path, "r") as f:
            return Int(f.seek(0, 2))
    except:
        return 0


def _split_words(s: String) -> List[String]:
    var out = List[String]()
    for w in s.split():
        out.append(String(w))
    return out^


def news_for(text: String, cid: String, listed: List[String]) -> Bool:
    """Is a logged line for coordinator cid? `[c1,c2]` before the task ID names whom
    it's for; lines without, or for coordinators no longer listed, are everyone's."""
    var at = text.find(NEWS_MARK)
    var head = String(text[byte=:at]) if at >= 0 else text
    var to = List[String]()
    var named = False
    for w in _split_words(head):
        if w.byte_length() >= 2 and w.startswith("[") and w.endswith("]"):
            for x in String(w[byte=1:w.byte_length() - 1]).split(","):
                to.append(String(x))
            named = True
            break
    if not cid or not named:
        return True
    for x in to:
        if x == cid:
            return True
    for x in to:
        for l in listed:
            if x == l:
                return False
    return True


def news_records(data: List[UInt8], start: Int) -> List[List[Int]]:
    """The whole news lines in data from start, each with the tab-led lines after it, as
    lists of the byte offsets where its raw lines end. One still being written isn't given."""
    var out = List[List[Int]]()
    var rec = List[Int]()
    var i = start
    var n = len(data)
    while i < n:
        var j = i
        while j < n and data[j] != 0x0A:
            j += 1
        if j >= n:
            return out^  # a line still being written: it and the record it ends wait
        if len(rec) > 0 and data[i] != 0x09:
            out.append(rec^)
            rec = List[Int]()
        rec.append(j + 1)
        i = j + 1
    if len(rec) > 0:
        out.append(rec^)
    return out^


def _text(data: List[UInt8], a: Int, b: Int) -> String:
    """bytes a..b decoded with errors="replace"."""
    var part = List[UInt8](capacity=b - a)
    for i in range(a, b):
        part.append(data[i])
    return String(from_utf8_lossy=part)


def _read_bytes(path: String) raises -> List[UInt8]:
    with open(path, "r") as f:
        return f.read_bytes()


struct SigSet(Movable):
    """A sigset_t (128 bytes on glibc) for pthread_sigmask."""

    var raw: List[UInt8]

    def __init__(out self):
        self.raw = List[UInt8](length=128, fill=0)

    def ptr(self) -> Int:
        return Int(self.raw.unsafe_ptr())


def block_signals() -> SigSet:
    """Hold SIGTERM, SIGINT and SIGHUP: printing a line and moving the cursor go together."""
    var set = SigSet()
    var old = SigSet()
    _ = external_call["sigemptyset", c_int](set.ptr())
    for sig in [15, 2, 1]:
        _ = external_call["sigaddset", c_int](set.ptr(), c_int(sig))
    _ = external_call["pthread_sigmask", c_int](c_int(0), set.ptr(), old.ptr())  # SIG_BLOCK
    _ = set^
    return old^


def restore_signals(old: SigSet):
    _ = external_call["pthread_sigmask", c_int](c_int(2), old.ptr(), 0)  # SIG_SETMASK


def print_news(repo: Repo, offset0: Int, cid: String) raises -> Int:
    """Print coordinator cid's news logged after offset, moving its cursor past each line
    once it is out (or skipped). A line's tab-led lines go with it. Returns the new offset."""
    var offset = offset0
    var size = news_size(repo)
    if offset > size:  # the log was removed or cut short: what's there now is new
        offset = 0
        set_news_cursor(repo, 0, cid)
    if offset == size:
        return offset
    var listed = List[String]()
    var cfg = load_config(repo.dir)
    var cs = cfg.get(0, "coordinators")
    for i in range(cfg.len(cs)):
        listed.append(cfg.str(cfg.item(cs, i), "id"))
    var data = _read_bytes(repo.dir + "/news.log")
    var start = offset
    for rec in news_records(data, offset):
        var first = rec[0]
        var text = _text(data, start, first)
        if text.endswith("\n"):
            var cut = String(text[byte=:text.byte_length() - 1])
            text = cut^
        var old = block_signals()
        if not text.startswith("\t") and news_for(text, cid, listed):
            var at = text.find(NEWS_MARK)
            var out = String(text[byte=at:]) if at >= 0 else text
            var a = first
            for k in range(1, len(rec)):
                var line = _text(data, a + 1, rec[k])  # without its tab
                while line.endswith("\n"):
                    var cut = String(line[byte=:line.byte_length() - 1])
                    line = cut^
                out += "\n" + line
                a = rec[k]
            print(out, flush=True)
        offset = rec[len(rec) - 1]
        start = offset
        set_news_cursor(repo, offset, cid)  # after printing: if nobody was reading, it comes again
        restore_signals(old)
    return offset


def news_reader(repo: Repo) -> String:
    """Whose news `bullpen news` prints: the coordinator in whose pane it runs, else the
    primary one. "" (all news) when the repo has no coordinator."""
    var c = calling_coordinator(repo)
    if c < 0:
        c = primary(repo)
    return repo.cfg.str(c, "id") if c >= 0 else ""


def not_a_reader(mut repo: Repo, cid: String) raises -> Bool:
    """Is `bullpen news` running in a Rex pane other than coordinator cid's?"""
    var block = getenv("REX_BLOCK")
    if not block or not cid:
        return False
    repo.cfg = load_config(repo.dir)
    var me = coordinator_at(repo, block, False)
    return me < 0 or repo.cfg.str(me, "id") != cid


def lock_news(repo: Repo, cid: String) raises:
    """Take the reader's lock (news-<cid>.lock), without waiting; held until we exit."""
    var path = repo.dir + "/" + ("news-" + safe(cid) + ".lock" if cid else "news.lock") + "\0"
    var fd = external_call["creat", c_int](Int(path.unsafe_ptr()), c_int(0o666))  # open(path, "w"); the stdlib holds libc open's name
    _ = path^
    if fd < 0:
        raise Error("couldn't open the news lock")
    if external_call["flock", c_int](fd, c_int(6)) != 0:  # LOCK_EX | LOCK_NB
        raise Error("news is being followed already (`bullpen news --follow` is running)")


def cmd_news(args: Args) raises:
    if args.flag("--follow"):
        raise Error("__python__")
    var repo = Repo(repo_root())
    makedirs(repo.dir, exist_ok=True)
    var cid = news_reader(repo)
    if not_a_reader(repo, cid):
        raise Error(NOT_A_COORDINATOR)
    lock_news(repo, cid)
    var offset = news_cursor(repo, cid)
    if offset < 0:  # first time: old news stays old
        offset = news_size(repo)
        set_news_cursor(repo, offset, cid)
    _ = print_news(repo, offset, cid)
