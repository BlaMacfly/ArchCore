-- Prélude des AddOns ArchRust : compatibilité Lua 5.1 et fonctions globales de World of Warcraft.
-- Exécuté une fois à la création de l'état Lua, avant l'API du jeu et les AddOns.

local type, select, tostring, tonumber, pairs, ipairs, next, error, rawget, rawset, setmetatable, getmetatable =
      type, select, tostring, tonumber, pairs, ipairs, next, error, rawget, rawset, setmetatable, getmetatable
local floor, huge = math.floor, math.huge
local sfind, sgsub, ssub, slen, sbyte, schar, sformat, srep, slower, supper, smatch, sgmatch =
      string.find, string.gsub, string.sub, string.len, string.byte, string.char, string.format,
      string.rep, string.lower, string.upper, string.match, string.gmatch
local tinsert_, tremove_, tconcat, tsort, tunpack = table.insert, table.remove, table.concat, table.sort, table.unpack

---------------------------------------------------------------------------------------------------
-- Compatibilité Lua 5.1 (WoW tourne sous Lua 5.1)
---------------------------------------------------------------------------------------------------
unpack = tunpack
table.getn = function(t) return #t end
getn = table.getn
table.setn = function() end
math.pow = function(a, b) return a ^ b end
math.mod = math.fmod
math.ldexp = function(m, e) return m * 2.0 ^ e end
math.frexp = function(x)
  if x == 0 then return 0.0, 0 end
  local e = floor(math.log(math.abs(x), 2)) + 1
  return x / 2.0 ^ e, e
end
math.log10 = function(x) return math.log(x, 10) end
math.cosh = function(x) return (math.exp(x) + math.exp(-x)) / 2 end
math.sinh = function(x) return (math.exp(x) - math.exp(-x)) / 2 end
math.tanh = function(x) local a, b = math.exp(x), math.exp(-x) return (a - b) / (a + b) end
string.gfind = string.gmatch

-- string.format façon Lua 5.1 : `%d`, `%x`, `%c`… acceptent un nombre non entier (tronqué) au lieu
-- de lever « number has no integer representation » (code WoW : format("%d", GetFramerate())).
do
  local rawformat, pcall, mtype, pack = string.format, pcall, math.type, table.pack
  local INT = { d = true, i = true, x = true, X = true, o = true, c = true, u = true }
  local function format51(fmt, ...)
    local ok, out = pcall(rawformat, fmt, ...)
    if ok then return out end
    local args = pack(...)
    local i = 0
    for conv in sgmatch(fmt, "%%[-+ #0]*%d*%.?%d*([%a%%])") do
      if conv ~= "%" then
        i = i + 1
        local v = args[i]
        if INT[conv] then
          if type(v) == "string" then v = tonumber(v) or v end
          if type(v) == "number" and mtype(v) == "float" then
            if v ~= v or v == huge or v == -huge then v = 0
            elseif v >= 0 then v = floor(v) else v = -floor(-v) end
          end
          args[i] = v
        end
      end
    end
    return rawformat((sgsub(fmt, "%%([-+ #0]*%d*)u", "%%%1d")), tunpack(args, 1, args.n))
  end
  string.format = format51
  sformat = format51
end
table.foreach = function(t, f) for k, v in pairs(t) do local r = f(k, v) if r ~= nil then return r end end end
table.foreachi = function(t, f) for i, v in ipairs(t) do local r = f(i, v) if r ~= nil then return r end end end
table.maxn = function(t) local m = 0 for k in pairs(t) do if type(k) == "number" and k > m then m = k end end return m end
foreach, foreachi = table.foreach, table.foreachi
-- Les environnements par fonction n'existent plus en 5.4 : versions simplifiées.
getfenv = function() return _G end
setfenv = function(f) return f end
newproxy = function(withMeta)
  local p = {}
  if withMeta then setmetatable(p, {}) end
  return p
end

---------------------------------------------------------------------------------------------------
-- bit (LuaBitOp, entiers 32 bits signés comme sous WoW) et bit32 (Lua 5.2, non signés)
---------------------------------------------------------------------------------------------------
local function u32(x) return floor(tonumber(x) or 0) & 0xFFFFFFFF end
local function s32(x) x = u32(x) if x >= 0x80000000 then return x - 0x100000000 end return x end

