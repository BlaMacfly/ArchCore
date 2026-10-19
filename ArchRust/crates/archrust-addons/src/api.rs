//! Fonctions globales exposées aux AddOns : sous-ensemble de l'API de WoW adapté à ArchLord, plus
//! l'espace `ArchRust` pour ce qui est propre au client (caméra, rendu, couleurs de l'interface,
//! raccourcis, unités en vue…).

use crate::frame::{create_frame, frame_id, frame_object, num};
use crate::manager::{
    fire, load_by_name, report_error, CombatValue, HostCommand, Shared, SharedState, Timer,
    UnitInfo,
};
use crate::ui::{FrameKind, Rect};
use mlua::{ChunkMode, Function, Lua, MultiValue, Table, Value, Variadic};

/// Interface utilisateur en Lua (polices, GameTooltip, modèles, fenêtres utilitaires).
pub const INTERFACE: &str = include_str!("lua/interface.lua");

/// Texte d'une valeur Lua (pour `print`).
pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::Nil => "nil".into(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{n:.1}")
            } else {
                n.to_string()
            }
        }
        Value::String(s) => s.to_str().map(|s| s.to_string()).unwrap_or_default(),
        Value::Table(t) => match crate::frame::handle(v) {
            Some(h) => h
                .read(|f| f.name.clone())
                .flatten()
                .map(|n| format!("Frame<{n}>"))
                .unwrap_or_else(|| format!("Frame<{}>", h.id)),
            None => format!("table: {:p}", t.to_pointer()),
        },
        Value::Function(f) => format!("function: {:p}", f.to_pointer()),
        other => format!("{other:?}"),
    }
}

/// GUID d'une unité : `Player-<cid>` pour le joueur, `Unit-<tid>-<cid>` pour les autres.
pub fn guid_of(u: &UnitInfo, is_player: bool) -> String {
    if is_player {
        format!("Player-{}", u.cid)
    } else {
        format!("Unit-{}-{}", u.tid, u.cid)
    }
}

/// Unité désignée par un jeton (`player`, `target`, `nameplateN`, `mouseover`) ou un GUID.
/// Rend l'unité et `true` si c'est le joueur.
pub fn resolve_unit(s: &SharedState, token: &str) -> Option<(UnitInfo, bool)> {
    let t = token.trim().to_ascii_lowercase();
    let player_cid = s.host.player.as_ref().map(|p| p.cid);
    let by_cid = |cid: i32| -> Option<(UnitInfo, bool)> {
        if Some(cid) == player_cid {
            return s.host.player.clone().map(|p| (p, true));
        }
        s.host
            .units
            .iter()
            .find(|u| u.cid == cid)
            .cloned()
            .map(|u| (u, false))
    };
    match t.as_str() {
        "player" => s.host.player.clone().map(|p| (p, true)),
        "target" => s.host.target.and_then(by_cid),
        "" | "none" => None,
        _ => {
            if let Some(n) = t.strip_prefix("nameplate") {
                let i: usize = n.parse().ok()?;
                return s.host.units.get(i.checked_sub(1)?).cloned().map(|u| (u, false));
            }
            // GUID : le CID est le dernier champ.
            if t.starts_with("player-") || t.starts_with("unit-") {
                let cid: i32 = t.rsplit('-').next()?.parse().ok()?;
                return by_cid(cid);
            }
            None
        }
    }
}

fn unit_arg(v: &Value) -> String {
    match v {
        Value::String(s) => s.to_str().map(|s| s.to_string()).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Table Lua décrivant une unité (`ArchRust.GetUnit`, `ArchRust.GetUnits`).
fn unit_table(lua: &Lua, u: &UnitInfo, is_player: bool, token: &str) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("guid", guid_of(u, is_player))?;
    t.set("token", token)?;
    t.set("cid", u.cid)?;
    t.set("tid", u.tid)?;
    t.set("name", u.name.as_str())?;
    t.set("level", u.level)?;
    t.set("hp", u.hp)?;
    t.set("hpMax", u.hp_max)?;
    t.set("mp", u.mp)?;
    t.set("mpMax", u.mp_max)?;
    t.set("x", u.position[0])?;
    t.set("y", u.position[1])?;
    t.set("z", u.position[2])?;
    t.set("facing", u.facing)?;
    t.set("moving", u.moving)?;
    t.set("isPlayer", is_player)?;
    Ok(t)
}

/// Projette un point du monde à l'écran (repère de l'interface, origine en bas à gauche).
pub fn world_to_screen(s: &SharedState, p: [f32; 3]) -> Option<(f32, f32)> {
    let m = &s.host.view_proj;
    let (w, h) = s.host.screen;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    // Colonnes : m[0..4] = colonne 0, etc.
    let x = m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12];
    let y = m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13];
    let ww = m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15];
    if ww <= 1e-4 {
        return None;
    }
    let (nx, ny) = (x / ww, y / ww);
    if !(-1.2..=1.2).contains(&nx) || !(-1.2..=1.2).contains(&ny) {
        return None;
    }
    Some(((nx + 1.0) * 0.5 * w, (ny + 1.0) * 0.5 * h))
}

