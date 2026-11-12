"""The Lua watcher and its helpers, run with a fake rex."""

import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from support import setUpModule, tearDownModule, ROOT  # a temp folder for each module


STUCK_LUA = """
local q = dofile(rex.args.lib).new()
local limits = { a = 20, b = 20, off = 0 }
local function limit(block) return limits[block] end
local function due(minute)
  local out = {}
  for _, p in ipairs(q.due(minute * 60, limit)) do out[#out + 1] = p.block .. "=" .. p.minutes end
  table.sort(out)
  return table.concat(out, ",")
end
local got = {}
local function check(name, value, want)
  if value ~= want then got[#got + 1] = name .. ": " .. tostring(value) .. ", not " .. tostring(want) end
end
check("first event", q.event("a", true, 0), false)
q.event("b", false, 0)       -- not working: never due
q.event("off", true, 0)      -- its repo turned this off
check("too early", due(19), "")
check("due at the limit", due(20), "a=20")
check("once per spell", due(45), "")
check("the next event ends the spell", q.event("a", true, 46 * 60), true)
check("and only that one", q.event("a", true, 47 * 60), false)
check("counted from the last event", due(66), "")
check("a new spell", due(68), "a=21")
q.event("b", true, 60 * 60)
q.forget("b")
check("a pane that's gone", due(200), "")
limits.off = 30
check("turned on later", due(201), "off=201")
return #got == 0 and "ok" or table.concat(got, "; ")
"""


WATCH_LUA = """
local args = rex.args
local clock, handlers, timers, calls = 0, {}, {}, {}
os.time = function() return clock end
rex = {
  args = { bin = "bullpen", state = args.state },
  on = function(name, fn) handlers[name] = fn end,
  every = function(seconds, fn) timers[seconds] = fn end,
  after = function(seconds, fn) fn() end,
  exec = function(argv)
    calls[#calls + 1] = table.concat(argv, " ", 4)
    return { ok = true }
  end,
}
print = function() end
dofile(args.watch)
local function status(block, state, msg, minute)
  clock = minute * 60
  handlers["terminal.program_status_changed"]({ block_id = block }, { id = "", record = { state = state, msg = msg } })
end
local function tick(minute)
  clock = minute * 60
  timers[60]()
end
status("blk1", "working", "Reading", 0)
status("blk1", "working", "Testing", 5)   -- dropped, but a sign of life
status("other", "working", "", 0)         -- not bullpen's pane
status("blk2", "blocked", "Allow?", 0)    -- waiting on the person, not stuck
tick(24)
tick(25)
tick(40)
status("blk1", "working", "Testing", 41)  -- repeats are dropped, but not this one
status("blk1", "working", "Testing", 42)
tick(61)
tick(62)
handlers["block_closed"]({}, { block_id = "blk1" })
tick(200)
timers[30]()                              -- no limit noted: nothing to call
local f = io.open(args.state .. "/limit.json", "w") f:write("{}") f:close()
timers[30]()
timers[180]()                             -- status drift, every 3 minutes
return table.concat(calls, "|")
"""


