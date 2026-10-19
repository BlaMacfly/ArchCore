//! Environnement Lua des AddOns : bibliothèques standard sûres, compatibilité Lua 5.1 / WoW, bac à
//! sable.
//!
//! Ouvertes : `string`, `table`, `math`, `utf8`, `coroutine`, et un `os` réduit (`time`, `date`,
//! `clock`, `difftime`). Ajoutées : `bit` (façon LuaBitOp, 32 bits signés, comme sous WoW) et
//! `bit32` (Lua 5.2, non signé), `unpack`, `loadstring`, `getfenv`/`setfenv` (simplifiés),
//! `table.getn`, `math.pow`, `math.mod`, `string.gfind`… plus les alias de WoW (`strsplit`,
//! `strjoin`, `strtrim`, `tinsert`, `wipe`, `format`, `date`, `time`, `floor`, `sin` en degrés…).
//!
//! Interdits : `io`, `package.loadlib`, `os.execute`/`remove`/`rename`/`exit`/`getenv`, `dofile`,
//! `loadfile`, le chargement de bytecode (`load` n'accepte que du texte) et `debug` (seul
//! `debug.traceback` est fourni). `require` ne lit que des modules `.lua` dans les dossiers des
//! AddOns (`require("libs.json")` → `<AddOn>/libs/json.lua`).

use crate::manager::{safe_relative, Shared};
use mlua::{ChunkMode, Function, Lua, LuaOptions, MultiValue, StdLib, Table, Value};
use std::path::PathBuf;

/// Prélude Lua : alias et fonctions utilitaires façon WoW.
pub const PRELUDE: &str = include_str!("lua/prelude.lua");

/// Crée un état Lua avec les seules bibliothèques standard sûres.
pub fn new_state() -> mlua::Result<Lua> {
    Lua::new_with(
        StdLib::TABLE
            | StdLib::STRING
            | StdLib::MATH
            | StdLib::UTF8
            | StdLib::COROUTINE
            | StdLib::OS,
        LuaOptions::default(),
    )
}

/// Installe le bac à sable, `require`, `load`, les bibliothèques de compatibilité et le prélude.
pub fn install(lua: &Lua, shared: Shared) -> mlua::Result<()> {
    let g = lua.globals();

    // os réduit
    let os: Table = g.get("os")?;
    let safe_os = lua.create_table()?;
    for k in ["time", "date", "clock", "difftime"] {
        safe_os.set(k, os.get::<Value>(k)?)?;
    }
    g.set("os", safe_os)?;
    for name in ["dofile", "loadfile", "io", "debug"] {
        g.set(name, Value::Nil)?;
    }

    // load : texte seulement (pas de bytecode), environnement facultatif.
    let raw_load: Function = g.get("load")?;
    let raw_load2 = raw_load.clone();
    g.set(
        "load",
        lua.create_function(move |lua, args: MultiValue| {
            let mut v: Vec<Value> = args.into_iter().collect();
            v.resize(4, Value::Nil);
            v[2] = Value::String(lua.create_string("t")?);
            if matches!(v[3], Value::Nil) {
                v.truncate(3);
            }
            raw_load2.call::<MultiValue>(MultiValue::from_iter(v))
        })?,
    )?;
    {
        let raw = raw_load.clone();
        g.set(
            "loadstring",
            lua.create_function(move |lua, (src, name): (mlua::String, Option<String>)| {
                raw.call::<MultiValue>((
                    src,
                    name.unwrap_or_else(|| "=(loadstring)".into()),
                    lua.create_string("t")?,
                ))
            })?,
        )?;
    }

    // debug réduit
    let debug = lua.create_table()?;
    debug.set(
        "traceback",
        lua.create_function(|lua, (msg, level): (Option<Value>, Option<usize>)| {
            let m = match &msg {
                Some(Value::String(s)) => Some(s.to_str()?.to_string()),
                Some(Value::Nil) | None => None,
                Some(other) => return Ok(other.clone()),
            };
            Ok(Value::String(lua.traceback(m.as_deref(), level.unwrap_or(1))?))
        })?,
    )?;
    g.set("debug", debug)?;
    g.set(
        "debugstack",
        lua.create_function(|lua, (start, _top, _bottom): (Option<usize>, Option<usize>, Option<usize>)| {
            lua.traceback(None, start.unwrap_or(2))
        })?,
    )?;
    {
        let s = shared.clone();
        g.set(
            "debugprofilestop",
            lua.create_function(move |_, ()| Ok(crate::manager::uptime(&s) * 1000.0))?,
        )?;
        let s = shared.clone();
        g.set(
            "GetTimePreciseSec",
            lua.create_function(move |_, ()| Ok(crate::manager::uptime(&s)))?,
        )?;
    }

    // collectgarbage limité aux options inoffensives.
    let raw_gc: Function = g.get("collectgarbage")?;
    g.set(
        "collectgarbage",
        lua.create_function(move |_, (opt, arg): (Option<String>, Option<Value>)| {
            let opt = opt.unwrap_or_else(|| "collect".into());
            match opt.as_str() {
                "collect" | "count" | "step" | "isrunning" => {
                    raw_gc.call::<MultiValue>((opt, arg))
                }
                _ => Err(mlua::Error::RuntimeError(format!(
                    "collectgarbage(\"{opt}\") n'est pas autorisé"
                ))),
            }
        })?,
    )?;

    // Gestionnaire d'erreurs (geterrorhandler / seterrorhandler).
    g.set(
        "seterrorhandler",
        lua.create_function(|lua, f: Value| {
            lua.set_named_registry_value("ARCHRUST_ERRORHANDLER", f)?;
            Ok(())
        })?,
    )?;
    {
        let s = shared.clone();
        g.set(
            "geterrorhandler",
            lua.create_function(move |lua, ()| {
                if let Ok(Value::Function(f)) =
                    lua.named_registry_value::<Value>("ARCHRUST_ERRORHANDLER")
                {
                    return Ok(f);
                }
                let s = s.clone();
                lua.create_function(move |lua, e: Value| {
                    let msg = crate::api::value_to_string(&e);
                    crate::manager::report_error(
                        lua,
                        &s,
                        "erreur",
                        &mlua::Error::RuntimeError(msg.clone()),
                    );
                    Ok(e)
                })
            })?,
        )?;
    }

    // package / require (dossiers des AddOns uniquement).
    let package = lua.create_table()?;
    package.set("loaded", lua.create_table()?)?;
    package.set("preload", lua.create_table()?)?;
    package.set("path", "?.lua;?/init.lua")?;
    package.set("config", "/\n;\n?\n!\n-\n")?;
    g.set("package", package)?;
    {
        let s = shared.clone();
        g.set(
            "require",
            lua.create_function(move |lua, name: String| require(lua, &s, &name))?,
        )?;
    }

    lua.load(PRELUDE)
        .set_name("=prelude")
        .set_mode(ChunkMode::Text)
        .exec()?;
    Ok(())
}

