-- AceTimer-3.0 (implémentation ArchRust) : minuteurs ponctuels et répétitifs annulables, sur C_Timer.
local MAJOR, MINOR = "AceTimer-3.0", 0
local AceTimer = LibStub:NewLibrary(MAJOR, MINOR)
if not AceTimer then return end

local type, select, error, pairs, unpack = type, select, error, pairs, table.unpack

AceTimer.activeTimers = AceTimer.activeTimers or {}
AceTimer.embeds = AceTimer.embeds or {}
local active = AceTimer.activeTimers

local function schedule(self, loop, func, delay, ...)
  if delay < 0.01 then delay = 0.01 end
  local timer = { object = self, func = func, looping = loop, argsCount = select("#", ...), delay = delay, ends = GetTime() + delay, ... }
  active[timer] = timer
  timer.callback = function()
    if timer.cancelled then return end
    if type(timer.func) == "string" then
      securecall(timer.object[timer.func], timer.object, unpack(timer, 1, timer.argsCount))
    else
      securecall(timer.func, unpack(timer, 1, timer.argsCount))
    end
    if timer.looping and not timer.cancelled then
      local now = GetTime()
      local nextDelay = timer.delay - (now - timer.ends)
      if nextDelay < 0.01 then nextDelay = 0.01 end
      timer.ends = now + nextDelay
      C_Timer.After(nextDelay, timer.callback)
    else
      active[timer] = nil
    end
  end
  C_Timer.After(delay, timer.callback)
  return timer
end

local function check(self, func, delay, usage)
  if not func or not delay then error(usage .. ": 'callback' and 'delay' must have set values.", 3) end
  if type(func) == "string" then
    if type(self) ~= "table" then error(usage .. ": 'self' - must be a table.", 3)
    elseif not self[func] then error(usage .. ": Tried to register '" .. func .. "' as the callback, but it doesn't exist in the module.", 3) end
  end
end

function AceTimer:ScheduleTimer(func, delay, ...)
  check(self, func, delay, "ScheduleTimer(callback, delay, args...)")
  return schedule(self, nil, func, delay, ...)
end

function AceTimer:ScheduleRepeatingTimer(func, delay, ...)
  check(self, func, delay, "ScheduleRepeatingTimer(callback, delay, args...)")
  return schedule(self, true, func, delay, ...)
end

function AceTimer:CancelTimer(id)
  local timer = active[id]
  if not timer then return false end
  timer.cancelled = true
  active[id] = nil
  return true
end

function AceTimer:CancelAllTimers()
  for id, timer in pairs(active) do
    if timer.object == self then AceTimer.CancelTimer(self, id) end
  end
end

function AceTimer:TimeLeft(id)
  local timer = active[id]
  if not timer then return 0 end
  return timer.ends - GetTime()
end

local mixins = { "ScheduleTimer", "ScheduleRepeatingTimer", "CancelTimer", "CancelAllTimers", "TimeLeft" }

function AceTimer:Embed(target)
  self.embeds[target] = true
  for _, name in ipairs(mixins) do target[name] = AceTimer[name] end
  return target
end

function AceTimer:OnEmbedDisable(target) target:CancelAllTimers() end

for target in pairs(AceTimer.embeds) do AceTimer:Embed(target) end
