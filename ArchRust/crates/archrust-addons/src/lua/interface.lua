-- Interface Lua de base d'ArchRust : objets police, GameTooltip, modèles de frames, filtres de
-- discussion, fenêtres de confirmation (StaticPopup), panneau d'options des AddOns, commandes
-- slash utilitaires. Exécuté après l'API native, avant les AddOns ; tout est remplaçable.

local Templates = ArchRust.Templates

---------------------------------------------------------------------------------------------------
-- Objets police (CreateFont, GameFontNormal…)
---------------------------------------------------------------------------------------------------
local FontMethods = {}
FontMethods.__index = FontMethods
function FontMethods:SetFont(path, size, flags) self._path, self._size, self._flags = path, size or self._size, flags or "" return true end
function FontMethods:GetFont() return self._path or "Fonts\\FRIZQT__.TTF", self._size or 12, self._flags or "" end
function FontMethods:SetTextColor(r, g, b, a)
  if type(r) == "table" then r, g, b, a = r.r, r.g, r.b, r.a end
  self._r, self._g, self._b, self._a = r, g, b, a or 1
end
function FontMethods:GetTextColor() return self._r or 1, self._g or 1, self._b or 1, self._a or 1 end
function FontMethods:SetJustifyH(j) self._justifyH = j end
function FontMethods:GetJustifyH() return self._justifyH or "CENTER" end
function FontMethods:SetJustifyV(j) self._justifyV = j end
function FontMethods:SetShadowColor() end
function FontMethods:SetShadowOffset() end
function FontMethods:GetShadowOffset() return 1, -1 end
function FontMethods:SetSpacing() end
function FontMethods:GetName() return self._name end
function FontMethods:GetObjectType() return "Font" end
function FontMethods:IsObjectType(t) return t == "Font" end
function FontMethods:CopyFontObject(o)
  if type(o) == "string" then o = _G[o] end
  if type(o) ~= "table" then return end
  for k, v in pairs(o) do
    if type(k) == "string" and k:sub(1, 1) == "_" and k ~= "_name" then self[k] = v end
  end
end
FontMethods.SetFontObject = FontMethods.CopyFontObject

function CreateFont(name)
  local f = setmetatable({ _name = name, _size = 12 }, FontMethods)
  if name then _G[name] = f end
  return f
end

local function font(name, size, r, g, b, flags, justify)
  local f = CreateFont(name)
  f:SetFont(nil, size, flags)
  f:SetTextColor(r, g, b, 1)
  f._justifyH = justify
  return f
end
font("GameFontNormal", 12, 1, 0.82, 0)
font("GameFontHighlight", 12, 1, 1, 1)
font("GameFontDisable", 12, 0.5, 0.5, 0.5)
font("GameFontNormalSmall", 10, 1, 0.82, 0)
font("GameFontHighlightSmall", 10, 1, 1, 1)
font("GameFontDisableSmall", 10, 0.5, 0.5, 0.5)
font("GameFontNormalLarge", 16, 1, 0.82, 0)
font("GameFontHighlightLarge", 16, 1, 1, 1)
font("GameFontNormalHuge", 20, 1, 0.82, 0)
font("GameFontRed", 12, 1, 0.1, 0.1)
font("GameFontGreen", 12, 0.1, 1, 0.1)
font("GameFontWhite", 12, 1, 1, 1)
font("GameFontNormalLeft", 12, 1, 0.82, 0, nil, "LEFT")
font("GameFontHighlightLeft", 12, 1, 1, 1, nil, "LEFT")
font("ChatFontNormal", 14, 1, 1, 1)
font("ChatFontSmall", 12, 1, 1, 1)
font("NumberFontNormal", 12, 1, 1, 1, "OUTLINE")
font("NumberFontNormalSmall", 10, 1, 1, 1, "OUTLINE")
font("NumberFontNormalLarge", 16, 1, 1, 1, "OUTLINE")
font("SystemFont_Small", 10, 1, 1, 1)
font("SystemFont_Med1", 12, 1, 1, 1)
font("SystemFont_Large", 16, 1, 1, 1)
font("SystemFont_Shadow_Med1", 12, 1, 1, 1)
font("SystemFont_Outline", 12, 1, 1, 1, "OUTLINE")
font("GameTooltipText", 12, 1, 1, 1)
font("GameTooltipTextSmall", 10, 1, 1, 1)
font("GameTooltipHeaderText", 14, 1, 1, 1)
font("QuestFont", 13, 0.2, 0.1, 0)
font("SubZoneTextFont", 22, 1, 0.82, 0, "OUTLINE")
font("ZoneTextFont", 28, 1, 0.82, 0, "OUTLINE")
STANDARD_TEXT_FONT = "Fonts\\FRIZQT__.TTF"
UNIT_NAME_FONT = STANDARD_TEXT_FONT
DAMAGE_TEXT_FONT = STANDARD_TEXT_FONT
NAMEPLATE_FONT = STANDARD_TEXT_FONT

