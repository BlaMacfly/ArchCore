-- AceComm-3.0 (implémentation ArchRust) : communication entre AddOns de joueurs différents, par les
-- messages d'AddOn des extensions ArchRust. Les messages plus longs que la charge permise
-- (C_ArchCore.GetLimits().maxPayload, 255 octets) sont découpés et recomposés à l'arrivée, avec les
-- mêmes marqueurs que la bibliothèque d'origine (\001 premier, \002 suivant, \003 dernier,
-- \004 échappement). Envois par ChatThrottleLib (priorités ALERT, NORMAL, BULK).
--   self:RegisterComm("MonPfx" [, méthode])     -- méthode(prefix, message, distribution, sender)
--   self:SendCommMessage("MonPfx", texte, "WHISPER" | "SAY" | "PARTY" | "GUILD", cible, prio, rappel, arg)
local MAJOR, MINOR = "AceComm-3.0", 0
local AceComm = LibStub:NewLibrary(MAJOR, MINOR)
if not AceComm then return end

local CallbackHandler = LibStub("CallbackHandler-1.0")
local type, error, tostring, select, pairs, concat, strsub, strbyte = type, error, tostring, select, pairs, table.concat, string.sub, string.byte

local MSG_MULTI_FIRST, MSG_MULTI_NEXT, MSG_MULTI_LAST, MSG_ESCAPE = "\001", "\002", "\003", "\004"

AceComm.embeds = AceComm.embeds or {}
AceComm.multipart_spool = AceComm.multipart_spool or {}
if not AceComm.callbacks then
  AceComm.callbacks = CallbackHandler:New(AceComm, "_RegisterComm", "UnregisterComm", "UnregisterAllComm")
end
AceComm.frame = AceComm.frame or CreateFrame("Frame", "AceComm30Frame")

local function maxLen()
  local limits = C_ArchCore.GetLimits()
  return (limits and limits.maxPayload and limits.maxPayload > 1) and limits.maxPayload or 255
end

function AceComm:RegisterComm(prefix, method)
  if method == nil then method = "OnCommReceived" end
  if type(prefix) ~= "string" or #prefix > 16 then
    error("AceComm:RegisterComm(prefix, method): prefix length is limited to 16 characters", 2)
  end
  C_ChatInfo.RegisterAddonMessagePrefix(prefix)
  return AceComm._RegisterComm(self, prefix, method)
end

-- Envoie `text` ; `callbackFn(callbackArg, octetsEnvoyés, octetsTotal)` après chaque morceau.
function AceComm:SendCommMessage(prefix, text, distribution, target, prio, callbackFn, callbackArg)
  prio = prio or "NORMAL"
  if type(prefix) ~= "string" or type(text) ~= "string" or type(distribution) ~= "string"
    or (target ~= nil and type(target) ~= "string" and type(target) ~= "number")
    or (prio ~= "BULK" and prio ~= "NORMAL" and prio ~= "ALERT") then
    error('Usage: SendCommMessage(addon, "prefix", "text", "distribution"[, "target"[, "prio"[, callbackFn, callbackarg]]])', 2)
  end
  local textlen = #text
  local maxtextlen = maxLen()
  local queueName = prefix .. distribution .. (target or "")
  -- ChatThrottleLib rappelle avec l'argument donné à chaque morceau : la position atteinte.
  local ctlCallback
  if callbackFn then
    ctlCallback = function(sent) return callbackFn(callbackArg, math.min(sent, textlen), textlen) end
  end

  local first = strbyte(text, 1)
  local forceMultipart = false
  if first and first >= 1 and first <= 4 then
    -- Le premier octet est un marqueur : on l'échappe.
    text = MSG_ESCAPE .. text
    if #text > maxtextlen then forceMultipart = true end
  end

  if not forceMultipart and #text <= maxtextlen then
    ChatThrottleLib:SendAddonMessage(prio, prefix, text, distribution, target, queueName, ctlCallback, textlen)
  else
    maxtextlen = maxtextlen - 1
    local chunk = strsub(text, 1, maxtextlen)
    ChatThrottleLib:SendAddonMessage(prio, prefix, MSG_MULTI_FIRST .. chunk, distribution, target, queueName, ctlCallback, maxtextlen)
    local pos = 1 + maxtextlen
    while pos + maxtextlen <= #text do
      chunk = strsub(text, pos, pos + maxtextlen - 1)
      ChatThrottleLib:SendAddonMessage(prio, prefix, MSG_MULTI_NEXT .. chunk, distribution, target, queueName, ctlCallback, pos + maxtextlen - 1)
      pos = pos + maxtextlen
    end
    chunk = strsub(text, pos)
    ChatThrottleLib:SendAddonMessage(prio, prefix, MSG_MULTI_LAST .. chunk, distribution, target, queueName, ctlCallback, textlen)
  end
end

-- Réception : recomposition des messages en plusieurs morceaux, par (préfixe, portée, émetteur).
local spool = AceComm.multipart_spool

local function onReceive(prefix, message, distribution, sender)
  local control = strsub(message, 1, 1)
  local key = prefix .. "\t" .. distribution .. "\t" .. sender
  if control == MSG_MULTI_FIRST then
    spool[key] = { strsub(message, 2) }
  elseif control == MSG_MULTI_NEXT then
    local parts = spool[key]
    if parts then parts[#parts + 1] = strsub(message, 2) end
  elseif control == MSG_MULTI_LAST then
    local parts = spool[key]
    if parts then
      parts[#parts + 1] = strsub(message, 2)
      spool[key] = nil
      AceComm.callbacks:Fire(prefix, concat(parts), distribution, sender)
    end
  elseif control == MSG_ESCAPE then
    AceComm.callbacks:Fire(prefix, strsub(message, 2), distribution, sender)
  else
    AceComm.callbacks:Fire(prefix, message, distribution, sender)
  end
end

AceComm.frame:RegisterEvent("CHAT_MSG_ADDON")
AceComm.frame:SetScript("OnEvent", function(_, event, prefix, message, distribution, sender)
  if event == "CHAT_MSG_ADDON" then onReceive(prefix, message, distribution, sender) end
end)

local mixins = { "RegisterComm", "UnregisterComm", "UnregisterAllComm", "SendCommMessage" }

function AceComm:Embed(target)
  for _, name in pairs(mixins) do target[name] = self[name] end
  self.embeds[target] = true
  return target
end

function AceComm:OnEmbedDisable(target) target:UnregisterAllComm() end

for target in pairs(AceComm.embeds) do AceComm:Embed(target) end
