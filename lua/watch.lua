-- bullpen watcher: forwards status changes of bullpen panes to the bullpen CLI.
--
--   rex do --all watch.lua bin=/path/to/bullpen state=/path/to/state [quiet=30] [prs=120] [check=180] [pickup=300] [issues=60]
--
-- Only panes bullpen started (listed in <state>/blocks/) are forwarded, and
-- only when their state changes, so a worker's stream of "working: ..."
-- updates costs one call, not hundreds. A pane that says it's working but sends
-- no event for the repo's `stuck_after` minutes is reported once (stuck.lua).
--
-- What a worker or reviewer says it's doing (the text of "working: ...") goes to
-- <state>/doing/<block>.json for the board, at most every `doing` seconds a pane
-- (default 3): a file write, no bullpen call and no news.

local bin = rex.args.bin
local state_dir = rex.args.state
assert(bin and state_dir, "usage: rex do --all watch.lua bin=... state=...")

local quiet_after = tonumber(rex.args.quiet or "") or 30 -- seconds

local last = {}
local here = debug.getinfo(1, "S").source:gsub("^@", ""):gsub("[^/]*$", "")
local quiet = dofile(here .. "stuck.lua").new()

local function safe(s)
  return (s:gsub("[^%w%._%-]", "_"))
end

-- A bullpen pane's index entry (JSON text), or nil for any other pane.
local function entry(block)
  if not block then return nil end
  local f = io.open(state_dir .. "/blocks/" .. safe(block) .. ".json", "r")
  if not f then return nil end
  local text = f:read("*a")
  f:close()
  return text
end

local function tracked(block)
  return entry(block) ~= nil
end

-- Minutes a working pane may be quiet (the repo's `stuck_after`, copied into the
-- entry); 0 is off, and so is a pane that's no longer bullpen's. The default is a
-- copy of core/src/settings.rs's: a test in tests/test_config.py holds it to that table.
local function stuck_after(block)
  local e = entry(block)
  if not e then return 0 end
  return tonumber(e:match('"stuck_after":%s*(%d+)')) or 20
end

-- bullpen notes in the entry that the pane's agent is compacting its conversation
-- (Claude Code calls that working): its working events go through then, as one of
-- them may be the start of a real turn.
local function compacting(block)
  local e = entry(block)
  return e ~= nil and e:match('"compacting":%s*true') ~= nil
end

-- The pane's role in its entry: a task ID, "<ID> review", "coordinator" or "board".
local function role(block)
  local e = entry(block)
  return e and e:match('"role":%s*"([^"]*)"') or ""
end

local function json_string(s)
  return '"' .. s:gsub('[%c"\\]', function(c)
    if c == '"' or c == "\\" then return "\\" .. c end
    return string.format("\\u%04x", c:byte())
  end) .. '"'
end

-- block -> { state = ..., text = ..., written = time of the last write, due = a write is waiting }
local doing = {}
local doing_every = tonumber(rex.args.doing or "") or 3 -- seconds

local function doing_path(block)
  return state_dir .. "/doing/" .. safe(block) .. ".json"
end

local function write_doing(block)
  local d = doing[block]
  if not d then return end
  d.due, d.written = false, os.time()
  local path = doing_path(block)
  local f = io.open(path .. ".tmp", "w")
  if not f then return end -- no doing/ folder yet: bullpen makes it when it starts a pane
  f:write(string.format('{"state": %s, "text": %s, "at": %d}\n', json_string(d.state), json_string(d.text), d.written))
  f:close()
  os.rename(path .. ".tmp", path)
end

-- Keep the pane's latest text where the board reads it. A change of state is written
-- at once; new text while working at most every doing_every seconds, the latest last.
local function note_doing(block, state, text)
  local d = doing[block]
  if d and d.state == state and d.text == text then return end
  if not d then
    local r = role(block)
    if r == "" or r == "coordinator" or r == "board" then return end
    d = { written = os.time() - doing_every }
    doing[block] = d
  end
  local now_too = d.state ~= state
  d.state, d.text = state, text
  local wait = d.written + doing_every - os.time()
  if now_too or wait <= 0 then
    write_doing(block)
  elseif not d.due then
    d.due = true
    rex.after(wait, function()
      if doing[block] == d and d.due then write_doing(block) end
    end)
  end
end

local function forget_doing(block)
  if doing[block] then os.remove(doing_path(block)) end
  doing[block] = nil
end

local function call(...)
  local r, err = rex.exec({ "env", "BULLPEN_STATE=" .. state_dir, bin, ... })
  if not r then
    print("bullpen: " .. tostring(err))
  elseif not r.ok then
    print("bullpen " .. table.concat({ ... }, " ") .. ": " .. (r.stderr or ""))
  end
