"""The base of the Mojo port (bullpen/core.py): paths, small helpers, a repo's
config and tasks, and a task's status in words.

A task is its JSON record (a Doc, node 0 its object): code reads it with `t.s(key)`,
`t.has(key)`, `t.truthy(key)` and, for an object inside it, `t.doc.get(0, "review")`
then the Doc's own getters. A command's error (core.Fail in Python) is a raised
Error; main prints it as `bullpen: <message>` and exits 1."""

from std.os import getenv, listdir, makedirs
from std.os.path import exists, isdir
from std.ffi import external_call, c_int

from bullpen.json import Doc, Node, copy_into, OBJ, ARR, STR, NULL
from bullpen.proc import run, Done
from bullpen.timeutil import iso_now


# ---------- paths ----------


def user_home() -> String:
    return getenv("HOME")


def state_dir() -> String:
    """BULLPEN_STATE, else ~/.local/state/bullpen."""
    var s = getenv("BULLPEN_STATE")
    return s if s else user_home() + "/.local/state/bullpen"


def rex_program() -> String:
    var r = getenv("BULLPEN_REX")
    return r if r else "rex"


def self_exe() raises -> String:
    """This binary's resolved path."""
    var buf = List[UInt8](length=4096, fill=0)
    var link = String("/proc/self/exe\0")
    var n = external_call["readlink", Int](Int(link.unsafe_ptr()), Int(buf.unsafe_ptr()), 4096)
    _ = link^  # alive through the call: a value is destroyed after its last use
    buf.resize(max(n, 0), 0)
    return String(from_utf8=buf) if n > 0 else ""


def checkout() raises -> String:
    """bullpen's checkout: the binary is <checkout>/mojo/build/bullpen."""
    return dirname(dirname(dirname(self_exe())))


def python_self() raises -> String:
    """The Python entry script (core.SELF), which messages name."""
    return checkout() + "/bin/bullpen"


def dirname(p: String) -> String:
    var i = p.rfind("/")
    return "/" if i == 0 else String(p[byte=:i]) if i > 0 else ""


def basename(p: String) -> String:
    return String(p[byte=p.rfind("/") + 1:])


def read_text(path: String) raises -> String:
    with open(path, "r") as f:
        return f.read()


def write_text(path: String, text: String) raises:
    """Write a file whole, through a temporary file renamed over it (as write_json does)."""
    var tmp = dirname(path) + "/.tmp-" + basename(path) + "-" + String(external_call["getpid", c_int]())
    with open(tmp, "w") as f:
        f.write(text)
    var a = tmp + "\0"
    var b = path + "\0"
    var rc = external_call["rename", c_int](Int(a.unsafe_ptr()), Int(b.unsafe_ptr()))
    _ = a^  # alive through the call
    _ = b^
    if rc != 0:
        raise Error("couldn't write " + path)


def write_json(path: String, doc: Doc) raises:
    mkdirs(dirname(path))
    write_text(path, doc.dumps() + "\n")


def mkdirs(path: String) raises:
    if path and not isdir(path):
        makedirs(path, exist_ok=True)


def read_json_or(path: String, default: String) raises -> Doc:
    """The JSON file at path, or default (JSON text) when there is none."""
    if not exists(path):
        return Doc.parse(default)
    return Doc.read(path)


# ---------- small helpers ----------


def git(repo: String, args: List[String], check: Bool = True) raises -> Done:
    var argv: List[String] = ["git", "-C", repo]
    argv += args.copy()
    return run_cmd(argv, check)


def run_cmd(argv: List[String], check: Bool = True, cwd: String = "") raises -> Done:
    """core.run: a failure (check) is an Error saying the command and what it said."""
    var p = run(argv, cwd)
    if check and p.code != 0:
        var msg = String(p.out.strip())
        if p.err.byte_length() > 0:
            msg = String(p.err.strip())
        raise Error("`" + String(" ").join(argv) + "` failed: " + msg)
    return p^