OKAY, CANCEL, YES, NO, ACCEPT, DECLINE, CLOSE = "OK", "Annuler", "Oui", "Non", "Accepter", "Refuser", "Fermer"
ENABLE, DISABLE, DEFAULTS, SETTINGS = "Activer", "Désactiver", "Par défaut", "Options"

local BACKDROP = {
  bgFile = "Interface\\Tooltips\\UI-Tooltip-Background",
  edgeFile = "Interface\\Tooltips\\UI-Tooltip-Border",
  edgeSize = 1,
}
ArchRust.DefaultBackdrop = BACKDROP

---------------------------------------------------------------------------------------------------
-- Modèles de frames (`CreateFrame(type, nom, parent, "Modèle")`) ; un AddOn peut en ajouter :
-- `ArchRust.Templates.MonModele = function(frame) … end`.
---------------------------------------------------------------------------------------------------
function ArchRust.RegisterTemplate(name, fn) Templates[name] = fn end

Templates.BackdropTemplate = function() end
Templates.SecureHandlerBaseTemplate = function() end
Templates.SecureActionButtonTemplate = function() end

Templates.UIPanelButtonTemplate = function(f)
  if f:GetWidth() == 0 then f:SetSize(100, 22) end
end
Templates.UIPanelButtonNoTooltipTemplate = Templates.UIPanelButtonTemplate
Templates.GameMenuButtonTemplate = Templates.UIPanelButtonTemplate

Templates.UIPanelCloseButton = function(f)
  f:SetSize(22, 22)
  f:SetText("x")
  f:SetScript("OnClick", function(self) local p = self:GetParent() if p then p:Hide() end end)
end
Templates.UIPanelCloseButtonNoScripts = Templates.UIPanelCloseButton

local function check_template(f)
  f:SetSize(22, 22)
  f.Text = f:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
  f.Text:SetJustifyH("LEFT")
  f.Text:SetPoint("LEFT", f, "RIGHT", 4, 0)
  f.text = f.Text
  local name = f:GetName()
  if name then _G[name .. "Text"] = f.Text end
end
Templates.UICheckButtonTemplate = check_template
Templates.InterfaceOptionsCheckButtonTemplate = check_template
Templates.ChatConfigCheckButtonTemplate = check_template
Templates.OptionsBaseCheckButtonTemplate = check_template

local function slider_template(f)
  f:SetSize(160, 16)
  f:SetOrientation("HORIZONTAL")
  f:SetBackdrop(BACKDROP)
  f:SetBackdropColor(0.1, 0.1, 0.1, 0.8)
  f.Text = f:CreateFontString(nil, "ARTWORK", "GameFontNormal")
  f.Text:SetPoint("BOTTOM", f, "TOP", 0, 2)
  f.Low = f:CreateFontString(nil, "ARTWORK", "GameFontHighlightSmall")
  f.Low:SetPoint("TOPLEFT", f, "BOTTOMLEFT", 0, -2)
  f.High = f:CreateFontString(nil, "ARTWORK", "GameFontHighlightSmall")
  f.High:SetPoint("TOPRIGHT", f, "BOTTOMRIGHT", 0, -2)
  local name = f:GetName()
  if name then
    _G[name .. "Text"], _G[name .. "Low"], _G[name .. "High"] = f.Text, f.Low, f.High
  end
