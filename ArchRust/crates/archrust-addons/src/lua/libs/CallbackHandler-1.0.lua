-- CallbackHandler-1.0 (implémentation ArchRust) : registres de rappels pour les bibliothèques.
-- registry = CallbackHandler:New(cible [, "RegisterCallback", "UnregisterCallback", "UnregisterAllCallbacks"])
-- cible.RegisterCallback(objet, "evenement", "methode" | fonction [, argument])
-- registry:Fire("evenement", ...)   registry.OnUsed / registry.OnUnused (premier / dernier abonné)
local MAJOR, MINOR = "CallbackHandler-1.0", 0
local CallbackHandler = LibStub:NewLibrary(MAJOR, MINOR)
if not CallbackHandler then return end

local type, select, next, pairs, rawget, error, setmetatable = type, select, next, pairs, rawget, error, setmetatable

local autotable = { __index = function(t, k) local v = {} t[k] = v return v end }

local function dispatch(handlers, ...)
  local key, fn = next(handlers)
  while fn do
    securecallfunction(fn, ...)
    key, fn = next(handlers, key)
  end
end

function CallbackHandler.New(_lib, target, registerName, unregisterName, unregisterAllName)
  registerName = registerName or "RegisterCallback"
  unregisterName = unregisterName or "UnregisterCallback"
  if unregisterAllName == nil then unregisterAllName = "UnregisterAllCallbacks" end

  local events = setmetatable({}, autotable)
  local registry = { recurse = 0, events = events }

  local function used(eventname)
    if registry.OnUsed then registry.OnUsed(registry, target, eventname) end
  end

  -- `Fire` n'utilise pas `self` : on peut l'exposer directement (ex. `SendMessage`).
  function registry:Fire(eventname, ...)
    if not rawget(events, eventname) or not next(events[eventname]) then return end
    local depth = registry.recurse
    registry.recurse = depth + 1
    dispatch(events[eventname], eventname, ...)
    registry.recurse = depth
    if registry.insertQueue and depth == 0 then
      for ev, callbacks in pairs(registry.insertQueue) do
        local first = not rawget(events, ev) or not next(events[ev])
        for who, fn in pairs(callbacks) do
          events[ev][who] = fn
          if first then used(ev) first = false end
        end
      end
      registry.insertQueue = nil
    end
  end

  target[registerName] = function(self, eventname, method, ...)
    if type(eventname) ~= "string" then
      error("Usage: " .. registerName .. "(eventname, method[, arg]): 'eventname' - string expected.", 2)
    end
    method = method or eventname
    local first = not rawget(events, eventname) or not next(events[eventname])
    local fn
    if type(method) == "string" then
      if type(self) ~= "table" then
        error("Usage: " .. registerName .. "(\"eventname\", \"methodname\"): self was not a table?", 2)
      elseif self == target then
        error("Usage: " .. registerName .. "(\"eventname\", \"methodname\"): do not use Library:" .. registerName .. "(), use your own 'self'", 2)
      elseif type(self[method]) ~= "function" then
        error("Usage: " .. registerName .. "(\"eventname\", \"methodname\"): 'methodname' - method '" .. tostring(method) .. "' not found on self.", 2)
      end
      if select("#", ...) >= 1 then
        local arg = select(1, ...)
        fn = function(...) self[method](self, arg, ...) end
      else
        fn = function(...) self[method](self, ...) end
      end
    elseif type(method) == "function" then
      if type(self) ~= "table" and type(self) ~= "string" and type(self) ~= "thread" then
        error("Usage: " .. registerName .. "(self or \"addonId\", eventname, method): 'self or addonId': table or string or thread expected.", 2)
      end
      if select("#", ...) >= 1 then
        local arg = select(1, ...)
        fn = function(...) method(arg, ...) end
      else
        fn = method
      end
    else
      error("Usage: " .. registerName .. "(eventname, method[, arg]): 'method' - string or function expected.", 2)
    end

    if events[eventname][self] or registry.recurse < 1 then
      events[eventname][self] = fn
      if first then used(eventname) end
    else
      -- Pendant un Fire : l'ajout est différé à la fin de la diffusion.
      registry.insertQueue = registry.insertQueue or setmetatable({}, autotable)
      registry.insertQueue[eventname][self] = fn
    end
  end

  target[unregisterName] = function(self, eventname)
    if not self or self == target then
      error("Usage: " .. unregisterName .. "(eventname): bad 'self'", 2)
    end
    if type(eventname) ~= "string" then
      error("Usage: " .. unregisterName .. "(eventname): 'eventname' - string expected.", 2)
    end
    if rawget(events, eventname) and events[eventname][self] then
      events[eventname][self] = nil
      if registry.OnUnused and not next(events[eventname]) then
        registry.OnUnused(registry, target, eventname)
      end
    end
    if registry.insertQueue and rawget(registry.insertQueue, eventname) then
      registry.insertQueue[eventname][self] = nil
    end
  end

  if unregisterAllName then
    target[unregisterAllName] = function(...)
      local n = select("#", ...)
      if n < 1 or (n == 1 and ... == target) then
        error("Usage: " .. unregisterAllName .. "([whatFor]): missing 'self' or \"addonId\" to unregister events for.", 2)
      end
      for i = 1, n do
        local who = select(i, ...)
        if registry.insertQueue then
          for _, callbacks in pairs(registry.insertQueue) do callbacks[who] = nil end
        end
        for eventname, callbacks in pairs(events) do
          if callbacks[who] then
            callbacks[who] = nil
            if registry.OnUnused and not next(callbacks) then
              registry.OnUnused(registry, target, eventname)
            end
          end
        end
      end
    end
  end

  return registry
end
