//! Fonctions globales exposées aux AddOns (sous-ensemble de l'API WoW, adapté à ArchLord).

use crate::frame::FrameHandle;
use crate::manager::{HostCommand, Shared};
use crate::ui::FrameKind;
use mlua::{Function, Lua, Table, Value, Variadic};

/// `string.format` simplifié pour `SetFormattedText` : gère `%s`, `%d`, `%f`, `%.nf`, `%%`.
pub fn lua_format(fmt: &str, args: Vec<Value>) -> String {
    let mut out = String::new();
    let mut it = args.into_iter();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&n) = chars.peek() {
            chars.next();
            if n.is_ascii_alphabetic() || n == '%' {
                spec.push(n);
                break;
            }
            spec.push(n);
        }
        let conv = spec.chars().last().unwrap_or('s');
        let arg = if conv == '%' { None } else { it.next() };
        match conv {
            '%' => out.push('%'),
            'd' | 'i' => out.push_str(&match arg {
                Some(Value::Integer(i)) => i.to_string(),
                Some(Value::Number(n)) => (n as i64).to_string(),
                Some(Value::String(s)) => s.to_str().map(|s| s.to_string()).unwrap_or_default(),
                _ => "0".into(),
            }),
            'f' => {
                let prec = spec.trim_start_matches('.').trim_end_matches('f').parse::<usize>().unwrap_or(6);
                let n = match arg {
                    Some(Value::Integer(i)) => i as f64,
                    Some(Value::Number(n)) => n,
                    _ => 0.0,
                };
                out.push_str(&format!("{n:.prec$}"));
            }
            _ => out.push_str(&match arg {
                Some(Value::String(s)) => s.to_str().map(|s| s.to_string()).unwrap_or_default(),
                Some(Value::Integer(i)) => i.to_string(),
                Some(Value::Number(n)) => n.to_string(),
                Some(Value::Boolean(b)) => b.to_string(),
                Some(Value::Nil) | None => "nil".into(),
                Some(other) => format!("{other:?}"),
            }),
        }
    }
    out
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::Nil => "nil".into(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.to_str().map(|s| s.to_string()).unwrap_or_default(),
        Value::Table(_) => "table".into(),
        Value::Function(_) => "function".into(),
        Value::UserData(ud) => match ud.borrow::<FrameHandle>() {
            Ok(f) => format!("Frame<{}>", f.id),
            Err(_) => "userdata".into(),
        },
        other => format!("{other:?}"),
    }
}