fn new_timer(
    lua: &Lua,
    shared: &Shared,
    delay: f64,
    func: Function,
    period: Option<f64>,
    iterations: Option<i64>,
    with_handle: bool,
) -> mlua::Result<Option<Table>> {
    let key = lua.create_registry_value(func)?;
    let id = {
        let mut s = shared.borrow_mut();
        let id = s.next_timer;
        s.next_timer += 1;
        id
    };
    let handle = if with_handle {
        let h = lua.create_table()?;
        h.set("_id", id)?;
        let s1 = shared.clone();
        h.set(
            "Cancel",
            lua.create_function(move |_, h: Table| {
                let id: u64 = h.get("_id")?;
                h.set("_cancelled", true)?;
                if let Some(t) = s1.borrow_mut().timers.iter_mut().find(|t| t.id == id) {
                    t.cancelled = true;
                }
                Ok(())
            })?,
        )?;
        h.set(
            "IsCancelled",
            lua.create_function(|_, h: Table| Ok(h.get::<Option<bool>>("_cancelled")?.unwrap_or(false)))?,
        )?;
        Some(h)
    } else {
        None
    };
    let now = shared.borrow().host.time;
    let handle_key = match &handle {
        Some(h) => Some(lua.create_registry_value(h.clone())?),
        None => None,
    };
    shared.borrow_mut().timers.push(Timer {
        id,
        due: now + delay.max(0.0),
        period,
        remaining: iterations.filter(|&n| n > 0),
        func: key,
        handle: handle_key,
        cancelled: false,
    });
    Ok(handle)
}

/// Exécute une commande slash des AddOns (`SLASH_<CLÉ><n>` → `SlashCmdList[<CLÉ>]`).
pub fn run_slash(lua: &Lua, line: &str) -> bool {
    let line = line.trim();
    if !line.starts_with('/') {
        return false;
    }
    let (cmd, args) = line
        .split_once(char::is_whitespace)
        .map(|(c, a)| (c, a.trim()))
        .unwrap_or((line, ""));
    let g = lua.globals();
    let Ok(list) = g.get::<Table>("SlashCmdList") else {
        return false;
    };
    // Comme sous WoW : pour chaque clé de SlashCmdList, `SLASH_<CLÉ>1`, `SLASH_<CLÉ>2`…
    let mut found: Option<(String, Function)> = None;
    'keys: for pair in list.pairs::<Value, Value>() {
        let Ok((Value::String(key), Value::Function(f))) = pair else {
            continue;
        };
        let Ok(key) = key.to_str().map(|k| k.to_string()) else {
            continue;
        };
        for i in 1.. {
            let alias: Value = g.get(format!("SLASH_{key}{i}")).unwrap_or(Value::Nil);
            let Value::String(alias) = alias else { break };
            if alias.to_str().is_ok_and(|a| a.eq_ignore_ascii_case(cmd)) {
                found = Some((key, f));
                break 'keys;
            }
        }
    }
    match found {
        Some((key, f)) => {
            let editbox = g.get::<Value>("DEFAULT_CHAT_FRAME").unwrap_or(Value::Nil);
            if let Err(e) = f.call::<()>((args.to_string(), editbox)) {
                log::error!("addons : /{key} : {e}");
                if let Ok(Value::Function(h)) = g.get::<Value>("geterrorhandler") {
                    if let Ok(handler) = h.call::<Function>(()) {
                        let _ = handler.call::<()>(format!("/{key} : {e}"));
                    }
                }
            }
            true
        }
        None => false,
    }
}