end
Templates.OptionsSliderTemplate = slider_template
Templates.UISliderTemplate = slider_template
Templates.MinimalSliderTemplate = slider_template

Templates.InputBoxTemplate = function(f)
  f:SetBackdrop(BACKDROP)
  f:SetBackdropColor(0, 0, 0, 0.6)
  f:SetBackdropBorderColor(0.5, 0.5, 0.5, 1)
  f:SetFontObject(ChatFontNormal)
  if f:GetHeight() == 0 then f:SetHeight(20) end
  f:SetScript("OnEscapePressed", function(self) self:ClearFocus() end)
end
Templates.InputBoxInstructionsTemplate = Templates.InputBoxTemplate
Templates.SearchBoxTemplate = Templates.InputBoxTemplate

local function basic_frame(f)
  f:SetBackdrop(BACKDROP)
  f:SetBackdropColor(0.05, 0.04, 0.03, 0.92)
  f:SetBackdropBorderColor(0.78, 0.6, 0.3, 1)
  f.TitleText = f:CreateFontString(nil, "OVERLAY", "GameFontNormal")
  f.TitleText:SetPoint("TOP", f, "TOP", 0, -6)
  f.title = f.TitleText
  f.CloseButton = CreateFrame("Button", nil, f, "UIPanelCloseButton")
  f.CloseButton:SetPoint("TOPRIGHT", f, "TOPRIGHT", -4, -4)
  f.SetTitle = function(self, t) self.TitleText:SetText(t) end
end
Templates.BasicFrameTemplate = basic_frame
Templates.BasicFrameTemplateWithInset = basic_frame
Templates.UIPanelDialogTemplate = basic_frame
Templates.PortraitFrameTemplate = basic_frame
Templates.ButtonFrameTemplate = basic_frame
Templates.DefaultPanelTemplate = basic_frame
Templates.TooltipBackdropTemplate = function(f)
  f:SetBackdrop(BACKDROP)
  f:SetBackdropColor(0.05, 0.05, 0.1, 0.92)
  f:SetBackdropBorderColor(0.6, 0.6, 0.7, 1)
end

---------------------------------------------------------------------------------------------------
-- GameTooltip
---------------------------------------------------------------------------------------------------
local TooltipMixin = {}
ArchRust.TooltipMixin = TooltipMixin
local LINE_H = 15

local function tip_line(self, i)
  local l = self._left[i]
  if not l then
    l = self:CreateFontString(nil, "OVERLAY", i == 1 and "GameTooltipHeaderText" or "GameTooltipText")
    l:SetJustifyH("LEFT")
    local r = self:CreateFontString(nil, "OVERLAY", "GameTooltipText")
    r:SetJustifyH("RIGHT")
    self._left[i], self._right[i] = l, r
    local name = self:GetName()
    if name then _G[name .. "TextLeft" .. i], _G[name .. "TextRight" .. i] = l, r end
  end
  return l, self._right[i]
end

function TooltipMixin:Layout()
  local w = 0
  for i = 1, self._n do
    local l, r = self._left[i], self._right[i]
    local lw = l:GetStringWidth() + (r:IsShown() and (r:GetStringWidth() + 20) or 0)
    if lw > w then w = lw end
    l:ClearAllPoints()
    l:SetPoint("TOPLEFT", self, "TOPLEFT", 8, -8 - (i - 1) * LINE_H)
    r:ClearAllPoints()
    r:SetPoint("TOPRIGHT", self, "TOPRIGHT", -8, -8 - (i - 1) * LINE_H)
  end
  self:SetSize(math.max(w + 16, 60), self._n * LINE_H + 14)
