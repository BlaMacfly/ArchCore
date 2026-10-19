-- Plaques de nom : un texte au-dessus de chaque unité en vue, placé chaque image avec
-- ArchRust.WorldToScreen (projection du monde 3D vers l'écran). Couleur selon les PV, cible en jaune.
local ArchAce = LibStub("AceAddon-3.0"):GetAddon("ArchAce")

local HEAD = 190 -- hauteur au-dessus des pieds (unités du monde)

function ArchAce:CreateNameplates()
  local holder = CreateFrame("Frame", "ArchAceNameplates", UIParent)
  holder:SetAllPoints(UIParent)
  holder:SetFrameStrata("BACKGROUND")
  self.plates = holder
  local pool = {}

  local function plate(i)
    local fs = pool[i]
    if not fs then
      fs = holder:CreateFontString(nil, "OVERLAY", "GameFontNormalSmall")
      fs:SetFont(nil, 12, "OUTLINE")
      pool[i] = fs
    end
    return fs
  end

  local acc = 0
  holder:SetScript("OnUpdate", function(_, elapsed)
    acc = acc + elapsed
    local units = ArchRust.GetUnits()
    local maxDist = tonumber(GetCVar("nameplateDistance")) or 6000
    local me = ArchRust.GetUnit("player")
    local targetGUID = UnitGUID("target")
    local shown = 0
    for _, u in ipairs(units) do
      local dx, dz = me and (u.x - me.x) or 0, me and (u.z - me.z) or 0
      if dx * dx + dz * dz <= maxDist * maxDist then
        local sx, sy = ArchRust.WorldToScreen(u.x, u.y + HEAD, u.z)
        if sx then
          shown = shown + 1
          local fs = plate(shown)
          local text = u.name
          if u.hp and u.hpMax and u.hpMax > 0 then
            local pct = u.hp / u.hpMax
            local hex = pct > 0.6 and "40ff40" or (pct > 0.3 and "ffd040" or "ff4040")
            text = format("%s |cff%s%d%%|r", u.name, hex, Round(pct * 100))
          end
          if u.guid == targetGUID then text = "|cffffff00> |r" .. text .. "|cffffff00 <|r" end
          fs:SetText(text)
          fs:ClearAllPoints()
          fs:SetPoint("BOTTOM", UIParent, "BOTTOMLEFT", sx, sy)
          fs:Show()
        end
      end
    end
    for i = shown + 1, #pool do pool[i]:Hide() end
  end)
end
