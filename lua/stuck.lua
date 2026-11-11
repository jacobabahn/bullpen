-- Which panes say they're working but have gone quiet. No Rex calls in here, so
-- the tests can run it with made-up times.
--
--   local quiet = dofile("stuck.lua").new()
--   quiet.event(block, working, now)  -- on every status event; true if it ends a reported spell
--   quiet.due(now, limit)             -- { { block = ..., minutes = ... } }, each spell once
--   quiet.forget(block)               -- the pane is gone
--
-- Times are in seconds. limit(block) is that pane's limit in minutes; 0 is off.

local M = {}

function M.new()
  local panes = {} -- block -> { at = time of its last event, working = ..., told = ... }
  local T = {}

  function T.event(block, working, now)
    local p = panes[block]
    panes[block] = { at = now, working = working, told = false }
    return p ~= nil and p.told
  end

  function T.forget(block)
    panes[block] = nil
  end

  function T.due(now, limit)
    local out = {}
    for block, p in pairs(panes) do
      local minutes = p.working and not p.told and limit(block) or 0
      if minutes > 0 and now - p.at >= minutes * 60 then
        p.told = true
        out[#out + 1] = { block = block, minutes = math.floor((now - p.at) / 60) }
      end
    end
    return out
  end

  return T
end

return M