bit = {}
bit.tobit = s32
function bit.band(a, ...) local r = u32(a) for i = 1, select("#", ...) do r = r & u32((select(i, ...))) end return s32(r) end
function bit.bor(a, ...) local r = u32(a) for i = 1, select("#", ...) do r = r | u32((select(i, ...))) end return s32(r) end
function bit.bxor(a, ...) local r = u32(a) for i = 1, select("#", ...) do r = r ~ u32((select(i, ...))) end return s32(r) end
function bit.bnot(a) return s32(~u32(a)) end
function bit.lshift(a, n) return s32(u32(a) << (n & 31)) end
function bit.rshift(a, n) return s32(u32(a) >> (n & 31)) end
function bit.arshift(a, n)
  local x = s32(a)
  n = n & 31
  if x >= 0 then return s32(x >> n) end
  return s32(((x >> n) | ~(0xFFFFFFFF >> n)) & 0xFFFFFFFF)
end
function bit.rol(a, n) n = n & 31 local x = u32(a) return s32(((x << n) | (x >> (32 - n))) & 0xFFFFFFFF) end
function bit.ror(a, n) n = n & 31 local x = u32(a) return s32(((x >> n) | (x << (32 - n))) & 0xFFFFFFFF) end
function bit.bswap(a)
  local x = u32(a)
  return s32(((x & 0xFF) << 24) | ((x & 0xFF00) << 8) | ((x >> 8) & 0xFF00) | (x >> 24))
end
function bit.tohex(a, n)
  n = n or 8
  local upper = n < 0
  if upper then n = -n end
  local s = sformat(upper and "%08X" or "%08x", u32(a))
  return ssub(s, -n)
end
bit.mod = math.fmod

bit32 = {}
function bit32.band(...) local r = 0xFFFFFFFF for i = 1, select("#", ...) do r = r & u32((select(i, ...))) end return r end
function bit32.bor(...) local r = 0 for i = 1, select("#", ...) do r = r | u32((select(i, ...))) end return r end
function bit32.bxor(...) local r = 0 for i = 1, select("#", ...) do r = r ~ u32((select(i, ...))) end return r end
function bit32.bnot(a) return (~u32(a)) & 0xFFFFFFFF end
function bit32.btest(...) return bit32.band(...) ~= 0 end
function bit32.lshift(a, n) if n >= 32 or n <= -32 then return 0 end if n < 0 then return u32(a) >> -n end return (u32(a) << n) & 0xFFFFFFFF end
function bit32.rshift(a, n) if n >= 32 or n <= -32 then return 0 end if n < 0 then return (u32(a) << -n) & 0xFFFFFFFF end return u32(a) >> n end
function bit32.arshift(a, n) return u32(bit.arshift(a, n)) end
function bit32.lrotate(a, n) return u32(bit.rol(a, n)) end
function bit32.rrotate(a, n) return u32(bit.ror(a, n)) end
function bit32.extract(a, f, w) w = w or 1 return (u32(a) >> f) & ((1 << w) - 1) end
function bit32.replace(a, v, f, w)
  w = w or 1
  local mask = ((1 << w) - 1) << f
  return (u32(a) & ~mask & 0xFFFFFFFF) | ((u32(v) << f) & mask)
end

---------------------------------------------------------------------------------------------------
-- Chaînes (alias et fonctions de WoW)
---------------------------------------------------------------------------------------------------
strbyte, strchar, strfind, strformat, strlen, strlower, strupper, strmatch, strrep, strsub, gsub, gmatch =
  sbyte, schar, sfind, sformat, slen, slower, supper, smatch, srep, ssub, sgsub, sgmatch
format = sformat
strrev = string.reverse

local function escape_class(chars) return (sgsub(chars, "[%]%^%-%%]", "%%%0")) end

-- strsplit(séparateurs, texte [, limite]) : chaque caractère de `sep` sépare.
function strsplit(sep, s, limit)
  if s == nil then return nil end
  s = tostring(s)
  local out, n, pos = {}, 0, 1
  local class = "[" .. escape_class(sep) .. "]"
  while true do
    if limit and n == limit - 1 then break end
    local a, b = sfind(s, class, pos)
    if not a then break end
    n = n + 1
    out[n] = ssub(s, pos, a - 1)
    pos = b + 1
  end
  n = n + 1
  out[n] = ssub(s, pos)
  return tunpack(out, 1, n)
