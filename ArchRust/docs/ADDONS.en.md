# ArchRust Lua AddOns

*[Version française](ADDONS.md)*

ArchRust loads Lua AddOns the World of Warcraft way: same `.toc` files, same function names, same
libraries (LibStub, Ace3…). The goal is to let an AddOn change as much of the client as possible —
UI, chat, camera, rendering, key bindings — and to let WoW code and habits carry over.

- Lua engine: **Lua 5.4** (mlua) with a Lua 5.1 / WoW compatibility layer.
- Code: `crates/archrust-addons` (API, frames, libraries) and
  `crates/archrust-engine/src/addon_ui.rs`, `app_addons.rs` (drawing and game glue).
- Samples: `addons/HelloArch` (core API), `addons/ArchAce` (LibStub + Ace3 + extended API).

## Contents

1. [AddOn layout](#addon-layout)
2. [Life cycle and events](#life-cycle-and-events)
3. [Standard Lua libraries](#standard-lua-libraries)
4. [Built-in AddOn libraries](#built-in-addon-libraries)
5. [Frames and UI](#frames-and-ui)
6. [Game API](#game-api)
7. [The `ArchRust` namespace: changing the client](#the-archrust-namespace-changing-the-client)
8. [CVars](#cvars)
9. [Provided slash commands](#provided-slash-commands)
10. [Differences from WoW](#differences-from-wow)

## AddOn layout

```text
addons/MyAddOn/
  MyAddOn.toc
  embeds.xml          (optional: libraries shipped with the AddOn)
  libs/…              (optional)
  core.lua
  media/bar.png       (textures, .ttf/.otf fonts)
```

```text
## Title: My AddOn
## Version: 1.0
## Author: Me
## Interface: 10000
## Notes: What the AddOn does
## SavedVariables: MyAddOnDB
## SavedVariablesPerCharacter: MyAddOnCharDB
## Dependencies: OtherAddOn
## OptionalDeps: Ace3
## LoadOnDemand: 0
## X-Website: https://…

embeds.xml
core.lua
```

- `.lua` lines run in order; every file receives `local addonName, ns = ...` (`ns` is a private
  table shared by the AddOn's files).
- `.xml` lines (e.g. `embeds.xml`, `libs\AceAddon-3.0\AceAddon-3.0.xml`) are read for their
  `<Script file="…"/>` and `<Include file="…"/>` tags, recursively. Frames declared in XML are not
  interpreted: create them in Lua.
- `## Dependencies` loads required AddOns first (error if missing), `## OptionalDeps` loads the
  existing ones first.
- `## LoadOnDemand: 1`: loaded only by `LoadAddOn("Name")` (or `/loadaddon Name`).
- SavedVariables are written to `saved-variables/<AddOn>.lua` (account) and
  `saved-variables/<Realm>/<Character>/<AddOn>.lua` (per character), and read back before the
  AddOn's first Lua file.

## Life cycle and events

As in WoW, AddOns load **when entering the world** (the character is known by then):

1. for each AddOn: SavedVariables, files, then `ADDON_LOADED(addonName)`;
2. `VARIABLES_LOADED`, `ADDONS_LOADED`;
3. `PLAYER_LOGIN`, then `PLAYER_ENTERING_WORLD(isInitialLogin, isReloadingUi)`;
4. on disconnect, `/reload` and exit: `PLAYER_LEAVING_WORLD`, `PLAYER_LOGOUT`, then
   SavedVariables are written. `/reload` then reloads every AddOn.

AddOn UI only shows in game (not on the login screen).

Events fired by the client:

| Event | Arguments |
|---|---|
| `ADDON_LOADED` | AddOn name |
| `VARIABLES_LOADED`, `ADDONS_LOADED` | — |
| `PLAYER_LOGIN`, `PLAYER_LOGOUT`, `PLAYER_LEAVING_WORLD` | — |
| `PLAYER_ENTERING_WORLD` | isInitialLogin, isReloadingUi |
| `CHAT_MSG_SAY`, `CHAT_MSG_WHISPER`, `CHAT_MSG_PARTY`, `CHAT_MSG_SYSTEM` | text, author, language, channel, author |
| `UNIT_HEALTH`, `UNIT_MAXHEALTH` | unit (`player`, `target`, `nameplateN`) |
| `UNIT_POWER_UPDATE` | unit, `"MANA"` |
| `UNIT_COMBAT` | unit, `"WOUND"`/`"HEAL"`, "", amount, 1 |
| `COMBAT_LOG_EVENT_UNFILTERED` | — (read `CombatLogGetCurrentEventInfo()`) |
| `PLAYER_TARGET_CHANGED` | — |
| `NAME_PLATE_UNIT_ADDED`, `NAME_PLATE_UNIT_REMOVED` | `nameplateN` token, GUID |
| `PLAYER_STARTED_MOVING`, `PLAYER_STOPPED_MOVING` | — |
| `BAG_UPDATE`, `BAG_UPDATE_DELAYED`, `PLAYER_EQUIPMENT_CHANGED`, `UNIT_INVENTORY_CHANGED` | 0 / — / — / `player` |
| `CVAR_UPDATE` | name, value |
| `ARCHRUST_KEY_DOWN` | key name (`F5`, `SHIFT-A`, `ESCAPE`…) |

AddOns can fire their own events with `ArchRust.Fire("MY_EVENT", ...)`, or messages with AceEvent
(`SendMessage`).

## Standard Lua libraries

| Library | Status |
|---|---|
| `string`, `table`, `math`, `utf8`, `coroutine` | complete |
| `os` | `time`, `date`, `clock`, `difftime` only |
| `bit` (LuaBitOp, signed 32-bit like WoW) | `band bor bxor bnot lshift rshift arshift rol ror bswap tobit tohex mod` |
| `bit32` (Lua 5.2, unsigned) | `band bor bxor bnot btest lshift rshift arshift lrotate rrotate extract replace` |
| `load`, `loadstring` | text only (no bytecode) |
| `require` | `.lua` modules from AddOn folders: `require("libs.json")` → `<AddOn>/libs/json.lua`, `require("OtherAddOn.module")`; `require("AceDB-3.0")` returns the LibStub library |
| `debug` | `debug.traceback` only; `debugstack()`, `debugprofilestop()` |
| `collectgarbage` | `"collect"`, `"count"`, `"step"`, `"isrunning"` |
| `io`, `os.execute/remove/rename/exit/getenv`, `dofile`, `loadfile`, `package.loadlib` | **absent** (sandbox) |

Lua 5.1 / WoW compatibility: `unpack`, `getn`/`table.getn`, `table.setn`, `table.foreach(i)`,
`table.maxn`, `math.pow`, `math.mod`, `math.ldexp`, `math.frexp`, `math.log10`, `string.gfind`,
`getfenv`/`setfenv` (simplified), `newproxy`; `string.format` accepts non-integer numbers for
`%d`/`%x`/`%c` (truncated, like Lua 5.1); integral numbers returned by the API (`GetWidth`,
`GetValue`…) are real integers, so `"x" .. v` prints `8`, not `8.0`.

WoW aliases and helpers:

- strings: `strsplit(sep, s [, limit])`, `strsplittable`, `strjoin`, `strconcat`, `strtrim(s [, chars])`,
  `strlower`, `strupper`, `strlen`, `strlenutf8`, `strsub`, `strfind`, `strmatch`, `strrep`,
  `strrev`, `strbyte`, `strchar`, `format`, `gsub`, `gmatch`, `tostringall`, `strcmputf8i`,
  `string.split/join/trim`;
- tables: `tinsert`, `tremove`, `sort`, `wipe`/`table.wipe`, `tContains`, `tIndexOf`,
  `tDeleteItem`, `tInvert`, `tFilter`, `tAppendAll`, `tCount`, `CopyTable`, `MergeTable`,
  `GetOrCreateTableEntry`;
- math: `abs ceil floor max min sqrt exp log log10 fmod mod random`, `sin cos tan asin acos atan
  atan2` **in degrees** (like WoW), `PI`, `Clamp`, `Saturate`, `Lerp`, `Round`;
- dates: `date`, `time`, `difftime`, `GetServerTime`, `C_DateAndTime.GetCurrentCalendarTime`;
- calls: `securecall`, `securecallfunction`, `securecallmethod`, `secureexecuterange`,
  `hooksecurefunc([table,] "name", hook)`, `issecure`, `geterrorhandler`, `seterrorhandler`,
  `InCombatLockdown`;
- mixins and colours: `Mixin`, `CreateFromMixins`, `CreateAndInitFromMixin`, `ColorMixin`,
  `CreateColor`, `CreateColorFromHexString`, `WrapTextInColorCode`, `NORMAL_FONT_COLOR`,
  `HIGHLIGHT_FONT_COLOR`, `RED/GREEN/GRAY/YELLOW/ORANGE_FONT_COLOR`, `ITEM_QUALITY_COLORS`,
  `RAID_CLASS_COLORS`;
- misc: `GenerateClosure`, `FormatLargeNumber`, `BreakUpLargeNumbers`, `AbbreviateNumbers`,
  `SecondsToTime`, `nop`.

## Built-in AddOn libraries

The client provides these libraries; an AddOn gets them through LibStub without shipping them:

| Library | Contents |
|---|---|
| `LibStub` | versioned registry (`LibStub:NewLibrary`, `LibStub("Name")`, `IterateLibraries`) |
| `CallbackHandler-1.0` | `CallbackHandler:New(target, ...)`, `Fire`, `OnUsed`/`OnUnused` |
| `AceAddon-3.0` | `NewAddon`, `GetAddon`, modules (`NewModule`, `GetModule`…), `OnInitialize`/`OnEnable`/`OnDisable`, `Enable`/`Disable`, embedded libraries |
| `AceEvent-3.0` | `RegisterEvent`, `UnregisterEvent`, `RegisterMessage`, `SendMessage`… |
| `AceTimer-3.0` | `ScheduleTimer`, `ScheduleRepeatingTimer`, `CancelTimer`, `CancelAllTimers`, `TimeLeft` |
| `AceConsole-3.0` | `Print`, `Printf`, `RegisterChatCommand`, `UnregisterChatCommand`, `GetArgs` |
| `AceDB-3.0` | `AceDB:New(sv, defaults, profile)`, sections `profile global char realm class race faction factionrealm locale`, `*`/`**` wildcards, profiles (`SetProfile`, `CopyProfile`, `DeleteProfile`, `ResetProfile`, `GetProfiles`), namespaces, `OnProfileChanged`… callbacks; defaults are not saved |
| `AceHook-3.0` | `Hook`, `RawHook`, `SecureHook`, `HookScript`, `RawHookScript`, `SecureHookScript`, `Unhook`, `UnhookAll`, `IsHooked` |
| `AceLocale-3.0` | `NewLocale(app, locale, isDefault)`, `GetLocale(app)` (client locale: `frFR`) |
| `AceSerializer-3.0` | `Serialize(...)`, `Deserialize(string)` |
| `LibDataBroker-1.1` | `NewDataObject`, `DataObjectIterator`, `LibDataBroker_AttributeChanged_*` callbacks |
| `LibSharedMedia-3.0` | `Register`, `Fetch`, `List`, `HashTable`, `SetDefault`… (types `font statusbar background border sound`) |

They are ArchRust's own implementations following the public API of the original libraries,
loaded on first use. Their minor version is **0**: if an AddOn ships an original copy in its `libs/`
folder (loaded by its `.toc` or `embeds.xml`), LibStub keeps that one. AceGUI, AceConfig and
AceComm are not provided (no AddOn network channel).

Example:

```lua
local MyAddOn = LibStub("AceAddon-3.0"):NewAddon("MyAddOn", "AceConsole-3.0", "AceEvent-3.0", "AceTimer-3.0")

function MyAddOn:OnInitialize()
  self.db = LibStub("AceDB-3.0"):New("MyAddOnDB", { profile = { size = 12 } }, true)
  self:RegisterChatCommand("myaddon", function(arg) self:Print("size", self.db.profile.size, arg) end)
end

function MyAddOn:OnEnable()
  self:RegisterEvent("UNIT_HEALTH")
  self:ScheduleRepeatingTimer(function() self:Print(format("%d fps", GetFramerate())) end, 30)
end

function MyAddOn:UNIT_HEALTH(event, unit)
  if unit == "target" then self:Print(UnitName("target"), UnitHealth("target")) end
end
```

## Frames and UI

`CreateFrame(type, name, parent, templates)` returns a **Lua table** (as in WoW):

- you can store your own fields on it (`frame.label = fs`), override a method on one frame, and
  `hooksecurefunc(frame, "Show", f)` works;
- `getmetatable(frame).__index` (also `ArchRust.FrameMethods`) is the method table of **all**
  frames: AddOns can add methods to it;
- the same table is always returned (`GetParent`, events…): usable as a table key.

Types: `Frame`, `Button`, `CheckButton`, `EditBox`, `StatusBar`, `Slider`, `ScrollFrame`,
`GameTooltip`; regions: `FontString`, `Texture` (`CreateFontString`, `CreateTexture`).

Main methods:

- identity and tree: `GetName`, `GetObjectType`, `IsObjectType`, `GetParent`, `SetParent`,
  `GetChildren`, `GetRegions`, `GetNumChildren`, `SetID`/`GetID`;
- visibility: `Show`, `Hide`, `SetShown`, `IsShown`, `IsVisible` (with `OnShow`/`OnHide`, also on
  children);
- geometry: `SetSize`, `SetWidth`, `SetHeight`, `GetWidth/Height/Size/Rect/Left/Right/Top/Bottom/Center`,
  `SetPoint(point, [relativeTo], [relativePoint], x, y)` (several anchors, e.g. `TOPLEFT` +
  `BOTTOMRIGHT`), `SetAllPoints`, `ClearAllPoints`, `ClearPoint`, `GetPoint(i)`, `GetNumPoints`,
  `SetScale`, `GetEffectiveScale`, `SetClampedToScreen`;
- ordering: `SetFrameStrata` (`BACKGROUND` … `TOOLTIP`), `SetFrameLevel`, `Raise`, `Lower`,
  `SetDrawLayer` (`BACKGROUND`, `BORDER`, `ARTWORK`, `OVERLAY`, `HIGHLIGHT`); strata up to
  `MEDIUM` draw below the client's native windows, `HIGH` and above on top;
- look: `SetAlpha`, `SetBackdrop{bgFile, edgeFile, edgeSize}`, `SetBackdropColor`,
  `SetBackdropBorderColor`, `SetColorTexture`, `SetVertexColor`, `SetTexture`, `SetTexCoord`
  (4 or 8 values), `SetAtlas`;
- text: `SetText`, `GetText`, `SetFormattedText`, `SetFont(path, size, "OUTLINE")`,
  `SetFontObject`, `SetTextColor`, `SetJustifyH`, `SetJustifyV`, `SetWordWrap`, `GetStringWidth`
  (measured with the real font); `|cAARRGGBB…|r` codes render in colour, `|n` = new line;
- buttons and values: `SetNormalTexture`, `Enable`, `Disable`, `Click`, `GetFontString`,
  `SetChecked`/`GetChecked`, `SetMinMaxValues`, `SetValue`, `GetValue`, `SetValueStep`,
  `SetOrientation`, `SetStatusBarTexture`, `SetStatusBarColor`;
- input (`EditBox`): `SetText`, `GetText`, `SetFocus`, `ClearFocus`, `HasFocus`, `SetMultiLine`,
  `SetNumeric`, `GetNumber`, `SetPassword`, `SetMaxLetters`, `Insert` — it is a real text field;
- mouse and keyboard: `EnableMouse`, `EnableMouseWheel`, `EnableKeyboard`, `SetMovable`,
  `RegisterForDrag`, `StartMoving`, `StopMovingOrSizing`, `IsMouseOver`;
- scripts: `SetScript`, `GetScript`, `HookScript`; events: `RegisterEvent`,
  `RegisterUnitEvent`, `RegisterAllEvents`, `UnregisterEvent`, `UnregisterAllEvents`.

Script handlers: `OnEvent`, `OnUpdate(elapsed)`, `OnShow`, `OnHide`, `OnClick(button, down)`,
`PreClick`, `PostClick`, `OnEnter`, `OnLeave`, `OnMouseDown`, `OnMouseUp`, `OnMouseWheel(delta)`,
`OnDragStart`, `OnDragStop`, `OnValueChanged(value, userInput)`, `OnTextChanged(userInput)`,
`OnEnterPressed`, `OnEscapePressed`, `OnTabPressed`, `OnEditFocusGained`, `OnEditFocusLost`,
`OnKeyDown`, `OnKeyUp`.

**Textures and fonts.** `SetTexture` / `SetFont` / `bgFile` accept:

- `Interface\AddOns\<AddOn>\media\file` (PNG, TGA, JPG, BMP, DDS; extension optional);
- a path relative to the AddOn while it loads (`media\bar.png`);
- a client texture by RenderWare name (`A00005AA`) or path (`texture/ui/…`);
- WoW textures (`Interface\Buttons\WHITE8X8`, `Interface\TargetingFrame\UI-StatusBar`…) become a
  solid colour; `Fonts\FRIZQT__.TTF` = the client font;
- AddOn `.ttf`/`.otf` fonts are loaded by the client (active from the next frame).

**Provided objects.** `UIParent`, `WorldFrame`, `GameTooltip` (`SetOwner`, `AddLine`,
`AddDoubleLine`, `SetText`, `SetUnit`, `ClearLines`, `ANCHOR_*` anchors including
`ANCHOR_CURSOR`), `DEFAULT_CHAT_FRAME`/`ChatFrame1` (`AddMessage(text, r, g, b)`), font objects
(`CreateFont`, `GameFontNormal`, `GameFontHighlight`, `GameFontNormalLarge`, `ChatFontNormal`,
`NumberFontNormal`…), `StaticPopupDialogs` + `StaticPopup_Show`/`StaticPopup_Hide`, an options
panel (`Settings.RegisterCanvasLayoutCategory(panel, name)`, `InterfaceOptions_AddCategory`,
`Settings.OpenToCategory`, `/options`), `UISpecialFrames` (closed by Escape),
`CloseSpecialWindows`.

**Templates** (`CreateFrame(type, name, parent, "Template")`): `BackdropTemplate`,
`UIPanelButtonTemplate`, `UIPanelCloseButton`, `UICheckButtonTemplate` (with `.Text`),
`OptionsSliderTemplate` (`.Text`, `.Low`, `.High`), `InputBoxTemplate`, `BasicFrameTemplate`
(`.TitleText`, `.CloseButton`, `:SetTitle`), `TooltipBackdropTemplate`, `GameTooltipTemplate`…
AddOns can declare more: `ArchRust.Templates.MyTemplate = function(frame) … end`.

**Native frames you can drive.** The client's panels are frames: `ChatFrame1:Hide()` hides the
chat, `ArchRustInfoFrame:Hide()` the info panel, `ArchRustInventoryButton:Hide()` the inventory
button.

## Game API

Unit tokens: `player`, `target`, `nameplate1`…`nameplateN` (units in view), or a GUID
(`Player-<cid>`, `Unit-<tid>-<cid>`).

| Area | Functions |
|---|---|
| Units | `UnitExists`, `UnitName`, `GetUnitName`, `UnitGUID`, `UnitIsPlayer`, `UnitIsUnit`, `UnitIsDead`, `UnitHealth`, `UnitHealthMax` (highest seen when the server does not send it), `UnitPower`, `UnitPowerMax`, `UnitPowerType`, `UnitLevel`, `UnitClass`, `UnitRace`, `UnitFactionGroup`, `UnitPosition` (ArchLord x, y, z), `UnitDistanceSquared`, `CheckInteractDistance`, `UnitCanAttack`, `UnitIsFriend`, `UnitReaction`, `GetPlayerFacing`, `IsPlayerMoving`, `GetPlayerMapPosition` |
| Target (client side) | `TargetUnit(unit or name)`, `TargetNearest`, `ClearTarget` |
| Chat | `print`, `SendChatMessage(text, type)` (typed messages also go through it: hookable), `DEFAULT_CHAT_FRAME:AddMessage`, `ChatFrame_AddMessageEventFilter(event, filter)` (hide or rewrite incoming messages), `RunMacroText("/goto …")` |
| Inventory | `GetItemInfo(tid or name)`, `GetItemCount`, `C_Container.GetContainerNumSlots`, `C_Container.GetContainerItemInfo`, `ArchRust.GetInventory()` |
| Combat | `CombatLogGetCurrentEventInfo()` → timestamp, `"SWING_DAMAGE"`/`"SPELL_HEAL"`, false, source GUID, source name, 0, 0, dest GUID, dest name, 0, 0, amount, remaining HP |
| Time | `GetTime`, `GetTimePreciseSec`, `debugprofilestop`, `GetFramerate`, `C_Timer.After`, `C_Timer.NewTimer`, `C_Timer.NewTicker(period, f, n)` (`:Cancel()`, `:IsCancelled()`) |
| Client | `GetRealmName`, `GetLocale` (`frFR`), `GetBuildInfo`, `IsLoggedIn`, `GetScreenWidth/Height`, `GetCursorPosition`, `GetMouseFocus`, `IsShiftKeyDown`, `IsControlKeyDown`, `IsAltKeyDown`, `ReloadUI`, `Screenshot([file])` (in `saved-variables/Screenshots`), `Quit`, `PlaySound`/`PlaySoundFile(path)` (`sound/ui/block.wav`, `sd0000.wav` from a `.pk` pack, `Interface\AddOns\<AddOn>\sound.wav` or a path relative to the AddOn; a WoW sound-kit number is ignored), `PlayMusic(path)` (looped MP3, crossfade, replaces the zone music), `StopMusic()` (back to the zone music) |
| AddOns | `GetNumAddOns`, `GetAddOnInfo`, `GetAddOnMetadata(name, field)` (including `X-…`), `IsAddOnLoaded`, `IsAddOnLoadOnDemand`, `LoadAddOn`, `C_AddOns.*` |
| CVars | `GetCVar`, `SetCVar`, `GetCVarBool`, `GetCVarDefault`, `RegisterCVar`, `C_CVar.*` |
| Commands | `SLASH_<KEY>1 = "/cmd"` + `SlashCmdList.<KEY> = function(msg, editBox) … end` |

## The `ArchRust` namespace: changing the client

| Function | Effect |
|---|---|
| `ArchRust.GetUnits()` | units in view: `{guid, token, cid, tid, name, level, hp, hpMax, x, y, z, facing, moving}` |
| `ArchRust.GetUnit(unit)` / `GetNumUnits()` | one unit (same shape) / how many |
| `ArchRust.GetInventory()` | items: `{id, tid, name, count, where = "bag"/"equipped"/"other", tab, row, col}` |
| `ArchRust.GetCamera()` | distance, yaw°, pitch°, field of view° |
| `ArchRust.GetCameraPosition()` | eye x, y, z, target x, y, z |
| `ArchRust.SetCamera(distance, yaw°, pitch°)` | moves the camera (`nil` = unchanged) |
| `ArchRust.WorldToScreen(x, y, z)` | screen position (UI coordinates) or `nil` — for nameplates, markers… |
| `ArchRust.MoveTo(x, y, z)` | walks the character |
| `ArchRust.RunCommand("/cmd …")` | runs client or AddOn commands |
| `ArchRust.SetUnitTint(unit, r, g, b [, a])` / `ClearUnitTint([unit])` | tints a unit's 3D model |
| `ArchRust.SetUIColor(key, r, g, b [, a])` / `GetUIColor` / `ResetUIColors()` | native window colours: `windowFill`, `panelFill`, `windowStroke`, `text`, `accent`, `hyperlink`, `widgetFill`, `widgetHover`, `extremeBg`, `faintBg` |
| `ArchRust.SetBinding(key, function or "/command")` / `GetBinding` | key binding (`F5`, `CTRL-T`, `ALT-SHIFT-1`…); `nil` clears it |
| `ArchRust.Fire(event, ...)` | fires an event to every frame |
| `ArchRust.OpenOptions([panel])` | opens the AddOn options panel |
| `ArchRust.Templates`, `ArchRust.RegisterTemplate` | frame templates |
| `ArchRust.FrameMethods` | methods shared by every frame |
| `ArchRust.FilterChat` | called for every incoming message (replaceable) |
| `ArchRust.GetErrors()` / `ReportError(text)` | recent Lua errors / report one |
| `ArchRust.IsInWorld()`, `GetLoadedSectors()`, `GetFrameRect(frame)` | client state |
| `ArchRust.Dump(value [, depth])` | readable text of a table |
| `ArchRust.BuiltinLibraries` | loaders of the built-in libraries |

Lua errors show once in red in the chat and in the log; `/errors` lists them.

## CVars

`SetCVar(name, value)` is applied at once by the engine; `CVAR_UPDATE` is fired.

| CVar | Default | Effect |
|---|---|---|
| `skyColor` | `0.35 0.45 0.6` | sky colour (scene background) |
| `cameraFov` | `55` | vertical field of view (degrees) |
| `cameraFarClip` | `200000` | maximum render distance |
| `objectDrawDistance` | `25000` | draw distance of world objects |
| `lightDirection` | `0.4 0.8 0.3` | sun direction |
| `ambientColor` | `0.45 0.45 0.5` | ambient light |
| `uiScale` | `1` | scale of the whole UI (0.5 to 3) |
| `nameplateDistance` | `6000` | suggested nameplate distance (read by AddOns) |
| `loginMusic` | `sound/bgm/htt_001.mp3` | music of the login, selection and creation screens (empty = silence) |
| `worldMusic` | `sound/bgm/hft_001.mp3` | in-game music when the player's region has none (`RegionTemplate.ini` → `skyset.ini`) |
| `worldAmbience` | `sound/stream/a_cid_d.mp3` | in-game ambience (empty = none) |

AddOns can create their own CVars (`RegisterCVar`).

## Provided slash commands

| Command | Effect |
|---|---|
| `/run code`, `/script code` | runs Lua |
| `/dump expression` | prints a value (tables included) |
| `/target [name]`, `/tar`, `/cleartarget` | targets a unit (the nearest without a name) |
| `/options [panel]` | AddOn options panel |
| `/addons` | lists AddOns and their state |
| `/errors` | recent Lua errors |
| `/loadaddon Name` | loads a `LoadOnDemand` AddOn |
| `/reload`, `/rl` | reloads the UI (save, then reload) |

Client commands: `/goto x y z`, `/cam distance [yaw°] [pitch°]`, `/music <path>|off|auto` (forced,
muted or automatic music), `/sound <path>` (plays an effect), `/quit`.
The Lua console (`` ` `` key) also runs code in the AddOn state.

## Differences from WoW

- Lua 5.4, not 5.1: separate integers and floats (`math.type`), `goto`, native bitwise operators;
  `setfenv`/`getfenv` are simplified; no bytecode.
- No "secure" environment and no combat lockdown: everything can be changed.
- No XML frames (only `<Script>`/`<Include>` are read); animations (`CreateAnimationGroup`) are
  dummy objects.
- Class, race, level, mana and max HP are not decoded from the server yet: neutral values
  (`ADVENTURER`, level 1, max HP = highest seen).
- The server only has the `SAY` channel: other `SendChatMessage` types are sent as `SAY`.
- `PlaySound(number)` with a WoW sound-kit id plays nothing (no kit table); pass a path. `StopSound`
  and the channel argument (`Master`, `SFX`…) are ignored: volumes come from the client's Options
  window.

## Tests

`cargo test -p archrust-addons` loads Lua: standard libraries and aliases, bounded `require`, a
full Ace3 life cycle (AceDB, profiles, timers, hooks, locales), the serializer, LibDataBroker,
LibSharedMedia, `embeds.xml`, the mouse (click, drag, slider), world events, and a session with the
repository's sample AddOns (`tests/sample_addons.rs`).