def is_word_char(c: Int) -> Bool:
    return (c >= ord("a") and c <= ord("z")) or (c >= ord("A") and c <= ord("Z")) or (c >= ord("0") and c <= ord("9"))


def safe(name: String) -> String:
    """Every character but [A-Za-z0-9._-] as _."""
    var out = String()
    for cp in name.codepoints():
        var c = Int(cp)
        out += String(cp) if c < 128 and (is_word_char(c) or c == ord(".") or c == ord("_") or c == ord("-")) else "_"
    return out^


def home(path: String) -> String:
    """A path with $HOME as ~, for people to read."""
    var h = user_home()
    if h != "/" and (path == h or path.startswith(h + "/")):
        return "~" + String(path[byte=h.byte_length():])
    return path


def homes(text: String) -> String:
    """Text with each path under $HOME written with ~ (one not followed by a word
    character, . or -)."""
    var h = user_home()
    if h == "/" or h == "":
        return text
    var out = String()
    var i = 0
    var n = text.byte_length()
    var hl = h.byte_length()
    while i < n:
        var j = text.find(h, i)
        if j < 0:
            break
        var k = j + hl
        var next_ok = True
        if k < n:
            var c = Int(text.as_bytes()[k])
            next_ok = not (is_word_char(c) or c == ord("_") or c == ord(".") or c == ord("-") or c >= 128)
        out += text[byte=i:j]
        out += "~" if next_ok else h
        i = k
    out += text[byte=i:]
    return out^


def commits(n: Int) -> String:
    return String(n) + " commit" + ("s" if n != 1 else "")


def clip(text: String, n: Int = 300) -> String:
    var words = text.split()
    var t = String(" ").join(words)
    var cps = List[String]()
    for cp in t.codepoints():
        cps.append(String(cp))
    if len(cps) <= n:
        return t
    return String("").join(cps[:n - 1]) + "…"


# ---------- config ----------

# The built-in defaults (core.SETTINGS), in its order.
comptime DEFAULTS = """{"prefix": "BP", "next": 1, "base": null, "max_running": 3, "merge": "rebase",
"rebase_others": true, "push_remote": null, "auto_accept": false, "setup": null, "teardown": null,
"gh": null, "worker": ["claude", "--permission-mode", "auto"],
"coordinator_cmd": ["claude", "--permission-mode", "auto"],
"reviewer": ["claude", "--permission-mode", "auto"], "tour": ["claude"], "workers": "float",
"worker_model": null, "reviewer_model": null, "reviewer_followup_model": "sonnet",
"tour_model": "sonnet", "review": true, "review_rounds": 2, "light_review_lines": 40,
"board_layout": "grouped", "board_bottom_gap": 1, "board_repos": true, "news": "type",
"notify": "needs", "stuck_after": 20, "triage_minutes": 10, "coordinators": [],
"next_coordinator": 1}"""
# Word sets, space-separated: in_words(key, REPO_ONLY).
comptime REPO_ONLY = " prefix next base push_remote news coordinators next_coordinator "
comptime INTERNAL = " root triage_migrated stops_migrated layered "


def global_config_path() -> String:
    var x = getenv("XDG_CONFIG_HOME")
    return (x if x else user_home() + "/.config") + "/bullpen/config.json"


def in_words(key: String, words: String) -> Bool:
    return (" " + key + " ") in words


def overlay(mut cfg: Doc, src: Doc, skip_repo_only: Bool):
    """Set cfg's keys from src's object (the global file: not the repo-only or internal ones)."""
    ref node = src.nodes[0]
    for i in range(len(node.keys)):
        var key = node.keys[i]
        if skip_repo_only and (in_words(key, REPO_ONLY) or in_words(key, INTERNAL)):
            continue
        var v = copy_into(cfg, src, node.kids[i])
        var at = cfg.get(0, key)
        if at >= 0:
            ref root = cfg.nodes[0]
            for j in range(len(root.keys)):
                if root.keys[j] == key:
                    root.kids[j] = v
        else:
            cfg.nodes[0].keys.append(key)
            cfg.nodes[0].kids.append(v)