/// Installe les globales de l'API dans l'état Lua.
pub fn install(lua: &Lua, shared: Shared) -> mlua::Result<()> {
    let g = lua.globals();
    let frame_methods = crate::frame::install(lua)?;

    let set_fn = |name: &str, f: Function| g.set(name, f);
    macro_rules! func {
        ($name:expr, $s:ident, $body:expr) => {{
            let $s = shared.clone();
            set_fn($name, lua.create_function($body)?)?;
        }};
    }

    // print → fenêtre de discussion.
    func!("print", s, move |_, args: Variadic<Value>| {
        let line = args.iter().map(value_to_string).collect::<Vec<_>>().join(" ");
        s.borrow_mut().commands.push(HostCommand::Print(line));
        Ok(())
    });

    // Frames
    func!("CreateFrame", s, move |lua, (kind, name, parent, template, _id): (String, Option<String>, Option<Value>, Option<String>, Option<i64>)| {
        let parent_id = parent.as_ref().and_then(|p| match p {
            Value::String(n) => n.to_str().ok().and_then(|n| s.borrow().ui.by_name(&n)),
            v => frame_id(v),
        });
        let k = FrameKind::parse(&kind);
        create_frame(lua, &s, k, name, parent_id, template.as_deref())
    });
    // UIParent : couvre tout l'écran. WorldFrame : idem (monde 3D sous l'interface).
    for name in ["UIParent", "WorldFrame"] {
        let t = create_frame(lua, &shared, FrameKind::Frame, Some(name.into()), None, None)?;
        if let Some(h) = crate::frame::handle(&Value::Table(t.clone())) {
            h.with(|f| {
                f.anchors = vec![
                    crate::ui::Anchor {
                        point: "TOPLEFT".into(),
                        relative_to: None,
                        relative_point: "TOPLEFT".into(),
                        x: 0.0,
                        y: 0.0,
                    },
                    crate::ui::Anchor {
                        point: "BOTTOMRIGHT".into(),
                        relative_to: None,
                        relative_point: "BOTTOMRIGHT".into(),
                        x: 0.0,
                        y: 0.0,
                    },
                ];
                f.strata = Some(if name == "WorldFrame" { "WORLD" } else { "MEDIUM" }.into());
                f.level = Some(0);
                f.builtin = true;
            });
        }
    }
    // Frames natives du client pilotables par les AddOns (Show/Hide/SetAlpha).
    let ui_parent = shared.borrow().ui.by_name("UIParent");
    for name in [
        "ChatFrame1",
        "ArchRustInfoFrame",
        "ArchRustInventoryButton",
        "ArchRustConsoleFrame",
    ] {
        let t = create_frame(lua, &shared, FrameKind::Frame, Some(name.into()), ui_parent, None)?;
        if let Some(h) = crate::frame::handle(&Value::Table(t)) {
            h.with(|f| f.builtin = true);
        }
    }
    g.set("DEFAULT_CHAT_FRAME", g.get::<Value>("ChatFrame1")?)?;
    g.set("SELECTED_CHAT_FRAME", g.get::<Value>("ChatFrame1")?)?;
    g.set("NUM_CHAT_WINDOWS", 1)?;
    func!("EnumerateFrames", s, move |lua, cur: Option<Value>| {
        let start = cur.as_ref().and_then(frame_id).map(|i| i + 1).unwrap_or(0);
        let next = {
            let st = s.borrow();
            (start..st.ui.frames.len()).find(|&i| !st.ui.frames[i].kind.is_region())
        };
        match next {
            Some(i) => Ok(Value::Table(frame_object(lua, &s, i)?)),
            None => Ok(Value::Nil),
        }
    });
    func!("GetMouseFocus", s, move |lua, ()| {
        let id = {
            let st = s.borrow();
            st.host
                .cursor
                .and_then(|(x, y)| st.ui.hit_test(x, y))
        };
        match id {
            Some(i) => Ok(Value::Table(frame_object(lua, &s, i)?)),
            None => Ok(Value::Nil),
        }
    });
    g.set(
        "GetMouseFoci",
        lua.load("return function() return { GetMouseFocus() } end")
            .set_name("=GetMouseFoci")
            .eval::<Function>()?,
    )?;
    func!("GetScreenWidth", s, move |_, ()| Ok(num(s.borrow().host.screen.0)));
    func!("GetScreenHeight", s, move |_, ()| Ok(num(s.borrow().host.screen.1)));
    func!("GetCursorPosition", s, move |_, ()| {
        let c = s.borrow().host.cursor.unwrap_or((0.0, 0.0));
        Ok((num(c.0), num(c.1)))
    });
    func!("IsShiftKeyDown", s, move |_, ()| Ok(s.borrow().host.modifiers[0]));
    func!("IsControlKeyDown", s, move |_, ()| Ok(s.borrow().host.modifiers[1]));
    func!("IsAltKeyDown", s, move |_, ()| Ok(s.borrow().host.modifiers[2]));
    func!("IsModifierKeyDown", s, move |_, ()| {
        Ok(s.borrow().host.modifiers.iter().any(|&m| m))
    });

    // Temps, royaume, version
    func!("GetTime", s, move |_, ()| Ok(s.borrow().host.time));
    func!("GetFramerate", s, move |_, ()| Ok(s.borrow().host.framerate));
    func!("GetRealmName", s, move |_, ()| Ok(s.borrow().host.realm.clone()));
    func!("GetNormalizedRealmName", s, move |_, ()| {
        Ok(s.borrow().host.realm.replace([' ', '-'], ""))
    });
    func!("GetLocale", s, move |_, ()| Ok(s.borrow().host.locale.clone()));
    func!("IsLoggedIn", s, move |_, ()| Ok(s.borrow().logged_in));
    g.set(
        "GetBuildInfo",
        lua.create_function(|_, ()| {
            Ok((
                env!("CARGO_PKG_VERSION"),
                "1",
                "2026-10-18",
                crate::INTERFACE_VERSION,
            ))
        })?,
    )?;
    g.set("GetCurrentRegion", lua.create_function(|_, ()| Ok(3))?)?;
    g.set("GetCurrentRegionName", lua.create_function(|_, ()| Ok("EU"))?)?;
    g.set("GetZoneText", lua.create_function(|_, ()| Ok("ArchLord"))?)?;
    g.set("GetRealZoneText", lua.create_function(|_, ()| Ok("ArchLord"))?)?;
    g.set("GetSubZoneText", lua.create_function(|_, ()| Ok(""))?)?;
    g.set("GetMinimapZoneText", lua.create_function(|_, ()| Ok("ArchLord"))?)?;
    g.set("GetMoney", lua.create_function(|_, ()| Ok(0))?)?;
    g.set("IsInGroup", lua.create_function(|_, _: MultiValue| Ok(false))?)?;
    g.set("IsInRaid", lua.create_function(|_, _: MultiValue| Ok(false))?)?;
    g.set("IsInGuild", lua.create_function(|_, ()| Ok(false))?)?;
    g.set("GetNumGroupMembers", lua.create_function(|_, _: MultiValue| Ok(0))?)?;
    g.set("IsInInstance", lua.create_function(|_, ()| Ok((false, "none")))?)?;

    // Unités
    func!("UnitExists", s, move |_, u: Value| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&u)).is_some())
    });
    func!("UnitName", s, move |_, u: Value| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&u)).map(|(u, _)| u.name))
    });
    g.set("GetUnitName", g.get::<Function>("UnitName")?)?;
    g.set("UnitFullName", g.get::<Function>("UnitName")?)?;
    func!("UnitGUID", s, move |_, u: Value| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&u)).map(|(u, p)| guid_of(&u, p)))
    });
    func!("UnitIsPlayer", s, move |_, u: Value| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&u)).is_some_and(|(_, p)| p))
    });
    func!("UnitIsUnit", s, move |_, (a, b): (Value, Value)| {
        let st = s.borrow();
        Ok(
            match (resolve_unit(&st, &unit_arg(&a)), resolve_unit(&st, &unit_arg(&b))) {
                (Some((x, _)), Some((y, _))) => x.cid == y.cid,
                _ => false,
            },
        )
    });
    func!("UnitIsDead", s, move |_, u: Value| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&u)).is_some_and(|(u, _)| u.hp == Some(0)))
    });
    g.set("UnitIsDeadOrGhost", g.get::<Function>("UnitIsDead")?)?;
    g.set("UnitIsCorpse", g.get::<Function>("UnitIsDead")?)?;
    func!("UnitHealth", s, move |_, u: Value| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&u))
            .and_then(|(u, _)| u.hp)
            .unwrap_or(0))
    });
    func!("UnitHealthMax", s, move |_, u: Value| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&u))
            .and_then(|(u, _)| u.hp_max.or(u.hp))
            .unwrap_or(0))
    });
    func!("UnitPower", s, move |_, (u, _t): (Value, Option<Value>)| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&u))
            .and_then(|(u, _)| u.mp)
            .unwrap_or(0))
    });
    func!("UnitPowerMax", s, move |_, (u, _t): (Value, Option<Value>)| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&u))
            .and_then(|(u, _)| u.mp_max.or(u.mp))
            .unwrap_or(0))
    });
    g.set(
        "UnitPowerType",
        lua.create_function(|_, _: MultiValue| Ok((0, "MANA")))?,
    )?;
    func!("UnitLevel", s, move |_, u: Value| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&u))
            .map(|(u, _)| u.level)
            .unwrap_or(0))
    });
    g.set("UnitEffectiveLevel", g.get::<Function>("UnitLevel")?)?;
    // Classe / race : pas encore décodées depuis le serveur → valeurs neutres.
    g.set(
        "UnitClass",
        lua.create_function(|_, _: MultiValue| Ok(("Aventurier", "ADVENTURER", 0)))?,
    )?;
    g.set("UnitClassBase", g.get::<Function>("UnitClass")?)?;
    g.set(
        "UnitRace",
        lua.create_function(|_, _: MultiValue| Ok(("Humain", "Human", 1)))?,
    )?;
    g.set(
        "UnitFactionGroup",
        lua.create_function(|_, _: MultiValue| Ok(("Neutral", "Neutre")))?,
    )?;
    g.set("UnitSex", lua.create_function(|_, _: MultiValue| Ok(1))?)?;
    g.set(
        "UnitClassification",
        lua.create_function(|_, _: MultiValue| Ok("normal"))?,
    )?;
    for name in [
        "UnitAffectingCombat",
        "UnitIsAFK",
        "UnitIsDND",
        "UnitIsGhost",
        "UnitIsPVP",
        "UnitInParty",
        "UnitInRaid",
        "UnitIsTapDenied",
        "UnitIsGroupLeader",
    ] {
        g.set(name, lua.create_function(|_, _: MultiValue| Ok(false))?)?;
    }
    g.set(
        "UnitIsConnected",
        lua.create_function(|_, _: MultiValue| Ok(true))?,
    )?;
    func!("UnitIsFriend", s, move |_, (_a, b): (Value, Value)| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&b)).is_some_and(|(_, p)| p))
    });
    func!("UnitCanAttack", s, move |_, (_a, b): (Value, Value)| {
        Ok(resolve_unit(&s.borrow(), &unit_arg(&b)).is_some_and(|(_, p)| !p))
    });
    g.set("UnitIsEnemy", g.get::<Function>("UnitCanAttack")?)?;
    g.set(
        "UnitReaction",
        lua.create_function(|_, _: MultiValue| Ok(4))?,
    )?;
    func!("UnitPosition", s, move |_, u: Value| {
        let p = resolve_unit(&s.borrow(), &unit_arg(&u)).map(|(u, _)| u.position);
        Ok(match p {
            Some(p) => (Some(num(p[0])), Some(num(p[1])), Some(num(p[2])), Some(0)),
            None => (None, None, None, None),
        })
    });
    func!("GetPlayerMapPosition", s, move |_, _u: Option<Value>| {
        let p = s
            .borrow()
            .host
            .player
            .as_ref()
            .map(|p| p.position)
            .unwrap_or_default();
        Ok((num(p[0]), num(p[1]), num(p[2])))
    });
    func!("UnitDistanceSquared", s, move |_, u: Value| {
        let st = s.borrow();
        let me = st.host.player.as_ref().map(|p| p.position);
        Ok(match (me, resolve_unit(&st, &unit_arg(&u))) {
            (Some(a), Some((b, _))) => {
                let d: f32 = (0..3).map(|i| (a[i] - b.position[i]).powi(2)).sum();
                (Some(d), true)
            }
            _ => (None, false),
        })
    });
    func!("CheckInteractDistance", s, move |_, (u, _i): (Value, Option<i64>)| {
        let st = s.borrow();
        let me = st.host.player.as_ref().map(|p| p.position);
        Ok(match (me, resolve_unit(&st, &unit_arg(&u))) {
            (Some(a), Some((b, _))) => {
                ((a[0] - b.position[0]).powi(2) + (a[2] - b.position[2]).powi(2)).sqrt() < 1000.0
            }
            _ => false,
        })
    });
    g.set(
        "UnitInRange",
        lua.create_function(|_, _: MultiValue| Ok((true, true)))?,
    )?;
    func!("GetPlayerFacing", s, move |_, ()| {
        Ok(s.borrow()
            .host
            .player
            .as_ref()
            .map(|p| p.facing.to_radians()))
    });
    func!("IsPlayerMoving", s, move |_, ()| {
        Ok(s.borrow().host.player.as_ref().is_some_and(|p| p.moving))
    });
    g.set("UnitXP", lua.create_function(|_, _: MultiValue| Ok(0))?)?;
    g.set("UnitXPMax", lua.create_function(|_, _: MultiValue| Ok(1))?)?;

    // Cible (côté client)
    func!("TargetUnit", s, move |lua, (u, _exact): (Value, Option<bool>)| {
        let arg = unit_arg(&u);
        let cid = {
            let st = s.borrow();
            resolve_unit(&st, &arg).map(|(u, _)| u.cid).or_else(|| {
                // Par nom (insensible à la casse), le plus proche d'abord.
                let lower = arg.to_lowercase();
                let me = st.host.player.as_ref().map(|p| p.position).unwrap_or_default();
                st.host
                    .units
                    .iter()
                    .filter(|u| !lower.is_empty() && u.name.to_lowercase().contains(&lower))
                    .min_by(|a, b| {
                        let da = (a.position[0] - me[0]).powi(2) + (a.position[2] - me[2]).powi(2);
                        let db = (b.position[0] - me[0]).powi(2) + (b.position[2] - me[2]).powi(2);
                        da.total_cmp(&db)
                    })
                    .map(|u| u.cid)
            })
        };
        let changed = {
            let mut st = s.borrow_mut();
            let changed = st.host.target != cid && cid.is_some();
            if cid.is_some() {
                st.host.target = cid;
            }
            changed
        };
        if changed {
            fire(lua, &s, "PLAYER_TARGET_CHANGED", MultiValue::new());
        }
        Ok(cid.is_some())
    });
    func!("ClearTarget", s, move |lua, ()| {
        let had = s.borrow_mut().host.target.take().is_some();
        if had {
            fire(lua, &s, "PLAYER_TARGET_CHANGED", MultiValue::new());
        }
        Ok(had)
    });
    func!("TargetNearest", s, move |lua, _: MultiValue| {
        let cid = {
            let st = s.borrow();
            let me = st.host.player.as_ref().map(|p| p.position).unwrap_or_default();
            st.host
                .units
                .iter()
                .filter(|u| u.hp != Some(0))
                .min_by(|a, b| {
                    let da = (a.position[0] - me[0]).powi(2) + (a.position[2] - me[2]).powi(2);
                    let db = (b.position[0] - me[0]).powi(2) + (b.position[2] - me[2]).powi(2);
                    da.total_cmp(&db)
                })
                .map(|u| u.cid)
        };
        if cid.is_some() && cid != s.borrow().host.target {
            s.borrow_mut().host.target = cid;
            fire(lua, &s, "PLAYER_TARGET_CHANGED", MultiValue::new());
        }
        Ok(cid.is_some())
    });
    g.set("TargetNearestEnemy", g.get::<Function>("TargetNearest")?)?;

    // Discussion, son, commandes
    func!("SendChatMessage", s, move |_, (msg, kind, _lang, target): (String, Option<String>, Option<Value>, Option<String>)| {
        s.borrow_mut().commands.push(HostCommand::Chat {
            message: msg,
            kind: kind.unwrap_or_else(|| "SAY".into()).to_ascii_uppercase(),
            target,
        });
        Ok(())
    });
    func!("PlaySoundFile", s, move |_, (path, _channel): (Value, Option<String>)| {
        s.borrow_mut()
            .commands
            .push(HostCommand::PlaySound(value_to_string(&path)));
        Ok(true)
    });
    g.set("PlaySound", g.get::<Function>("PlaySoundFile")?)?;
    g.set("StopSound", lua.create_function(|_, _: MultiValue| Ok(()))?)?;
    func!("PlayMusic", s, move |_, path: Value| {
        s.borrow_mut()
            .commands
            .push(HostCommand::PlayMusic(Some(value_to_string(&path))));
        Ok(())
    });
    func!("StopMusic", s, move |_, ()| {
        s.borrow_mut().commands.push(HostCommand::PlayMusic(None));
        Ok(())
    });
    func!("ReloadUI", s, move |_, ()| {
        s.borrow_mut().commands.push(HostCommand::ReloadUi);
        Ok(())
    });
    func!("Screenshot", s, move |_, file: Option<String>| {
        s.borrow_mut().commands.push(HostCommand::Screenshot(file));
        Ok(())
    });
    func!("Quit", s, move |_, ()| {
        s.borrow_mut().commands.push(HostCommand::Quit);
        Ok(())
    });
    g.set("ForceQuit", g.get::<Function>("Quit")?)?;
    g.set("Logout", g.get::<Function>("Quit")?)?;
    // RunMacroText("/goto 1 2 3\n/cam 800") : chaque ligne est une commande du client ou d'un AddOn.
    func!("RunMacroText", s, move |lua, text: String| {
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if !run_slash(lua, line) {
                s.borrow_mut().commands.push(HostCommand::Slash(line.to_string()));
            }
        }
        Ok(())
    });
    g.set("SlashCmdList", lua.create_table()?)?;
    g.set("hash_SlashCmdList", lua.create_table()?)?;

    // AddOns
    func!("GetNumAddOns", s, move |_, ()| Ok(s.borrow().manifests.len()));
    func!("GetAddOnInfo", s, move |_, a: Value| {
        let st = s.borrow();
        let m = match &a {
            Value::Integer(i) => st.manifests.get((*i as usize).wrapping_sub(1)),
            Value::Number(n) => st.manifests.get((*n as usize).wrapping_sub(1)),
            Value::String(n) => {
                let n = n.to_str()?.to_string();
                st.manifests.iter().find(|m| m.name.eq_ignore_ascii_case(&n))
            }
            _ => None,
        };
        Ok(match m {
            Some(m) => {
                let loaded = st.addons.iter().find(|x| x.manifest.name == m.name);
                let reason = match loaded {
                    Some(l) if l.error.is_some() => Some("CORRUPT".to_string()),
                    None if m.load_on_demand => Some("DEMAND_LOADED".to_string()),
                    _ => None,
                };
                (
                    Some(m.name.clone()),
                    Some(m.title.clone()),
                    Some(m.notes.clone()),
                    true,
                    reason,
                    Some("INSECURE".to_string()),
                    false,
                )
            }
            None => (None, None, None, false, Some("MISSING".to_string()), None, false),
        })
    });
    func!("GetAddOnMetadata", s, move |_, (a, field): (Value, String)| {
        let st = s.borrow();
        let m = match &a {
            Value::Integer(i) => st.manifests.get((*i as usize).wrapping_sub(1)),
            Value::String(n) => {
                let n = n.to_str()?.to_string();
                st.manifests.iter().find(|m| m.name.eq_ignore_ascii_case(&n))
            }
            _ => None,
        };
        Ok(m.and_then(|m| match field.to_ascii_lowercase().as_str() {
            "title" => Some(m.title.clone()),
            "version" => Some(m.version.clone()),
            "author" => Some(m.author.clone()),
            "notes" => Some(m.notes.clone()),
            "interface" => Some(m.interface.to_string()),
            "savedvariables" => Some(m.saved_variables.join(", ")),
            "dependencies" => Some(m.dependencies.join(", ")),
            _ => m
                .extra
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&field))
                .map(|(_, v)| v.clone()),
        }))
    });
    func!("IsAddOnLoaded", s, move |_, name: Value| {
        let n = unit_arg(&name);
        let st = s.borrow();
        let loaded = st
            .addons
            .iter()
            .any(|a| a.loaded && a.manifest.name.eq_ignore_ascii_case(&n));
        Ok((loaded, loaded))
    });
    func!("IsAddOnLoadOnDemand", s, move |_, name: Value| {
        let n = unit_arg(&name);
        Ok(s.borrow()
            .manifests
            .iter()
            .any(|m| m.load_on_demand && m.name.eq_ignore_ascii_case(&n)))
    });
    func!("LoadAddOn", s, move |lua, name: Value| {
        let n = match &name {
            Value::Integer(i) => s
                .borrow()
                .manifests
                .get((*i as usize).wrapping_sub(1))
                .map(|m| m.name.clone())
                .unwrap_or_default(),
            v => unit_arg(v),
        };
        Ok(load_by_name(lua, &s, &n))
    });
    for name in ["EnableAddOn", "DisableAddOn", "EnableAllAddOns", "DisableAllAddOns"] {
        g.set(name, lua.create_function(|_, _: MultiValue| Ok(()))?)?;
    }
    g.set(
        "GetAddOnEnableState",
        lua.create_function(|_, _: MultiValue| Ok(2))?,
    )?;
    let c_addons = lua.create_table()?;
    for (k, v) in [
        ("GetNumAddOns", "GetNumAddOns"),
        ("GetAddOnInfo", "GetAddOnInfo"),
        ("GetAddOnMetadata", "GetAddOnMetadata"),
        ("IsAddOnLoaded", "IsAddOnLoaded"),
        ("IsAddOnLoadOnDemand", "IsAddOnLoadOnDemand"),
        ("LoadAddOn", "LoadAddOn"),
        ("EnableAddOn", "EnableAddOn"),
        ("DisableAddOn", "DisableAddOn"),
        ("GetAddOnEnableState", "GetAddOnEnableState"),
    ] {
        c_addons.set(k, g.get::<Function>(v)?)?;
    }
    g.set("C_AddOns", c_addons)?;

    // CVars
    func!("GetCVar", s, move |_, name: String| {
        let st = s.borrow();
        Ok(st
            .cvars
            .get(&name)
            .or_else(|| st.cvar_defaults.get(&name))
            .cloned())
    });
    func!("GetCVarDefault", s, move |_, name: String| {
        Ok(s.borrow().cvar_defaults.get(&name).cloned())
    });
    func!("GetCVarBool", s, move |_, name: String| {
        let st = s.borrow();
        Ok(st
            .cvars
            .get(&name)
            .or_else(|| st.cvar_defaults.get(&name))
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")))
    });
    func!("SetCVar", s, move |lua, (name, value): (String, Value)| {
        let v = match &value {
            Value::Boolean(b) => if *b { "1".to_string() } else { "0".to_string() },
            v => value_to_string(v),
        };
        s.borrow_mut().cvars.insert(name.clone(), v.clone());
        fire(
            lua,
            &s,
            "CVAR_UPDATE",
            MultiValue::from_iter([
                Value::String(lua.create_string(&name)?),
                Value::String(lua.create_string(&v)?),
            ]),
        );
        Ok(true)
    });
    func!("RegisterCVar", s, move |_, (name, default): (String, Option<Value>)| {
        let d = default.map(|v| value_to_string(&v)).unwrap_or_default();
        s.borrow_mut().cvar_defaults.entry(name).or_insert(d);
        Ok(())
    });
    let c_cvar = lua.create_table()?;
    for (k, v) in [
        ("GetCVar", "GetCVar"),
        ("GetCVarBool", "GetCVarBool"),
        ("GetCVarDefault", "GetCVarDefault"),
        ("SetCVar", "SetCVar"),
        ("RegisterCVar", "RegisterCVar"),
    ] {
        c_cvar.set(k, g.get::<Function>(v)?)?;
    }
    g.set("C_CVar", c_cvar)?;

    // Objets
    func!("GetItemInfo", s, move |_, item: Value| {
        let st = s.borrow();
        let found = match &item {
            Value::Integer(tid) => st.host.inventory.iter().find(|i| i.tid == Some(*tid as i32)),
            Value::String(n) => {
                let n = n.to_str()?.to_string();
                st.host.inventory.iter().find(|i| i.name.eq_ignore_ascii_case(&n))
            }
            _ => None,
        };
        Ok(found.map(|i| i.name.clone()))
    });
    func!("GetItemCount", s, move |_, item: Value| {
        let st = s.borrow();
        let n: i64 = st
            .host
            .inventory
            .iter()
            .filter(|i| match &item {
                Value::Integer(tid) => i.tid == Some(*tid as i32),
                Value::String(n) => n.to_str().is_ok_and(|n| i.name.eq_ignore_ascii_case(&n)),
                _ => false,
            })
            .map(|i| i.count.unwrap_or(1) as i64)
            .sum();
        Ok(n)
    });
    {
        let c_container = lua.create_table()?;
        let s = shared.clone();
        c_container.set(
            "GetContainerNumSlots",
            lua.create_function(move |_, _bag: Option<i64>| {
                Ok(s.borrow()
                    .host
                    .inventory
                    .iter()
                    .filter(|i| i.status == Some(2))
                    .count())
            })?,
        )?;
        let s = shared.clone();
        c_container.set(
            "GetContainerItemInfo",
            lua.create_function(move |lua, (_bag, slot): (Option<i64>, usize)| {
                let st = s.borrow();
                let item = st
                    .host
                    .inventory
                    .iter()
                    .filter(|i| i.status == Some(2))
                    .nth(slot.wrapping_sub(1));
                match item {
                    Some(i) => {
                        let t = lua.create_table()?;
                        t.set("itemID", i.tid)?;
                        t.set("itemName", i.name.as_str())?;
                        t.set("stackCount", i.count.unwrap_or(1))?;
                        t.set("hyperlink", format!("|cffffffff[{}]|r", i.name))?;
                        Ok(Value::Table(t))
                    }
                    None => Ok(Value::Nil),
                }
            })?,
        )?;
        g.set("C_Container", c_container)?;
    }

    // Journal de combat
    func!("CombatLogGetCurrentEventInfo", s, move |lua, ()| {
        let st = s.borrow();
        let mut out = Variadic::new();
        for v in &st.combat_log {
            out.push(match v {
                CombatValue::Nil => Value::Nil,
                CombatValue::Bool(b) => Value::Boolean(*b),
                CombatValue::Num(n) => Value::Number(*n),
                CombatValue::Int(i) => Value::Integer(*i),
                CombatValue::Str(t) => Value::String(lua.create_string(t)?),
            });
        }
        Ok(out)
    });

    // Minuteurs : C_Timer.After / NewTimer / NewTicker
    {
        let timer = lua.create_table()?;
        let s = shared.clone();
        timer.set(
            "After",
            lua.create_function(move |lua, (delay, func): (f64, Function)| {
                new_timer(lua, &s, delay, func, None, None, false)?;
                Ok(())
            })?,
        )?;
        let s = shared.clone();
        timer.set(
            "NewTimer",
            lua.create_function(move |lua, (delay, func): (f64, Function)| {
                new_timer(lua, &s, delay, func, None, None, true)
            })?,
        )?;
        let s = shared.clone();
        timer.set(
            "NewTicker",
            lua.create_function(
                move |lua, (period, func, iters): (f64, Function, Option<i64>)| {
                    new_timer(lua, &s, period, func, Some(period.max(0.0)), iters, true)
                },
            )?,
        )?;
        g.set("C_Timer", timer)?;
    }

    // Espace ArchRust : extensions propres au client.
    let ar = lua.create_table()?;
    ar.set("Version", env!("CARGO_PKG_VERSION"))?;
    ar.set("FrameMethods", frame_methods)?;
    ar.set("Templates", lua.create_table()?)?;
    {
        let s = shared.clone();
        ar.set(
            "GetUnits",
            lua.create_function(move |lua, ()| {
                let st = s.borrow();
                let t = lua.create_table()?;
                for (i, u) in st.host.units.iter().enumerate() {
                    t.set(i + 1, unit_table(lua, u, false, &format!("nameplate{}", i + 1))?)?;
                }
                Ok(t)
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "GetNumUnits",
            lua.create_function(move |_, ()| Ok(s.borrow().host.units.len()))?,
        )?;
        let s = shared.clone();
        ar.set(
            "GetUnit",
            lua.create_function(move |lua, u: Value| {
                let tok = unit_arg(&u);
                match resolve_unit(&s.borrow(), &tok) {
                    Some((u, p)) => Ok(Value::Table(unit_table(lua, &u, p, &tok)?)),
                    None => Ok(Value::Nil),
                }
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "GetInventory",
            lua.create_function(move |lua, ()| {
                let st = s.borrow();
                let t = lua.create_table()?;
                for (i, it) in st.host.inventory.iter().enumerate() {
                    let e = lua.create_table()?;
                    e.set("id", it.id)?;
                    e.set("tid", it.tid)?;
                    e.set("name", it.name.as_str())?;
                    e.set("count", it.count)?;
                    e.set(
                        "where",
                        match it.status {
                            Some(2) => "bag",
                            Some(3) => "equipped",
                            _ => "other",
                        },
                    )?;
                    if let Some([tab, row, col]) = it.grid {
                        e.set("tab", tab)?;
                        e.set("row", row)?;
                        e.set("col", col)?;
                    }
                    t.set(i + 1, e)?;
                }
                Ok(t)
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "GetCamera",
            lua.create_function(move |_, ()| {
                let c = s.borrow().host.camera;
                Ok((num(c.distance), num(c.yaw), num(c.pitch), num(c.fov)))
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "GetCameraPosition",
            lua.create_function(move |_, ()| {
                let c = s.borrow().host.camera;
                Ok((c.eye[0], c.eye[1], c.eye[2], c.target[0], c.target[1], c.target[2]))
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "SetCamera",
            lua.create_function(
                move |_, (d, y, p): (Option<f32>, Option<f32>, Option<f32>)| {
                    s.borrow_mut().commands.push(HostCommand::SetCamera {
                        distance: d,
                        yaw: y,
                        pitch: p,
                    });
                    Ok(())
                },
            )?,
        )?;
        let s = shared.clone();
        ar.set(
            "WorldToScreen",
            lua.create_function(move |_, (x, y, z): (f32, f32, f32)| {
                Ok(match world_to_screen(&s.borrow(), [x, y, z]) {
                    Some((sx, sy)) => (Some(num(sx)), Some(num(sy))),
                    None => (None, None),
                })
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "MoveTo",
            lua.create_function(move |_, (x, y, z): (f32, f32, f32)| {
                s.borrow_mut().commands.push(HostCommand::MoveTo([x, y, z]));
                Ok(())
            })?,
        )?;
        ar.set("RunCommand", g.get::<Function>("RunMacroText")?)?;
        let s = shared.clone();
        ar.set(
            "SetUnitTint",
            lua.create_function(move |_, (u, c): (Value, Variadic<Value>)| {
                let tok = unit_arg(&u);
                let cid = resolve_unit(&s.borrow(), &tok).map(|(u, _)| u.cid);
                if let Some(cid) = cid {
                    let col = crate::frame::color_args(&c, 1.0);
                    s.borrow_mut().unit_tints.insert(cid, col);
                }
                Ok(cid.is_some())
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "ClearUnitTint",
            lua.create_function(move |_, u: Option<Value>| {
                match u {
                    Some(u) => {
                        let tok = unit_arg(&u);
                        let cid = resolve_unit(&s.borrow(), &tok).map(|(u, _)| u.cid);
                        if let Some(cid) = cid {
                            s.borrow_mut().unit_tints.remove(&cid);
                        }
                    }
                    None => s.borrow_mut().unit_tints.clear(),
                }
                Ok(())
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "SetUIColor",
            lua.create_function(move |_, (key, c): (String, Variadic<Value>)| {
                let col = crate::frame::color_args(&c, 1.0);
                let mut st = s.borrow_mut();
                st.ui_colors.insert(key, col);
                st.ui_colors_dirty = true;
                Ok(())
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "GetUIColor",
            lua.create_function(move |_, key: String| {
                Ok(match s.borrow().ui_colors.get(&key) {
                    Some(c) => (Some(c[0]), Some(c[1]), Some(c[2]), Some(c[3])),
                    None => (None, None, None, None),
                })
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "ResetUIColors",
            lua.create_function(move |_, ()| {
                let mut st = s.borrow_mut();
                st.ui_colors.clear();
                st.ui_colors_dirty = true;
                Ok(())
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "SetBinding",
            lua.create_function(move |lua, (key, action): (String, Value)| {
                let key = key.to_ascii_uppercase();
                match action {
                    Value::Nil => {
                        s.borrow_mut().bindings.remove(&key);
                    }
                    v @ (Value::Function(_) | Value::String(_)) => {
                        let k = lua.create_registry_value(v)?;
                        s.borrow_mut().bindings.insert(key, k);
                    }
                    _ => {
                        return Err(mlua::Error::RuntimeError(
                            "SetBinding : fonction ou commande attendue".into(),
                        ))
                    }
                }
                Ok(true)
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "GetBinding",
            lua.create_function(move |lua, key: String| {
                let st = s.borrow();
                match st.bindings.get(&key.to_ascii_uppercase()) {
                    Some(k) => lua.registry_value::<Value>(k),
                    None => Ok(Value::Nil),
                }
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "GetErrors",
            lua.create_function(move |_, ()| Ok(s.borrow().errors.clone()))?,
        )?;
        let s = shared.clone();
        ar.set(
            "IsInWorld",
            lua.create_function(move |_, ()| Ok(s.borrow().host.in_world))?,
        )?;
        let s = shared.clone();
        ar.set(
            "GetLoadedSectors",
            lua.create_function(move |_, ()| Ok(s.borrow().host.sectors))?,
        )?;
        let s = shared.clone();
        ar.set(
            "GetFrameRect",
            lua.create_function(move |_, f: Value| {
                let r: Option<Rect> = frame_id(&f).and_then(|id| s.borrow().ui.rects.get(id).copied().flatten());
                Ok(match r {
                    Some(r) => (Some(num(r.left)), Some(num(r.bottom)), Some(num(r.width)), Some(num(r.height))),
                    None => (None, None, None, None),
                })
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "Fire",
            lua.create_function(move |lua, (event, args): (String, MultiValue)| {
                fire(lua, &s, &event, args);
                Ok(())
            })?,
        )?;
        let s = shared.clone();
        ar.set(
            "ReportError",
            lua.create_function(move |lua, msg: String| {
                report_error(lua, &s, "AddOn", &mlua::Error::RuntimeError(msg));
                Ok(())
            })?,
        )?;
    }
    g.set("ArchRust", ar)?;

    lua.load(INTERFACE)
        .set_name("=interface")
        .set_mode(ChunkMode::Text)
        .exec()?;
    Ok(())
}
