-- AceHook-3.0 (implémentation ArchRust) : crochets sur fonctions, méthodes et scripts de frames.
--   Hook(obj, "m", h)        le gestionnaire est appelé avant l'original (sûr)
--   RawHook(obj, "m", h)     le gestionnaire remplace l'original (appeler self.hooks[obj].m)
--   SecureHook(obj, "m", h)  le gestionnaire est appelé après l'original (hooksecurefunc)
--   HookScript / RawHookScript / SecureHookScript : idem pour les scripts de frames
--   Unhook, UnhookAll, IsHooked ; les originaux sont dans self.hooks[obj][m] ou self.hooks[m].
local MAJOR, MINOR = "AceHook-3.0", 0
local AceHook = LibStub:NewLibrary(MAJOR, MINOR)
if not AceHook then return end

local type, pairs, next, error, format = type, pairs, next, error, string.format

AceHook.embeded = AceHook.embeded or {}
AceHook.registry = AceHook.registry or setmetatable({}, { __index = function(t, k) local v = {} t[k] = v return v end })
AceHook.handlers = AceHook.handlers or {}
AceHook.actives = AceHook.actives or {}
AceHook.scripts = AceHook.scripts or {}
AceHook.hooks = AceHook.hooks or {}

local registry, handlers, actives, scripts = AceHook.registry, AceHook.handlers, AceHook.actives, AceHook.scripts

local function createHook(self, handler, orig, secure, failsafe)
  local uid
  local isMethod = type(handler) == "string"
  if failsafe and not secure then
    uid = function(...)
      if actives[uid] then
        if isMethod then self[handler](self, ...) else handler(...) end
      end
      return orig(...)
    end
  else
    uid = function(...)
      if actives[uid] then
        if isMethod then return self[handler](self, ...) end
        return handler(...)
      elseif not secure then
        return orig(...)
      end
    end
  end
  return uid
end

local function hook(self, obj, method, handler, script, secure, raw, usage)
  if not handler then handler = method end
  if obj and type(obj) ~= "table" then error(format("%s: 'object' - nil or table expected got %s", usage, type(obj)), 3) end
  if type(method) ~= "string" then error(format("%s: 'method' - string expected got %s", usage, type(method)), 3) end
  if type(handler) == "string" and type(self[handler]) ~= "function" then
    error(format("%s: 'handler' - Handler specified does not exist at self[handler]", usage), 3)
  end
  local uid
  if obj then uid = registry[self][obj] and registry[self][obj][method] else uid = registry[self][method] end
  if uid then
    if actives[uid] then
      error(format("Attempting to rehook already active hook %s.", method), 3)
    end
    if handlers[uid] == handler then
      actives[uid] = true
      return
    end
    self:Unhook(obj or method, obj and method or nil)
  end
  local orig
  if script then
    if not obj or not obj.GetScript then error(format("%s: You can only hook a script on a frame object", usage), 3) end
    orig = obj:GetScript(method) or function() end
  else
    orig = (obj or _G)[method]
  end
  if type(orig) ~= "function" then
    error(format("%s: Attempting to hook a non existing target", usage), 3)
  end
  uid = createHook(self, handler, orig, secure, not (raw or secure))
  if obj then
    self.hooks[obj] = self.hooks[obj] or {}
    self.hooks[obj][method] = orig
    if script then
      if secure then obj:HookScript(method, uid) else obj:SetScript(method, uid) end
    elseif secure then
      hooksecurefunc(obj, method, uid)
    else
      obj[method] = uid
    end
    registry[self][obj] = registry[self][obj] or {}
    registry[self][obj][method] = uid
  else
    self.hooks[method] = orig
    if secure then hooksecurefunc(method, uid) else _G[method] = uid end
    registry[self][method] = uid
  end
  actives[uid], handlers[uid], scripts[uid] = true, handler, script and true or nil
end

local function args(object, method, handler)
  if type(object) == "string" then return nil, object, method end
  return object, method, handler
end

function AceHook:Hook(object, method, handler, hookSecure)
  object, method, handler = args(object, method, handler)
  hook(self, object, method, handler, false, hookSecure, false, "Usage: Hook([object], method, [handler], [hookSecure])")
end
function AceHook:RawHook(object, method, handler, hookSecure)
  object, method, handler = args(object, method, handler)
  hook(self, object, method, handler, false, false, true, "Usage: RawHook([object], method, [handler], [hookSecure])")
end
function AceHook:SecureHook(object, method, handler)
  object, method, handler = args(object, method, handler)
  hook(self, object, method, handler, false, true, false, "Usage: SecureHook([object], method, [handler])")
end
function AceHook:HookScript(frame, script, handler)
  hook(self, frame, script, handler, true, false, false, "Usage: HookScript(object, method, [handler])")
end
function AceHook:RawHookScript(frame, script, handler)
  hook(self, frame, script, handler, true, false, true, "Usage: RawHookScript(object, method, [handler])")
end
function AceHook:SecureHookScript(frame, script, handler)
  hook(self, frame, script, handler, true, true, false, "Usage: SecureHookScript(object, method, [handler])")
end

function AceHook:Unhook(obj, method)
  if type(obj) == "string" then method, obj = obj, nil end
  local uid
  if obj then uid = registry[self][obj] and registry[self][obj][method] else uid = registry[self][method] end
  if not uid or not actives[uid] then return false end
  actives[uid], handlers[uid] = nil, nil
  if obj then
    registry[self][obj][method] = nil
    if not next(registry[self][obj]) then registry[self][obj] = nil end
    local orig = self.hooks[obj] and self.hooks[obj][method]
    if scripts[uid] then
      if obj:GetScript(method) == uid then obj:SetScript(method, orig) end
    elseif rawget(obj, method) == uid then
      obj[method] = orig
    end
    if self.hooks[obj] then
      self.hooks[obj][method] = nil
      if not next(self.hooks[obj]) then self.hooks[obj] = nil end
    end
  else
    registry[self][method] = nil
    if _G[method] == uid then _G[method] = self.hooks[method] end
    self.hooks[method] = nil
  end
  scripts[uid] = nil
  return true
end

function AceHook:UnhookAll()
  for key, value in pairs(registry[self]) do
    if type(key) == "table" then
      for method in pairs(value) do self:Unhook(key, method) end
    else
      self:Unhook(key)
    end
  end
end

function AceHook:IsHooked(obj, method)
  if type(obj) == "string" then
    if registry[self][obj] and actives[registry[self][obj]] then return true, handlers[registry[self][obj]] end
  elseif registry[self][obj] and registry[self][obj][method] and actives[registry[self][obj][method]] then
    return true, handlers[registry[self][obj][method]]
  end
  return false, nil
end

local mixins = { "Hook", "SecureHook", "HookScript", "SecureHookScript", "Unhook", "UnhookAll", "IsHooked", "RawHook", "RawHookScript" }

function AceHook:Embed(target)
  for _, name in ipairs(mixins) do target[name] = self[name] end
  self.embeded[target] = true
  target.hooks = target.hooks or {}
  return target
end

function AceHook:OnEmbedDisable(target) target:UnhookAll() end

for target in pairs(AceHook.embeded) do AceHook:Embed(target) end