end

function TooltipMixin:SetOwner(owner, anchor, x, y)
  self:ClearLines()
  self._owner, self._anchor = owner, anchor or "ANCHOR_RIGHT"
  self:ClearAllPoints()
  x, y = x or 0, y or 0
  local a = self._anchor
  if a == "ANCHOR_RIGHT" then self:SetPoint("BOTTOMLEFT", owner, "TOPRIGHT", x, y)
  elseif a == "ANCHOR_LEFT" then self:SetPoint("BOTTOMRIGHT", owner, "TOPLEFT", x, y)
  elseif a == "ANCHOR_TOP" then self:SetPoint("BOTTOM", owner, "TOP", x, y)
  elseif a == "ANCHOR_BOTTOM" then self:SetPoint("TOP", owner, "BOTTOM", x, y)
  elseif a == "ANCHOR_TOPLEFT" then self:SetPoint("BOTTOMLEFT", owner, "TOPLEFT", x, y)
  elseif a == "ANCHOR_TOPRIGHT" then self:SetPoint("BOTTOMRIGHT", owner, "TOPRIGHT", x, y)
  elseif a == "ANCHOR_BOTTOMLEFT" then self:SetPoint("TOPRIGHT", owner, "BOTTOMLEFT", x, y)
  elseif a == "ANCHOR_BOTTOMRIGHT" then self:SetPoint("TOPLEFT", owner, "BOTTOMRIGHT", x, y)
  elseif a == "ANCHOR_PRESERVE" or a == "ANCHOR_NONE" then
  else self:SetPoint("BOTTOMRIGHT", UIParent, "BOTTOMRIGHT", -20, 120)
  end
  self._cursorOffset = { x, y }
end
function TooltipMixin:GetOwner() return self._owner end
function TooltipMixin:IsOwned(f) return self._owner == f end
function TooltipMixin:SetAnchorType(a, x, y) self:SetOwner(self._owner, a, x, y) end
function TooltipMixin:GetAnchorType() return self._anchor end
function TooltipMixin:ClearLines()
  for i = 1, #self._left do self._left[i]:Hide() self._right[i]:Hide() end
  self._n = 0
end
function TooltipMixin:AddLine(text, r, g, b)
  self._n = self._n + 1
  local l, rr = tip_line(self, self._n)
  l:SetText(text or "")
  if r then l:SetTextColor(r, g, b, 1) else l:SetTextColor(self._n == 1 and 1 or 1, self._n == 1 and 1 or 0.82, self._n == 1 and 1 or 0, 1) end
  l:Show()
  rr:Hide()
  self:Layout()
end
function TooltipMixin:AddDoubleLine(left, right, lr, lg, lb, rr, rg, rb)
  self:AddLine(left, lr, lg, lb)
  local r = self._right[self._n]
  r:SetText(right or "")
  if rr then r:SetTextColor(rr, rg, rb, 1) else r:SetTextColor(1, 1, 1, 1) end
  r:Show()
  self:Layout()
end
function TooltipMixin:SetText(text, r, g, b)
  self:ClearLines()
  self:AddLine(text, r or 1, g or 1, b or 1)
end
function TooltipMixin:NumLines() return self._n end
function TooltipMixin:SetUnit(unit)
  if not UnitExists(unit) then return end
  self:SetText(UnitName(unit))
  local hp, hpMax = UnitHealth(unit), UnitHealthMax(unit)
  if hpMax > 0 then self:AddDoubleLine("PV", hp .. " / " .. hpMax, 1, 0.82, 0, 0.2, 1, 0.2) end
  self:Show()
end
function TooltipMixin:GetUnit()
  return nil
