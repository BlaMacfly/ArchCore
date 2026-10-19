-- Panneau d'options (Settings / InterfaceOptions) : cases à cocher, curseur, boutons de thème,
-- champ de saisie de profil. Ouvert par /archace options, /options ou clic droit sur le cadre.
local ArchAce = LibStub("AceAddon-3.0"):GetAddon("ArchAce")

local function check(parent, label, y, get, set)
  local cb = CreateFrame("CheckButton", nil, parent, "UICheckButtonTemplate")
  cb:SetPoint("TOPLEFT", parent, "TOPLEFT", 16, y)
  cb.Text:SetText(label)
  cb:SetScript("OnShow", function(self) self:SetChecked(get()) end)
  cb:SetScript("OnClick", function(self) set(self:GetChecked()) end)
  return cb
end

function ArchAce:CreateOptions()
  local panel = CreateFrame("Frame", "ArchAceOptions")
  panel.name = "ArchAce"
  local p = function() return self.db.profile end

  local title = panel:CreateFontString(nil, "ARTWORK", "GameFontNormalLarge")
  title:SetPoint("TOPLEFT", panel, "TOPLEFT", 16, -12)
  title:SetText("ArchAce " .. (GetAddOnMetadata("ArchAce", "Version") or ""))

  check(panel, "Cadres du joueur et de la cible", -44, function() return p().unitFrames end,
    function(v) p().unitFrames = v ArchAce:ApplySettings() end)
  check(panel, "Plaques de nom", -72, function() return p().plates end,
    function(v) p().plates = v ArchAce:ApplySettings() end)

  local fov = CreateFrame("Slider", "ArchAceFovSlider", panel, "OptionsSliderTemplate")
  fov:SetPoint("TOPLEFT", panel, "TOPLEFT", 20, -124)
  fov:SetWidth(220)
  fov:SetMinMaxValues(35, 110)
  fov:SetValueStep(1)
  fov.Low:SetText("35°")
  fov.High:SetText("110°")
  fov:SetScript("OnShow", function(s) s:SetValue(p().fov) s.Text:SetFormattedText("Champ de vision : %d°", p().fov) end)
  fov:SetScript("OnValueChanged", function(s, v, user)
    s.Text:SetFormattedText("Champ de vision : %d°", v)
    if user then p().fov = v SetCVar("cameraFov", v) end
  end)

  local x = 16
  for _, name in ipairs({ "defaut", "sombre", "ambre" }) do
    local b = CreateFrame("Button", nil, panel, "UIPanelButtonTemplate")
    b:SetSize(110, 24)
    b:SetPoint("TOPLEFT", panel, "TOPLEFT", x, -176)
    b:SetText("Thème " .. name)
    b:SetScript("OnClick", function() p().theme = name ArchAce:ApplySettings() end)
    x = x + 120
  end

  local profileLabel = panel:CreateFontString(nil, "ARTWORK", "GameFontNormal")
  profileLabel:SetPoint("TOPLEFT", panel, "TOPLEFT", 16, -222)
  profileLabel:SetText("Profil (Entrée pour changer) :")
  local box = CreateFrame("EditBox", "ArchAceProfileBox", panel, "InputBoxTemplate")
  box:SetSize(180, 22)
  box:SetPoint("LEFT", profileLabel, "RIGHT", 10, 0)
  box:SetScript("OnShow", function(s) s:SetText(ArchAce.db:GetCurrentProfile()) end)
  box:SetScript("OnEnterPressed", function(s)
    local name = strtrim(s:GetText())
    if name ~= "" then ArchAce.db:SetProfile(name) end
    s:ClearFocus()
  end)

  local reset = CreateFrame("Button", nil, panel, "UIPanelButtonTemplate")
  reset:SetSize(160, 24)
  reset:SetPoint("TOPLEFT", panel, "TOPLEFT", 16, -262)
  reset:SetText("Réinitialiser le profil")
  reset:SetScript("OnClick", function()
    StaticPopup_Show("ARCHACE_RESET")
  end)
  StaticPopupDialogs["ARCHACE_RESET"] = {
    text = "Réinitialiser le profil « %s » d'ArchAce ?",
    button1 = YES, button2 = NO,
    OnShow = function(dialog) dialog.text:SetFormattedText(StaticPopupDialogs.ARCHACE_RESET.text, ArchAce.db:GetCurrentProfile()) end,
    OnAccept = function() ArchAce.db:ResetProfile() end,
  }

  local category = Settings.RegisterCanvasLayoutCategory(panel, "ArchAce")
  Settings.RegisterAddOnCategory(category)
  self.optionsCategory = category
end

function ArchAce:OpenOptions()
  Settings.OpenToCategory(self.optionsCategory and self.optionsCategory:GetID() or "ArchAce")
end
