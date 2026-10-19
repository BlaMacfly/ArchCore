-- LibStub : registre de bibliothèques versionnées (même contrat que le LibStub du domaine public
-- utilisé par les AddOns de WoW). Une bibliothèque n'est remplacée que par une version plus récente.
local MAJOR, MINOR = "LibStub", 2
local LibStub = _G[MAJOR]

if not LibStub or LibStub.minor < MINOR then
  LibStub = LibStub or { libs = {}, minors = {} }
  _G[MAJOR] = LibStub
  LibStub.minor = MINOR

  function LibStub:NewLibrary(major, minor)
    assert(type(major) == "string", "Bad argument #2 to `NewLibrary' (string expected)")
    minor = assert(tonumber(string.match(tostring(minor), "%d+")),
      "Minor version must either be a number or contain a number.")
    local old = self.minors[major]
    if old and old >= minor then return nil end
    self.minors[major], self.libs[major] = minor, self.libs[major] or {}
    return self.libs[major], old
  end

  function LibStub:GetLibrary(major, silent)
    if not self.libs[major] and not silent then
      error(("Cannot find a library instance of %q."):format(tostring(major)), 2)
    end
    return self.libs[major], self.minors[major]
  end

  function LibStub:IterateLibraries() return pairs(self.libs) end

  setmetatable(LibStub, { __call = LibStub.GetLibrary })
end

-- ArchRust : les bibliothèques intégrées au client se chargent à la première demande, sauf si un
-- AddOn en a déjà fourni une version (elle est alors prioritaire).
local builtin = ArchRust and ArchRust.BuiltinLibraries
if builtin then
  local get = LibStub.GetLibrary
  function LibStub:GetLibrary(major, silent)
    if not self.libs[major] then
      local load = builtin[major]
      if load then load() end
    end
    return get(self, major, silent)
  end
  getmetatable(LibStub).__call = LibStub.GetLibrary
end
