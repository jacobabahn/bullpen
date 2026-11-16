-- bullpen key bindings and actions for your Rex config (init.lua).
--
--   dofile("/path/to/bullpen/lua/keys.lua").setup{ bin = "/path/to/bullpen/bin/bullpen" }
--
-- `bullpen keys` prints that line with your paths filled in. It defines five
-- actions, which also show in the command palette, and binds a key to each:
--
--   bullpen_next         cmd+;>n  the next task that needs you (again: the one after)
--   bullpen_board        cmd+;>b  the task board
--   bullpen_coordinator  cmd+;>c  the coordinator
--   bullpen_jump         cmd+;>j  type part of any open task, in any repository, and go to it
--   bullpen_mission      cmd+;>m  mission control: every repository on one screen
--
-- and ctrl+escape, in a worker's or reviewer's floating pane, puts it away (back
-- into its capsule, or into the background) and returns to the board. Anywhere
-- else the key goes through.
--
-- Options: prefix = "ctrl+b" moves all five behind another key;
-- keys = { next = "cmd+shift+j", hide = "cmd+." } sets single ones (false leaves one unbound;
-- hide also takes a list of keys);
-- state = "..." when you set BULLPEN_STATE.

local M = {}

function M.setup(opts)
  opts = opts or {}
  local bin = assert(opts.bin, "bullpen keys.lua: setup{ bin = ... } is required")
  local prefix = opts.prefix or "cmd+;"
  local keys = opts.keys or {}
  local env = opts.state and { BULLPEN_STATE = opts.state } or nil

  -- `bullpen go` works out which pane (from the one that's focused) and focuses it;
  -- `bullpen pick --pane` opens a picker over it that goes to the task you choose.
  local function go(ctx, cmd, flag)
    local argv = { bin, cmd, "--from", ctx.block_id or "", "--session", ctx.session_id or "" }
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
    { name = "bullpen_next", title = "Bullpen: Next Task Needing You", cmd = "go", flag = "--next", key = "n" },
    { name = "bullpen_board", title = "Bullpen: Task Board", cmd = "go", flag = "--board", key = "b" },
    { name = "bullpen_coordinator", title = "Bullpen: Coordinator", cmd = "go", flag = nil, key = "c" },
    { name = "bullpen_jump", title = "Bullpen: Jump to a Task", cmd = "pick", flag = "--pane", key = "j" },
    { name = "bullpen_mission", title = "Bullpen: Mission Control", cmd = "mission", flag = nil, key = "m" },
  }
  for _, a in ipairs(actions) do
    rex.action{
      name = a.name,
      title = a.title,
      category = "Bullpen",
      run = function(ctx) return go(ctx, a.cmd, a.flag) end,
    }
    local short = a.name:gsub("^bullpen_", "")
    local key = keys[short]
    if key == nil then key = prefix .. ">" .. a.key end
    -- Bound as a function: the Mac app ignored keys bound to these actions by name.
    if key then rex.bind(key, function(ctx) go(ctx, a.cmd, a.flag) return true end) end
  end

  -- A function, not an action, so the key reaches the terminal when the pane
  -- isn't one of bullpen's (bullpen hide exits 3 then).
  local hide = keys.hide
  if hide == nil then hide = "ctrl+escape" end
  if type(hide) == "string" then hide = { hide } end
  for _, key in ipairs(hide or {}) do
    rex.bind(key, function(ctx)
      if not ctx.block_id then return false end
      local r = rex.exec({ bin, "hide", "--from", ctx.block_id, "--session", ctx.session_id or "" },
                         { env = env, timeout = 10 })
      return r ~= nil and r.ok
    end)
  end
end

return M
