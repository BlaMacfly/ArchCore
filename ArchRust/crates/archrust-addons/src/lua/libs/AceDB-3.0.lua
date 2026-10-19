-- AceDB-3.0 (implémentation ArchRust) : base de données d'AddOn sur une SavedVariable, avec
-- sections (profile, global, char, realm, class, race, faction, factionrealm, locale), profils,
-- valeurs par défaut (y compris les jokers "*" et "**"), espaces de noms et rappels
-- (OnNewProfile, OnProfileChanged, OnProfileCopied, OnProfileDeleted, OnProfileReset,
-- OnDatabaseReset, OnProfileShutdown, OnDatabaseShutdown).
-- Les valeurs par défaut ne sont pas écrites dans le fichier : elles sont retirées à PLAYER_LOGOUT.
local MAJOR, MINOR = "AceDB-3.0", 0
local AceDB = LibStub:NewLibrary(MAJOR, MINOR)
if not AceDB then return end

local type, pairs, next, error, rawget, rawset, setmetatable, getmetatable =
      type, pairs, next, error, rawget, rawset, setmetatable, getmetatable

AceDB.db_registry = AceDB.db_registry or {}
AceDB.frame = AceDB.frame or CreateFrame("Frame", "AceDB30Frame")

local CallbackHandler
local CallbackDummy = { Fire = function() end }
local DBObjectLib = {}

local function copyTable(src, dest)
  if type(dest) ~= "table" then dest = {} end
  if type(src) == "table" then
    for k, v in pairs(src) do
      if type(v) == "table" then v = copyTable(v, dest[k]) end
      dest[k] = v
    end
  end
  return dest
end

-- Applique les valeurs par défaut (sans écraser), y compris "*" (toute clé) et "**" (toute clé,
-- héritée aussi par les clés explicites).
local function copyDefaults(dest, src)
  for k, v in pairs(src) do
    if k == "*" or k == "**" then
      if type(v) == "table" then
        setmetatable(dest, {
          __index = function(t, key)
            if key == nil then return nil end
            local tbl = {}
            copyDefaults(tbl, v)
            rawset(t, key, tbl)
            return tbl
          end,
        })
        for dk, dv in pairs(dest) do
          if not rawget(src, dk) and type(dv) == "table" then copyDefaults(dv, v) end
        end
      else
        setmetatable(dest, { __index = function(_, key) if key ~= nil then return v end end })
      end
    elseif type(v) == "table" then
      if not rawget(dest, k) then rawset(dest, k, {}) end
      if type(dest[k]) == "table" then
        copyDefaults(dest[k], v)
        if src["**"] then copyDefaults(dest[k], src["**"]) end
      end
    elseif rawget(dest, k) == nil then
      rawset(dest, k, v)
    end
  end
end

-- Retire ce qui est égal aux valeurs par défaut (et les tables devenues vides).
local function removeDefaults(db, defaults, blocker)
  setmetatable(db, nil)
  for k, v in pairs(defaults) do
    if k == "*" or k == "**" then
      if type(v) == "table" then
        for key, value in pairs(db) do
          if type(value) == "table" then
            if defaults[key] == nil and (not blocker or blocker[key] == nil) then
              removeDefaults(value, v)
              if next(value) == nil then db[key] = nil end
            elseif k == "**" then
              removeDefaults(value, v, defaults[key])
            end
          end
        end
      elseif k == "*" then
        for key, value in pairs(db) do
          if defaults[key] == nil and v == value then db[key] = nil end
        end
      end
    elseif type(v) == "table" and type(db[k]) == "table" then
      removeDefaults(db[k], v, blocker and blocker[k])
      if next(db[k]) == nil then db[k] = nil end
    elseif db[k] == defaults[k] then
      db[k] = nil
    end
  end
end

local function initSection(db, section, svstore, key, defaults)
  local sv = rawget(db, "sv")
  local created
  if not sv[svstore] then sv[svstore] = {} end
  if not sv[svstore][key] then sv[svstore][key] = {} created = true end
  local tbl = sv[svstore][key]
  if defaults then copyDefaults(tbl, defaults) end
  rawset(db, section, tbl)
  return created
end

local dbmt = {
  __index = function(t, section)
    local keys = rawget(t, "keys")
    local key = keys and keys[section]
    if key then
      local defaultTbl = rawget(t, "defaults")
      local defaults = defaultTbl and defaultTbl[section]
      if section == "profile" then
        if initSection(t, section, "profiles", key, defaults) then
          t.callbacks:Fire("OnNewProfile", t, key)
        end
      elseif section == "profiles" then
        local sv = rawget(t, "sv")
        if not sv.profiles then sv.profiles = {} end
        rawset(t, "profiles", sv.profiles)
      elseif section == "global" then
        local sv = rawget(t, "sv")
        if not sv.global then sv.global = {} end
        if defaults then copyDefaults(sv.global, defaults) end
        rawset(t, section, sv.global)
      else
        initSection(t, section, section, key, defaults)
      end
    end
    return rawget(t, section)
  end,
}

