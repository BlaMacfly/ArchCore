//! Bibliothèques d'AddOns intégrées au client.
//!
//! `LibStub` est toujours présent. Les autres bibliothèques se chargent à la première demande
//! (`LibStub("AceAddon-3.0")`, `require("AceDB-3.0")`) — sauf si un AddOn en fournit déjà une
//! version dans son dossier `libs/` (elle est chargée par son `.toc`/`embeds.xml` et LibStub garde
//! la plus récente). Ce sont des implémentations propres à ArchRust qui suivent l'API publique des
//! bibliothèques de WoW du même nom ; leur version mineure est 0, donc une copie d'origine livrée
//! par un AddOn a toujours la priorité.

use mlua::{ChunkMode, Lua, Table, Value};

/// `(nom LibStub, source Lua)`, dans l'ordre de dépendance.
pub const BUILTIN: &[(&str, &str)] = &[
    (
        "CallbackHandler-1.0",
        include_str!("lua/libs/CallbackHandler-1.0.lua"),
    ),
    ("AceAddon-3.0", include_str!("lua/libs/AceAddon-3.0.lua")),
    ("AceEvent-3.0", include_str!("lua/libs/AceEvent-3.0.lua")),
    ("AceTimer-3.0", include_str!("lua/libs/AceTimer-3.0.lua")),
    ("AceConsole-3.0", include_str!("lua/libs/AceConsole-3.0.lua")),
    ("AceDB-3.0", include_str!("lua/libs/AceDB-3.0.lua")),
    ("AceHook-3.0", include_str!("lua/libs/AceHook-3.0.lua")),
    ("AceLocale-3.0", include_str!("lua/libs/AceLocale-3.0.lua")),
    (
        "AceSerializer-3.0",
        include_str!("lua/libs/AceSerializer-3.0.lua"),
    ),
    (
        "LibDataBroker-1.1",
        include_str!("lua/libs/LibDataBroker-1.1.lua"),
    ),
    (
        "LibSharedMedia-3.0",
        include_str!("lua/libs/LibSharedMedia-3.0.lua"),
    ),
];

pub const LIBSTUB: &str = include_str!("lua/libs/LibStub.lua");