end
function TooltipMixin:FadeOut() self:Hide() end
function TooltipMixin:AddTexture() end
function TooltipMixin:SetMinimumWidth(w) self._minWidth = w end
function TooltipMixin:SetPadding() end
function TooltipMixin:SetHyperlink(link) self:SetText(link) end
function TooltipMixin:SetItemByID(id) self:SetText(GetItemInfo(id) or ("Objet " .. tostring(id))) end
function TooltipMixin:SetSpellByID(id) self:SetText("Sort " .. tostring(id)) end

local function init_tooltip(f)
  Mixin(f, TooltipMixin)
  f._left, f._right, f._n = {}, {}, 0
  f:SetFrameStrata("TOOLTIP")
  f:SetBackdrop(BACKDROP)
  f:SetBackdropColor(0.04, 0.04, 0.08, 0.92)
  f:SetBackdropBorderColor(0.6, 0.6, 0.7, 1)
  f:Hide()
  f:SetScript("OnUpdate", function(self)
    if self._owner and type(self._owner) == "table" and self._owner.IsVisible and not self._owner:IsVisible() then
      self:Hide()
      return
    end
    if self._anchor == "ANCHOR_CURSOR" then
      local x, y = GetCursorPosition()
      self:ClearAllPoints()
      self:SetPoint("BOTTOMLEFT", UIParent, "BOTTOMLEFT", x + 14 + self._cursorOffset[1], y + 10 + self._cursorOffset[2])
    end
    self:Layout()
  end)
end
Templates.GameTooltipTemplate = init_tooltip
Templates.SharedTooltipTemplate = init_tooltip

local rawCreateFrame = CreateFrame
function CreateFrame(kind, name, parent, template, id)
  local f = rawCreateFrame(kind, name, parent, template, id)
  if type(kind) == "string" and kind:lower() == "gametooltip" and not f._left then init_tooltip(f) end
  return f
end

CreateFrame("GameTooltip", "GameTooltip", UIParent)
CreateFrame("GameTooltip", "ItemRefTooltip", UIParent)
function GameTooltip_SetDefaultAnchor(tip, parent) tip:SetOwner(parent or UIParent, "ANCHOR_NONE") tip:SetPoint("BOTTOMRIGHT", UIParent, "BOTTOMRIGHT", -20, 120) end
function GameTooltip_Hide() GameTooltip:Hide() end

