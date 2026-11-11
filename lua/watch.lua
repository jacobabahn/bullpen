-- bullpen watcher: forwards status changes of bullpen panes to the bullpen CLI.
--
--   rex do --all watch.lua bin=/path/to/bullpen state=/path/to/state [quiet=30] [prs=120] [check=180]
--
-- Only panes bullpen started (listed in <state>/blocks/) are forwarded, and
-- only when their state changes, so a worker's stream of "working: ..."
-- updates costs one call, not hundreds. A pane that says it's working but sends
-- no event for the repo's `stuck_after` minutes is reported once (stuck.lua).

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
-- entry); 0 is off, and so is a pane that's no longer bullpen's.
local function stuck_after(block)
  local e = entry(block)
  if not e then return 0 end
  return tonumber(e:match('"stuck_after":%s*(%d+)')) or 20
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
  -- every event counts as a sign of life, the ones dropped below too
  local woke = quiet.event(block, state == "working", os.time())
  -- working -> working with a new message is noise; a new blocked reason is not.
  -- But the first event after a reported quiet spell goes through, to clear it.
  if not woke and prev and prev.state == state and (state ~= "blocked" or prev.msg == r.msg) then return end
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
  if tracked(block) then call("_gone", block) end
end

rex.on("terminal.child_exited", gone)
rex.on("block_closed", gone)

-- News that couldn't be typed (the person was typing in the coordinator's
-- input) waits in an outbox; <state>/outbox exists while any does.
rex.every(3, function()
  local f = io.open(state_dir .. "/outbox", "r")
  if f then f:close() call("_flush") end
end)

-- Pull requests past ready (failed checks, people's comments, a merge outside
-- bullpen): <state>/prs exists while any task has one to follow, so with none
-- this costs a file check, not a `gh` call.
local pr_every = tonumber(rex.args.prs or "") or 120 -- seconds
rex.every(pr_every, function()
  local f = io.open(state_dir .. "/prs", "r")
  if f then f:close() call("_prs") end
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
