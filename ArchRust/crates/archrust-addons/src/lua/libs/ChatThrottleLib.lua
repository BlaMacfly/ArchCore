-- ChatThrottleLib (implémentation ArchRust) : files d'envoi par priorité (ALERT, NORMAL, BULK) pour
-- les messages d'AddOn et de discussion. Le client lisse déjà les messages d'AddOn selon les limites
-- du serveur (C_ArchCore.GetLimits) ; cette bibliothèque ajoute les priorités, les files nommées et les
-- rappels de fin d'envoi, avec l'API de la bibliothèque d'origine. Version 1 : une copie d'origine
-- livrée par un AddOn (version plus haute) la remplace.
local CTL_VERSION = 1

if ChatThrottleLib and (ChatThrottleLib.version or 0) >= CTL_VERSION then return end

ChatThrottleLib = ChatThrottleLib or {}
local CTL = ChatThrottleLib
CTL.version = CTL_VERSION
CTL.MAX_CPS = 800
CTL.MSG_OVERHEAD = 40
CTL.BURST = 4000
CTL.MIN_FPS = 20

local PRIOS = { "ALERT", "NORMAL", "BULK" }
CTL.Prio = CTL.Prio or {}
for _, p in ipairs(PRIOS) do CTL.Prio[p] = CTL.Prio[p] or { queue = {} } end

-- Messages d'AddOn confiés au client au plus en même temps : le reste attend ici, par priorité.
local IN_FLIGHT = 4

local function push(prio, msg)
  local q = (CTL.Prio[prio] or CTL.Prio.NORMAL).queue
  q[#q + 1] = msg
end

local function nextMessage()
  for _, p in ipairs(PRIOS) do
    local q = CTL.Prio[p].queue
    if #q > 0 then return table.remove(q, 1) end
  end
end

local function pending()
  local n = 0
  for _, p in ipairs(PRIOS) do n = n + #CTL.Prio[p].queue end
  return n
end

local function deliver(msg)
  if msg.kind == "addon" then
    local r = C_ChatInfo.SendAddonMessage(msg.prefix, msg.text, msg.chattype, msg.target)
    if r == Enum.SendAddonMessageResult.AddonMessageThrottle then return false end
    if msg.callbackFn then securecall(msg.callbackFn, msg.callbackArg, r == 0) end
  else
    SendChatMessage(msg.text, msg.chattype, msg.language, msg.target)
    if msg.callbackFn then securecall(msg.callbackFn, msg.callbackArg, true) end
  end
  return true
end

CTL.Frame = CTL.Frame or CreateFrame("Frame", "ChatThrottleLibFrame")
CTL.Frame:SetScript("OnUpdate", function()
  while pending() > 0 and (C_ArchCore.GetPendingMessages() < IN_FLIGHT) do
    local msg = nextMessage()
    if not deliver(msg) then
      table.insert(CTL.Prio.ALERT.queue, 1, msg)
      break
    end
  end
end)

function CTL:SendAddonMessage(prio, prefix, text, chattype, target, queueName, callbackFn, callbackArg)
  if not prio or not prefix or not text or not chattype or not CTL.Prio[prio] then
    error('ChatThrottleLib:SendAddonMessage(prio, prefix, text, chattype[, target, queueName, callbackFn, callbackArg]): invalid arguments', 2)
  end
  push(prio, { kind = "addon", prefix = prefix, text = text, chattype = chattype, target = target,
    queueName = queueName, callbackFn = callbackFn, callbackArg = callbackArg })
end
CTL.SendAddonMessageLogged = CTL.SendAddonMessage

function CTL:SendChatMessage(prio, prefix, text, chattype, language, destination, queueName, callbackFn, callbackArg)
  if not prio or not text or not CTL.Prio[prio] then
    error('ChatThrottleLib:SendChatMessage(prio, prefix, text[, chattype, language, destination, queueName, callbackFn, callbackArg]): invalid arguments', 2)
  end
  push(prio, { kind = "chat", text = text, chattype = chattype or "SAY", language = language, target = destination,
    queueName = queueName, callbackFn = callbackFn, callbackArg = callbackArg })
end

-- Messages en attente dans les files de la bibliothèque.
function CTL:GetPending() return pending() end