end

rex.on("terminal.program_status_changed", function(ctx, ev)
  if (ev.id or "") ~= "" then return end -- the program's own (root) record only
  local block = ctx.block_id
  if not tracked(block) then return end
  local r = ev.record or {}
  local state = r.state or ""
  local prev = last[block]
  note_doing(block, state, r.msg or "")
  -- every event counts as a sign of life, the ones dropped below too
  local woke = quiet.event(block, state == "working", os.time())
  -- working -> working with a new message is noise; a new blocked reason is not.
  -- But the first event after a reported quiet spell goes through, to clear it.
  if not woke and prev and prev.state == state and (state ~= "blocked" or prev.msg == r.msg)
      and not (state == "working" and compacting(block)) then return end
  local n = (prev and prev.n or 0) + 1
  last[block] = { state = state, msg = r.msg, n = n }
  if state == "done" or state == "idle" then
    -- An agent can end a turn and pick up again moments later (a background
    -- agent finishing, its own follow-up): only report it if it stays quiet.
    rex.after(quiet_after, function()
      local cur = last[block]
      if cur and cur.n == n then call("_event", block, state, r.kind or "", r.msg or "") end
    end)
    return
  end
  call("_event", block, state, r.kind or "", r.msg or "")
end)

local function gone(ctx, ev)
  local block = ev.block_id or ctx.block_id
  last[block or ""] = nil
  quiet.forget(block or "")
  forget_doing(block or "")
  if tracked(block) then call("_gone", block) end
end

rex.on("terminal.child_exited", gone)
rex.on("block_closed", gone)

local function exists(path)
  local f = io.open(path, "r")
  if f then f:close() end
  return f ~= nil
end

-- News that couldn't be typed (the person was typing in the coordinator's
-- input) waits in an outbox; <state>/outbox exists while any does.
-- The backstop (docs/news.md): <state>/wake-due exists while news that isn't typed
-- may sit unread by a coordinator; `_wake_check` types it once no reader takes it.
rex.every(3, function()
  if exists(state_dir .. "/outbox") then call("_flush") end
  if exists(state_dir .. "/wake-due") then call("_wake_check") end
end)

-- Pull requests past ready (failed checks, people's comments, a merge outside
-- bullpen): <state>/prs exists while any task has one to follow, so with none
-- this costs a file check, not a `gh` call.
local pr_every = tonumber(rex.args.prs or "") or 120 -- seconds
rex.every(pr_every, function()
  local f = io.open(state_dir .. "/prs", "r")
  if f then f:close() call("_prs") end
end)

-- Idle pickup (docs/backlog.md): <state>/pickup exists while some repo has pickup on
-- and a kept backlog item. A save that frees a slot runs pickup itself; this catches
-- what no save does (pickup just turned on, the usage limit reset, a cap lifted).
local pickup_every = tonumber(rex.args.pickup or "") or 300 -- seconds
rex.every(pickup_every, function()
  local f = io.open(state_dir .. "/pickup", "r")
  if f then f:close() call("_pickup") end
end)

-- GitHub issues (docs/issues.md): <state>/issues exists while some repo has `issues`
-- on, and holds when the next read is due (seconds since 1970; empty: now). `_issues`
-- starts the reads that are due and writes when the next one is, so this is one small
-- file read a tick, and nothing at all with issues off.
local issues_every = tonumber(rex.args.issues or "") or 60 -- seconds between looks
rex.every(issues_every, function()
  local f = io.open(state_dir .. "/issues", "r")
  if not f then return end
  local due = tonumber(f:read("*a")) or 0
  f:close()
  if os.time() >= due then call("_issues") end
end)

-- Agents stopped at the usage limit are continued once it has reset:
-- <state>/limit.json exists while a limit is noted.
rex.every(30, function()
  local f = io.open(state_dir .. "/limit.json", "r")
  if f then f:close() call("_limit") end
end)

-- Working panes with no event for too long: hung, in a loop, or waiting on
-- something Rex doesn't call blocked.
rex.every(60, function()
  for _, p in ipairs(quiet.due(os.time(), stuck_after)) do
    call("_stuck", p.block, string.format("%d", p.minutes))
  end
end)

-- Status drift: a task whose status is out of step with what its agent is doing
-- (an event missed across a watcher or Rex restart) is caught up, with the news
-- the missed event would have given. With no agent running this is one quick look.
local check_every = tonumber(rex.args.check or "") or 180 -- seconds
rex.every(check_every, function()
  call("check", "--fix", "--quiet", "--all-repos")
end)

print("bullpen watcher: listening")
