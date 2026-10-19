-- ArchAce : AddOn d'exemple pour ArchRust, construit sur LibStub et les bibliothèques Ace3 intégrées
-- au client. Il montre comment modifier l'interface et le rendu depuis Lua :
--   * AceAddon/AceDB : objet AddOn, profils et valeurs par défaut sauvegardés (ArchAceDB) ;
--   * AceEvent : événements du jeu (UNIT_HEALTH, PLAYER_TARGET_CHANGED…) et messages internes ;
--   * AceTimer : minuteurs répétitifs ; AceConsole : commande /archace ;
--   * AceHook : crochet sur SendChatMessage ; filtre de discussion ;
--   * LibDataBroker : objet de données « ArchAce » (images par seconde) ;
--   * API ArchRust : couleurs de l'interface, CVars de rendu, teinte des modèles, raccourcis.
local addonName, ns = ...

local ArchAce = LibStub("AceAddon-3.0"):NewAddon(ns, "ArchAce", "AceConsole-3.0", "AceEvent-3.0", "AceTimer-3.0", "AceHook-3.0")
local L = LibStub("AceLocale-3.0"):GetLocale("ArchAce")
_G.ArchAce = ArchAce

local defaults = {
  profile = {
    plates = true,
    unitFrames = true,
    theme = "defaut",
    fov = 55,
    framePos = { x = 270, y = -70 },
    targetTint = { r = 1, g = 0.45, b = 0.45 },
  },
  global = { sessions = 0 },
  char = { messagesSent = 0 },
}

-- Thèmes : couleurs de l'interface native (ArchRust.SetUIColor) + ciel (CVar skyColor).
ArchAce.themes = {
  defaut = { sky = "0.35 0.45 0.6" },
  sombre = {
    sky = "0.08 0.09 0.14",
    ui = { windowFill = { 0.05, 0.05, 0.07, 0.94 }, panelFill = { 0.07, 0.07, 0.1, 0.94 },
           windowStroke = { 0.35, 0.35, 0.45, 1 }, accent = { 0.3, 0.45, 0.9, 1 } },
  },
  ambre = {
    sky = "0.55 0.38 0.2",
    ui = { windowFill = { 0.16, 0.09, 0.03, 0.93 }, panelFill = { 0.2, 0.12, 0.04, 0.93 },
           windowStroke = { 0.95, 0.65, 0.2, 1 }, accent = { 0.95, 0.6, 0.1, 1 }, text = { 1, 0.9, 0.7, 1 } },
  },
}

function ArchAce:OnInitialize()
  self.db = LibStub("AceDB-3.0"):New("ArchAceDB", defaults, true)
  self.db.global.sessions = self.db.global.sessions + 1
  self.db.RegisterCallback(self, "OnProfileChanged", "ApplySettings")
  self.db.RegisterCallback(self, "OnProfileCopied", "ApplySettings")
  self.db.RegisterCallback(self, "OnProfileReset", "ApplySettings")
  self:RegisterChatCommand("archace", "SlashCommand")
  self:RegisterChatCommand("aace", "SlashCommand")

  -- Objet LibDataBroker : d'autres AddOns (barres d'informations) peuvent l'afficher.
  self.broker = LibStub("LibDataBroker-1.1"):NewDataObject("ArchAce", {
    type = "data source", label = "ArchAce", text = "…",
    OnClick = function() ArchAce:OpenOptions() end,
  })
end

function ArchAce:OnEnable()
  self:RegisterEvent("PLAYER_ENTERING_WORLD")
  self:RegisterEvent("PLAYER_TARGET_CHANGED", "UpdateUnitFrames")
  self:RegisterEvent("UNIT_HEALTH", "OnUnitHealth")
  self:RegisterEvent("CVAR_UPDATE")
  self:RegisterMessage("ARCHACE_SETTINGS_CHANGED", "ApplySettings")
  self:ScheduleRepeatingTimer("UpdateBroker", 1)
  -- Crochet sécurisé : compte les messages envoyés (y compris ceux tapés dans la discussion).
  self:SecureHook("SendChatMessage", function() self.db.char.messagesSent = self.db.char.messagesSent + 1 end)
  -- Filtre : met en couleur les messages qui citent le joueur.
  ChatFrame_AddMessageEventFilter("CHAT_MSG_SAY", function(_, _, msg, author)
    local me = UnitName("player")
    if me and msg:lower():find(me:lower(), 1, true) then
      return false, "|cffffd200" .. msg .. "|r", author
    end
  end)
  self:CreateUnitFrames()
  self:CreateNameplates()
  self:CreateOptions()
  self:ApplySettings()
  self:Print(L["LOADED"])
