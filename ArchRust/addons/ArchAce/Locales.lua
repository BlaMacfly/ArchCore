-- Traductions (AceLocale-3.0) : l'anglais sert de langue par défaut, le français la remplace.
local L = LibStub("AceLocale-3.0"):NewLocale("ArchAce", "enUS", true)
L["LOADED"] = "ArchAce loaded — type /archace"
L["TARGET_NONE"] = "No target"
L["THEME_SET"] = "Theme: %s"
L["PLATES"] = "Nameplates: %s"
L["ON"] = "on"
L["OFF"] = "off"
L["HELP"] = "Commands: demo, plates on|off, theme dark|amber|default, sky r g b, fov n, target <name>, tint, bind <key>, profile <name>, options, reset"
L["BOUND"] = "%s now targets the nearest unit"
L["PROFILE"] = "Profile: %s"
L["MESSAGES_SENT"] = "Messages sent this session: %d"

L = LibStub("AceLocale-3.0"):NewLocale("ArchAce", "frFR")
if not L then return end
L["LOADED"] = "ArchAce chargé — tapez /archace"
L["TARGET_NONE"] = "Pas de cible"
L["THEME_SET"] = "Thème : %s"
L["PLATES"] = "Plaques de nom : %s"
L["ON"] = "activées"
L["OFF"] = "désactivées"
L["HELP"] = "Commandes : demo, plates on|off, theme sombre|ambre|defaut, sky r g b, fov n, target <nom>, tint, bind <touche>, profile <nom>, options, reset"
L["BOUND"] = "%s cible maintenant l'unité la plus proche"
L["PROFILE"] = "Profil : %s"
L["MESSAGES_SENT"] = "Messages envoyés pendant la session : %d"
