-- AceConsole-3.0 (implémentation ArchRust) : Print/Printf préfixés, commandes slash, découpage
-- d'arguments.
local MAJOR, MINOR = "AceConsole-3.0", 0
local AceConsole = LibStub:NewLibrary(MAJOR, MINOR)
if not AceConsole then return end

local type, select, tostring, error, pairs, format = type, select, tostring, error, pairs, string.format

AceConsole.embeds = AceConsole.embeds or {}
AceConsole.commands = AceConsole.commands or {}
AceConsole.weakcommands = AceConsole.weakcommands or {}

local function out(self, frame, ...)
  local parts = {}
  if self ~= AceConsole then parts[1] = "|cff33ff99" .. tostring(self) .. "|r:" end
  for i = 1, select("#", ...) do parts[#parts + 1] = tostring((select(i, ...))) end
  frame:AddMessage(table.concat(parts, " "))
end

function AceConsole:Print(...)
  local frame = ...
  if type(frame) == "table" and frame.AddMessage then
    return out(self, frame, select(2, ...))
  end
  return out(self, DEFAULT_CHAT_FRAME, ...)
end

function AceConsole:Printf(...)
  local frame = ...
  if type(frame) == "table" and frame.AddMessage then
    return out(self, frame, format(select(2, ...)))
  end
  return out(self, DEFAULT_CHAT_FRAME, format(...))
end

function AceConsole:RegisterChatCommand(command, func, persist)
  if type(command) ~= "string" then error([[Usage: AceConsole:RegisterChatCommand( "command", func[, persist ]): 'command' - expected a string]], 2) end
  if persist == nil then persist = true end
  local name = "ACECONSOLE_" .. command:upper()
  if type(func) == "string" then
    SlashCmdList[name] = function(input, editBox) self[func](self, input, editBox) end
  else
    SlashCmdList[name] = func
  end
  _G["SLASH_" .. name .. "1"] = "/" .. command:lower()
  AceConsole.commands[command] = name
  if not persist then
    AceConsole.weakcommands[self] = AceConsole.weakcommands[self] or {}
    AceConsole.weakcommands[self][command] = func
  end
  return true
end

function AceConsole:UnregisterChatCommand(command)
  local name = AceConsole.commands[command]
  if name then
    SlashCmdList[name] = nil
    _G["SLASH_" .. name .. "1"] = nil
    hash_SlashCmdList["/" .. command:upper()] = nil
    AceConsole.commands[command] = nil
  end
end

function AceConsole:IterateChatCommands() return pairs(AceConsole.commands) end

-- GetArgs(texte, n, départ) → arg1, …, argN, position suivante (1e9 à la fin). Les arguments
-- entre guillemets sont gardés entiers.
function AceConsole:GetArgs(str, numargs, startpos)
  numargs = numargs or 1
  startpos = math.max(startpos or 1, 1)
  local pos = str:find("[^ ]", startpos)
  if not pos then pos = 1e9 end
  if numargs < 1 then return pos end
  if pos >= 1e9 then return nil, 1e9 end
  local arg, nextpos
  if str:sub(pos, pos) == '"' then
    local close = str:find('"', pos + 1, true)
    arg = str:sub(pos + 1, (close or (#str + 1)) - 1)
    nextpos = close and (close + 1) or 1e9
  else
    local space = str:find(" ", pos, true)
    arg = str:sub(pos, (space or (#str + 1)) - 1)
    nextpos = space or 1e9
  end
  if nextpos > #str then nextpos = 1e9 end
  if numargs > 1 then
    if nextpos >= 1e9 then return arg, nil, 1e9 end
    return arg, AceConsole:GetArgs(str, numargs - 1, nextpos)
  end
  return arg, nextpos
end

local mixins = { "Print", "Printf", "RegisterChatCommand", "UnregisterChatCommand", "GetArgs" }

function AceConsole:Embed(target)
  for _, name in ipairs(mixins) do target[name] = self[name] end
  self.embeds[target] = true
  return target
end

function AceConsole:OnEmbedEnable(target)
  if AceConsole.weakcommands[target] then
    for command, func in pairs(AceConsole.weakcommands[target]) do
      target:RegisterChatCommand(command, func, false)
    end
  end
end

function AceConsole:OnEmbedDisable(target)
  if AceConsole.weakcommands[target] then
    for command in pairs(AceConsole.weakcommands[target]) do
      target:UnregisterChatCommand(command)
    end
  end
end

for target in pairs(AceConsole.embeds) do AceConsole:Embed(target) end
