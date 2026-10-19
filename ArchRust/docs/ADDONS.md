# AddOns Lua d'ArchRust

*[English version](ADDONS.en.md)*

ArchRust charge des AddOns Lua à la manière de World of Warcraft : mêmes fichiers `.toc`, mêmes
noms de fonctions, mêmes bibliothèques (LibStub, Ace3…). Le but est qu'un AddOn puisse modifier un
maximum de choses du client — interface, discussion, caméra, rendu, raccourcis — et qu'une bonne
partie du code et des habitudes de WoW se transposent.

- Moteur Lua : **Lua 5.4** (mlua), avec une couche de compatibilité Lua 5.1 / WoW.
- Code : `crates/archrust-addons` (API, frames, bibliothèques) et
  `crates/archrust-engine/src/addon_ui.rs`, `app_addons.rs` (dessin et liaison avec le jeu).
- Exemples : `addons/HelloArch` (API de base), `addons/ArchAce` (LibStub + Ace3 + API étendue).

## Sommaire

1. [Structure d'un AddOn](#structure-dun-addon)
2. [Cycle de vie et événements](#cycle-de-vie-et-événements)
3. [Bibliothèques Lua standard](#bibliothèques-lua-standard)
4. [Bibliothèques d'AddOns intégrées](#bibliothèques-dadons-intégrées)
5. [Frames et interface](#frames-et-interface)
6. [API du jeu](#api-du-jeu)
7. [Espace `ArchRust` : modifier le client](#espace-archrust--modifier-le-client)
8. [CVars](#cvars)
9. [Commandes slash fournies](#commandes-slash-fournies)
10. [Différences avec WoW](#différences-avec-wow)

## Structure d'un AddOn

```text
addons/MonAddOn/
  MonAddOn.toc
  embeds.xml          (facultatif : bibliothèques livrées avec l'AddOn)
  libs/…              (facultatif)
  core.lua
  media/barre.png     (textures, polices .ttf/.otf)
```

```text
## Title: Mon AddOn
## Version: 1.0
## Author: Moi
## Interface: 10000
## Notes: Ce que fait l'AddOn
## SavedVariables: MonAddOnDB
## SavedVariablesPerCharacter: MonAddOnCharDB
## Dependencies: AutreAddOn
## OptionalDeps: Ace3
## LoadOnDemand: 0
## X-Site: https://…

embeds.xml
core.lua
```

- Les lignes `.lua` sont exécutées dans l'ordre ; chaque fichier reçoit `local addonName, ns = ...`
  (`ns` est une table propre à l'AddOn, partagée entre ses fichiers).
- Les lignes `.xml` (ex. `embeds.xml`, `libs\AceAddon-3.0\AceAddon-3.0.xml`) sont lues pour leurs
  balises `<Script file="…"/>` et `<Include file="…"/>`, récursivement. Les frames décrites en XML
  ne sont pas interprétées : créez-les en Lua.
- `## Dependencies` charge d'abord les AddOns requis (erreur s'ils manquent),
  `## OptionalDeps` d'abord ceux qui existent.
- `## LoadOnDemand: 1` : l'AddOn n'est chargé que par `LoadAddOn("Nom")` (ou `/loadaddon Nom`).
- SavedVariables : écrites dans `saved-variables/<AddOn>.lua` (par compte) et
  `saved-variables/<Royaume>/<Personnage>/<AddOn>.lua` (par personnage), relues avant le premier
  fichier Lua de l'AddOn.

## Cycle de vie et événements

Comme sous WoW, les AddOns se chargent **à l'entrée en jeu** (le personnage est alors connu) :

1. pour chaque AddOn : SavedVariables, fichiers, puis `ADDON_LOADED(nomAddOn)` ;
2. `VARIABLES_LOADED`, `ADDONS_LOADED` ;
3. `PLAYER_LOGIN`, puis `PLAYER_ENTERING_WORLD(isInitialLogin, isReloadingUi)` ;
4. à la déconnexion, à `/reload` et à la fermeture : `PLAYER_LEAVING_WORLD`, `PLAYER_LOGOUT`, puis
   écriture des SavedVariables. `/reload` recharge ensuite tous les AddOns.

L'interface des AddOns n'apparaît qu'en jeu (pas sur l'écran de connexion).

Événements émis par le client :

| Événement | Arguments |
|---|---|
| `ADDON_LOADED` | nom de l'AddOn |
| `VARIABLES_LOADED`, `ADDONS_LOADED` | — |
| `PLAYER_LOGIN`, `PLAYER_LOGOUT`, `PLAYER_LEAVING_WORLD` | — |
| `PLAYER_ENTERING_WORLD` | isInitialLogin, isReloadingUi |
| `CHAT_MSG_SAY`, `CHAT_MSG_WHISPER`, `CHAT_MSG_PARTY`, `CHAT_MSG_SYSTEM` | texte, auteur, langue, canal, auteur |
| `UNIT_HEALTH`, `UNIT_MAXHEALTH` | unité (`player`, `target`, `nameplateN`) |
| `UNIT_POWER_UPDATE` | unité, `"MANA"` |
| `UNIT_COMBAT` | unité, `"WOUND"`/`"HEAL"`, "", montant, 1 |
| `COMBAT_LOG_EVENT_UNFILTERED` | — (lire `CombatLogGetCurrentEventInfo()`) |
| `PLAYER_TARGET_CHANGED` | — |
| `NAME_PLATE_UNIT_ADDED`, `NAME_PLATE_UNIT_REMOVED` | jeton `nameplateN`, GUID |
| `PLAYER_STARTED_MOVING`, `PLAYER_STOPPED_MOVING` | — |
| `BAG_UPDATE`, `BAG_UPDATE_DELAYED`, `PLAYER_EQUIPMENT_CHANGED`, `UNIT_INVENTORY_CHANGED` | 0 / — / — / `player` |
| `CVAR_UPDATE` | nom, valeur |
| `ARCHRUST_KEY_DOWN` | nom de touche (`F5`, `SHIFT-A`, `ESCAPE`…) |

Un AddOn peut émettre ses propres événements avec `ArchRust.Fire("MON_EVENEMENT", ...)`, ou des
messages avec AceEvent (`SendMessage`).

## Bibliothèques Lua standard

| Bibliothèque | État |
|---|---|
| `string`, `table`, `math`, `utf8`, `coroutine` | complètes |
| `os` | `time`, `date`, `clock`, `difftime` seulement |
| `bit` (LuaBitOp, entiers 32 bits signés comme WoW) | `band bor bxor bnot lshift rshift arshift rol ror bswap tobit tohex mod` |
| `bit32` (Lua 5.2, non signé) | `band bor bxor bnot btest lshift rshift arshift lrotate rrotate extract replace` |
| `load`, `loadstring` | texte seulement (pas de bytecode) |
| `require` | modules `.lua` des dossiers d'AddOns : `require("libs.json")` → `<AddOn>/libs/json.lua`, `require("AutreAddOn.module")` ; `require("AceDB-3.0")` rend la bibliothèque LibStub |
| `debug` | `debug.traceback` seulement ; `debugstack()`, `debugprofilestop()` |
| `collectgarbage` | `"collect"`, `"count"`, `"step"`, `"isrunning"` |
| `io`, `os.execute/remove/rename/exit/getenv`, `dofile`, `loadfile`, `package.loadlib` | **absents** (bac à sable) |

Compatibilité Lua 5.1 / WoW : `unpack`, `getn`/`table.getn`, `table.setn`, `table.foreach(i)`,
`table.maxn`, `math.pow`, `math.mod`, `math.ldexp`, `math.frexp`, `math.log10`, `string.gfind`,
`getfenv`/`setfenv` (simplifiés), `newproxy` ; `string.format` accepte un nombre non entier pour
`%d`/`%x`/`%c` (tronqué, comme Lua 5.1) ; les nombres entiers rendus par l'API (`GetWidth`,
`GetValue`…) sont de vrais entiers, donc `"x" .. v` affiche `8` et non `8.0`.

Alias et utilitaires de WoW :

- chaînes : `strsplit(sep, s [, limite])`, `strsplittable`, `strjoin`, `strconcat`, `strtrim(s [, car])`,
  `strlower`, `strupper`, `strlen`, `strlenutf8`, `strsub`, `strfind`, `strmatch`, `strrep`,
  `strrev`, `strbyte`, `strchar`, `format`, `gsub`, `gmatch`, `tostringall`, `strcmputf8i`,
  `string.split/join/trim` ;
- tables : `tinsert`, `tremove`, `sort`, `wipe`/`table.wipe`, `tContains`, `tIndexOf`,
  `tDeleteItem`, `tInvert`, `tFilter`, `tAppendAll`, `tCount`, `CopyTable`, `MergeTable`,
  `GetOrCreateTableEntry` ;
- maths : `abs ceil floor max min sqrt exp log log10 fmod mod random`, `sin cos tan asin acos atan
  atan2` **en degrés** (comme WoW), `PI`, `Clamp`, `Saturate`, `Lerp`, `Round` ;
- dates : `date`, `time`, `difftime`, `GetServerTime`, `C_DateAndTime.GetCurrentCalendarTime` ;
- appels : `securecall`, `securecallfunction`, `securecallmethod`, `secureexecuterange`,
  `hooksecurefunc([table,] "nom", crochet)`, `issecure`, `geterrorhandler`, `seterrorhandler`,
  `InCombatLockdown` ;
- mixins et couleurs : `Mixin`, `CreateFromMixins`, `CreateAndInitFromMixin`, `ColorMixin`,
  `CreateColor`, `CreateColorFromHexString`, `WrapTextInColorCode`, `NORMAL_FONT_COLOR`,
  `HIGHLIGHT_FONT_COLOR`, `RED/GREEN/GRAY/YELLOW/ORANGE_FONT_COLOR`, `ITEM_QUALITY_COLORS`,
  `RAID_CLASS_COLORS` ;
- divers : `GenerateClosure`, `FormatLargeNumber`, `BreakUpLargeNumbers`, `AbbreviateNumbers`,
  `SecondsToTime`, `nop`.

## Bibliothèques d'AddOns intégrées

Le client fournit ces bibliothèques ; un AddOn les obtient par LibStub sans les livrer :

| Bibliothèque | Contenu |
|---|---|
| `LibStub` | registre versionné (`LibStub:NewLibrary`, `LibStub("Nom")`, `IterateLibraries`) |
| `CallbackHandler-1.0` | `CallbackHandler:New(cible, ...)`, `Fire`, `OnUsed`/`OnUnused` |
| `AceAddon-3.0` | `NewAddon`, `GetAddon`, modules (`NewModule`, `GetModule`…), `OnInitialize`/`OnEnable`/`OnDisable`, `Enable`/`Disable`, bibliothèques embarquées |
| `AceEvent-3.0` | `RegisterEvent`, `UnregisterEvent`, `RegisterMessage`, `SendMessage`… |
| `AceTimer-3.0` | `ScheduleTimer`, `ScheduleRepeatingTimer`, `CancelTimer`, `CancelAllTimers`, `TimeLeft` |
| `AceConsole-3.0` | `Print`, `Printf`, `RegisterChatCommand`, `UnregisterChatCommand`, `GetArgs` |
| `AceDB-3.0` | `AceDB:New(sv, défauts, profil)`, sections `profile global char realm class race faction factionrealm locale`, jokers `*`/`**`, profils (`SetProfile`, `CopyProfile`, `DeleteProfile`, `ResetProfile`, `GetProfiles`), espaces de noms, rappels `OnProfileChanged`… ; les valeurs par défaut ne sont pas sauvegardées |
| `AceHook-3.0` | `Hook`, `RawHook`, `SecureHook`, `HookScript`, `RawHookScript`, `SecureHookScript`, `Unhook`, `UnhookAll`, `IsHooked` |
| `AceLocale-3.0` | `NewLocale(app, locale, défaut)`, `GetLocale(app)` (langue du client : `frFR`) |
| `AceSerializer-3.0` | `Serialize(...)`, `Deserialize(chaîne)` |
| `LibDataBroker-1.1` | `NewDataObject`, `DataObjectIterator`, rappels `LibDataBroker_AttributeChanged_*` |
| `LibSharedMedia-3.0` | `Register`, `Fetch`, `List`, `HashTable`, `SetDefault`… (types `font statusbar background border sound`) |

Ce sont des implémentations propres à ArchRust qui suivent l'API publique des bibliothèques
d'origine, chargées à la première demande. Leur version mineure est **0** : si un AddOn livre une
copie d'origine dans son dossier `libs/` (chargée par son `.toc` ou son `embeds.xml`), c'est elle
que LibStub garde. AceGUI, AceConfig et AceComm ne sont pas fournis (pas de canal réseau d'AddOns).

Exemple :

```lua
local MonAddOn = LibStub("AceAddon-3.0"):NewAddon("MonAddOn", "AceConsole-3.0", "AceEvent-3.0", "AceTimer-3.0")

function MonAddOn:OnInitialize()
  self.db = LibStub("AceDB-3.0"):New("MonAddOnDB", { profile = { taille = 12 } }, true)
  self:RegisterChatCommand("monaddon", function(arg) self:Print("taille", self.db.profile.taille, arg) end)
end

function MonAddOn:OnEnable()
  self:RegisterEvent("UNIT_HEALTH")
  self:ScheduleRepeatingTimer(function() self:Print(format("%d ips", GetFramerate())) end, 30)
end

function MonAddOn:UNIT_HEALTH(event, unit)
  if unit == "target" then self:Print(UnitName("target"), UnitHealth("target")) end
end
```

## Frames et interface

`CreateFrame(type, nom, parent, modèles)` rend une **table Lua** (comme sous WoW) :

- on peut y ranger ses champs (`frame.texte = fs`), surcharger une méthode sur une frame, et
  `hooksecurefunc(frame, "Show", f)` fonctionne ;
- `getmetatable(frame).__index` (aussi `ArchRust.FrameMethods`) est la table des méthodes de
  **toutes** les frames : un AddOn peut y ajouter des méthodes ;
- la même table est rendue à chaque fois (`GetParent`, événements…) : utilisable comme clé.

Types : `Frame`, `Button`, `CheckButton`, `EditBox`, `StatusBar`, `Slider`, `ScrollFrame`,
`GameTooltip` ; régions : `FontString`, `Texture` (`CreateFontString`, `CreateTexture`).

Méthodes principales :

- identité et arbre : `GetName`, `GetObjectType`, `IsObjectType`, `GetParent`, `SetParent`,
  `GetChildren`, `GetRegions`, `GetNumChildren`, `SetID`/`GetID` ;
- visibilité : `Show`, `Hide`, `SetShown`, `IsShown`, `IsVisible` (avec `OnShow`/`OnHide`, propagés
  aux enfants) ;
- géométrie : `SetSize`, `SetWidth`, `SetHeight`, `GetWidth/Height/Size/Rect/Left/Right/Top/Bottom/Center`,
  `SetPoint(point, [relatif], [pointRelatif], x, y)` (plusieurs ancres, ex. `TOPLEFT` + `BOTTOMRIGHT`),
  `SetAllPoints`, `ClearAllPoints`, `ClearPoint`, `GetPoint(i)`, `GetNumPoints`, `SetScale`,
  `GetEffectiveScale`, `SetClampedToScreen` ;
- ordre : `SetFrameStrata` (`BACKGROUND` … `TOOLTIP`), `SetFrameLevel`, `Raise`, `Lower`,
  `SetDrawLayer` (`BACKGROUND`, `BORDER`, `ARTWORK`, `OVERLAY`, `HIGHLIGHT`) ; les strates jusqu'à
  `MEDIUM` passent sous les fenêtres natives du client, `HIGH` et plus au-dessus ;
- apparence : `SetAlpha`, `SetBackdrop{bgFile, edgeFile, edgeSize}`, `SetBackdropColor`,
  `SetBackdropBorderColor`, `SetColorTexture`, `SetVertexColor`, `SetTexture`, `SetTexCoord`
  (4 ou 8 valeurs), `SetAtlas` ;
- texte : `SetText`, `GetText`, `SetFormattedText`, `SetFont(chemin, taille, "OUTLINE")`,
  `SetFontObject`, `SetTextColor`, `SetJustifyH`, `SetJustifyV`, `SetWordWrap`, `GetStringWidth`
  (mesurée avec la vraie police) ; les codes `|cAARRGGBB…|r` sont affichés en couleur, `|n` = saut
  de ligne ;
- boutons et valeurs : `SetNormalTexture`, `Enable`, `Disable`, `Click`, `GetFontString`,
  `SetChecked`/`GetChecked`, `SetMinMaxValues`, `SetValue`, `GetValue`, `SetValueStep`,
  `SetOrientation`, `SetStatusBarTexture`, `SetStatusBarColor` ;
- saisie (`EditBox`) : `SetText`, `GetText`, `SetFocus`, `ClearFocus`, `HasFocus`, `SetMultiLine`,
  `SetNumeric`, `GetNumber`, `SetPassword`, `SetMaxLetters`, `Insert` — c'est un vrai champ de
  saisie ;
- souris et clavier : `EnableMouse`, `EnableMouseWheel`, `EnableKeyboard`, `SetMovable`,
  `RegisterForDrag`, `StartMoving`, `StopMovingOrSizing`, `IsMouseOver` ;
- scripts : `SetScript`, `GetScript`, `HookScript` ; événements : `RegisterEvent`,
  `RegisterUnitEvent`, `RegisterAllEvents`, `UnregisterEvent`, `UnregisterAllEvents`.

Scripts appelés : `OnEvent`, `OnUpdate(elapsed)`, `OnShow`, `OnHide`, `OnClick(button, down)`,
`PreClick`, `PostClick`, `OnEnter`, `OnLeave`, `OnMouseDown`, `OnMouseUp`, `OnMouseWheel(delta)`,
`OnDragStart`, `OnDragStop`, `OnValueChanged(value, userInput)`, `OnTextChanged(userInput)`,
`OnEnterPressed`, `OnEscapePressed`, `OnTabPressed`, `OnEditFocusGained`, `OnEditFocusLost`,
`OnKeyDown`, `OnKeyUp`.

**Textures et polices.** `SetTexture` / `SetFont` / `bgFile` acceptent :

- `Interface\AddOns\<AddOn>\media\fichier` (PNG, TGA, JPG, BMP, DDS ; extension facultative) ;
- un chemin relatif à l'AddOn pendant son chargement (`media\barre.png`) ;
- une texture du client par son nom RenderWare (`A00005AA`) ou son chemin (`texture/ui/…`) ;
- les textures de WoW (`Interface\Buttons\WHITE8X8`, `Interface\TargetingFrame\UI-StatusBar`…)
  deviennent une couleur unie ; `Fonts\FRIZQT__.TTF` = police du client ;
- les polices `.ttf`/`.otf` des AddOns sont chargées par le client (actives à l'image suivante).

**Objets fournis.** `UIParent`, `WorldFrame`, `GameTooltip` (`SetOwner`, `AddLine`,
`AddDoubleLine`, `SetText`, `SetUnit`, `ClearLines`, ancres `ANCHOR_*` dont `ANCHOR_CURSOR`),
`DEFAULT_CHAT_FRAME`/`ChatFrame1` (`AddMessage(texte, r, g, b)`), objets police (`CreateFont`,
`GameFontNormal`, `GameFontHighlight`, `GameFontNormalLarge`, `ChatFontNormal`,
`NumberFontNormal`…), `StaticPopupDialogs` + `StaticPopup_Show`/`StaticPopup_Hide`,
panneau d'options (`Settings.RegisterCanvasLayoutCategory(panel, nom)`,
`InterfaceOptions_AddCategory`, `Settings.OpenToCategory`, `/options`), `UISpecialFrames`
(fermées par Échap), `CloseSpecialWindows`.

**Modèles** (`CreateFrame(type, nom, parent, "Modèle")`) : `BackdropTemplate`,
`UIPanelButtonTemplate`, `UIPanelCloseButton`, `UICheckButtonTemplate` (avec `.Text`),
`OptionsSliderTemplate` (`.Text`, `.Low`, `.High`), `InputBoxTemplate`, `BasicFrameTemplate`
(`.TitleText`, `.CloseButton`, `:SetTitle`), `TooltipBackdropTemplate`, `GameTooltipTemplate`…
Un AddOn peut en déclarer : `ArchRust.Templates.MonModele = function(frame) … end`.

**Frames natives pilotables.** Les panneaux du client sont des frames : `ChatFrame1:Hide()`
masque la discussion, `ArchRustInfoFrame:Hide()` le panneau d'informations,
`ArchRustInventoryButton:Hide()` le bouton d'inventaire.

## API du jeu

Jetons d'unité : `player`, `target`, `nameplate1`…`nameplateN` (unités en vue), ou un GUID
(`Player-<cid>`, `Unit-<tid>-<cid>`).

| Domaine | Fonctions |
|---|---|
| Unités | `UnitExists`, `UnitName`, `GetUnitName`, `UnitGUID`, `UnitIsPlayer`, `UnitIsUnit`, `UnitIsDead`, `UnitHealth`, `UnitHealthMax` (plus haut observé si le serveur ne le donne pas), `UnitPower`, `UnitPowerMax`, `UnitPowerType`, `UnitLevel`, `UnitClass`, `UnitRace`, `UnitFactionGroup`, `UnitPosition` (x, y, z ArchLord), `UnitDistanceSquared`, `CheckInteractDistance`, `UnitCanAttack`, `UnitIsFriend`, `UnitReaction`, `GetPlayerFacing`, `IsPlayerMoving`, `GetPlayerMapPosition` |
| Cible (côté client) | `TargetUnit(unité ou nom)`, `TargetNearest`, `ClearTarget` |
| Discussion | `print`, `SendChatMessage(texte, type)` (les messages tapés passent aussi par elle : accrochable), `DEFAULT_CHAT_FRAME:AddMessage`, `ChatFrame_AddMessageEventFilter(event, filtre)` (masquer ou réécrire les messages reçus), `RunMacroText("/goto …")` |
| Inventaire | `GetItemInfo(tid ou nom)`, `GetItemCount`, `C_Container.GetContainerNumSlots`, `C_Container.GetContainerItemInfo`, `ArchRust.GetInventory()` |
| Combat | `CombatLogGetCurrentEventInfo()` → horodatage, `"SWING_DAMAGE"`/`"SPELL_HEAL"`, false, GUID source, nom source, 0, 0, GUID cible, nom cible, 0, 0, montant, PV restants |
| Temps | `GetTime`, `GetTimePreciseSec`, `debugprofilestop`, `GetFramerate`, `C_Timer.After`, `C_Timer.NewTimer`, `C_Timer.NewTicker(période, f, n)` (`:Cancel()`, `:IsCancelled()`) |
| Client | `GetRealmName`, `GetLocale` (`frFR`), `GetBuildInfo`, `IsLoggedIn`, `GetScreenWidth/Height`, `GetCursorPosition`, `GetMouseFocus`, `IsShiftKeyDown`, `IsControlKeyDown`, `IsAltKeyDown`, `ReloadUI`, `Screenshot([fichier])` (dans `saved-variables/Screenshots`), `Quit`, `PlaySound`/`PlaySoundFile(chemin)` (`sound/ui/block.wav`, `sd0000.wav` d'un pack `.pk`, `Interface\AddOns\<AddOn>\son.wav` ou chemin relatif à l'AddOn ; un numéro de kit WoW est ignoré), `PlayMusic(chemin)` (MP3 en boucle, fondu, remplace la musique de zone), `StopMusic()` (retour à la musique de zone) |
| AddOns | `GetNumAddOns`, `GetAddOnInfo`, `GetAddOnMetadata(nom, champ)` (dont `X-…`), `IsAddOnLoaded`, `IsAddOnLoadOnDemand`, `LoadAddOn`, `C_AddOns.*` |
| CVars | `GetCVar`, `SetCVar`, `GetCVarBool`, `GetCVarDefault`, `RegisterCVar`, `C_CVar.*` |
| Commandes | `SLASH_<CLÉ>1 = "/cmd"` + `SlashCmdList.<CLÉ> = function(msg, editBox) … end` |

## Espace `ArchRust` : modifier le client

| Fonction | Effet |
|---|---|
| `ArchRust.GetUnits()` | liste des unités en vue : `{guid, token, cid, tid, name, level, hp, hpMax, x, y, z, facing, moving}` |
| `ArchRust.GetUnit(unité)` / `GetNumUnits()` | une unité (même forme) / leur nombre |
| `ArchRust.GetInventory()` | objets : `{id, tid, name, count, where = "bag"/"equipped"/"other", tab, row, col}` |
| `ArchRust.GetCamera()` | distance, lacet°, tangage°, champ de vision° |
| `ArchRust.GetCameraPosition()` | œil x, y, z, cible x, y, z |
| `ArchRust.SetCamera(distance, lacet°, tangage°)` | place la caméra (`nil` = inchangé) |
| `ArchRust.WorldToScreen(x, y, z)` | position à l'écran (repère de l'interface) ou `nil` — pour des plaques de nom, des marqueurs… |
| `ArchRust.MoveTo(x, y, z)` | fait marcher le personnage |
| `ArchRust.RunCommand("/cmd …")` | exécute des commandes du client ou d'AddOns |
| `ArchRust.SetUnitTint(unité, r, g, b [, a])` / `ClearUnitTint([unité])` | teinte le modèle 3D d'une unité |
| `ArchRust.SetUIColor(clé, r, g, b [, a])` / `GetUIColor` / `ResetUIColors()` | couleurs des fenêtres natives : `windowFill`, `panelFill`, `windowStroke`, `text`, `accent`, `hyperlink`, `widgetFill`, `widgetHover`, `extremeBg`, `faintBg` |
| `ArchRust.SetBinding(touche, fonction ou "/commande")` / `GetBinding` | raccourci clavier (`F5`, `CTRL-T`, `ALT-SHIFT-1`…) ; `nil` l'efface |
| `ArchRust.Fire(événement, ...)` | émet un événement vers toutes les frames |
| `ArchRust.OpenOptions([panneau])` | ouvre le panneau d'options des AddOns |
| `ArchRust.Templates`, `ArchRust.RegisterTemplate` | modèles de frames |
| `ArchRust.FrameMethods` | méthodes partagées de toutes les frames |
| `ArchRust.FilterChat` | fonction appelée pour chaque message reçu (remplaçable) |
| `ArchRust.GetErrors()` / `ReportError(texte)` | erreurs Lua récentes / en signaler une |
| `ArchRust.IsInWorld()`, `GetLoadedSectors()`, `GetFrameRect(frame)` | état du client |
| `ArchRust.Dump(valeur [, profondeur])` | texte lisible d'une table |
| `ArchRust.BuiltinLibraries` | chargeurs des bibliothèques intégrées |

Les erreurs Lua s'affichent une fois en rouge dans la discussion et dans le journal ; `/errors`
les liste.

## CVars

`SetCVar(nom, valeur)` est appliqué immédiatement par le moteur ; `CVAR_UPDATE` est émis.

| CVar | Défaut | Effet |
|---|---|---|
| `skyColor` | `0.35 0.45 0.6` | couleur du ciel (fond de la scène) |
| `cameraFov` | `55` | champ de vision vertical (degrés) |
| `cameraFarClip` | `200000` | distance maximale de rendu |
| `objectDrawDistance` | `25000` | distance de tracé des objets du monde |
| `lightDirection` | `0.4 0.8 0.3` | direction du soleil |
| `ambientColor` | `0.45 0.45 0.5` | lumière ambiante |
| `uiScale` | `1` | échelle de toute l'interface (0,5 à 3) |
| `nameplateDistance` | `6000` | distance indicative pour les plaques de nom (lue par les AddOns) |
| `loginMusic` | `sound/bgm/htt_001.mp3` | musique des écrans de connexion, de sélection et de création (vide = silence) |
| `worldMusic` | `sound/bgm/hft_001.mp3` | musique en jeu quand la région du joueur n'en indique pas (`RegionTemplate.ini` → `skyset.ini`) |
| `worldAmbience` | `sound/stream/a_cid_d.mp3` | ambiance en jeu (vide = aucune) |

Un AddOn peut aussi créer ses propres CVars (`RegisterCVar`).

## Commandes slash fournies

| Commande | Effet |
|---|---|
| `/run code`, `/script code` | exécute du Lua |
| `/dump expression` | affiche une valeur (tables comprises) |
| `/target [nom]`, `/tar`, `/cleartarget` | cible une unité (la plus proche sans nom) |
| `/options [panneau]` | panneau d'options des AddOns |
| `/addons` | liste des AddOns et leur état |
| `/errors` | erreurs Lua récentes |
| `/loadaddon Nom` | charge un AddOn `LoadOnDemand` |
| `/reload`, `/rl` | recharge l'interface (sauvegarde puis rechargement) |

Commandes du client : `/goto x y z`, `/cam distance [lacet°] [tangage°]`, `/music <chemin>|off|auto`
(musique forcée, coupée ou automatique), `/sound <chemin>` (joue un effet), `/quit`.
La console Lua (touche `` ` ``) exécute aussi du code dans l'état des AddOns.

## Différences avec WoW

- Lua 5.4 et non 5.1 : entiers et flottants distincts (`math.type`), `goto`, opérateurs binaires
  natifs ; `setfenv`/`getfenv` sont simplifiés ; pas de bytecode.
- Pas d'environnement « sécurisé » ni de combat lockdown : tout est modifiable.
- Pas de frames XML (seuls `<Script>`/`<Include>` sont lus) ; les animations
  (`CreateAnimationGroup`) sont des objets factices.
- Classe, race, niveau, mana et PV max ne sont pas encore décodés depuis le serveur : valeurs
  neutres (`ADVENTURER`, niveau 1, PV max = plus haut observé).
- Seul le canal `SAY` existe côté serveur : les autres types de `SendChatMessage` partent en `SAY`.
- `PlaySound(numéro)` avec un identifiant de kit sonore WoW ne joue rien (pas de table de kits) ;
  passer un chemin. `StopSound` et le canal (`Master`, `SFX`…) sont ignorés : les volumes sont ceux
  de la fenêtre Options du client.

## Tests

`cargo test -p archrust-addons` charge du Lua : bibliothèques standard et alias, `require` borné,
cycle de vie Ace3 complet (AceDB, profils, minuteurs, crochets, locales), sérialiseur, LibDataBroker,
LibSharedMedia, `embeds.xml`, souris (clic, glisser, curseur), événements du monde, et une session
avec les AddOns d'exemple du dépôt (`tests/sample_addons.rs`).