@unittest.skipUnless(shutil.which("rex"), "needs Rex, to run Lua")
class WatcherTest(unittest.TestCase):
    def lua(self, script, **args):
        if subprocess.run(["rex", "do", "-e", "return 1"], capture_output=True).returncode != 0:
            self.skipTest("rex do doesn't run here")
        tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, tmp, ignore_errors=True)
        path = Path(tmp) / "test.lua"
        path.write_text(script)
        p = subprocess.run(["rex", "do", str(path), *(f"{k}={v}" for k, v in args.items())],
                           text=True, capture_output=True)
        self.assertEqual(p.returncode, 0, p.stderr)
        return json.loads(p.stdout)

    def test_a_quiet_spell_is_due_once(self):
        self.assertEqual(self.lua(STUCK_LUA, lib=ROOT / "lua" / "stuck.lua"), "ok")

    def test_the_watcher_passes_working_events_on_while_compacting(self):
        state = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, state, ignore_errors=True)
        (state / "blocks").mkdir()
        entry = state / "blocks" / "blk1.json"
        entry.write_text(json.dumps({"repo": "/r", "role": "BP-1", "stuck_after": 20}, indent=2))
        script = WATCH_LUA.split("status(\"blk1\", \"working\", \"Reading\", 0)")[0] + """
status("blk1", "working", "", 0)
status("blk1", "working", "Dropped", 1)
local f = io.open(args.state .. "/blocks/blk1.json", "w")
f:write('{"repo": "/r", "role": "BP-1", "compacting": true}') f:close()
status("blk1", "working", "Passed on", 2)
return table.concat(calls, "|")
"""
        calls = self.lua(script, watch=ROOT / "lua" / "watch.lua", state=state).split("|")
        self.assertEqual(calls, ["_event blk1 working  ", "_event blk1 working  Passed on"])

    def test_the_watcher_reports_and_clears_a_quiet_pane(self):
        state = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, state, ignore_errors=True)
        (state / "blocks").mkdir()
        for block, minutes in (("blk1", 20), ("blk2", 20)):
            (state / "blocks" / f"{block}.json").write_text(
                json.dumps({"repo": "/r", "role": "BP-1", "stuck_after": minutes}, indent=2))
        calls = self.lua(WATCH_LUA, watch=ROOT / "lua" / "watch.lua", state=state).split("|")
        self.assertEqual(calls, [
            "_event blk1 working  Reading",
            "_event blk2 blocked  Allow?",
            "_stuck blk1 20",              # at minute 25: 20 after its last event
            "_event blk1 working  Testing",  # the event that ends the spell
            "_stuck blk1 20",              # quiet again from minute 42
            "_gone blk1",
            "_limit",                      # only while <state>/limit.json exists
            "check --fix --quiet --all-repos",
        ])

    def test_the_watcher_keeps_what_a_worker_is_doing_for_the_board(self):
        state = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, state, ignore_errors=True)
        (state / "blocks").mkdir()
        (state / "doing").mkdir()
        for block, role in (("blk1", "BP-1"), ("rev1", "BP-1 review"), ("co", "coordinator")):
            (state / "blocks" / f"{block}.json").write_text(json.dumps({"repo": "/r", "role": role}))
        # rex.after waits for run_due(); each step returns what the doing files say
        script = WATCH_LUA.split("print = function() end")[0].replace(
            "after = function(seconds, fn) fn() end",
            "after = function(seconds, fn) later[#later + 1] = { at = clock + seconds, fn = fn } end",
        ).replace("local args = rex.args", "local args = rex.args\nlater = {}") + """
print = function() end
dofile(args.watch)
local function status(block, state, msg, second)
  clock = second
  handlers["terminal.program_status_changed"]({ block_id = block }, { id = "", record = { state = state, msg = msg } })
end
local function run_due(second)
  clock = second
  local due = later
  later = {}
  for _, l in ipairs(due) do if l.at <= clock then l.fn() else later[#later + 1] = l end end
end
local seen = {}
local function look()
  local out = {}
  for _, b in ipairs({ "blk1", "rev1", "co" }) do
    local f = io.open(args.state .. "/doing/" .. b .. ".json")
    if f then out[#out + 1] = b .. "=" .. f:read("*a"):gsub("\\n$", "") f:close() end
  end
  seen[#seen + 1] = table.concat(out, " ")
end
status("blk1", "working", 'Reading "a.py"', 100) look()
status("blk1", "working", "Editing", 101) look()           -- too soon: waits
status("blk1", "working", "Testing", 102) look()           -- still waiting, latest wins
run_due(103) look()
status("blk1", "blocked", "Allow?", 103) look()            -- a new state is written at once
status("rev1", "working", "Reviewing", 104)
status("co", "working", "Thinking", 104) look()            -- not a task's pane
handlers["block_closed"]({}, { block_id = "blk1" }) look()
return table.concat(seen, "|")
"""
        seen = self.lua(script, watch=ROOT / "lua" / "watch.lua", state=state).split("|")
        line = lambda st, text, at: json.dumps({"state": st, "text": text, "at": at})
        self.assertEqual(seen, [
            f"blk1={line('working', 'Reading \"a.py\"', 100)}",
            f"blk1={line('working', 'Reading \"a.py\"', 100)}",
            f"blk1={line('working', 'Reading \"a.py\"', 100)}",
            f"blk1={line('working', 'Testing', 103)}",
            f"blk1={line('blocked', 'Allow?', 103)}",
            f"blk1={line('blocked', 'Allow?', 103)} rev1={line('working', 'Reviewing', 104)}",
            f"rev1={line('working', 'Reviewing', 104)}",
        ])
