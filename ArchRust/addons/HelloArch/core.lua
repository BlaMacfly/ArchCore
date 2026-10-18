-- HelloArch : AddOn d'exemple pour ArchRust (API façon World of Warcraft).
local addonName, ns = ...

HelloArchDB = HelloArchDB or { logins = 0 }

-- Une petite fenêtre en haut de l'écran.
local frame = CreateFrame("Frame", "HelloArchFrame", UIParent)
frame:SetSize(320, 40)
frame:SetPoint("TOP", UIParent, "TOP", 0, -20)
frame:SetBackdropColor(0, 0, 0, 0.5)
frame:EnableMouse(true)
frame:SetMovable(true)

local label = frame:CreateFontString(nil, "OVERLAY")
label:SetPoint("CENTER")
label:SetFont(nil, 14)
label:SetText("Chargement…")

local fps = frame:CreateFontString(nil, "OVERLAY")
fps:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -4, 2)
fps:SetFont(nil, 10)

frame:RegisterEvent("PLAYER_LOGIN")
frame:RegisterEvent("PLAYER_ENTERING_WORLD")
frame:RegisterEvent("CHAT_MSG_SAY")
frame:SetScript("OnEvent", function(self, event, ...)
  if event == "PLAYER_LOGIN" then
    HelloArchDB.logins = HelloArchDB.logins + 1
    label:SetFormattedText("Bienvenue %s (connexion n°%d)", UnitName("player") or "?", HelloArchDB.logins)
    print("|cff00ff00HelloArch|r chargé — tapez /hello")
  elseif event == "PLAYER_ENTERING_WORLD" then
    print("Vous entrez dans " .. (GetRealmName() or "le monde"))
  elseif event == "CHAT_MSG_SAY" then
    local msg, author = ...
    ns.last = { msg = msg, author = author }
  end
end)

local acc = 0
frame:SetScript("OnUpdate", function(self, elapsed)
  acc = acc + elapsed
  if acc > 0.5 then
    acc = 0
    fps:SetFormattedText("%.0f ips", GetFramerate())
  end
end)

SLASH_HELLOARCH1 = "/hello"
SLASH_HELLOARCH2 = "/bonjour"
SlashCmdList["HELLOARCH"] = function(msg)
  if msg == "hide" then frame:Hide() elseif msg == "show" then frame:Show()
  else SendChatMessage("Bonjour depuis HelloArch ! " .. msg, "SAY") end
end

C_Timer.After(5, function() print("HelloArch : 5 secondes se sont écoulées") end)