/// Installe LibStub, les chargeurs paresseux (`ArchRust.BuiltinLibraries`) et `package.preload`.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    let g = lua.globals();
    let ar: Table = g.get("ArchRust")?;
    let loaders = lua.create_table()?;
    for &(name, src) in BUILTIN {
        let loader = lua.create_function(move |lua, ()| {
            let libstub: Table = lua.globals().get("LibStub")?;
            let libs: Table = libstub.get("libs")?;
            if !matches!(libs.get::<Value>(name)?, Value::Nil) {
                return Ok(());
            }
            lua.load(src)
                .set_name(format!("=builtin/{name}"))
                .set_mode(ChunkMode::Text)
                .exec()
        })?;
        loaders.set(name, loader)?;
    }
    ar.set("BuiltinLibraries", loaders)?;
    lua.load(LIBSTUB)
        .set_name("=builtin/LibStub")
        .set_mode(ChunkMode::Text)
        .exec()?;
    // require("AceAddon-3.0") → LibStub("AceAddon-3.0") ; require("LibStub") → LibStub.
    let package: Table = g.get("package")?;
    let preload: Table = package.get("preload")?;
    for &(name, _) in BUILTIN {
        preload.set(
            name,
            lua.create_function(move |lua, _: Value| {
                let libstub: Table = lua.globals().get("LibStub")?;
                let get: mlua::Function = libstub.get("GetLibrary")?;
                get.call::<Value>((libstub, name))
            })?,
        )?;
    }
    preload.set(
        "LibStub",
        lua.create_function(|lua, _: Value| lua.globals().get::<Value>("LibStub"))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::AddonManager;
    use std::path::Path;

    fn write(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// Un AddOn Ace3 complet : AceAddon + AceEvent + AceTimer + AceConsole + AceDB + AceHook,
    /// avec un module, des messages, des profils et une SavedVariable.
    #[test]
    fn ace3_addon_lifecycle() {
        let tmp = tempfile::tempdir().unwrap();
        let addons = tmp.path().join("addons");
        let dir = addons.join("AceTest");
        write(
            &dir,
            "AceTest.toc",
            "## Title: AceTest\n## SavedVariables: AceTestDB\nlocales.lua\ncore.lua\n",
        );
        write(
            &dir,
            "locales.lua",
            r#"
local L = LibStub("AceLocale-3.0"):NewLocale("AceTest", "enUS", true)
L["Hello"] = true
L["Bye"] = "Bye!"
L = LibStub("AceLocale-3.0"):NewLocale("AceTest", "frFR")
if L then L["Hello"] = "Bonjour" end
"#,
        );
        write(
            &dir,
            "core.lua",
            r#"
local addonName, ns = ...
local A = LibStub("AceAddon-3.0"):NewAddon("AceTest", "AceEvent-3.0", "AceTimer-3.0", "AceConsole-3.0", "AceHook-3.0")
local L = LibStub("AceLocale-3.0"):GetLocale("AceTest")
Log = {}
local function log(s) Log[#Log + 1] = s end
local defaults = { profile = { size = 10, colors = { ["*"] = { r = 1 } } }, global = { runs = 0 }, char = { seen = 0 } }

function A:OnInitialize()
  self.db = LibStub("AceDB-3.0"):New("AceTestDB", defaults, true)
  self.db.global.runs = self.db.global.runs + 1
  self.db.RegisterCallback(self, "OnProfileChanged", "Refresh")
  self:RegisterChatCommand("acetest", "Slash")
  log("init " .. L["Hello"] .. " " .. L["Bye"])
end
function A:OnEnable()
  log("enable " .. tostring(IsLoggedIn()))
  self:RegisterEvent("CHAT_MSG_SAY")
  self:RegisterMessage("ACETEST_PING", "OnPing")
  self.ticks = 0
  self.timer = self:ScheduleRepeatingTimer(function() self.ticks = self.ticks + 1 end, 1)
  self:ScheduleTimer("Once", 2.5, "argument")
  self:SecureHook("SendChatMessage", function(msg) log("hooked " .. msg) end)
end
function A:CHAT_MSG_SAY(event, msg, who) log(event .. " " .. msg .. " " .. who) end
function A:OnPing(msg, value) log("ping " .. value) end
function A:Once(arg) log("once " .. arg) end
function A:Refresh(event, db, profile) log("profile " .. profile) end
function A:Slash(input)
  local a, b = self:GetArgs(input, 2)
  log("slash " .. tostring(a) .. "/" .. tostring(b))
  self:Print("bonjour", a)
end

local M = A:NewModule("Sub", "AceEvent-3.0")
function M:OnEnable() log("module " .. self:GetName()) end
"#,
        );
        let mut mgr = AddonManager::new(tmp.path().join("sv")).unwrap();
        mgr.set_host(|h| {
            h.player = Some(crate::UnitInfo {
                cid: 1,
                name: "Testeur".into(),
                ..Default::default()
            })
        });
        mgr.load_dir(&addons).unwrap();
        mgr.fire_event("PLAYER_LOGIN", &[]);
        mgr.fire_event_str("CHAT_MSG_SAY", &["salut", "Autre"]);
        mgr.run_script("LibStub('AceAddon-3.0'):GetAddon('AceTest'):SendMessage('ACETEST_PING', 42)")
            .unwrap();
        assert!(mgr.run_slash("/acetest un \"deux mots\""));
        mgr.run_script("SendChatMessage('coucou', 'SAY')").unwrap();
        for _ in 0..6 {
            mgr.update(0.5);
        }
        mgr.run_script(
            r#"
            local A = LibStub("AceAddon-3.0"):GetAddon("AceTest")
            assert(A.db.profile.size == 10, "défaut")
            assert(A.db.profile.colors.anything.r == 1, "joker *")
            A.db.profile.size = 20
            A.db:SetProfile("Autre")
            assert(A.db.profile.size == 10, "nouveau profil = défauts")
            A.db:SetProfile("Default")
            assert(A.db.profile.size == 20, "retour au profil")
            A:CancelTimer(A.timer)
            TicksAtCancel = A.ticks
        "#,
        )
        .unwrap();
        mgr.update(5.0);
        assert!(mgr.errors().is_empty(), "{:?}", mgr.errors());
        let log = mgr.eval_string("table.concat(Log, ' | ')").unwrap();
        for want in [
            "init Bonjour Bye!",
            "enable true",
            "module Sub",
            "CHAT_MSG_SAY salut Autre",
            "ping 42",
            "slash un/deux mots",
            "hooked coucou",
            "once argument",
            "profile Autre",
            "profile Default",
        ] {
            assert!(log.contains(want), "« {want} » absent de : {log}");
        }
        assert_eq!(mgr.eval_string("TicksAtCancel").unwrap(), "3");
        assert_eq!(
            mgr.eval_string("LibStub('AceAddon-3.0'):GetAddon('AceTest').ticks").unwrap(),
            "3",
            "le minuteur annulé ne tourne plus"
        );
        let printed = mgr.drain_commands();
        assert!(
            printed.iter().any(|c| matches!(c, crate::HostCommand::Print(l) if l.contains("AceTest") && l.contains("bonjour un"))),
            "{printed:?}"
        );
        // À la déconnexion, AceDB retire les valeurs par défaut avant l'écriture.
        mgr.shutdown();
        let saved = std::fs::read_to_string(tmp.path().join("sv/AceTest.lua")).unwrap();
        assert!(saved.contains("size = 20"), "{saved}");
        assert!(!saved.contains("size = 10"), "{saved}");
        assert!(saved.contains("runs = 1"), "{saved}");
    }

    #[test]
    fn serializer_broker_media_and_embedded_libs() {
        let tmp = tempfile::tempdir().unwrap();
        let addons = tmp.path().join("addons");
        let dir = addons.join("Embeds");
        write(
            &dir,
            "Embeds.toc",
            "## Title: Embeds\n## OptionalDeps: Ace3\nembeds.xml\ncore.lua\n",
        );
        write(
            &dir,
            "embeds.xml",
            r#"<Ui xmlns="http://www.blizzard.com/wow/ui/">
  <Script file="libs\LibStub\LibStub.lua"/>
  <Include file="libs\MyLib\MyLib.xml"/>
</Ui>"#,
        );
        // LibStub d'origine (même version) : ne doit rien remplacer.
        write(
            &dir,
            "libs/LibStub/LibStub.lua",
            "local LibStub = _G.LibStub; if not LibStub or LibStub.minor < 2 then error('remplacé') end",
        );
        write(
            &dir,
            "libs/MyLib/MyLib.xml",
            r#"<Ui><Script file="MyLib.lua"/></Ui>"#,
        );
        write(
            &dir,
            "libs/MyLib/MyLib.lua",
            "local lib = LibStub:NewLibrary('MyLib-1.0', 3); if lib then function lib:Hello() return 'mylib' end end",
        );
        write(
            &dir,
            "core.lua",
            r#"
local S = LibStub("AceSerializer-3.0")
local s = S:Serialize(1, "a b^~", { x = true, [2] = 2.5, n = { -3 } }, nil, false, 1e300)
local ok, a, b, c, d, e, f = S:Deserialize(s)
assert(ok, a)
assert(a == 1 and b == "a b^~" and c.x == true and c[2] == 2.5 and c.n[1] == -3 and d == nil and e == false and f == 1e300)
MyLibOut = LibStub("MyLib-1.0"):Hello()

local ldb = LibStub("LibDataBroker-1.1")
Changes = 0
ldb.RegisterCallback("Embeds", "LibDataBroker_AttributeChanged_Demo_text", function() Changes = Changes + 1 end)
local obj = ldb:NewDataObject("Demo", { type = "data source", text = "0" })
obj.text = "1"
obj.text = "1"
obj.text = "2"

local LSM = LibStub("LibSharedMedia-3.0")
LSM:Register("statusbar", "Embeds", "Interface\\AddOns\\Embeds\\bar.png")
MediaOk = LSM:Fetch("statusbar", "Embeds") == "Interface\\AddOns\\Embeds\\bar.png" and LSM:Fetch("statusbar", "absent") ~= nil
"#,
        );
        let mut mgr = AddonManager::new(tmp.path().join("sv")).unwrap();
        mgr.load_dir(&addons).unwrap();
        assert!(mgr.errors().is_empty(), "{:?}", mgr.errors());
        assert_eq!(mgr.eval_string("MyLibOut").unwrap(), "mylib");
        assert_eq!(mgr.eval_string("Changes").unwrap(), "2");
        assert_eq!(mgr.eval_string("tostring(MediaOk)").unwrap(), "true");
    }
}