def inherited_config() raises -> Doc:
    """What a repository gets when its own config.json doesn't say: the global settings
    over the built-in defaults."""
    var cfg = Doc.parse(materialize[DEFAULTS]())
    var gpath = global_config_path()
    if exists(gpath):
        overlay(cfg, Doc.read(gpath), True)
    return cfg^


def load_config(dir: String) raises -> Doc:
    """A repo's settings: its own config.json over the global one, over the defaults.
    (An old config with one "coordinator" becomes coordinator c1, as in Python.)"""
    var cfg = Doc.parse(materialize[DEFAULTS]())
    var gpath = global_config_path()
    if exists(gpath):
        var g = Doc.read(gpath)
        if g.kind(0) != OBJ:
            raise Error(gpath + " isn't a JSON object ({...}): fix it, or move it away")
        overlay(cfg, g, True)
    var path = dir + "/config.json"
    if exists(path):
        overlay(cfg, Doc.read(path), False)
    if cfg.has(0, "coordinator") or cfg.has(0, "board"):
        var old = cfg.get(0, "coordinator")
        var board = cfg.get(0, "board")
        if cfg.is_true(old) and cfg.len(cfg.get(0, "coordinators")) == 0:
            var c = cfg.add(Node(OBJ))
            _ = cfg.put_str(c, "id", "c1")
            _ = cfg.put(c, "block", Node(STR, cfg.str(old, "block")) if cfg.kind(cfg.get(old, "block")) == STR else Node(NULL))
            _ = cfg.put(c, "session", Node(STR, cfg.str(old, "session")) if cfg.kind(cfg.get(old, "session")) == STR else Node(NULL))
            var list = cfg.put(0, "coordinators", Node(ARR))
            cfg.nodes[list].kids.append(c)
            var b = board if board >= 0 else cfg.add(Node(NULL))
            cfg.nodes[c].keys.append("board")
            cfg.nodes[c].kids.append(b)
            _ = cfg.put_int(0, "next_coordinator", 2)
        cfg.pop(0, "coordinator")
        cfg.pop(0, "board")
    return cfg^


# ---------- tasks ----------



def closed(status: String) -> Bool:
    return status == "done" or status == "dropped"


def waiting(status: String) -> Bool:
    return status == "needs" or status == "triage"


struct Task(Copyable, Movable):
    """A task record: doc's node 0 is its object."""

    var doc: Doc

    def __init__(out self, var doc: Doc):
        self.doc = doc^

    def s(self, key: String) -> String:
        """Its string for key; "" when missing, null or not a string."""
        return self.doc.str(0, key)

    def has(self, key: String) -> Bool:
        return self.doc.has(0, key)

    def truthy(self, key: String) -> Bool:
        return self.doc.truthy(0, key)

    def get(self, key: String) -> Int:
        return self.doc.get(0, key)

    def sub(self, n: Int, key: String) -> String:
        """The string for key in the object at n (an object inside the task)."""
        return self.doc.str(n, key)

    def id(self) -> String:
        return self.s("id")

    def status(self) -> String:
        return self.s("status")

    def num(self) -> Int:
        var id = self.id()
        try:
            return Int(String(id[byte=id.rfind("-") + 1:]))
        except:
            return 0


def task_num_of(id: String) -> Int:
    try:
        return Int(String(id[byte=id.rfind("-") + 1:]))
    except:
        return 0


def sort_tasks(mut tasks: List[Task]):
    """By number (insertion sort: stable, and lists are short)."""
    for i in range(1, len(tasks)):
        var j = i
        while j > 0 and tasks[j - 1].num() > tasks[j].num():
            tasks.swap_elements(j - 1, j)
            j -= 1


