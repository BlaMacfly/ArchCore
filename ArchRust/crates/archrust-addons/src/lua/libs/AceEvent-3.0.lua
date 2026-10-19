-- AceEvent-3.0 (implémentation ArchRust) : événements du jeu (RegisterEvent) et messages entre
-- AddOns (RegisterMessage / SendMessage), via CallbackHandler-1.0.
local MAJOR, MINOR = "AceEvent-3.0", 0
local AceEvent = LibStub:NewLibrary(MAJOR, MINOR)
if not AceEvent then return end

local CallbackHandler = LibStub("CallbackHandler-1.0")

AceEvent.frame = AceEvent.frame or CreateFrame("Frame", "AceEvent30Frame")
AceEvent.embeds = AceEvent.embeds or {}

if not AceEvent.events then
  AceEvent.events = CallbackHandler:New(AceEvent, "RegisterEvent", "UnregisterEvent", "UnregisterAllEvents")
end
function AceEvent.events:OnUsed(_, eventname) AceEvent.frame:RegisterEvent(eventname) end
function AceEvent.events:OnUnused(_, eventname) AceEvent.frame:UnregisterEvent(eventname) end

if not AceEvent.messages then
  AceEvent.messages = CallbackHandler:New(AceEvent, "RegisterMessage", "UnregisterMessage", "UnregisterAllMessages")
  AceEvent.SendMessage = AceEvent.messages.Fire
end

local mixins = {
  "RegisterEvent", "UnregisterEvent", "RegisterMessage", "UnregisterMessage",
  "SendMessage", "UnregisterAllEvents", "UnregisterAllMessages",
}

function AceEvent:Embed(target)
  for _, name in ipairs(mixins) do target[name] = self[name] end
  self.embeds[target] = true
  return target
end

function AceEvent:OnEmbedDisable(target)
  target:UnregisterAllEvents()
  target:UnregisterAllMessages()
end

local events = AceEvent.events
AceEvent.frame:SetScript("OnEvent", function(_, event, ...) events:Fire(event, ...) end)

for target in pairs(AceEvent.embeds) do AceEvent:Embed(target) end