end
function strsplittable(sep, s, limit) return { strsplit(sep, s, limit) } end
function strjoin(sep, ...) return tconcat({ ... }, sep) end
function strconcat(...) return tconcat({ ... }) end
function strtrim(s, chars)
  if s == nil then return nil end
  if chars then
    local c = escape_class(chars)
    return (sgsub(sgsub(s, "^[" .. c .. "]+", ""), "[" .. c .. "]+$", ""))
  end
  return (sgsub(sgsub(s, "^%s+", ""), "%s+$", ""))
end
function strlenutf8(s) return utf8.len(s) or slen(s) end
function strcmputf8i(a, b)
  a, b = slower(a), slower(b)
  if a == b then return 0 elseif a < b then return -1 else return 1 end
end
function tostringall(...)
  local n = select("#", ...)
  if n == 0 then return end
  local t = { ... }
  for i = 1, n do t[i] = tostring(t[i]) end
  return tunpack(t, 1, n)
end
string.split = function(s, sep, limit) return strsplit(sep, s, limit) end
string.join = strjoin
string.trim = strtrim
string.concat = strconcat

---------------------------------------------------------------------------------------------------
-- Tables
---------------------------------------------------------------------------------------------------
tinsert, tremove, sort = tinsert_, tremove_, tsort
function wipe(t)
  for k in next, t do t[k] = nil end
  return t
end
table.wipe = wipe
function tContains(t, v) for _, x in pairs(t) do if x == v then return true end end return false end
function tIndexOf(t, v) for i, x in ipairs(t) do if x == v then return i end end return nil end
function tDeleteItem(t, v)
  local n = 0
  for i = #t, 1, -1 do if t[i] == v then tremove_(t, i) n = n + 1 end end
  return n
