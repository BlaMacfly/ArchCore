-- AceLocale-3.0 (implémentation ArchRust) : tables de traduction par AddOn.
--   local L = LibStub("AceLocale-3.0"):NewLocale("MonAddOn", "enUS", true)  -- langue par défaut
--   if L then L["Hello"] = true end
--   local L = LibStub("AceLocale-3.0"):NewLocale("MonAddOn", "frFR")
--   if L then L["Hello"] = "Bonjour" end
--   local L = LibStub("AceLocale-3.0"):GetLocale("MonAddOn")
local MAJOR, MINOR = "AceLocale-3.0", 0
local AceLocale = LibStub:NewLibrary(MAJOR, MINOR)
if not AceLocale then return end

local rawset, rawget, setmetatable, getmetatable, tostring, error = rawset, rawget, setmetatable, getmetatable, tostring, error

AceLocale.apps = AceLocale.apps or {}
AceLocale.appnames = AceLocale.appnames or {}

local function gameLocale()
  local l = GAME_LOCALE or GetLocale()
  if l == "enGB" then l = "enUS" end
  return l
end

-- Lecture d'une clé absente : la clé elle-même (avec un avertissement, sauf en mode silencieux).
local readmeta = {
  __index = function(self, key)
    rawset(self, key, key)
    geterrorhandler()(MAJOR .. ": " .. tostring(AceLocale.appnames[self]) .. ": Missing entry for '" .. tostring(key) .. "'")
    return key
  end,
}
local readmetasilent = { __index = function(self, key) rawset(self, key, key) return key end }

local registering
local writeproxy = setmetatable({}, {
  __newindex = function(_, key, value) rawset(registering, key, value == true and key or value) end,
  __index = function() error("AceLocale: proxies can only be written to") end,
})
local writedefaultproxy = setmetatable({}, {
  __newindex = function(_, key, value)
    if not rawget(registering, key) then rawset(registering, key, value == true and key or value) end
  end,
  __index = function() error("AceLocale: proxies can only be written to") end,
})

function AceLocale:NewLocale(application, locale, isDefault, silent)
  local app = AceLocale.apps[application]
  if silent and app and getmetatable(app) ~= readmetasilent then
    error("Usage: NewLocale(application, locale[, isDefault[, silent]]): 'silent' must be specified for the first locale registered", 2)
  end
  if not app then
    if silent == "raw" then app = {} else app = setmetatable({}, silent and readmetasilent or readmeta) end
    AceLocale.apps[application] = app
    AceLocale.appnames[app] = application
  end
  if locale ~= gameLocale() and not isDefault then return end
  registering = app
  if isDefault then return writedefaultproxy end
  return writeproxy
end

function AceLocale:GetLocale(application, silent)
  if not silent and not AceLocale.apps[application] then
    error("Usage: GetLocale(application[, silent]): 'application' - No locales registered for '" .. tostring(application) .. "'", 2)
  end
  return AceLocale.apps[application]
end