local function charKey() return (UnitName("player") or "?") .. " - " .. (GetRealmName() or "?") end

local function makeKeys(profileKey)
  local realm = GetRealmName() or "?"
  local _, class = UnitClass("player")
  local _, race = UnitRace("player")
  local faction = UnitFactionGroup("player") or "Neutral"
  local region = GetCurrentRegionName and GetCurrentRegionName() or "EU"
  return {
    char = charKey(), realm = realm, class = class or "UNKNOWN", race = race or "Unknown",
    faction = faction, factionrealm = faction .. " - " .. realm,
    factionrealmregion = faction .. " - " .. realm .. " - " .. region,
    locale = (GetLocale() or "frFR"):lower(), global = true, profiles = true, profile = profileKey,
  }
end

local function initdb(sv, defaults, defaultProfile, olddb, parent)
  if defaultProfile == true then defaultProfile = "Default" end
  local profileKey
  if not parent then
    if not sv.profileKeys then sv.profileKeys = {} end
    profileKey = sv.profileKeys[charKey()] or defaultProfile or charKey()
    sv.profileKeys[charKey()] = profileKey
  else
    profileKey = parent.keys.profile or defaultProfile or charKey()
  end
  local db = setmetatable(olddb or {}, dbmt)
  if not rawget(db, "callbacks") then
    CallbackHandler = CallbackHandler or LibStub("CallbackHandler-1.0", true)
    db.callbacks = CallbackHandler and CallbackHandler:New(db) or CallbackDummy
  end
  if not parent then
    for name, fn in pairs(DBObjectLib) do db[name] = fn end
  else
    db.RegisterDefaults = DBObjectLib.RegisterDefaults
    db.ResetProfile = DBObjectLib.ResetProfile
  end
  db.keys = makeKeys(profileKey)
  db.sv = sv
  db.defaults = defaults
  db.parent = parent
  AceDB.db_registry[db] = true
  return db
end

local function logoutHandler(_, event)
  if event ~= "PLAYER_LOGOUT" then return end
  for db in pairs(AceDB.db_registry) do
    db.callbacks:Fire("OnDatabaseShutdown", db)
    db:RegisterDefaults(nil)
    local sv = rawget(db, "sv")
    for section in pairs(db.keys) do
      if rawget(sv, section) then
        if section ~= "global" and (section ~= "profiles" or rawget(db, "parent")) then
          for key in pairs(sv[section]) do
            if type(sv[section][key]) == "table" and not next(sv[section][key]) then sv[section][key] = nil end
          end
        end
        if type(sv[section]) == "table" and not next(sv[section]) then sv[section] = nil end
      end
    end
  end
end
AceDB.frame:RegisterEvent("PLAYER_LOGOUT")
AceDB.frame:SetScript("OnEvent", logoutHandler)

-- Méthodes de l'objet base de données

function DBObjectLib:RegisterDefaults(defaults)
  if defaults and type(defaults) ~= "table" then
    error(("Usage: AceDBObject:RegisterDefaults(defaults): 'defaults' - table or nil expected, got %q."):format(type(defaults)), 2)
  end
  local old = self.defaults
  if old then
    for section in pairs(self.keys) do
      if old[section] and rawget(self, section) then removeDefaults(self[section], old[section]) end
    end
  end
  self.defaults = defaults
  if defaults then
    for section in pairs(self.keys) do
      if defaults[section] and rawget(self, section) then copyDefaults(self[section], defaults[section]) end
    end
  end
end

function DBObjectLib:SetProfile(name)
  if type(name) ~= "string" then
    error(("Usage: AceDBObject:SetProfile(name): 'name' - string expected, got %q."):format(type(name)), 2)
  end
  if name == self.keys.profile then return end
  local old = rawget(self, "profile")
  local defaults = self.defaults and self.defaults.profile
  self.callbacks:Fire("OnProfileShutdown", self)
  if old and defaults then removeDefaults(old, defaults) end
  rawset(self, "profile", nil)
  self.keys.profile = name
  if self.sv.profileKeys then self.sv.profileKeys[charKey()] = name end
  if self.children then
    for _, child in pairs(self.children) do DBObjectLib.SetProfile(child, name) end
  end
  self.callbacks:Fire("OnProfileChanged", self, name)
end

function DBObjectLib:GetProfiles(tbl)
  tbl = tbl or {}
  local seen, n = {}, 0
  for _ in pairs(tbl) do n = n + 1 end
  for key in pairs(self.profiles) do
    n = n + 1
    tbl[n] = key
    seen[key] = true
  end
  if not seen[self.keys.profile] then
    n = n + 1
    tbl[n] = self.keys.profile
  end
  return tbl, n
end

function DBObjectLib:GetCurrentProfile() return self.keys.profile end