fn require(lua: &Lua, shared: &Shared, name: &str) -> mlua::Result<Value> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    let cached: Value = loaded.get(name)?;
    if !matches!(cached, Value::Nil) {
        return Ok(cached);
    }
    let preload: Table = package.get("preload")?;
    if let Value::Function(f) = preload.get::<Value>(name)? {
        let v: Value = f.call(name)?;
        let v = if matches!(v, Value::Nil) {
            Value::Boolean(true)
        } else {
            v
        };
        loaded.set(name, v.clone())?;
        return Ok(v);
    }
    let rel = name.replace('.', "/");
    let Some(rel) = safe_relative(&rel) else {
        return Err(mlua::Error::RuntimeError(format!(
            "module '{name}' : nom invalide"
        )));
    };
    let (dirs, root) = {
        let s = shared.borrow();
        let mut dirs: Vec<(String, PathBuf)> = Vec::new();
        if let Some(c) = &s.current {
            dirs.push(c.clone());
        }
        for a in &s.addons {
            dirs.push((a.manifest.name.clone(), a.manifest.dir.clone()));
        }
        (dirs, s.addons_root.clone())
    };
    let mut candidates: Vec<(String, PathBuf)> = Vec::new();
    for (addon, dir) in &dirs {
        candidates.push((addon.clone(), dir.join(&rel).with_extension("lua")));
        candidates.push((addon.clone(), dir.join(&rel).join("init.lua")));
    }
    // `require("MonAddOn.module")` depuis un autre AddOn.
    candidates.push((String::new(), root.join(&rel).with_extension("lua")));
    candidates.push((String::new(), root.join(&rel).join("init.lua")));
    for (addon, path) in candidates {
        if !path.is_file() {
            continue;
        }
        let src = std::fs::read(&path)
            .map_err(|e| mlua::Error::RuntimeError(format!("{} : {e}", path.display())))?;
        let chunk = format!(
            "@{}{}",
            if addon.is_empty() { String::new() } else { format!("{addon}/") },
            rel.display().to_string().replace('\\', "/")
        );
        let v: Value = lua
            .load(&src)
            .set_name(chunk)
            .set_mode(ChunkMode::Text)
            .call((name.to_string(), path.display().to_string()))?;
        let v = if matches!(v, Value::Nil) {
            Value::Boolean(true)
        } else {
            v
        };
        loaded.set(name, v.clone())?;
        return Ok(v);
    }
    Err(mlua::Error::RuntimeError(format!(
        "module '{name}' introuvable (cherché dans les dossiers des AddOns)"
    )))
}

#[cfg(test)]
mod tests {
    use crate::AddonManager;