struct Repo(Movable):
    var root: String
    var dir: String
    var cfg: Doc
    var own: List[String]  # the keys this repo's config.json sets itself

    def __init__(out self, root: String) raises:
        self.root = root
        var stripped = root.strip("/")
        self.dir = state_dir() + "/repos/" + safe(String(stripped).replace("/", "-"))
        self.cfg = load_config(self.dir)
        self.own = List[String]()
        if exists(self.dir + "/config.json"):
            self.own = Doc.read(self.dir + "/config.json").nodes[0].keys.copy()
        # Python's Repo migrates an old config.json here (triage, stops, layers) and saves it.
        # The port reads only: it says so rather than read a config Python hasn't migrated.
        if exists(self.dir + "/config.json") and not (
            self.cfg.truthy(0, "triage_migrated") and self.cfg.truthy(0, "stops_migrated") and self.cfg.truthy(0, "layered")
        ):
            raise Error("this repo's config.json is from an older bullpen: run the Python bullpen once first")

    def save(mut self) raises:
        """Write this repo's config.json: only what it sets itself (and the keys only a
        repo has, and bullpen's own notes); the rest comes from the global one."""
        _ = self.cfg.put_str(0, "root", self.root)
        _ = self.cfg.put_bool(0, "triage_migrated", True)
        _ = self.cfg.put_bool(0, "stops_migrated", True)
        _ = self.cfg.put_bool(0, "layered", True)
        var inherited = inherited_config()
        var out = Doc()
        _ = out.add(Node(OBJ))
        var own = List[String]()
        ref root = self.cfg.nodes[0]
        for i in range(len(root.keys)):
            var k = root.keys[i]
            var mine = False
            for o in self.own:
                if o == k:
                    mine = True
            if mine or in_words(k, REPO_ONLY) or in_words(k, INTERNAL) or not inherited.has(0, k):
                var v = copy_into(out, self.cfg, root.kids[i])
                out.nodes[0].keys.append(k)
                out.nodes[0].kids.append(v)
                own.append(k)
        self.own = own^
        write_json(self.dir + "/config.json", out)

    def base(self) raises -> String:
        """The branch tasks start from: the config's, else origin's default, else the current one."""
        if self.cfg.truthy(0, "base"):
            return self.cfg_str("base")
        var p = git(self.root, ["symbolic-ref", "--short", "refs/remotes/origin/HEAD"], False)
        if p.code == 0:
            var s = String(p.out.strip())
            return String(s[byte=s.find("/") + 1:])
        var cur = String(git(self.root, ["branch", "--show-current"]).out.strip())
        return cur if cur else "main"

    def save_task(self, mut t: Task) raises:
        """Write a task whose status may have changed: when it was updated, and when it
        entered its status. (Python's on_save hooks, notifications and PRs to follow,
        do nothing for the statuses the port saves: todo.)"""
        _ = t.doc.put_str(0, "updated", iso_now())
        var path = self.task_path(t.id())
        var old_status = String("")
        var had = exists(path)
        if had:
            old_status = Doc.read(path).str(0, "status")
        if not had or old_status != t.status():
            _ = t.doc.put_null(0, "note")
            t.doc.pop(0, "stuck")
            t.doc.pop(0, "auto_wait")
        if not had or old_status != t.status() or not t.truthy("since"):
            _ = t.doc.put_str(0, "since", iso_now(True))
        if t.status() != "todo":
            raise Error("save_task: the port only saves todo tasks (notifications aren't ported)")
        write_json(path, t.doc)

    def cfg_str(self, key: String) -> String:
        return self.cfg.str(0, key)

    def task_path(self, tid: String) -> String:
        return self.dir + "/tasks/" + tid + ".json"

    def norm(self, tid: String) -> String:
        var t = tid.upper()
        return t if "-" in t else self.cfg_str("prefix") + "-" + t

    def task(self, tid: String) raises -> Task:
        var id = self.norm(tid)
        var path = self.task_path(id)
        if not exists(path):
            raise Error("no task " + id + " in " + home(self.root) + ": `bullpen ls --all` lists them")
        return Task(Doc.read(path))

    def tasks(self) raises -> List[Task]:
        var out = List[Task]()
        var d = self.dir + "/tasks"
        if isdir(d):
            for name in listdir(d):
                if name.endswith(".json") and not name.startswith("."):
                    out.append(Task(Doc.read(d + "/" + name)))
        sort_tasks(out)
        return out^

    def has_origin(self) raises -> Bool:
        """No origin remote means local mode."""
        return git(self.root, ["remote", "get-url", "origin"], False).code == 0

    def name(self) -> String:
        return basename(self.root)


