-- AceAddon-3.0 (implémentation ArchRust) : objets AddOn avec modules, bibliothèques embarquées et
-- cycle de vie OnInitialize (ADDON_LOADED) → OnEnable (PLAYER_LOGIN) → OnDisable.
local MAJOR, MINOR = "AceAddon-3.0", 0
local AceAddon = LibStub:NewLibrary(MAJOR, MINOR)
if not AceAddon then return end

local type, select, pairs, tostring, error, setmetatable, getmetatable = type, select, pairs, tostring, error, setmetatable, getmetatable
local tinsert, tremove, fmt = table.insert, table.remove, string.format

AceAddon.frame = AceAddon.frame or CreateFrame("Frame", "AceAddon30Frame")
AceAddon.addons = AceAddon.addons or {}
AceAddon.statuses = AceAddon.statuses or {}
AceAddon.initializequeue = AceAddon.initializequeue or {}
AceAddon.enablequeue = AceAddon.enablequeue or {}
AceAddon.embeds = AceAddon.embeds or setmetatable({}, { __index = function(t, k) local v = {} t[k] = v return v end })

local function safecall(fn, ...)
  if type(fn) == "function" then return securecallfunction(fn, ...) end
end

local function queued(addon)
  for _, a in ipairs(AceAddon.initializequeue) do if a == addon then return true end end
  return false
end

local mixins = {}
local defaults = { defaultModuleState = true, enabledState = true }

local function embed(target, skipDefaults)
  for k, v in pairs(mixins) do target[k] = v end
  if not skipDefaults then
    for k, v in pairs(defaults) do if target[k] == nil then target[k] = v end end
  end
end

function AceAddon:NewAddon(objectOrName, ...)
  local object, name, first = nil, nil, 1
  if type(objectOrName) == "table" then
    object, name, first = objectOrName, ..., 2
  else
    name = objectOrName
  end
  if type(name) ~= "string" then
    error(fmt("Usage: NewAddon([object,] name, [lib, lib, lib, ...]): 'name' - string expected got '%s'.", type(name)), 2)
  end
  if self.addons[name] then
    error(fmt("Usage: NewAddon([object,] name, [lib, lib, lib, ...]): 'name' - Addon '%s' already exists.", name), 2)
  end
  object = object or {}
  object.name = name
  local meta = {}
  local old = getmetatable(object)
  if old then for k, v in pairs(old) do meta[k] = v end end
  meta.__tostring = function() return name end
  setmetatable(object, meta)
  self.addons[name] = object
  object.modules = {}
  object.orderedModules = {}
  object.defaultModuleLibraries = {}
  embed(object)
  self:EmbedLibraries(object, select(first, ...))
  tinsert(self.initializequeue, object)
  return object
end

function AceAddon:GetAddon(name, silent)
  if not silent and not self.addons[name] then
    error(fmt("Usage: GetAddon(name): 'name' - Cannot find an AceAddon '%s'.", tostring(name)), 2)
  end
  return self.addons[name]
end

function AceAddon:EmbedLibraries(addon, ...)
  for i = 1, select("#", ...) do
    self:EmbedLibrary(addon, (select(i, ...)), false, 4)
  end
end

function AceAddon:EmbedLibrary(addon, libname, silent, offset)
  local lib = LibStub:GetLibrary(libname, true)
  if not lib then
    if not silent then
      error(fmt("Usage: EmbedLibrary(addon, libname, silent, offset): 'libname' - Cannot find a library instance of %q.", tostring(libname)), offset or 2)
    end
    return false
  elseif type(lib.Embed) ~= "function" then
    error(fmt("Usage: EmbedLibrary(addon, libname, silent, offset): 'libname' - Library '%s' is not Embed capable", libname), offset or 2)
  end
  lib:Embed(addon)
  tinsert(self.embeds[addon], libname)
  return true
end

function mixins:GetModule(name, silent)
  if not self.modules[name] and not silent then
    error(fmt("Usage: GetModule(name, silent): 'name' - Cannot find module '%s'.", tostring(name)), 2)
  end
  return self.modules[name]
end

local function isModule() return true end

function mixins:NewModule(name, prototype, ...)
  if type(name) ~= "string" then error(fmt("Usage: NewModule(name, [prototype, [lib, lib, lib, ...]): 'name' - string expected got '%s'.", type(name)), 2) end
  if type(prototype) ~= "string" and type(prototype) ~= "table" and prototype ~= nil then
    error(fmt("Usage: NewModule(name, [prototype, [lib, lib, lib, ...]): 'prototype' - table (prototype), string (lib) or nil expected got '%s'.", type(prototype)), 2)
  end
  if self.modules[name] then error(fmt("Usage: NewModule(name, [prototype, [lib, lib, lib, ...]): 'name' - Module '%s' already exists.", name), 2) end
  local module = AceAddon:NewAddon(fmt("%s_%s", self.name or tostring(self), name))
  module.IsModule = isModule
  module:SetEnabledState(self.defaultModuleState)
  module.moduleName = name
  if type(prototype) == "string" then
    AceAddon:EmbedLibraries(module, prototype, ...)
  else
    AceAddon:EmbedLibraries(module, ...)
  end
  AceAddon:EmbedLibraries(module, table.unpack(self.defaultModuleLibraries))
  if not prototype or type(prototype) == "string" then prototype = self.defaultModulePrototype end
  if type(prototype) == "table" then
    local mt = getmetatable(module)
    mt.__index = prototype
    setmetatable(module, mt)
  end
  safecall(self.OnModuleCreated, self, module)
  self.modules[name] = module
  tinsert(self.orderedModules, module)
  return module