/// Installe les globales de l'API dans l'état Lua.
pub fn install(lua: &Lua, shared: Shared) -> mlua::Result<()> {
    let g = lua.globals();

    // Bac à sable : pas d'accès disque ni système.
    for name in ["dofile", "loadfile", "require", "collectgarbage"] {
        g.set(name, Value::Nil)?;
    }
    g.set("os", Value::Nil)?;
    g.set("io", Value::Nil)?;
    g.set("package", Value::Nil)?;
    g.set("debug", Value::Nil)?;

    // print → journal du jeu + fenêtre de discussion.
    {
        let s = shared.clone();
        g.set(
            "print",
            lua.create_function(move |_, args: Variadic<Value>| {
                let line = args.iter().map(value_to_string).collect::<Vec<_>>().join(" ");
                s.borrow_mut().commands.push(HostCommand::Print(line));
                Ok(())
            })?,
        )?;
    }
    {
        let s = shared.clone();
        g.set(
            "CreateFrame",
            lua.create_function(move |lua, (kind, name, parent, _template): (String, Option<String>, Option<Value>, Option<String>)| {
                let parent_id = parent.as_ref().and_then(|p| match p {
                    Value::UserData(ud) => ud.borrow::<FrameHandle>().ok().map(|f| f.id),
                    Value::String(n) => n.to_str().ok().and_then(|n| s.borrow().ui.by_name(&n)),
                    _ => None,
                });
                let id = s.borrow_mut().ui.create(FrameKind::parse(&kind), name.clone(), parent_id);
                let h = FrameHandle { id, shared: s.clone() };
                let ud = lua.create_userdata(h)?;
                if let Some(n) = name {
                    lua.globals().set(n, ud.clone())?;
                }
                Ok(ud)
            })?,
        )?;
    }
    // UIParent : frame racine nommée, créée d'avance.
    {
        let id = shared.borrow_mut().ui.create(FrameKind::Frame, Some("UIParent".into()), None);
        g.set("UIParent", lua.create_userdata(FrameHandle { id, shared: shared.clone() })?)?;
    }
    {
        let s = shared.clone();
        g.set("GetTime", lua.create_function(move |_, ()| Ok(s.borrow().host.time))?)?;
    }
    {
        let s = shared.clone();
        g.set("GetFramerate", lua.create_function(move |_, ()| Ok(s.borrow().host.framerate))?)?;
    }
    {
        let s = shared.clone();
        g.set("GetRealmName", lua.create_function(move |_, ()| Ok(s.borrow().host.realm.clone()))?)?;
    }
    {
        let s = shared.clone();
        g.set("GetLocale", lua.create_function(move |_, ()| Ok(s.borrow().host.locale.clone()))?)?;
    }
    {
        let s = shared.clone();
        g.set(
            "UnitName",
            lua.create_function(move |_, unit: String| {
                let h = s.borrow();
                Ok(match unit.to_ascii_lowercase().as_str() {
                    "player" => Some(h.host.player_name.clone()),
                    "target" => h.host.target_name.clone(),
                    _ => None,
                })
            })?,
        )?;
    }
    {
        let s = shared.clone();
        g.set(
            "UnitLevel",
            lua.create_function(move |_, unit: String| {
                let h = s.borrow();
                Ok(match unit.to_ascii_lowercase().as_str() {
                    "player" => h.host.player_level,
                    _ => 0,
                })
            })?,
        )?;
    }
    {
        let s = shared.clone();
        g.set("UnitHealth", lua.create_function(move |_, unit: String| Ok(if unit.eq_ignore_ascii_case("player") { s.borrow().host.player_hp } else { 0 }))?)?;
    }
    {
        let s = shared.clone();
        g.set("UnitHealthMax", lua.create_function(move |_, unit: String| Ok(if unit.eq_ignore_ascii_case("player") { s.borrow().host.player_hp_max } else { 0 }))?)?;
    }
    {
        let s = shared.clone();
        g.set("UnitPower", lua.create_function(move |_, unit: String| Ok(if unit.eq_ignore_ascii_case("player") { s.borrow().host.player_mp } else { 0 }))?)?;
    }
    {
        let s = shared.clone();
        g.set("UnitPowerMax", lua.create_function(move |_, unit: String| Ok(if unit.eq_ignore_ascii_case("player") { s.borrow().host.player_mp_max } else { 0 }))?)?;
    }
    {
        let s = shared.clone();
        g.set(
            "GetPlayerMapPosition",
            lua.create_function(move |_, _unit: Option<String>| {
                let p = s.borrow().host.player_position;
                Ok((p[0], p[1], p[2]))
            })?,
        )?;
    }
    {
        let s = shared.clone();
        g.set(
            "SendChatMessage",
            lua.create_function(move |_, (msg, kind, _lang, target): (String, Option<String>, Option<String>, Option<String>)| {
                s.borrow_mut().commands.push(HostCommand::Chat { message: msg, kind: kind.unwrap_or_else(|| "SAY".into()), target });
                Ok(())
            })?,
        )?;
    }
    {
        let s = shared.clone();
        g.set(
            "PlaySoundFile",
            lua.create_function(move |_, (path, _channel): (String, Option<String>)| {
                s.borrow_mut().commands.push(HostCommand::PlaySound(path));
                Ok(())
            })?,
        )?;
    }
    {
        let s = shared.clone();
        g.set(
            "ReloadUI",
            lua.create_function(move |_, ()| {
                s.borrow_mut().commands.push(HostCommand::ReloadUi);
                Ok(())
            })?,
        )?;
    }
    g.set("SlashCmdList", lua.create_table()?)?;
    g.set("GetAddOnMetadata", lua.create_function(|_, (_addon, _field): (String, String)| Ok(Value::Nil))?)?;
    g.set("IsAddOnLoaded", lua.create_function(|_, _addon: String| Ok(true))?)?;
    g.set("InCombatLockdown", lua.create_function(|_, ()| Ok(false))?)?;
    g.set("GetBuildInfo", lua.create_function(|_, ()| Ok(("0.1.0", "1", "2026-10-18", crate::INTERFACE_VERSION)))?)?;
    g.set("geterrorhandler", lua.create_function(|lua, ()| lua.create_function(|_, e: Value| Ok(e)))?)?;
    g.set("seterrorhandler", lua.create_function(|_, _f: Value| Ok(()))?)?;
    g.set("date", lua.create_function(|_, _fmt: Option<String>| Ok("2026-10-18"))?)?;
    g.set(
        "strsplit",
        lua.create_function(|lua, (sep, s, _limit): (String, String, Option<i64>)| {
            let parts: Vec<Value> = s.split(|c| sep.contains(c)).map(|p| lua.create_string(p).map(Value::String)).collect::<mlua::Result<_>>()?;
            Ok(Variadic::from_iter(parts))
        })?,
    )?;
    g.set("strtrim", lua.create_function(|_, s: String| Ok(s.trim().to_string()))?)?;
    g.set("strlower", lua.create_function(|_, s: String| Ok(s.to_lowercase()))?)?;
    g.set("strupper", lua.create_function(|_, s: String| Ok(s.to_uppercase()))?)?;
    g.set("wipe", lua.create_function(|_, t: Table| {
        let keys: Vec<Value> = t.pairs::<Value, Value>().filter_map(|p| p.ok().map(|(k, _)| k)).collect();
        for k in keys {
            t.set(k, Value::Nil)?;
        }
        Ok(t)
    })?)?;
    g.set("tinsert", g.get::<Table>("table")?.get::<Function>("insert")?)?;
    g.set("tremove", g.get::<Table>("table")?.get::<Function>("remove")?)?;
    g.set("format", g.get::<Table>("string")?.get::<Function>("format")?)?;
    g.set("strfind", g.get::<Table>("string")?.get::<Function>("find")?)?;
    g.set("strsub", g.get::<Table>("string")?.get::<Function>("sub")?)?;
    g.set("strlen", g.get::<Table>("string")?.get::<Function>("len")?)?;
    g.set("gsub", g.get::<Table>("string")?.get::<Function>("gsub")?)?;

    // C_Timer.After / NewTimer / NewTicker
    {
        let timer = lua.create_table()?;
        let s = shared.clone();
        timer.set(
            "After",
            lua.create_function(move |lua, (delay, func): (f64, Function)| {
                let key = lua.create_registry_value(func)?;
                let now = s.borrow().host.time;
                s.borrow_mut().timers.push(crate::manager::Timer { due: now + delay, period: None, func: key, cancelled: false });
                Ok(())
            })?,
        )?;
        let s2 = shared.clone();
        timer.set(
            "NewTicker",
            lua.create_function(move |lua, (period, func, _iters): (f64, Function, Option<i64>)| {
                let key = lua.create_registry_value(func)?;
                let (idx, now) = {
                    let st = s2.borrow();
                    (st.timers.len(), st.host.time)
                };
                s2.borrow_mut().timers.push(crate::manager::Timer { due: now + period, period: Some(period), func: key, cancelled: false });
                let handle = lua.create_table()?;
                handle.set("id", idx)?;
                let s3 = s2.clone();
                handle.set(
                    "Cancel",
                    lua.create_function(move |_, h: Table| {
                        let id: usize = h.get("id")?;
                        if let Some(t) = s3.borrow_mut().timers.get_mut(id) {
                            t.cancelled = true;
                        }
                        Ok(())
                    })?,
                )?;
                Ok(handle)
            })?,
        )?;
        timer.set("NewTimer", timer.get::<Function>("NewTicker")?)?;
        g.set("C_Timer", timer)?;
    }
    Ok(())
}
