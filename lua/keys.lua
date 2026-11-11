-- bullpen key bindings and actions for your Rex config (init.lua).
--
--   dofile("/path/to/bullpen/lua/keys.lua").setup{ bin = "/path/to/bullpen/bin/bullpen" }
--
-- `bullpen keys` prints that line with your paths filled in. It defines three
-- actions, which also show in the command palette, and binds a key to each:
--
--   bullpen_next         cmd+;>n  the next task that needs you (again: the one after)
--   bullpen_board        cmd+;>b  the task board
--   bullpen_coordinator  cmd+;>c  the coordinator
--
-- and cmd+escape, in a worker's or reviewer's floating pane, puts it back into
-- its capsule and returns to the board. Anywhere else the key goes through.
--
-- Options: prefix = "ctrl+b" moves all three behind another key;
-- keys = { next = "cmd+shift+j", hide = "cmd+." } sets single ones (false leaves one unbound);
-- state = "..." when you set BULLPEN_STATE.

local M = {}

function M.setup(opts)
  opts = opts or {}
  local bin = assert(opts.bin, "bullpen keys.lua: setup{ bin = ... } is required")
  local prefix = opts.prefix or "cmd+;"
  local keys = opts.keys or {}
  local env = opts.state and { BULLPEN_STATE = opts.state } or nil

  -- `bullpen go` works out which pane (from the one that's focused) and focuses it.
  local function go(ctx, flag)
    local argv = { bin, "go", "--from", ctx.block_id or "", "--session", ctx.session_id or "" }
    if flag then argv[#argv + 1] = flag end
    local r, err = rex.exec(argv, { env = env, timeout = 10 })
    if not r then
      rex.log("warn", "bullpen: " .. tostring(err))
      return nil, err
    end
    if not r.ok then
      local why = (r.stderr or ""):gsub("^bullpen: ", ""):gsub("%s+$", "")
      rex.log("info", "bullpen: " .. why)
      return nil, why
    end
    return true
  end

  local actions = {
    { name = "bullpen_next", title = "Bullpen: Next Task Needing You", flag = "--next", key = "n" },
    { name = "bullpen_board", title = "Bullpen: Task Board", flag = "--board", key = "b" },
    { name = "bullpen_coordinator", title = "Bullpen: Coordinator", flag = nil, key = "c" },
  }
  for _, a in ipairs(actions) do
    rex.action{
      name = a.name,
      title = a.title,
      category = "Bullpen",
      run = function(ctx) return go(ctx, a.flag) end,
    }
    local short = a.name:gsub("^bullpen_", "")
    local key = keys[short]
    if key == nil then key = prefix .. ">" .. a.key end
    if key then rex.bind(key, a.name) end
  end

  -- A function, not an action, so the key reaches the terminal when the pane
  -- isn't one of bullpen's (bullpen hide exits 3 then).
  local hide = keys.hide
  if hide == nil then hide = "cmd+escape" end
  if hide then
    rex.bind(hide, function(ctx)
      if not ctx.block_id then return false end
      local r = rex.exec({ bin, "hide", "--from", ctx.block_id, "--session", ctx.session_id or "" },
                         { env = env, timeout = 10 })
      return r ~= nil and r.ok
    end)
  end
end

return M
