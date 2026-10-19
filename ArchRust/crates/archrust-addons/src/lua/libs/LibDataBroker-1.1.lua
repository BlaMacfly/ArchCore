-- LibDataBroker-1.1 (implémentation ArchRust) : objets de données partagés entre AddOns (barres
-- d'informations, lanceurs). Chaque modification d'attribut déclenche des rappels :
--   LibDataBroker_DataObjectCreated(nom, objet)
--   LibDataBroker_AttributeChanged[_<nom>[_<attribut>]] / LibDataBroker_AttributeChanged__<attribut>
local MAJOR, MINOR = "LibDataBroker-1.1", 0
local lib = LibStub:NewLibrary(MAJOR, MINOR)
if not lib then return end

lib.callbacks = lib.callbacks or LibStub("CallbackHandler-1.0"):New(lib)
lib.attributestorage = lib.attributestorage or {}
lib.namestorage = lib.namestorage or {}
lib.proxystorage = lib.proxystorage or {}
local attributes, names, callbacks = lib.attributestorage, lib.namestorage, lib.callbacks

lib.domt = {
  __metatable = "access denied",
  __index = function(self, key) return attributes[self] and attributes[self][key] end,
  __newindex = function(self, key, value)
    if not attributes[self] then attributes[self] = {} end
    if attributes[self][key] == value then return end
    attributes[self][key] = value
    local name = names[self]
    if not name then return end
    callbacks:Fire("LibDataBroker_AttributeChanged", name, key, value, self)
    callbacks:Fire("LibDataBroker_AttributeChanged_" .. name, name, key, value, self)
    callbacks:Fire("LibDataBroker_AttributeChanged_" .. name .. "_" .. key, name, key, value, self)
    callbacks:Fire("LibDataBroker_AttributeChanged__" .. key, name, key, value, self)
  end,
}

function lib:NewDataObject(name, dataobj)
  if self.proxystorage[name] then return end
  if dataobj then
    assert(type(dataobj) == "table", "Invalid dataobj, must be nil or a table")
    attributes[dataobj] = {}
    for k, v in pairs(dataobj) do
      attributes[dataobj][k] = v
      dataobj[k] = nil
    end
  end
  dataobj = setmetatable(dataobj or {}, self.domt)
  self.proxystorage[name], names[dataobj] = dataobj, name
  callbacks:Fire("LibDataBroker_DataObjectCreated", name, dataobj)
  return dataobj
end

function lib:DataObjectIterator() return pairs(self.proxystorage) end
function lib:GetDataObjectByName(name) return self.proxystorage[name] end
function lib:GetNameByDataObject(obj) return names[obj] end

function lib:pairs(obj)
  if type(obj) == "string" then obj = self.proxystorage[obj] end
  assert(attributes[obj], "Data object not found")
  return next, attributes[obj], nil
end

function lib:ipairs(obj)
  if type(obj) == "string" then obj = self.proxystorage[obj] end
  assert(attributes[obj], "Data object not found")
  return ipairs(attributes[obj])
end