    #[test]
    fn stdlib_and_wow_aliases() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = AddonManager::new(tmp.path()).unwrap();
        let checks = [
            ("type(string.format)", "function"),
            ("type(table.concat)", "function"),
            ("type(math.floor)", "function"),
            ("type(utf8.char)", "function"),
            ("type(coroutine.wrap)", "function"),
            ("type(os.time)", "function"),
            ("type(os.execute)", "nil"),
            ("type(os.remove)", "nil"),
            ("type(io)", "nil"),
            ("type(dofile)", "nil"),
            ("type(loadfile)", "nil"),
            ("select('#', strsplit(',', 'a,b,,c'))", "4"),
            ("select(3, strsplit(' ', 'a b c d', 3))", "c d"),
            ("strjoin('-', 'a', 'b', 'c')", "a-b-c"),
            ("strtrim('  x  ')", "x"),
            ("strtrim('xxhixx', 'x')", "hi"),
            ("format('%d-%s', 4, 'z')", "4-z"),
            ("format('%d ips', 59.7)", "59 ips"),
            ("('%d|%x|%5.1f'):format(-2.5, 255.9, 1.25)", "-2|ff|  1.2"),
            ("format('%u', 7)", "7"),
            ("pcall(format, '%d', {})", "false"),
            ("(function() local t = {1,2} tinsert(t, 3) tremove(t, 1) return table.concat(t, ',') end)()", "2,3"),
            ("(function() local t = {a=1, 2} wipe(t) return next(t) == nil end)()", "true"),
            ("bit.band(0xFF, 0x0F)", "15"),
            ("bit.bor(1, 2, 4)", "7"),
            ("bit.lshift(1, 4)", "16"),
            ("bit.bnot(0)", "-1"),
            ("bit32.bnot(0)", "4294967295"),
            ("bit.tohex(255, 4)", "00ff"),
            ("unpack({1, 2, 3})", "1"),
            ("select('#', unpack({1, 2, 3}))", "3"),
            ("loadstring('return 1 + 1')()", "2"),
            ("tostring(load(string.dump(function() end)))", "nil"),
            ("floor(2.7) + ceil(2.2)", "5"),
            ("math.pow(2, 10)", "1024.0"),
            ("string.format('%.1f', sin(90))", "1.0"),
            ("type(date('%Y'))", "string"),
            ("type(time())", "number"),
            ("tContains({'a','b'}, 'b')", "true"),
            ("CopyTable({x={y=1}}).x.y", "1"),
            ("CreateColor(1, 0, 0):GenerateHexColor()", "ffff0000"),
            ("WrapTextInColorCode('a', 'ff00ff00')", "|cff00ff00a|r"),
            ("(function() local t = {f = function(a) return a end}; local n = 0; hooksecurefunc(t, 'f', function() n = n + 1 end); return t.f(5) + n end)()", "6"),
            ("Mixin({}, {a = 1}).a", "1"),
            ("strlenutf8('été')", "3"),
            ("securecall(function(a) return a * 2 end, 21)", "42"),
            ("type(debugprofilestop())", "number"),
            ("type(debug.traceback('x'))", "string"),
            ("collectgarbage('count') > 0", "true"),
            ("pcall(collectgarbage, 'stop')", "false"),
            ("table.getn({1,2,3})", "3"),
            ("string.gfind == string.gmatch", "true"),
            ("tostringall(1, nil)", "1"),
        ];
        for (expr, want) in checks {
            let got = mgr
                .eval_string(expr)
                .unwrap_or_else(|e| panic!("{expr} : {e}"));
            assert_eq!(got, want, "{expr}");
        }
    }

    #[test]
    fn require_stays_inside_addons() {
        let tmp = tempfile::tempdir().unwrap();
        let addons = tmp.path().join("addons");
        let dir = addons.join("Req");
        std::fs::create_dir_all(dir.join("libs")).unwrap();
        std::fs::write(dir.join("Req.toc"), "core.lua\n").unwrap();
        std::fs::write(
            dir.join("libs/json.lua"),
            "local M = {} function M.encode(t) return '[' .. table.concat(t, ',') .. ']' end return M",
        )
        .unwrap();
        std::fs::write(
            dir.join("core.lua"),
            "local json = require('libs.json'); ReqOut = json.encode({1,2}); ReqSame = require('libs.json') == json\n\
             ReqEscape = pcall(require, '..secret')\n\
             ReqAce = require('AceAddon-3.0') == LibStub('AceAddon-3.0')",
        )
        .unwrap();
        std::fs::write(tmp.path().join("secret.lua"), "return 'fuite'").unwrap();
        let mut mgr = AddonManager::new(tmp.path().join("sv")).unwrap();
        mgr.load_dir(&addons).unwrap();
        assert!(mgr.errors().is_empty(), "{:?}", mgr.errors());
        assert_eq!(mgr.eval_string("ReqOut").unwrap(), "[1,2]");
        assert_eq!(mgr.eval_string("tostring(ReqSame)").unwrap(), "true");
        assert_eq!(mgr.eval_string("tostring(ReqEscape)").unwrap(), "false");
        assert_eq!(mgr.eval_string("tostring(ReqAce)").unwrap(), "true");
    }
}