def repo_root(path: String = "") raises -> String:
    """The repo's main checkout, from git's common dir (a worktree's too)."""
    var cwd = path
    if not cwd:
        var buf = List[UInt8](length=4096, fill=0)
        _ = external_call["getcwd", Int](Int(buf.unsafe_ptr()), 4096)
        var n = 0
        while buf[n] != 0:
            n += 1
        buf.resize(n, 0)
        cwd = String(from_utf8=buf)
    var p = git(cwd, ["rev-parse", "--path-format=absolute", "--git-common-dir"], False)
    if p.code != 0:
        raise Error("not in a git repository")
    var common = String(p.out.strip())
    return dirname(common) if basename(common) == ".git" else common


def known_repos() raises -> List[Repo]:
    """Every repository bullpen has state for whose checkout is still there, by state folder."""
    var out = List[Repo]()
    var d = state_dir() + "/repos"
    if not isdir(d):
        return out^
    var names = listdir(d)
    sort_strings(names)
    for name in names:
        var cfg = d + "/" + name + "/config.json"
        if not exists(cfg):
            continue
        var root: String
        try:
            root = Doc.read(cfg).str(0, "root")
        except:
            continue
        if root and isdir(root):
            out.append(Repo(root))
    return out^


def sort_strings(mut xs: List[String]):
    for i in range(1, len(xs)):
        var j = i
        while j > 0 and xs[j - 1] > xs[j]:
            xs.swap_elements(j - 1, j)
            j -= 1


def listed(repo: Repo, all: Bool) raises -> List[Task]:
    """The tasks `ls` lists: the open ones, or all with --all."""
    var out = List[Task]()
    for t in repo.tasks():
        if all or not closed(t.status()):
            out.append(t.copy())
    return out^


def task_base_ref(repo: Repo, t: Task) raises -> String:
    """origin's copy of the task's base when there is one, else the local base branch."""
    var base = t.s("base")
    if repo.has_origin() and git(t.s("worktree"), ["rev-parse", "-q", "--verify", "origin/" + base], False).code == 0:
        return "origin/" + base
    return base


def own_model(repo: Repo, t: Task) -> String:
    """The worker model a task chose for itself, if it isn't the config's ("" if none)."""
    var m = t.s("worker_model")
    return m if m and m != repo.cfg_str("worker_model") else ""


def pr_short(url: String) -> String:
    """ "PR #12" from a PR's URL."""
    var i = url.find("/pull/")
    if i < 0:
        return url
    var j = i + 6
    var n = url.byte_length()
    var k = j
    while k < n and url.as_bytes()[k] >= ord("0") and url.as_bytes()[k] <= ord("9"):
        k += 1
    return "PR #" + String(url[byte=j:k]) if k > j else url


def stamp(t: Task) -> String:
    """When it entered its current status."""
    var s = t.s("since")
    return s if s else t.s("updated")


def board_words(status: String) -> String:
    if status == "needs" or status == "stopped":
        return "needs you"
    if status == "triage":
        return "coordinator on it"
    if status == "ready":
        return "ready to accept"
    if status == "finished":
        return "finished"
    if status == "review":
        return "in review"
    if status == "working":
        return "working"
    if status == "todo":
        return "queued"
    if status == "done" or status == "dropped":
        return "closed"
    return status


def status_words(status: String) -> String:
    var words = board_words(status)
    return words if status in words else words + " (" + status + ")"


def is_status(t: Task) -> String:
    """ "BP-3 is queued (todo)", "BP-3 needs you (stopped)", for errors."""
    var words = status_words(t.status())
    if words.startswith("coordinator"):
        return t.id() + "'s " + words.replace(" on it", " is on it")
    return t.id() + " " + words if words.startswith("needs") else t.id() + " is " + words