function DBObjectLib:DeleteProfile(name, silent)
  if self.keys.profile == name then error("Cannot delete the active profile in an AceDBObject.", 2) end
  if not rawget(self.profiles, name) and not silent then
    error(("Cannot delete profile %q as it does not exist."):format(name), 2)
  end
  self.profiles[name] = nil
  if self.children then
    for _, child in pairs(self.children) do DBObjectLib.DeleteProfile(child, name, true) end
  end
  if self.sv.profileKeys then
    for key, profile in pairs(self.sv.profileKeys) do
      if profile == name then self.sv.profileKeys[key] = nil end
    end
  end
  self.callbacks:Fire("OnProfileDeleted", self, name)
end

function DBObjectLib:CopyProfile(name, silent)
  if name == self.keys.profile then error("Cannot have the same source and destination profiles.", 2) end
  if not rawget(self.profiles, name) and not silent then
    error(("Cannot copy profile %q as it does not exist."):format(name), 2)
  end
  DBObjectLib.ResetProfile(self, nil, true)
  local profile = self.profile
  local source = self.profiles[name]
  copyTable(source, profile)
  if self.children then
    for _, child in pairs(self.children) do DBObjectLib.CopyProfile(child, name, true) end
  end
  self.callbacks:Fire("OnProfileCopied", self, name)
end

function DBObjectLib:ResetProfile(noChildren, noCallbacks)
  local profile = self.profile
  for k in pairs(profile) do profile[k] = nil end
  local defaults = self.defaults and self.defaults.profile
  if defaults then copyDefaults(profile, defaults) end
  if self.children and not noChildren then
    for _, child in pairs(self.children) do DBObjectLib.ResetProfile(child, nil, noCallbacks) end
  end
  if not noCallbacks then self.callbacks:Fire("OnProfileReset", self) end
end

function DBObjectLib:ResetDB(defaultProfile)
  if defaultProfile and type(defaultProfile) ~= "string" and defaultProfile ~= true then
    error("Usage: AceDBObject:ResetDB(defaultProfile): 'defaultProfile' - string or true expected.", 2)
  end
  local sv = self.sv
  for k in pairs(sv) do sv[k] = nil end
  initdb(sv, self.defaults, defaultProfile, self)
  if self.children then
    if not sv.namespaces then sv.namespaces = {} end
    for name, child in pairs(self.children) do
      if not sv.namespaces[name] then sv.namespaces[name] = {} end
      initdb(sv.namespaces[name], child.defaults, self.keys.profile, child, self)
    end
  end
  self.callbacks:Fire("OnDatabaseReset", self)
  self.callbacks:Fire("OnProfileChanged", self, self.keys.profile)
  return self
end

function DBObjectLib:RegisterNamespace(name, defaults)
  if type(name) ~= "string" then error("Usage: AceDBObject:RegisterNamespace(name, defaults): 'name' - string expected.", 2) end
  if self.children and self.children[name] then
    error("Usage: AceDBObject:RegisterNamespace(name, defaults): 'name' - a namespace called '" .. name .. "' already exists.", 2)
  end
  local sv = self.sv
  if not sv.namespaces then sv.namespaces = {} end
  if not sv.namespaces[name] then sv.namespaces[name] = {} end
  local child = initdb(sv.namespaces[name], defaults, self.keys.profile, nil, self)
  if not self.children then self.children = {} end
  self.children[name] = child
  return child
end

function DBObjectLib:GetNamespace(name, silent)
  if not silent and not (self.children and self.children[name]) then
    error("Usage: AceDBObject:GetNamespace(name): 'name' - namespace '" .. tostring(name) .. "' does not exist.", 2)
  end
  return self.children and self.children[name]
end

-- AceDB:New(svOuNom, valeursParDefaut, profilParDefaut)
function AceDB:New(tbl, defaults, defaultProfile)
  if type(tbl) == "string" then
    local name = tbl
    tbl = _G[name]
    if not tbl then
      tbl = {}
      _G[name] = tbl
    end
  end
  if type(tbl) ~= "table" then
    error(("Usage: AceDB:New(tbl, defaults, defaultProfile): 'tbl' - table expected, got %q."):format(type(tbl)), 2)
  end
  if defaults and type(defaults) ~= "table" then
    error(("Usage: AceDB:New(tbl, defaults, defaultProfile): 'defaults' - table expected, got %q."):format(type(defaults)), 2)
  end
  if defaultProfile and type(defaultProfile) ~= "string" and defaultProfile ~= true then
    error("Usage: AceDB:New(tbl, defaults, defaultProfile): 'defaultProfile' - string or true expected.", 2)
  end
  return initdb(tbl, defaults, defaultProfile)
end

-- Mise à niveau : objets existants.
for db in pairs(AceDB.db_registry) do
  if not db.parent then
    for name, fn in pairs(DBObjectLib) do db[name] = fn end
  else
    db.RegisterDefaults = DBObjectLib.RegisterDefaults
    db.ResetProfile = DBObjectLib.ResetProfile
  end
end