end
function tInvert(t) local r = {} for k, v in pairs(t) do r[v] = k end return r end
function tFilter(t, pred, isIndex)
  local r = {}
  if isIndex then for _, v in ipairs(t) do if pred(v) then r[#r + 1] = v end end
  else for k, v in pairs(t) do if pred(v) then r[k] = v end end end
  return r
end
function tAppendAll(t, add) for _, v in ipairs(add) do t[#t + 1] = v end return t end
function tCount(t) local n = 0 for _ in pairs(t) do n = n + 1 end return n end
function tUnorderedRemove(t, i) t[i] = t[#t] t[#t] = nil end
function CopyTable(t, seen)
  if type(t) ~= "table" then return t end
  seen = seen or {}
  if seen[t] then return seen[t] end
  local r = {}
  seen[t] = r
  for k, v in pairs(t) do r[CopyTable(k, seen)] = CopyTable(v, seen) end
  return setmetatable(r, getmetatable(t))
end
function MergeTable(dst, src) for k, v in pairs(src) do dst[k] = v end return dst end
function GetOrCreateTableEntry(t, k, default) local v = t[k] if v == nil then v = default or {} t[k] = v end return v end
table.count = tCount

---------------------------------------------------------------------------------------------------
-- Mathématiques (globales de WoW ; `sin`/`cos`/`tan` en degrés comme sous WoW)
---------------------------------------------------------------------------------------------------
abs, ceil, _G.floor, max, min, sqrt, exp, log, fmod = math.abs, math.ceil, math.floor, math.max, math.min,
  math.sqrt, math.exp, math.log, math.fmod
mod = math.fmod
log10, frexp, ldexp = math.log10, math.frexp, math.ldexp
PI = math.pi
local rad, deg = math.rad, math.deg
function sin(d) return math.sin(rad(d)) end
function cos(d) return math.cos(rad(d)) end
function tan(d) return math.tan(rad(d)) end
function asin(x) return deg(math.asin(x)) end
function acos(x) return deg(math.acos(x)) end
function atan(x) return deg(math.atan(x)) end
function atan2(y, x) return deg(math.atan(y, x)) end
random = math.random
fastrandom = math.random
function Clamp(v, lo, hi) if v < lo then return lo elseif v > hi then return hi end return v end
function Saturate(v) return Clamp(v, 0, 1) end
function Lerp(a, b, t) return a + (b - a) * t end
function Round(v) return floor(v + 0.5) end
function RoundToSignificantDigits(v, d) local m = 10 ^ d return floor(v * m + 0.5) / m end
function ApproximatelyEqual(a, b, eps) return math.abs(a - b) < (eps or 1e-6) end

---------------------------------------------------------------------------------------------------
-- Dates
---------------------------------------------------------------------------------------------------
date, time, difftime = os.date, os.time, os.difftime
function GetServerTime() return os.time() end
function C_DateAndTime_GetCurrentCalendarTime()
  local d = os.date("*t")
  return { year = d.year, month = d.month, monthDay = d.day, weekday = d.wday, hour = d.hour, minute = d.min }
end
C_DateAndTime = { GetCurrentCalendarTime = C_DateAndTime_GetCurrentCalendarTime, GetServerTimeLocal = os.time }
C_DateAndTime_GetCurrentCalendarTime = nil

---------------------------------------------------------------------------------------------------
-- Appels « sécurisés », crochets
---------------------------------------------------------------------------------------------------
local function report(err) local h = geterrorhandler() if h then h(err) end end
local function pack_ok(ok, ...) if not ok then report((...)) return end return ... end
function securecall(f, ...)
  if type(f) == "string" then f = _G[f] end
  if type(f) ~= "function" then return end
  return pack_ok(xpcall(f, debug.traceback, ...))
end
function securecallfunction(f, ...) return securecall(f, ...) end
function securecallmethod(obj, name, ...) return securecall(obj[name], obj, ...) end
function secureexecuterange(t, f, ...)
  for k, v in pairs(t) do securecall(f, k, v, ...) end
end
function issecure() return false end
function issecurevariable() return false, nil end
function forceinsecure() end
function InCombatLockdown() return false end

-- hooksecurefunc([table,] "nom", crochet) : le crochet est appelé après l'original, avec les
-- mêmes arguments ; la valeur de retour de l'original est conservée.
function hooksecurefunc(t, name, hook)
  if type(t) == "string" then t, name, hook = _G, t, name end
  local orig = t[name]
  if type(orig) ~= "function" then
    error("hooksecurefunc : " .. tostring(name) .. " n'est pas une fonction", 2)
  end
  t[name] = function(...)
    local r = table.pack(orig(...))
    hook(...)
    return tunpack(r, 1, r.n)
  end
end

---------------------------------------------------------------------------------------------------
-- Mixins et couleurs
---------------------------------------------------------------------------------------------------
function Mixin(obj, ...)
  for i = 1, select("#", ...) do
    local m = select(i, ...)
    for k, v in pairs(m) do obj[k] = v end
  end
  return obj
end
function CreateFromMixins(...) return Mixin({}, ...) end
function CreateAndInitFromMixin(mixin, ...)
  local o = CreateFromMixins(mixin)
  o:Init(...)
  return o
end

ColorMixin = {}
function ColorMixin:OnLoad(r, g, b, a) self:SetRGBA(r, g, b, a) end
function ColorMixin:SetRGBA(r, g, b, a) self.r, self.g, self.b, self.a = r or 1, g or 1, b or 1, a end
function ColorMixin:SetRGB(r, g, b) self:SetRGBA(r, g, b, nil) end
function ColorMixin:GetRGB() return self.r, self.g, self.b end
function ColorMixin:GetRGBA() return self.r, self.g, self.b, self.a or 1 end
function ColorMixin:GetRGBAsBytes() return Round(self.r * 255), Round(self.g * 255), Round(self.b * 255) end
function ColorMixin:IsEqualTo(o) return self.r == o.r and self.g == o.g and self.b == o.b and (self.a or 1) == (o.a or 1) end
function ColorMixin:GenerateHexColor()
  return sformat("ff%02x%02x%02x", Round(self.r * 255), Round(self.g * 255), Round(self.b * 255))
end
function ColorMixin:GenerateHexColorMarkup() return "|c" .. self:GenerateHexColor() end
function ColorMixin:WrapTextInColorCode(text) return WrapTextInColorCode(text, self:GenerateHexColor()) end
function CreateColor(r, g, b, a)
  local c = CreateFromMixins(ColorMixin)
  c:OnLoad(r, g, b, a)
  return c
end
function WrapTextInColorCode(text, hex) return "|c" .. hex .. tostring(text) .. "|r" end
function WrapTextInColor(text, color) return color:WrapTextInColorCode(text) end
function CreateColorFromHexString(hex)
  local a, r, g, b = tonumber(ssub(hex, 1, 2), 16), tonumber(ssub(hex, 3, 4), 16),
    tonumber(ssub(hex, 5, 6), 16), tonumber(ssub(hex, 7, 8), 16)
  return CreateColor(r / 255, g / 255, b / 255, a / 255)
end

NORMAL_FONT_COLOR = CreateColor(1.0, 0.82, 0.0)
HIGHLIGHT_FONT_COLOR = CreateColor(1.0, 1.0, 1.0)
RED_FONT_COLOR = CreateColor(1.0, 0.1, 0.1)
GREEN_FONT_COLOR = CreateColor(0.1, 1.0, 0.1)
GRAY_FONT_COLOR = CreateColor(0.5, 0.5, 0.5)
YELLOW_FONT_COLOR = CreateColor(1.0, 1.0, 0.0)
ORANGE_FONT_COLOR = CreateColor(1.0, 0.5, 0.25)
DISABLED_FONT_COLOR = CreateColor(0.5, 0.5, 0.5)
WHITE_FONT_COLOR = CreateColor(1, 1, 1)
BLACK_FONT_COLOR = CreateColor(0, 0, 0)
LIGHTBLUE_FONT_COLOR = CreateColor(0.53, 0.67, 1.0)
NORMAL_FONT_COLOR_CODE = "|cffffd200"
HIGHLIGHT_FONT_COLOR_CODE = "|cffffffff"
RED_FONT_COLOR_CODE = "|cffff2020"
GREEN_FONT_COLOR_CODE = "|cff20ff20"
GRAY_FONT_COLOR_CODE = "|cff808080"
FONT_COLOR_CODE_CLOSE = "|r"

-- Qualités d'objets (codes de WoW, utiles aux AddOns d'inventaire).
ITEM_QUALITY_COLORS = {}
for i, hex in pairs({ [0] = "ff9d9d9d", "ffffffff", "ff1eff00", "ff0070dd", "ffa335ee", "ffff8000", "ffe6cc80", "ff00ccff" }) do
  local c = CreateColorFromHexString(hex)
  ITEM_QUALITY_COLORS[i] = { r = c.r, g = c.g, b = c.b, hex = "|c" .. hex, color = c }
end

-- Classes ArchLord (couleurs proposées ; jetons en majuscules comme sous WoW).
RAID_CLASS_COLORS = {}
for token, hex in pairs({ KNIGHT = "ffc79c6e", ARCHER = "ffabd473", MAGE = "ff69ccf0", BERSERKER = "ffc41f3b",
  HUNTER = "ffaad372", SORCERER = "ff8787ed", RANGER = "ff00ff96", ELEMENTALIST = "ff0070de", SCION = "fff58cba",
  WARRIOR = "ffc79c6e", PRIEST = "ffffffff", ROGUE = "fffff569", PALADIN = "fff58cba", SHAMAN = "ff0070de",
  WARLOCK = "ff8787ed", DRUID = "ffff7d0a", DEATHKNIGHT = "ffc41f3b", MONK = "ff00ff96", DEMONHUNTER = "ffa330c9" }) do
  local c = CreateColorFromHexString(hex)
  c.colorStr = hex
  RAID_CLASS_COLORS[token] = c
end

---------------------------------------------------------------------------------------------------
-- Divers
---------------------------------------------------------------------------------------------------
function nop() end
function GenerateClosure(f, ...)
  local n = select("#", ...)
  local args = { ... }
  return function(...)
    local all = { tunpack(args, 1, n) }
    local m = select("#", ...)
    for i = 1, m do all[n + i] = (select(i, ...)) end
    return f(tunpack(all, 1, n + m))
  end
end
function FormatLargeNumber(n)
  local s = tostring(floor(n))
  local r = sgsub(s:reverse(), "(%d%d%d)", "%1 "):reverse()
  return (sgsub(r, "^ ", ""))
end
function SecondsToTime(sec)
  sec = floor(sec)
  local h, m, s = floor(sec / 3600), floor(sec % 3600 / 60), sec % 60
  if h > 0 then return sformat("%d h %d min", h, m) end
  if m > 0 then return sformat("%d min %d s", m, s) end
  return sformat("%d s", s)
end
function BreakUpLargeNumbers(n) return FormatLargeNumber(n) end
function AbbreviateNumbers(n)
  if n >= 1e6 then return sformat("%.1fM", n / 1e6) elseif n >= 1e3 then return sformat("%.1fk", n / 1e3) end
  return tostring(n)
end
AbbreviateLargeNumbers = AbbreviateNumbers