end

function mixins:GetName() return self.moduleName or self.name end

function mixins:Enable()
  self:SetEnabledState(true)
  if not queued(self) then return AceAddon:EnableAddon(self) end
end

function mixins:Disable()
  self:SetEnabledState(false)
  return AceAddon:DisableAddon(self)
end

function mixins:EnableModule(name) return self:GetModule(name):Enable() end
function mixins:DisableModule(name) return self:GetModule(name):Disable() end

function mixins:SetDefaultModuleLibraries(...)
  if next(self.modules) then error("Usage: SetDefaultModuleLibraries(...): cannot change the module defaults after a module has been registered.", 2) end
  self.defaultModuleLibraries = { ... }
end
function mixins:SetDefaultModuleState(state)
  if next(self.modules) then error("Usage: SetDefaultModuleState(state): cannot change the module defaults after a module has been registered.", 2) end
  self.defaultModuleState = state
end
function mixins:SetDefaultModulePrototype(prototype)
  if next(self.modules) then error("Usage: SetDefaultModulePrototype(prototype): cannot change the module defaults after a module has been registered.", 2) end
  self.defaultModulePrototype = prototype
end
function mixins:SetEnabledState(state) self.enabledState = state end
function mixins:IterateModules() return pairs(self.modules) end
function mixins:IterateEmbeds() return pairs(AceAddon.embeds[self]) end
function mixins:IsEnabled() return self.enabledState end

function AceAddon:InitializeAddon(addon)
  safecall(addon.OnInitialize, addon)
  for _, libname in ipairs(self.embeds[addon]) do
    local lib = LibStub:GetLibrary(libname, true)
    if lib then safecall(lib.OnEmbedInitialize, lib, addon) end
  end
end

function AceAddon:EnableAddon(addon)
  if type(addon) == "string" then addon = AceAddon:GetAddon(addon) end
  if self.statuses[addon.name] or not addon.enabledState then return false end
  self.statuses[addon.name] = true
  safecall(addon.OnEnable, addon)
  if self.statuses[addon.name] then
    for _, libname in ipairs(self.embeds[addon]) do
      local lib = LibStub:GetLibrary(libname, true)
      if lib then safecall(lib.OnEmbedEnable, lib, addon) end
    end
    for _, module in ipairs(addon.orderedModules) do self:EnableAddon(module) end
  end
  return self.statuses[addon.name]
end

function AceAddon:DisableAddon(addon)
  if type(addon) == "string" then addon = AceAddon:GetAddon(addon) end
  if not self.statuses[addon.name] then return false end
  self.statuses[addon.name] = false
  safecall(addon.OnDisable, addon)
  if not self.statuses[addon.name] then
    for _, libname in ipairs(self.embeds[addon]) do
      local lib = LibStub:GetLibrary(libname, true)
      if lib then safecall(lib.OnEmbedDisable, lib, addon) end
    end
    for _, module in ipairs(addon.orderedModules) do self:DisableAddon(module) end
  end
  return not self.statuses[addon.name]
end

function AceAddon:IterateAddons() return pairs(self.addons) end
function AceAddon:IterateAddonStatus() return pairs(self.statuses) end
function AceAddon:IterateEmbedsOnAddon(addon) return pairs(self.embeds[addon]) end
function AceAddon:IterateModulesOfAddon(addon) return pairs(addon.modules) end

local function onEvent(_, event, arg1)
  if event == "ADDON_LOADED" or event == "PLAYER_LOGIN" then
    while #AceAddon.initializequeue > 0 do
      local addon = tremove(AceAddon.initializequeue, 1)
      if event == "ADDON_LOADED" then addon.baseName = arg1 end
      AceAddon:InitializeAddon(addon)
      tinsert(AceAddon.enablequeue, addon)
    end
    if IsLoggedIn() then
      while #AceAddon.enablequeue > 0 do
        AceAddon:EnableAddon(tremove(AceAddon.enablequeue, 1))
      end
    end
  end
end

AceAddon.frame:RegisterEvent("ADDON_LOADED")
AceAddon.frame:RegisterEvent("PLAYER_LOGIN")
AceAddon.frame:SetScript("OnEvent", onEvent)

for _, addon in pairs(AceAddon.addons) do embed(addon, true) end
