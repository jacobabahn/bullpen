"""Running another program: its exit code, stdout and stderr.

Mojo's own std.subprocess.run takes one shell string and gives back stdout alone (no
exit code; stderr goes to the terminal), so this calls libc's posix_spawnp itself. The
child's stdout and stderr go to two memfds, read back once it exits: no pipes to drain
while it runs. Its stdin is /dev/null, as bullpen's Python does (core.run).
Pointers go to libc as plain addresses (Int)."""

from std.ffi import external_call, c_int


struct Done(Movable):
    """A program that ran: its exit code (128 + the signal when one killed it, 127 when
    it couldn't start), stdout and stderr."""

    var code: Int
    var out: String
    var err: String

    def __init__(out self, code: Int, var out: String, var err: String):
        self.code = code
        self.out = out^
        self.err = err^


def _slurp(fd: c_int) raises -> String:
    with open("/proc/self/fd/" + String(fd), "r") as f:
        return f.read()


def run(argv: List[String], cwd: String = "") raises -> Done:
    """Run argv (found on PATH) in cwd and wait for it."""
    var cstrs = List[String]()
    for a in argv:
        cstrs.append(a + "\0")
    var ptrs = List[Int](length=len(cstrs) + 1, fill=0)
    for i in range(len(cstrs)):
        ptrs[i] = Int(cstrs[i].unsafe_ptr())
    var name = String("bullpen\0")
    var ofd = external_call["memfd_create", c_int](Int(name.unsafe_ptr()), c_int(0))
    var efd = external_call["memfd_create", c_int](Int(name.unsafe_ptr()), c_int(0))
    var acts = List[UInt8](length=256, fill=0)  # posix_spawn_file_actions_t (80 bytes on glibc)
    var a = Int(acts.unsafe_ptr())
    var devnull = String("/dev/null\0")
    var cwdz = cwd + "\0"
    _ = external_call["posix_spawn_file_actions_init", c_int](a)
    _ = external_call["posix_spawn_file_actions_addopen", c_int](a, c_int(0), Int(devnull.unsafe_ptr()), c_int(0), c_int(0))
    _ = external_call["posix_spawn_file_actions_adddup2", c_int](a, ofd, c_int(1))
    _ = external_call["posix_spawn_file_actions_adddup2", c_int](a, efd, c_int(2))
    if cwd:
        _ = external_call["posix_spawn_file_actions_addchdir_np", c_int](a, Int(cwdz.unsafe_ptr()))
    var pid = List[Int32](length=1, fill=0)
    var envsym = String("environ\0")
    var environ = external_call["dlsym", Int](0, Int(envsym.unsafe_ptr()))
    var envp = _deref(environ)
    var rc = external_call["posix_spawnp", c_int](
        Int(pid.unsafe_ptr()), Int(cstrs[0].unsafe_ptr()), a, 0, Int(ptrs.unsafe_ptr()), envp)
    _ = external_call["posix_spawn_file_actions_destroy", c_int](a)
    var code = 127
    if rc == 0:
        var status = List[Int32](length=1, fill=0)
        _ = external_call["waitpid", c_int](pid[0], Int(status.unsafe_ptr()), c_int(0))
        var st = Int(status[0])
        code = (st >> 8) & 0xFF if (st & 0x7F) == 0 else 128 + (st & 0x7F)
    var out = _slurp(ofd)
    var err = _slurp(efd)
    _ = external_call["close", c_int](ofd)
    _ = external_call["close", c_int](efd)
    _ = cstrs^
    _ = ptrs^
    _ = acts^
    if rc != 0:
        err = "No such file or directory: '" + argv[0] + "'"
    return Done(code, out^, err^)


def _deref(address: Int) -> Int:
    """The pointer-sized value at address (libc's environ)."""
    var out = List[Int](length=1, fill=0)
    _ = external_call["memcpy", Int](Int(out.unsafe_ptr()), address, 8)
    return out[0]
