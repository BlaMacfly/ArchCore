-- Cadres d'unité (joueur et cible) : frames, FontStrings, StatusBars, déplacement à la souris,
-- info-bulle (GameTooltip), position sauvegardée dans le profil AceDB.
local ArchAce = LibStub("AceAddon-3.0"):GetAddon("ArchAce")
local L = LibStub("AceLocale-3.0"):GetLocale("ArchAce")
local LSM = LibStub("LibSharedMedia-3.0")

local BAR = LSM:Fetch("statusbar", "Solid")

local function makeUnitFrame(name, title)
  local f = CreateFrame("Button", name, UIParent, "BackdropTemplate")
  f:SetSize(230, 58)
  f:SetBackdrop(ArchRust.DefaultBackdrop)
  f:SetBackdropColor(0.04, 0.03, 0.02, 0.82)
  f:SetBackdropBorderColor(0.85, 0.62, 0.25, 1)
  f:SetFrameStrata("LOW")

  f.title = f:CreateFontString(nil, "OVERLAY", "GameFontNormalSmall")
  f.title:SetPoint("TOPLEFT", f, "TOPLEFT", 8, -4)
  f.title:SetJustifyH("LEFT")
  f.title:SetText(title)

  f.name = f:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
  f.name:SetPoint("TOPRIGHT", f, "TOPRIGHT", -8, -4)
  f.name:SetJustifyH("RIGHT")

  f.health = CreateFrame("StatusBar", nil, f)
  f.health:SetPoint("TOPLEFT", f, "TOPLEFT", 8, -22)
  f.health:SetPoint("TOPRIGHT", f, "TOPRIGHT", -8, -22)
  f.health:SetHeight(14)
  f.health:SetStatusBarTexture(BAR)
  f.health:SetStatusBarColor(0.15, 0.8, 0.2, 1)
  f.health.text = f.health:CreateFontString(nil, "OVERLAY", "NumberFontNormalSmall")
  f.health.text:SetPoint("CENTER", f.health, "CENTER", 0, 0)

  f.info = f:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
  f.info:SetPoint("BOTTOMLEFT", f, "BOTTOMLEFT", 8, 5)
  f.info:SetJustifyH("LEFT")
  return f
end

local function fillUnit(f, unit)
  if not UnitExists(unit) then
    f.name:SetText("|cff888888" .. L["TARGET_NONE"] .. "|r")
    f.health:SetMinMaxValues(0, 1)
    f.health:SetValue(0)
    f.health.text:SetText("")
    f.info:SetText("")
    return
  end
  local hp, max = UnitHealth(unit), UnitHealthMax(unit)
  f.name:SetText(UnitName(unit))
  f.health:SetMinMaxValues(0, math.max(max, 1))
  f.health:SetValue(hp)
  local pct = max > 0 and hp / max or 0
  f.health:SetStatusBarColor(1 - pct, 0.25 + pct * 0.6, 0.15, 1)
  f.health.text:SetText(max > 0 and format("%d / %d", hp, max) or "?")
end

function ArchAce:CreateUnitFrames()
  local p = self.db.profile
  local player = makeUnitFrame("ArchAcePlayerFrame", "Joueur")
  self.playerFrame = player
  player:SetPoint("TOPLEFT", UIParent, "TOPLEFT", p.framePos.x, p.framePos.y)
  player:EnableMouse(true)
  player:SetMovable(true)
  player:SetClampedToScreen(true)
  player:RegisterForDrag("LeftButton")
  player:SetScript("OnDragStart", player.StartMoving)
  player:SetScript("OnDragStop", function(frame)
    frame:StopMovingOrSizing()
    local top = frame:GetTop() - GetScreenHeight()
    self.db.profile.framePos.x, self.db.profile.framePos.y = Round(frame:GetLeft()), Round(top)
  end)
  player:SetScript("OnEnter", function(frame)
    GameTooltip:SetOwner(frame, "ANCHOR_RIGHT")
    GameTooltip:SetUnit("player")
    GameTooltip:AddLine("Glisser pour déplacer · clic droit : options", 0.7, 0.7, 0.7)
    GameTooltip:Show()
  end)
  player:SetScript("OnLeave", function() GameTooltip:Hide() end)
  player:SetScript("OnClick", function(_, button)
    if button == "RightButton" then self:OpenOptions() end
  end)

  local target = makeUnitFrame("ArchAceTargetFrame", "Cible")
  self.targetFrame = target
  target:SetParent(player)
  target:SetPoint("TOPLEFT", player, "TOPRIGHT", 12, 0)

  -- Position du joueur rafraîchie quatre fois par seconde.
  local acc = 0
  player:SetScript("OnUpdate", function(frame, elapsed)
    acc = acc + elapsed
    if acc < 0.25 then return end
    acc = 0
    local x, y, z = UnitPosition("player")
    if x then frame.info:SetFormattedText("%.0f, %.0f, %.0f", x, y, z) end
    if UnitExists("target") then
      local d = UnitDistanceSquared("target")
      target.info:SetFormattedText("à %.0f", math.sqrt(d or 0))
    end
  end)
end

function ArchAce:UpdateUnitFrames()
  if not self.playerFrame then return end
  fillUnit(self.playerFrame, "player")
  fillUnit(self.targetFrame, "target")
  self.targetFrame:SetShown(UnitExists("target"))
end