end

function ArchAce:PLAYER_ENTERING_WORLD(_, isLogin, isReload)
  self:UpdateUnitFrames()
end

function ArchAce:CVAR_UPDATE(_, name, value)
  if name == "cameraFov" then self.db.profile.fov = tonumber(value) or self.db.profile.fov end
end

function ArchAce:OnUnitHealth(_, unit)
  if unit == "player" or unit == "target" then self:UpdateUnitFrames() end
end

function ArchAce:UpdateBroker()
  self.broker.text = format("%d ips", GetFramerate())
end

function ArchAce:ApplyTheme(name)
  local theme = self.themes[name] or self.themes.defaut
  ArchRust.ResetUIColors()
  for key, c in pairs(theme.ui or {}) do ArchRust.SetUIColor(key, unpack(c)) end
  SetCVar("skyColor", theme.sky)
end

function ArchAce:ApplySettings()
  local p = self.db.profile
  self:ApplyTheme(p.theme)
  SetCVar("cameraFov", p.fov)
  if self.playerFrame then
    self.playerFrame:SetShown(p.unitFrames)
    self.playerFrame:ClearAllPoints()
    self.playerFrame:SetPoint("TOPLEFT", UIParent, "TOPLEFT", p.framePos.x, p.framePos.y)
  end
  if self.plates then self.plates:SetShown(p.plates) end
  self:UpdateUnitFrames()
end

-- Démonstration en une commande (utilisée pour les tests en jeu) : cible, teinte, thème, fenêtre.
function ArchAce:Demo()
  local p = self.db.profile
  p.plates, p.unitFrames, p.theme = true, true, "ambre"
  self:ApplySettings()
  if not UnitExists("target") then TargetNearest() end
  if UnitExists("target") then
    ArchRust.SetUnitTint("target", p.targetTint.r, p.targetTint.g, p.targetTint.b, 1)
  end
  self:Print(format(L["THEME_SET"], p.theme), format(L["PLATES"], L["ON"]))
  self:Print(format(L["MESSAGES_SENT"], self.db.char.messagesSent), "—", format(L["PROFILE"], self.db:GetCurrentProfile()))
end

function ArchAce:SlashCommand(input)
  local cmd, rest = self:GetArgs(input, 1)
  local args = strtrim(input:sub(rest and rest < 1e9 and rest or #input + 1))
  local p = self.db.profile
  if cmd == "demo" then
    self:Demo()
  elseif cmd == "plates" then
    p.plates = (args == "on")
    self:ApplySettings()
    self:Print(format(L["PLATES"], p.plates and L["ON"] or L["OFF"]))
  elseif cmd == "theme" then
    p.theme = self.themes[args] and args or "defaut"
    self:ApplySettings()
    self:Print(format(L["THEME_SET"], p.theme))
  elseif cmd == "sky" then
    SetCVar("skyColor", args)
  elseif cmd == "fov" then
    p.fov = tonumber(args) or 55
    SetCVar("cameraFov", p.fov)
  elseif cmd == "target" then
    if TargetUnit(args) then self:Print(UnitName("target")) else self:Print(L["TARGET_NONE"]) end
  elseif cmd == "tint" then
    if UnitExists("target") then ArchRust.SetUnitTint("target", p.targetTint.r, p.targetTint.g, p.targetTint.b, 1) end
  elseif cmd == "bind" then
    local key = (args ~= "" and args or "F5"):upper()
    ArchRust.SetBinding(key, function() TargetNearest() end)
    self:Print(format(L["BOUND"], key))
  elseif cmd == "profile" then
    if args ~= "" then self.db:SetProfile(args) end
    self:Print(format(L["PROFILE"], self.db:GetCurrentProfile()))
  elseif cmd == "options" then
    self:OpenOptions()
  elseif cmd == "reset" then
    self.db:ResetProfile()
    ArchRust.ClearUnitTint()
  else
    self:Print(L["HELP"])
  end
  self:SendMessage("ARCHACE_COMMAND", cmd)
end