---------------------------------------------------------------------------------------------------
-- Discussion : filtres de messages (ChatFrame_AddMessageEventFilter)
---------------------------------------------------------------------------------------------------
local chatFilters = {}
function ChatFrame_AddMessageEventFilter(event, fn)
  local list = chatFilters[event] or {}
  chatFilters[event] = list
  for _, f in ipairs(list) do if f == fn then return end end
  list[#list + 1] = fn
end
function ChatFrame_RemoveMessageEventFilter(event, fn)
  local list = chatFilters[event]
  if list then tDeleteItem(list, fn) end
end
function ChatFrame_GetMessageEventFilters(event) return chatFilters[event] end
-- Appelé par le moteur pour chaque message reçu : rend `true` pour masquer, sinon le message et
-- l'auteur (éventuellement modifiés).
function ArchRust.FilterChat(event, msg, sender, ...)
  local list = chatFilters[event]
  if list then
    for _, f in ipairs(list) do
      local ok, discard, nmsg, nsender = pcall(f, DEFAULT_CHAT_FRAME, event, msg, sender, ...)
      if ok then
        if discard then return true end
        msg, sender = nmsg or msg, nsender or sender
      else
        geterrorhandler()(discard)
      end
    end
  end
  return false, msg, sender
end
function ChatFrame_OpenChat(text) ArchRust._openChat = text end
function ChatEdit_GetActiveWindow() return nil end
function ChatFrame_AddChannel() end

---------------------------------------------------------------------------------------------------
-- Fenêtres de confirmation (StaticPopup)
---------------------------------------------------------------------------------------------------
StaticPopupDialogs = {}
STATICPOPUP_NUMDIALOGS = 1
local popup
local function make_popup()
  local f = CreateFrame("Frame", "StaticPopup1", UIParent, "TooltipBackdropTemplate")
  f:SetSize(340, 120)
  f:SetPoint("TOP", UIParent, "TOP", 0, -140)
  f:SetFrameStrata("DIALOG")
  f:EnableMouse(true)
  f.text = f:CreateFontString("StaticPopup1Text", "OVERLAY", "GameFontHighlight")
  f.text:SetPoint("TOP", f, "TOP", 0, -16)
  f.text:SetWidth(310)
  f.editBox = CreateFrame("EditBox", "StaticPopup1EditBox", f, "InputBoxTemplate")
  f.editBox:SetSize(220, 22)
  f.editBox:SetPoint("CENTER", f, "CENTER", 0, 0)
  f.button1 = CreateFrame("Button", "StaticPopup1Button1", f, "UIPanelButtonTemplate")
  f.button2 = CreateFrame("Button", "StaticPopup1Button2", f, "UIPanelButtonTemplate")
  f.button1:SetSize(120, 24)
  f.button2:SetSize(120, 24)
  local function finish(which_button)
    local info = StaticPopupDialogs[f.which]
    if not info then f:Hide() return end
    local keep
    if which_button == 1 and info.OnAccept then keep = info.OnAccept(f, f.data, f.data2)
    elseif which_button == 2 and info.OnCancel then keep = info.OnCancel(f, f.data, "clicked") end
    if not keep then f:Hide() end
  end
  f.button1:SetScript("OnClick", function() finish(1) end)
  f.button2:SetScript("OnClick", function() finish(2) end)
  f.editBox:SetScript("OnEnterPressed", function()
    local info = StaticPopupDialogs[f.which]
    if info and info.EditBoxOnEnterPressed then info.EditBoxOnEnterPressed(f.editBox, f.data) else finish(1) end
  end)
  f:SetScript("OnHide", function(self)
    local info = StaticPopupDialogs[self.which]
    if info and info.OnHide then info.OnHide(self, self.data) end
  end)
  f:Hide()
  return f
end
function StaticPopup_Show(which, a1, a2, data)
  local info = StaticPopupDialogs[which]
  if not info then return nil end
  popup = popup or make_popup()
  popup.which, popup.data, popup.data2 = which, data, nil
  popup.text:SetText(format(info.text or "", a1, a2))
  popup.button1:SetText(info.button1 or OKAY)
  popup.button2:SetText(info.button2 or CANCEL)
  popup.button2:SetShown(info.button2 ~= nil)
  popup.button1:ClearAllPoints()
  if info.button2 then
    popup.button1:SetPoint("BOTTOMRIGHT", popup, "BOTTOM", -6, 12)
    popup.button2:SetPoint("BOTTOMLEFT", popup, "BOTTOM", 6, 12)
  else
    popup.button1:SetPoint("BOTTOM", popup, "BOTTOM", 0, 12)
  end
  popup.editBox:SetShown(info.hasEditBox and true or false)
  popup.editBox:SetText("")
  popup:Show()
  if info.OnShow then info.OnShow(popup, data) end
  return popup
end
function StaticPopup_Hide(which)
  if popup and popup:IsShown() and (not which or popup.which == which) then popup:Hide() end
end
function StaticPopup_Visible(which)
  if popup and popup:IsShown() and popup.which == which then return "StaticPopup1", popup end
end

---------------------------------------------------------------------------------------------------
-- Panneau d'options des AddOns (InterfaceOptions_AddCategory, Settings)
---------------------------------------------------------------------------------------------------
local categories = {}
ArchRust.OptionCategories = categories
local optionsFrame
local function select_panel(panel)
  for _, p in ipairs(categories) do
    if p ~= panel then p:Hide() end
  end
  panel:SetParent(optionsFrame.container)
  panel:ClearAllPoints()
  panel:SetAllPoints(optionsFrame.container)
  panel:Show()
  if panel.refresh then securecall(panel.refresh, panel) end
  if panel.OnRefresh then securecall(panel.OnRefresh, panel) end
  optionsFrame.current = panel
  optionsFrame:SetTitle("Options — " .. (panel.name or "?"))
end
local function build_options()
  local f = CreateFrame("Frame", "ArchRustOptionsFrame", UIParent, "BasicFrameTemplate")
  f:SetSize(680, 460)
  f:SetPoint("CENTER")
  f:SetFrameStrata("DIALOG")
  f:EnableMouse(true)
  f:SetMovable(true)
  f:RegisterForDrag("LeftButton")
  f:SetScript("OnDragStart", f.StartMoving)
  f:SetScript("OnDragStop", f.StopMovingOrSizing)
  f:SetTitle("Options des AddOns")
  f.container = CreateFrame("Frame", nil, f)
  f.container:SetPoint("TOPLEFT", f, "TOPLEFT", 176, -32)
  f.container:SetPoint("BOTTOMRIGHT", f, "BOTTOMRIGHT", -10, 10)
  f.buttons = {}
  f:SetScript("OnHide", function(self)
    local p = self.current
    if p and p.okay then securecall(p.okay, p) end
  end)
  tinsert(UISpecialFrames, "ArchRustOptionsFrame")
  f:Hide()
  return f
end
local function refresh_list()
  for i, panel in ipairs(categories) do
    local b = optionsFrame.buttons[i]
    if not b then
      b = CreateFrame("Button", nil, optionsFrame, "UIPanelButtonTemplate")
      b:SetSize(156, 22)
      b:SetPoint("TOPLEFT", optionsFrame, "TOPLEFT", 10, -32 - (i - 1) * 26)
      optionsFrame.buttons[i] = b
    end
    b:SetText((panel.parent and "  " or "") .. (panel.name or ("Panneau " .. i)))
    b:SetScript("OnClick", function() select_panel(panel) end)
    b:Show()
  end
end
function InterfaceOptions_AddCategory(panel)
  if type(panel) ~= "table" then return end
  for _, p in ipairs(categories) do if p == panel then return end end
  categories[#categories + 1] = panel
  if panel.Hide then panel:Hide() end
  if optionsFrame then refresh_list() end
end
function ArchRust.OpenOptions(which)
  optionsFrame = optionsFrame or build_options()
  refresh_list()
  local target
  for _, p in ipairs(categories) do
    if p == which or (type(which) == "string" and p.name == which) then target = p end
  end
  target = target or categories[1]
  optionsFrame:Show()
  if target then select_panel(target) end
end
InterfaceOptionsFrame_OpenToCategory = ArchRust.OpenOptions
InterfaceOptionsFrame_Show = ArchRust.OpenOptions
local function category(name) return { ID = name, name = name, GetID = function(self) return self.ID end, GetName = function(self) return self.name end } end
Settings = {
  RegisterCanvasLayoutCategory = function(panel, name)
    panel.name = panel.name or name
    InterfaceOptions_AddCategory(panel)
    return category(name), panel
  end,
  RegisterCanvasLayoutSubcategory = function(parent, panel, name)
    panel.name = panel.name or name
    panel.parent = parent and parent.name
    InterfaceOptions_AddCategory(panel)
    return category(name), panel
  end,
  RegisterVerticalLayoutCategory = function(name) return category(name) end,
  RegisterAddOnCategory = function() end,
  OpenToCategory = function(id) ArchRust.OpenOptions(id) end,
  RegisterAddOnSetting = function() return {} end,
  CreateCheckbox = function() end,
  CreateSlider = function() end,
  CreateDropdown = function() end,
}
UISpecialFrames = UISpecialFrames or {}
function CloseSpecialWindows()
  local closed = false
  for _, name in ipairs(UISpecialFrames) do
    local f = _G[name]
    if f and f.IsShown and f:IsShown() then f:Hide() closed = true end
  end
  return closed
end

-- Échap ferme les fenêtres de UISpecialFrames (le moteur envoie ARCHRUST_KEY_DOWN).
local keys = CreateFrame("Frame", "ArchRustKeyFrame")
keys:RegisterEvent("ARCHRUST_KEY_DOWN")
keys:SetScript("OnEvent", function(_, _, key)
  if key == "ESCAPE" then
    if GameTooltip:IsShown() then GameTooltip:Hide() end
    CloseSpecialWindows()
  end
end)

---------------------------------------------------------------------------------------------------
-- Commandes slash utilitaires
---------------------------------------------------------------------------------------------------
local function dump(v, depth, indent, seen)
  indent = indent or ""
  if type(v) ~= "table" or depth <= 0 then return tostring(v) end
  seen = seen or {}
  if seen[v] then return "<cycle>" end
  seen[v] = true
  local parts, n = {}, 0
  for k, x in pairs(v) do
    n = n + 1
    if n > 40 then parts[#parts + 1] = indent .. "  …" break end
    parts[#parts + 1] = indent .. "  [" .. tostring(k) .. "] = " .. dump(x, depth - 1, indent .. "  ", seen)
  end
  return "{\n" .. table.concat(parts, "\n") .. "\n" .. indent .. "}"
end
ArchRust.Dump = function(v, depth) return dump(v, depth or 2) end

SLASH_ARCHRUN1, SLASH_ARCHRUN2 = "/run", "/script"
SlashCmdList.ARCHRUN = function(msg)
  local f, err = loadstring(msg, "=/run")
  if not f then print("|cffff4040" .. tostring(err) .. "|r") return end
  securecall(f)
end
SLASH_ARCHDUMP1 = "/dump"
SlashCmdList.ARCHDUMP = function(msg)
  local f, err = loadstring("return " .. msg, "=/dump")
  if not f then print("|cffff4040" .. tostring(err) .. "|r") return end
  local r = table.pack(pcall(f))
  if not r[1] then print("|cffff4040" .. tostring(r[2]) .. "|r") return end
  print("|cff88ccff" .. msg .. "|r :")
  for i = 2, math.max(r.n, 2) do
    for line in tostring(dump(r[i], 2)):gmatch("[^\n]+") do print(line) end
  end
end
SLASH_ARCHTARGET1, SLASH_ARCHTARGET2 = "/target", "/tar"
SlashCmdList.ARCHTARGET = function(msg)
  msg = strtrim(msg or "")
  local ok = (msg == "") and TargetNearest() or TargetUnit(msg)
  if ok then print("Cible : " .. (UnitName("target") or "?")) else print("Aucune unité « " .. msg .. " » en vue") end
end
SLASH_ARCHCLEARTARGET1 = "/cleartarget"
SlashCmdList.ARCHCLEARTARGET = function() ClearTarget() end
SLASH_ARCHOPTIONS1, SLASH_ARCHOPTIONS2 = "/options", "/aoptions"
SlashCmdList.ARCHOPTIONS = function(msg) ArchRust.OpenOptions(msg ~= "" and msg or nil) end
SLASH_ARCHADDONS1 = "/addons"
SlashCmdList.ARCHADDONS = function()
  for i = 1, GetNumAddOns() do
    local name, title, notes, _, reason = GetAddOnInfo(i)
    local loaded = IsAddOnLoaded(name)
    print((loaded and "|cff40ff40✔|r " or "|cffff4040✘|r ") .. (title or name) .. (reason and (" (" .. reason .. ")") or ""))
  end
end
SLASH_ARCHERRORS1 = "/errors"
SlashCmdList.ARCHERRORS = function()
  local errs = ArchRust.GetErrors()
  if #errs == 0 then print("Aucune erreur Lua") end
  for _, e in ipairs(errs) do print("|cffff4040" .. e .. "|r") end
end
SLASH_ARCHLOAD1 = "/loadaddon"
SlashCmdList.ARCHLOAD = function(msg)
  local ok, reason = LoadAddOn(strtrim(msg))
  print(ok and ("AddOn chargé : " .. msg) or ("Échec du chargement de " .. msg .. " : " .. tostring(reason)))
end
