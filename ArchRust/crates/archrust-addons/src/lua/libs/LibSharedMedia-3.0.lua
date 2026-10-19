-- LibSharedMedia-3.0 (implémentation ArchRust) : registre partagé de polices, textures de barres,
-- fonds, bordures et sons. Les chemins `Interface\AddOns\<AddOn>\…` désignent les fichiers des
-- AddOns ; les polices `.ttf`/`.otf` sont chargées par le client.
local MAJOR, MINOR = "LibSharedMedia-3.0", 0
local lib = LibStub:NewLibrary(MAJOR, MINOR)
if not lib then return end

local type, pairs, next, sort = type, pairs, next, table.sort

lib.callbacks = lib.callbacks or LibStub("CallbackHandler-1.0"):New(lib)
lib.DefaultMedia = lib.DefaultMedia or {}
lib.MediaList = lib.MediaList or {}
lib.MediaTable = lib.MediaTable or {}
lib.OverrideMedia = lib.OverrideMedia or {}
lib.MediaType = {
  BACKGROUND = "background", BORDER = "border", FONT = "font", STATUSBAR = "statusbar", SOUND = "sound",
}
lib.LOCALE_BIT_koKR, lib.LOCALE_BIT_ruRU, lib.LOCALE_BIT_zhCN, lib.LOCALE_BIT_zhTW, lib.LOCALE_BIT_western = 1, 2, 4, 8, 128

function lib:Register(mediatype, key, data)
  if type(mediatype) ~= "string" then error(MAJOR .. ":Register(mediatype, key, data) - mediatype must be string, got " .. type(mediatype)) end
  if type(key) ~= "string" then error(MAJOR .. ":Register(mediatype, key, data) - key must be string, got " .. type(key)) end
  mediatype = mediatype:lower()
  if not self.MediaTable[mediatype] then self.MediaTable[mediatype] = {} end
  local mtable = self.MediaTable[mediatype]
  if mtable[key] then return false end
  mtable[key] = data
  self.MediaList[mediatype] = nil
  self.callbacks:Fire("LibSharedMedia_Registered", mediatype, key)
  return true
end

function lib:Fetch(mediatype, key, noDefault)
  local mtt = self.MediaTable[mediatype]
  local override = self.OverrideMedia[mediatype]
  local result = mtt and ((override and mtt[override]) or mtt[key])
  if result == nil and not noDefault then
    local default = self.DefaultMedia[mediatype]
    result = default and mtt and mtt[default]
  end
  return result
end

function lib:IsValid(mediatype, key)
  local t = self.MediaTable[mediatype]
  return t ~= nil and (not key or t[key] ~= nil)
end

function lib:HashTable(mediatype) return self.MediaTable[mediatype] end

function lib:List(mediatype)
  local t = self.MediaTable[mediatype]
  if not t then return nil end
  if not self.MediaList[mediatype] then
    local list = {}
    for k in pairs(t) do list[#list + 1] = k end
    sort(list)
    self.MediaList[mediatype] = list
  end
  return self.MediaList[mediatype]
end

function lib:GetGlobal(mediatype) return self.OverrideMedia[mediatype] end
function lib:SetGlobal(mediatype, key)
  if not self.MediaTable[mediatype] then return false end
  self.OverrideMedia[mediatype] = (key and self.MediaTable[mediatype][key]) and key or nil
  self.callbacks:Fire("LibSharedMedia_SetGlobal", mediatype, self.OverrideMedia[mediatype])
  return true
end
function lib:GetDefault(mediatype) return self.DefaultMedia[mediatype] end
function lib:SetDefault(mediatype, key)
  if self.MediaTable[mediatype] and self.MediaTable[mediatype][key] and not self.DefaultMedia[mediatype] then
    self.DefaultMedia[mediatype] = key
    return true
  end
  return false
end

-- Médias de base du client.
lib:Register("font", "Friz Quadrata TT", "Fonts\\FRIZQT__.TTF")
lib:Register("font", "ArchRust", "Fonts\\FRIZQT__.TTF")
lib:Register("statusbar", "Blizzard", "Interface\\TargetingFrame\\UI-StatusBar")
lib:Register("statusbar", "Solid", "Interface\\Buttons\\WHITE8X8")
lib:Register("background", "Solid", "Interface\\Buttons\\WHITE8X8")
lib:Register("background", "None", "")
lib:Register("border", "None", "")
lib:Register("sound", "None", "")
lib:SetDefault("font", "Friz Quadrata TT")
lib:SetDefault("statusbar", "Blizzard")
lib:SetDefault("background", "None")
lib:SetDefault("border", "None")
lib:SetDefault("sound", "None")
