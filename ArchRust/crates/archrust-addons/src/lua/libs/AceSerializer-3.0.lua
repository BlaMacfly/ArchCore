-- AceSerializer-3.0 (implémentation ArchRust) : sérialise des valeurs Lua (chaînes, nombres,
-- booléens, nil, tables imbriquées) en une chaîne imprimable, et l'inverse.
--   local s = LibStub("AceSerializer-3.0"):Serialize(1, "a", { x = true })
--   local ok, a, b, c = LibStub("AceSerializer-3.0"):Deserialize(s)
-- Format : "^1" puis jetons "^S" chaîne, "^N" nombre, "^F…^f…" flottant (mantisse, exposant),
-- "^B"/"^b" booléens, "^Z" nil, "^T…^t" table, et "^^" en fin.
local MAJOR, MINOR = "AceSerializer-3.0", 0
local AceSerializer = LibStub:NewLibrary(MAJOR, MINOR)
if not AceSerializer then return end

local type, tostring, tonumber, pairs, select, error, assert = type, tostring, tonumber, pairs, select, error, assert
local strbyte, strchar, gsub, gmatch, format, concat = string.byte, string.char, string.gsub, string.gmatch, string.format, table.concat
local frexp, ldexp, huge, floor = math.frexp, math.ldexp, math.huge, math.floor

AceSerializer.internals = AceSerializer.internals or {}
AceSerializer.embeds = AceSerializer.embeds or {}

local function escape(ch)
  local n = strbyte(ch)
  if n == 30 then return "\126\122" end   -- ^ de contrôle
  if n <= 32 then return "\126" .. strchar(n + 64) end
  if n == 94 then return "\126\125" end  -- ^
  if n == 126 then return "\126\124" end -- ~
  if n == 127 then return "\126\123" end -- DEL
  error("AceSerializer: impossible d'échapper " .. n)
end

local function unescape(code)
  if code == "\122" then return strchar(30) end
  if code == "\125" then return "^" end
  if code == "\124" then return "~" end
  if code == "\123" then return strchar(127) end
  local n = strbyte(code)
  if n >= 64 and n <= 96 then return strchar(n - 64) end
  error("AceSerializer: séquence d'échappement invalide ~" .. code)
end

local serialize
local function put(v, out)
  local t = type(v)
  if t == "string" then
    out[#out + 1] = "^S"
    out[#out + 1] = (gsub(v, "[%c \94\126\127]", escape))
  elseif t == "number" then
    if v ~= v then
      out[#out + 1] = "^N-nan"
    elseif v == huge then
      out[#out + 1] = "^N1.#INF"
    elseif v == -huge then
      out[#out + 1] = "^N-1.#INF"
    elseif math.type(v) == "integer" or (v == floor(v) and v > -2 ^ 53 and v < 2 ^ 53) then
      out[#out + 1] = "^N" .. format("%d", v)
    else
      local m, e = frexp(v)
      out[#out + 1] = format("^F%.0f^f%d", m * 2 ^ 53, e - 53)
    end
  elseif t == "table" then
    out[#out + 1] = "^T"
    for k, x in pairs(v) do
      put(k, out)
      put(x, out)
    end
    out[#out + 1] = "^t"
  elseif t == "boolean" then
    out[#out + 1] = v and "^B" or "^b"
  elseif t == "nil" then
    out[#out + 1] = "^Z"
  else
    error(MAJOR .. ": Cannot serialize a value of type '" .. t .. "'")
  end
end
serialize = put

function AceSerializer:Serialize(...)
  local out = { "^1" }
  for i = 1, select("#", ...) do put((select(i, ...)), out) end
  out[#out + 1] = "^^"
  return concat(out)
end

local function deserializeValue(iter, single, ctl, data)
  if not single then ctl, data = iter() end
  if not ctl then error("Supplied data misses AceSerializer terminator ('^^')") end
  if ctl == "^^" then return end
  local res
  if ctl == "^S" then
    res = gsub(data, "~(.)", unescape)
  elseif ctl == "^N" then
    if data == "1.#INF" or data == "inf" then res = huge
    elseif data == "-1.#INF" or data == "-inf" then res = -huge
    elseif data == "-nan" or data == "nan" then res = 0 / 0
    else
      res = tonumber(data)
      if not res then error("Invalid serialized number: '" .. tostring(data) .. "'") end
    end
  elseif ctl == "^F" then
    local ctl2, e = iter()
    if ctl2 ~= "^f" then error("Invalid serialized floating-point number, expected '^f', not '" .. tostring(ctl2) .. "'") end
    local m = tonumber(data)
    e = tonumber(e)
    if not (m and e) then error("Invalid serialized floating-point number") end
    res = m * (2 ^ e)
  elseif ctl == "^B" then
    res = true
  elseif ctl == "^b" then
    res = false
  elseif ctl == "^Z" then
    res = nil
  elseif ctl == "^T" then
    res = {}
    while true do
      local c, d = iter()
      if c == "^t" then break end
      local k = deserializeValue(iter, true, c, d)
      if k == nil then error("Invalid AceSerializer table format (no table end marker)") end
      local c2, d2 = iter()
      local v = deserializeValue(iter, true, c2, d2)
      if v == nil then error("Invalid AceSerializer table format (no table end marker)") end
      res[k] = v
    end
  else
    error("Invalid AceSerializer control code '" .. tostring(ctl) .. "'")
  end
  if not single then return res, deserializeValue(iter) end
  return res
end

function AceSerializer:Deserialize(str)
  str = gsub(str, "[%c ]", "")
  local iter = gmatch(str, "(^.)([^^]*)")
  local ctl, data = iter()
  if not ctl or ctl ~= "^1" then return false, "Supplied data is not AceSerializer data (rev 1)" end
  return pcall(deserializeValue, iter)
end

local mixins = { "Serialize", "Deserialize" }
function AceSerializer:Embed(target)
  for _, name in ipairs(mixins) do target[name] = self[name] end
  self.embeds[target] = true
  return target
end
for target in pairs(AceSerializer.embeds) do AceSerializer:Embed(target) end
