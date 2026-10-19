//! Objets `Frame` exposés à Lua.
//!
//! Comme sous WoW, une frame est une **table** Lua dont la clé `[0]` contient la poignée native
//! ([`FrameHandle`]) et dont la métatable partagée a pour `__index` la table des méthodes. Un
//! AddOn peut donc :
//! - ranger ses propres champs dans une frame (`frame.label = fs`) ;
//! - surcharger une méthode sur une frame (`frame.SetText = …`) ou `hooksecurefunc(frame, "Show", …)` ;
//! - ajouter des méthodes à **toutes** les frames via `getmetatable(frame).__index`
//!   (aussi exposée sous `ArchRust.FrameMethods`).
//!
//! La même table est toujours rendue pour une frame donnée (cache dans le registre), ce qui permet
//! de s'en servir comme clé de table et de la comparer avec `==`.

use crate::manager::Shared;
use crate::ui::{Anchor, Backdrop, FrameKind, FrameNode, TextureSource};
use mlua::{
    FromLuaMulti, Function, IntoLuaMulti, Lua, MultiValue, Table, UserData, UserDataMethods, Value,
    Variadic,
};

/// Poignée native sur une frame (rangée en `frame[0]`).
#[derive(Clone)]
pub struct FrameHandle {
    pub id: usize,
    pub shared: Shared,
}

impl UserData for FrameHandle {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
            Ok(format!("FrameHandle<{}>", this.id))
        });
    }
}

impl FrameHandle {
    /// Modifie le nœud (marque l'arbre comme modifié).
    pub fn with<R>(&self, f: impl FnOnce(&mut FrameNode) -> R) -> Option<R> {
        self.shared.borrow_mut().ui.get_mut(self.id).map(f)
    }
    /// Lit le nœud.
    pub fn read<R>(&self, f: impl FnOnce(&FrameNode) -> R) -> Option<R> {
        self.shared.borrow().ui.get(self.id).map(f)
    }
}

/// Table du registre : `SCRIPTS[id][handler] = fonction`.
pub const SCRIPTS_KEY: &str = "ARCHRUST_SCRIPTS";
/// Table du registre : `FRAMES[id] = table de la frame`.
pub const FRAMES_KEY: &str = "ARCHRUST_FRAMES";
/// Métatable partagée des frames (`__index` = méthodes).
pub const META_KEY: &str = "ARCHRUST_FRAME_META";

fn registry_table(lua: &Lua, key: &str) -> mlua::Result<Table> {
    match lua.named_registry_value::<Value>(key)? {
        Value::Table(t) => Ok(t),
        _ => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(key, t.clone())?;
            Ok(t)
        }
    }
}

pub fn scripts_table(lua: &Lua) -> mlua::Result<Table> {
    registry_table(lua, SCRIPTS_KEY)
}

/// Fonction de script d'une frame, si définie.
pub fn get_script(lua: &Lua, id: usize, handler: &str) -> mlua::Result<Option<Function>> {
    let scripts = scripts_table(lua)?;
    let Value::Table(per_frame) = scripts.get::<Value>(id)? else {
        return Ok(None);
    };
    match per_frame.get::<Value>(handler)? {
        Value::Function(f) => Ok(Some(f)),
        _ => Ok(None),
    }
}

/// Table Lua (unique) d'une frame.
pub fn frame_object(lua: &Lua, shared: &Shared, id: usize) -> mlua::Result<Table> {
    let frames = registry_table(lua, FRAMES_KEY)?;
    if let Value::Table(t) = frames.raw_get::<Value>(id)? {
        return Ok(t);
    }
    let t = lua.create_table()?;
    t.raw_set(
        0,
        lua.create_userdata(FrameHandle {
            id,
            shared: shared.clone(),
        })?,
    )?;
    let meta: Table = lua.named_registry_value(META_KEY)?;
    t.set_metatable(Some(meta))?;
    frames.raw_set(id, t.clone())?;
    Ok(t)
}

/// Poignée native d'une valeur Lua (table de frame ou poignée elle-même).
pub fn handle(v: &Value) -> Option<FrameHandle> {
    match v {
        Value::Table(t) => match t.raw_get::<Value>(0).ok()? {
            Value::UserData(ud) => ud.borrow::<FrameHandle>().ok().map(|h| h.clone()),
            _ => None,
        },
        Value::UserData(ud) => ud.borrow::<FrameHandle>().ok().map(|h| h.clone()),
        _ => None,
    }
}

pub fn frame_id(v: &Value) -> Option<usize> {
    handle(v).map(|h| h.id)
}

/// Appelle un script s'il existe : `handler(self, ...)`. Les erreurs sont journalisées.
pub fn call_script(
    lua: &Lua,
    shared: &Shared,
    id: usize,
    handler: &str,
    args: impl IntoLuaMulti,
) -> bool {
    let Ok(Some(f)) = get_script(lua, id, handler) else {
        return false;
    };
    let result = (|| -> mlua::Result<()> {
        let me = frame_object(lua, shared, id)?;
        let mut all = args.into_lua_multi(lua)?;
        all.push_front(Value::Table(me));
        f.call::<()>(all)
    })();
    if let Err(e) = result {
        crate::manager::report_error(lua, shared, &format!("{handler} (frame {id})"), &e);
    }
    true
}

/// Nombre rendu à Lua : entier quand la valeur est entière, pour que `"x" .. v` affiche « 8 » et
/// non « 8.0 » (comportement de Lua 5.1, donc de WoW).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Num(pub f64);

impl mlua::IntoLua for Num {
    fn into_lua(self, _: &Lua) -> mlua::Result<Value> {
        Ok(if self.0.fract() == 0.0 && self.0.abs() < 9.0e15 {
            Value::Integer(self.0 as i64)
        } else {
            Value::Number(self.0)
        })
    }
}

pub fn num(v: f32) -> Num {
    Num(v as f64)
}

/// Lit une couleur `(r, g, b [, a])` ou une table `{r=, g=, b=, a=}` / `ColorMixin`.
pub fn color_args(args: &[Value], default_alpha: f32) -> [f32; 4] {
    let num = |v: Option<&Value>| match v {
        Some(Value::Integer(i)) => Some(*i as f32),
        Some(Value::Number(n)) => Some(*n as f32),
        _ => None,
    };
    if let Some(Value::Table(t)) = args.first() {
        let g = |k: &str| t.get::<Option<f32>>(k).ok().flatten();
        return [
            g("r").unwrap_or(1.0),
            g("g").unwrap_or(1.0),
            g("b").unwrap_or(1.0),
            g("a").unwrap_or(default_alpha),
        ];
    }
    [
        num(args.first()).unwrap_or(1.0),
        num(args.get(1)).unwrap_or(1.0),
        num(args.get(2)).unwrap_or(1.0),
        num(args.get(3)).unwrap_or(default_alpha),
    ]
}

fn value_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => s.to_str().ok().map(|s| s.to_string()),
        Value::Integer(i) => Some(i.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Crée une frame (ou une région) et rend sa table. `name` peut contenir `$parent`.
pub fn create_frame(
    lua: &Lua,
    shared: &Shared,
    kind: FrameKind,
    name: Option<String>,
    parent: Option<usize>,
    templates: Option<&str>,
) -> mlua::Result<Table> {
    let name = name.map(|n| {
        if n.contains("$parent") || n.contains("$Parent") {
            let pname = parent
                .and_then(|p| shared.borrow().ui.get(p).and_then(|f| f.name.clone()))
                .unwrap_or_default();
            n.replace("$parent", &pname).replace("$Parent", &pname)
        } else {
            n
        }
    });
    let id = shared.borrow_mut().ui.create(kind, name.clone(), parent);
    let t = frame_object(lua, shared, id)?;
    if let Some(n) = &name {
        lua.globals().set(n.as_str(), t.clone())?;
    }
    // Modèles déclarés en Lua : `ArchRust.Templates[nom](frame)`.
    if let Some(list) = templates {
        let reg: Option<Table> = lua
            .globals()
            .get::<Option<Table>>("ArchRust")?
            .and_then(|a| a.get::<Option<Table>>("Templates").ok().flatten());
        if let Some(reg) = reg {
            for tpl in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if let Ok(Value::Function(f)) = reg.get::<Value>(tpl) {
                    f.call::<()>(t.clone())?;
                }
            }
        }
    }
    Ok(t)
}

/// Table des méthodes en construction.
struct Methods<'a> {
    lua: &'a Lua,
    t: Table,
}

impl Methods<'_> {
    fn add<A, R, F>(&self, name: &'static str, f: F) -> mlua::Result<()>
    where
        A: FromLuaMulti,
        R: IntoLuaMulti,
        F: Fn(&Lua, &FrameHandle, A) -> mlua::Result<R> + 'static,
    {
        let func = self.lua.create_function(move |lua, (this, a): (Value, A)| {
            let h = handle(&this).ok_or_else(|| {
                mlua::Error::RuntimeError(format!(
                    "{name} : appel sur un objet qui n'est pas une frame (utiliser « : »)"
                ))
            })?;
            f(lua, &h, a)
        })?;
        self.t.set(name, func)
    }

    /// Méthode sans effet (compatibilité).
    fn noop(&self, name: &'static str) -> mlua::Result<()> {
        self.t
            .set(name, self.lua.create_function(|_, _: MultiValue| Ok(()))?)
    }

    fn alias(&self, name: &'static str, of: &'static str) -> mlua::Result<()> {
        let f: Function = self.t.get(of)?;
        self.t.set(name, f)
    }
}

fn set_visible(lua: &Lua, h: &FrameHandle, shown: bool) {
    let was = h.read(|f| f.shown).unwrap_or(shown);
    if was == shown {
        return;
    }
    let was_visible = h.shared.borrow().ui.is_visible(h.id);
    h.with(|f| f.shown = shown);
    let visible_now = h.shared.borrow().ui.is_visible(h.id);
    if was_visible == visible_now {
        return;
    }
    // Comme sous WoW, OnShow/OnHide concernent aussi les descendants affichés dont la
    // visibilité change avec celle de la frame.
    let mut ids = vec![h.id];
    {
        let s = h.shared.borrow();
        let mut i = 0;
        while i < ids.len() {
            if let Some(f) = s.ui.get(ids[i]) {
                for &c in &f.children {
                    if s.ui.get(c).is_some_and(|n| n.shown) {
                        ids.push(c);
                    }
                }
            }
            i += 1;
        }
    }
    let handler = if visible_now { "OnShow" } else { "OnHide" };
    for id in ids {
        call_script(lua, &h.shared, id, handler, ());
    }
}

fn set_value(lua: &Lua, h: &FrameHandle, v: f32, user: bool) {
    let changed = h
        .with(|f| {
            let mut v = v.clamp(f.min_value.min(f.max_value), f.max_value.max(f.min_value));
            if f.value_step > 0.0 {
                v = f.min_value + ((v - f.min_value) / f.value_step).round() * f.value_step;
            }
            let changed = (f.value - v).abs() > f32::EPSILON;
            f.value = v;
            changed.then_some(v)
        })
        .flatten();
    if let Some(v) = changed {
        call_script(lua, &h.shared, h.id, "OnValueChanged", (num(v), user));
    }
}

/// Fixe le texte d'une EditBox/FontString/Button, avec `OnTextChanged` pour les EditBox.
pub fn set_text(lua: &Lua, h: &FrameHandle, text: String, user: bool) {
    // Bouton avec FontString créée par `GetFontString` : le texte va à la FontString.
    if let Some(c) = h.read(|f| f.font_string).flatten() {
        if let Some(n) = h.shared.borrow_mut().ui.get_mut(c) {
            n.text = text;
            n.measured = None;
        }
        return;
    }
    let (kind, changed) = h
        .with(|f| {
            let changed = f.text != text;
            f.text = text;
            f.measured = None;
            (f.kind, changed)
        })
        .unwrap_or((FrameKind::Frame, false));
    if kind == FrameKind::EditBox && changed {
        call_script(lua, &h.shared, h.id, "OnTextChanged", user);
    }
}

fn resolve_texture(h: &FrameHandle, path: &str) -> Option<TextureSource> {
    crate::manager::resolve_media(&h.shared.borrow(), path)
}

/// Installe la métatable partagée et la table des méthodes ; rend la table des méthodes.
pub fn install(lua: &Lua) -> mlua::Result<Table> {
    let m = Methods {
        lua,
        t: lua.create_table()?,
    };

    // Identité
    m.add("GetName", |_, h, ()| Ok(h.read(|f| f.name.clone()).flatten()))?;
    m.add("GetDebugName", |_, h, ()| {
        Ok(h.read(|f| f.name.clone().unwrap_or_else(|| format!("frame{}", f.id))))
    })?;
    m.add("GetObjectType", |_, h, ()| {
        Ok(h.read(|f| f.kind.name()).unwrap_or("Frame"))
    })?;
    m.add("IsObjectType", |_, h, t: String| {
        let kind = h.read(|f| f.kind).unwrap_or(FrameKind::Frame);
        let t = t.to_ascii_lowercase();
        Ok(t == kind.name().to_ascii_lowercase()
            || (t == "frame" && !kind.is_region())
            || t == "region" && kind.is_region()
            || t == "button" && kind == FrameKind::CheckButton
            || t == "object"
            || t == "scriptobject")
    })?;
    m.t.set("IsForbidden", lua.create_function(|_, _: MultiValue| Ok(false))?)?;
    m.t.set("IsProtected", lua.create_function(|_, _: MultiValue| Ok(false))?)?;
    m.t.set(
        "CanChangeProtectedState",
        lua.create_function(|_, _: MultiValue| Ok(true))?,
    )?;
    m.add("GetParent", |lua, h, ()| {
        match h.read(|f| f.parent).flatten() {
            Some(p) => Ok(Value::Table(frame_object(lua, &h.shared, p)?)),
            None => Ok(Value::Nil),
        }
    })?;
    m.add("SetParent", |lua, h, parent: Value| {
        let p = match &parent {
            Value::String(s) => h.shared.borrow().ui.by_name(&s.to_str()?),
            v => frame_id(v),
        };
        h.shared.borrow_mut().ui.set_parent(h.id, p);
        let _ = lua;
        Ok(())
    })?;
    m.add("SetID", |_, h, i: i64| {
        h.with(|f| f.id_number = i);
        Ok(())
    })?;
    m.add("GetID", |_, h, ()| Ok(h.read(|f| f.id_number).unwrap_or(0)))?;

    // Visibilité
    m.add("Show", |lua, h, ()| {
        set_visible(lua, h, true);
        Ok(())
    })?;
    m.add("Hide", |lua, h, ()| {
        set_visible(lua, h, false);
        Ok(())
    })?;
    m.add("SetShown", |lua, h, shown: Value| {
        set_visible(lua, h, !matches!(shown, Value::Nil | Value::Boolean(false)));
        Ok(())
    })?;
    m.add("IsShown", |_, h, ()| Ok(h.read(|f| f.shown).unwrap_or(false)))?;
    m.add("IsVisible", |_, h, ()| {
        Ok(h.shared.borrow().ui.is_visible(h.id))
    })?;

    // Géométrie
    m.add("SetSize", |_, h, (w, hh): (f32, Option<f32>)| {
        h.with(|f| {
            f.width = w;
            f.height = hh.unwrap_or(w);
        });
        Ok(())
    })?;
    m.add("SetWidth", |_, h, w: f32| {
        h.with(|f| f.width = w);
        Ok(())
    })?;
    m.add("SetHeight", |_, h, v: f32| {
        h.with(|f| f.height = v);
        Ok(())
    })?;
    fn rect(h: &FrameHandle) -> Option<crate::ui::Rect> {
        let s = h.shared.borrow();
        s.ui.rects.get(h.id).copied().flatten()
    }
    fn size(h: &FrameHandle) -> (f32, f32) {
        let (w, hh, scale) = h.read(|f| (f.width, f.height, f.scale)).unwrap_or_default();
        let eff = h.shared.borrow().ui.effective_scale(h.id).max(0.0001);
        match rect(h) {
            Some(r) if w == 0.0 || hh == 0.0 => (
                if w == 0.0 { r.width / eff } else { w },
                if hh == 0.0 { r.height / eff } else { hh },
            ),
            _ => {
                let _ = scale;
                (w, hh)
            }
        }
    }
    m.add("GetWidth", |_, h, ()| Ok(num(size(h).0)))?;
    m.add("GetHeight", |_, h, ()| Ok(num(size(h).1)))?;
    m.add("GetSize", |_, h, ()| {
        let s = size(h);
        Ok((num(s.0), num(s.1)))
    })?;
    m.add("GetRect", |_, h, ()| {
        let r = rect(h);
        Ok((r.map(|r| r.left), r.map(|r| r.bottom), r.map(|r| r.width), r.map(|r| r.height)))
    })?;
    m.add("GetLeft", |_, h, ()| Ok(rect(h).map(|r| num(r.left))))?;
    m.add("GetRight", |_, h, ()| Ok(rect(h).map(|r| num(r.left + r.width))))?;
    m.add("GetBottom", |_, h, ()| Ok(rect(h).map(|r| num(r.bottom))))?;
    m.add("GetTop", |_, h, ()| Ok(rect(h).map(|r| num(r.bottom + r.height))))?;
    m.add("GetCenter", |_, h, ()| {
        let r = rect(h);
        Ok((r.map(|r| r.left + r.width / 2.0), r.map(|r| r.bottom + r.height / 2.0)))
    })?;
    m.add("SetScale", |_, h, s: f32| {
        h.with(|f| f.scale = if s > 0.0 { s } else { 1.0 });
        Ok(())
    })?;
    m.add("GetScale", |_, h, ()| Ok(h.read(|f| f.scale).unwrap_or(1.0)))?;
    m.add("GetEffectiveScale", |_, h, ()| {
        Ok(h.shared.borrow().ui.effective_scale(h.id))
    })?;
    // SetPoint(point [, relativeTo] [, relativePoint] [, x, y])
    m.add("SetPoint", |_, h, args: Variadic<Value>| {
        let mut it = args.into_iter();
        let point = match it.next() {
            Some(Value::String(s)) => s.to_str()?.to_ascii_uppercase(),
            _ => return Err(mlua::Error::RuntimeError("SetPoint : point attendu".into())),
        };
        let mut relative_to = None;
        let mut relative_point = point.clone();
        let mut nums: Vec<f32> = Vec::new();
        for v in it {
            match &v {
                Value::Table(_) | Value::UserData(_) => relative_to = frame_id(&v),
                Value::String(s) => {
                    let s = s.to_str()?.to_string();
                    if nums.is_empty() {
                        // Un nom de frame ou un point relatif.
                        let by_name = h.shared.borrow().ui.by_name(&s);
                        match by_name {
                            Some(id) if relative_to.is_none() && !is_point(&s) => {
                                relative_to = Some(id)
                            }
                            _ => relative_point = s.to_ascii_uppercase(),
                        }
                    }
                }
                Value::Integer(i) => nums.push(*i as f32),
                Value::Number(n) => nums.push(*n as f32),
                _ => {}
            }
        }
        if relative_to == Some(h.id) {
            relative_to = None;
        }
        let anchor = Anchor {
            point,
            relative_to,
            relative_point,
            x: nums.first().copied().unwrap_or(0.0),
            y: nums.get(1).copied().unwrap_or(0.0),
        };
        h.with(|f| {
            f.anchors.retain(|a| a.point != anchor.point);
            f.anchors.push(anchor);
        });
        Ok(())
    })?;
    m.add("ClearAllPoints", |_, h, ()| {
        h.with(|f| f.anchors.clear());
        Ok(())
    })?;
    m.add("ClearPoint", |_, h, p: String| {
        let p = p.to_ascii_uppercase();
        h.with(|f| f.anchors.retain(|a| a.point != p));
        Ok(())
    })?;
    m.add("SetAllPoints", |_, h, rel: Option<Value>| {
        let relative_to = rel.as_ref().and_then(frame_id).filter(|&r| r != h.id);
        h.with(|f| {
            f.anchors = vec![
                Anchor {
                    point: "TOPLEFT".into(),
                    relative_to,
                    relative_point: "TOPLEFT".into(),
                    x: 0.0,
                    y: 0.0,
                },
                Anchor {
                    point: "BOTTOMRIGHT".into(),
                    relative_to,
                    relative_point: "BOTTOMRIGHT".into(),
                    x: 0.0,
                    y: 0.0,
                },
            ];
        });
        Ok(())
    })?;
    m.add("GetNumPoints", |_, h, ()| {
        Ok(h.read(|f| f.anchors.len()).unwrap_or(0))
    })?;
    m.add("GetPoint", |lua, h, idx: Option<usize>| {
        let i = idx.unwrap_or(1).max(1) - 1;
        let Some(a) = h.read(|f| f.anchors.get(i).cloned()).flatten() else {
            return Ok(Variadic::new());
        };
        let rel = match a.relative_to {
            Some(r) => Value::Table(frame_object(lua, &h.shared, r)?),
            None => match h.read(|f| f.parent).flatten() {
                Some(p) => Value::Table(frame_object(lua, &h.shared, p)?),
                None => Value::Nil,
            },
        };
        Ok(Variadic::from_iter([
            Value::String(lua.create_string(&a.point)?),
            rel,
            Value::String(lua.create_string(&a.relative_point)?),
            Value::Number(a.x as f64),
            Value::Number(a.y as f64),
        ]))
    })?;
    m.add("AdjustPointsOffset", |_, h, (dx, dy): (f32, f32)| {
        h.with(|f| {
            for a in &mut f.anchors {
                a.x += dx;
                a.y += dy;
            }
        });
        Ok(())
    })?;

    // Strates, niveaux, alpha
    m.add("SetAlpha", |_, h, a: f32| {
        h.with(|f| f.alpha = a.clamp(0.0, 1.0));
        Ok(())
    })?;
    m.add("GetAlpha", |_, h, ()| Ok(h.read(|f| f.alpha).unwrap_or(1.0)))?;
    m.add("GetEffectiveAlpha", |_, h, ()| {
        Ok(h.shared.borrow().ui.effective_alpha(h.id))
    })?;
    m.add("SetFrameStrata", |_, h, s: String| {
        h.with(|f| f.strata = Some(s.to_ascii_uppercase()));
        Ok(())
    })?;
    m.add("GetFrameStrata", |_, h, ()| {
        Ok(h.shared.borrow().ui.effective_strata(h.id).to_string())
    })?;
    m.add("SetFrameLevel", |_, h, l: i32| {
        h.with(|f| f.level = Some(l));
        Ok(())
    })?;
    m.add("GetFrameLevel", |_, h, ()| {
        Ok(h.shared.borrow().ui.effective_level(h.id))
    })?;
    m.add("Raise", |_, h, ()| {
        let top = {
            let s = h.shared.borrow();
            let strata = s.ui.effective_strata(h.id).to_string();
            s.ui.frames
                .iter()
                .filter(|f| s.ui.effective_strata(f.id) == strata)
                .map(|f| s.ui.effective_level(f.id))
                .max()
                .unwrap_or(0)
        };
        h.with(|f| f.level = Some(top + 1));
        Ok(())
    })?;
    m.add("Lower", |_, h, ()| {
        h.with(|f| f.level = Some(0));
        Ok(())
    })?;
    m.noop("SetToplevel")?;
    m.add("SetDrawLayer", |_, h, (layer, _sub): (String, Option<i32>)| {
        h.with(|f| f.layer = layer.to_ascii_uppercase());
        Ok(())
    })?;
    m.add("GetDrawLayer", |_, h, ()| {
        Ok(h.read(|f| f.layer.clone()).unwrap_or_default())
    })?;

    // Fond et bordure
    m.add("SetBackdrop", |_, h, t: Option<Table>| {
        let bd = match t {
            None => None,
            Some(t) => {
                let bg_file: Option<String> = t.get("bgFile").ok().flatten();
                let edge: Option<String> = t.get("edgeFile").ok().flatten();
                let edge_size: f32 = t.get::<Option<f32>>("edgeSize")?.unwrap_or(0.0);
                let bg_texture = bg_file.as_deref().and_then(|p| resolve_texture(h, p));
                Some(Backdrop {
                    bg: [1.0, 1.0, 1.0, if bg_file.is_some() { 1.0 } else { 0.0 }],
                    border: [1.0, 1.0, 1.0, if edge.is_some() { 1.0 } else { 0.0 }],
                    edge_size: if edge.is_some() { edge_size.max(1.0) } else { 0.0 },
                    bg_texture,
                })
            }
        };
        h.with(|f| f.backdrop = bd);
        Ok(())
    })?;
    m.add("SetBackdropColor", |_, h, c: Variadic<Value>| {
        let c = color_args(&c, 1.0);
        h.with(|f| f.backdrop.get_or_insert_with(Backdrop::default).bg = c);
        Ok(())
    })?;
    m.add("SetBackdropBorderColor", |_, h, c: Variadic<Value>| {
        let c = color_args(&c, 1.0);
        h.with(|f| {
            let b = f.backdrop.get_or_insert_with(Backdrop::default);
            b.border = c;
            if b.edge_size == 0.0 {
                b.edge_size = 1.0;
            }
        });
        Ok(())
    })?;
    m.add("GetBackdropColor", |_, h, ()| {
        let c = h.read(|f| f.background()).unwrap_or_default();
        Ok((c[0], c[1], c[2], c[3]))
    })?;
    m.add("GetBackdropBorderColor", |_, h, ()| {
        let c = h
            .read(|f| f.backdrop.as_ref().map(|b| b.border).unwrap_or_default())
            .unwrap_or_default();
        Ok((c[0], c[1], c[2], c[3]))
    })?;
    m.noop("ApplyBackdrop")?;
    m.noop("OnBackdropLoaded")?;

    // Couleurs et textures
    for name in [
        "SetVertexColor",
        "SetTextColor",
        "SetStatusBarColor",
    ] {
        m.add(name, |_, h, c: Variadic<Value>| {
            let c = color_args(&c, 1.0);
            h.with(|f| {
                f.color = c;
            });
            Ok(())
        })?;
    }
    m.add("SetColorTexture", |_, h, c: Variadic<Value>| {
        let c = color_args(&c, 1.0);
        h.with(|f| {
            f.color = c;
            f.texture = None;
            f.texture_source = None;
            f.solid = true;
        });
        Ok(())
    })?;
    for name in ["GetVertexColor", "GetTextColor", "GetStatusBarColor"] {
        m.add(name, |_, h, ()| {
            let c = h.read(|f| f.color).unwrap_or_default();
            Ok((c[0], c[1], c[2], c[3]))
        })?;
    }
    m.add("SetTexture", |_, h, (path, _wrap): (Value, MultiValue)| {
        let path = value_str(&path);
        let src = path.as_deref().and_then(|p| resolve_texture(h, p));
        h.with(|f| {
            // Texture WoW sans équivalent (ex. `InterfaceButtonsWHITE8X8`) : couleur unie.
            f.solid = path.is_some() && src.is_none();
            f.texture = path;
            f.texture_source = src;
        });
        Ok(())
    })?;
    m.alias("SetAtlas", "SetTexture")?;
    m.add("GetTexture", |_, h, ()| Ok(h.read(|f| f.texture.clone()).flatten()))?;
    m.add("SetTexCoord", |_, h, c: Variadic<f32>| {
        let tc = match c.len() {
            4 => [c[0], c[1], c[2], c[3]],
            // Huit valeurs : coins (UL, LL, UR, LR) → boîte englobante.
            8 => [
                c[0].min(c[2]),
                c[4].max(c[6]),
                c[1].min(c[5]),
                c[3].max(c[7]),
            ],
            _ => [0.0, 1.0, 0.0, 1.0],
        };
        h.with(|f| f.tex_coord = tc);
        Ok(())
    })?;
    m.add("GetTexCoord", |_, h, ()| {
        let c = h.read(|f| f.tex_coord).unwrap_or([0.0, 1.0, 0.0, 1.0]);
        Ok((c[0], c[2], c[0], c[3], c[1], c[2], c[1], c[3]))
    })?;
    for name in [
        "SetDesaturated",
        "SetBlendMode",
        "SetRotation",
        "SetGradient",
        "SetHorizTile",
        "SetVertTile",
        "SetSnapToPixelGrid",
        "SetTexelSnappingBias",
        "AddMaskTexture",
        "SetMask",
    ] {
        m.noop(name)?;
    }

    // Texte et polices
    m.add("SetText", |lua, h, text: Value| {
        set_text(lua, h, value_str(&text).unwrap_or_default(), false);
        Ok(())
    })?;
    m.add("GetText", |_, h, ()| {
        let s = h.shared.borrow();
        let id = s.ui.get(h.id).and_then(|f| f.font_string).unwrap_or(h.id);
        Ok(s.ui.get(id).map(|n| n.text.clone()).unwrap_or_default())
    })?;
    m.add("SetFormattedText", |lua, h, args: MultiValue| {
        let g = lua.globals();
        let string: Table = g.get("string")?;
        let format: Function = string.get("format")?;
        let text: String = format.call(args)?;
        set_text(lua, h, text, false);
        Ok(())
    })?;
    m.add(
        "SetFont",
        |_, h, (path, size, flags): (Option<String>, Option<f32>, Option<String>)| {
            let font = path.as_deref().and_then(|p| match resolve_texture(h, p) {
                Some(TextureSource::File(f)) => Some(f),
                _ => None,
            });
            h.with(|f| {
                f.font_path = font;
                if let Some(size) = size {
                    f.font_size = size.max(1.0);
                }
                f.font_flags = flags.unwrap_or_default().to_ascii_uppercase();
                f.measured = None;
            });
            Ok(true)
        },
    )?;
    m.add("GetFont", |_, h, ()| {
        Ok(h.read(|f| {
            (
                f.font_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "Fonts\\FRIZQT__.TTF".into()),
                f.font_size,
                f.font_flags.clone(),
            )
        }).unwrap_or_default())
    })?;
    m.add("SetTextHeight", |_, h, size: f32| {
        h.with(|f| {
            f.font_size = size.max(1.0);
            f.measured = None;
        });
        Ok(())
    })?;
    // Objet police (`CreateFont`, `GameFontNormal`…) : table avec `_size`, `_path`, `_flags`,
    // `_r/_g/_b/_a`, `_justifyH`.
    m.add("SetFontObject", |lua, h, o: Value| {
        let t = match o {
            Value::Table(t) => Some(t),
            Value::String(s) => lua.globals().get::<Option<Table>>(s.to_str()?.to_string())?,
            _ => None,
        };
        if let Some(t) = t {
            let size: Option<f32> = t.get("_size")?;
            let path: Option<String> = t.get("_path")?;
            let flags: Option<String> = t.get("_flags")?;
            let r: Option<f32> = t.get("_r")?;
            let justify: Option<String> = t.get("_justifyH")?;
            let color = r.map(|r| -> mlua::Result<[f32; 4]> {
                Ok([
                    r,
                    t.get::<Option<f32>>("_g")?.unwrap_or(1.0),
                    t.get::<Option<f32>>("_b")?.unwrap_or(1.0),
                    t.get::<Option<f32>>("_a")?.unwrap_or(1.0),
                ])
            });
            let color = match color {
                Some(c) => Some(c?),
                None => None,
            };
            let font = path.as_deref().and_then(|p| match resolve_texture(h, p) {
                Some(TextureSource::File(f)) => Some(f),
                _ => None,
            });
            h.with(|f| {
                if let Some(s) = size {
                    f.font_size = s;
                }
                if font.is_some() {
                    f.font_path = font;
                }
                if let Some(fl) = flags {
                    f.font_flags = fl.to_ascii_uppercase();
                }
                if let Some(c) = color {
                    f.color = c;
                }
                if let Some(j) = justify {
                    f.justify_h = j.to_ascii_uppercase();
                }
                f.measured = None;
            });
        }
        Ok(())
    })?;
    m.alias("SetNormalFontObject", "SetFontObject")?;
    m.alias("SetHighlightFontObject", "SetFontObject")?;
    m.noop("SetDisabledFontObject")?;
    m.add("SetJustifyH", |_, h, j: String| {
        h.with(|f| f.justify_h = j.to_ascii_uppercase());
        Ok(())
    })?;
    m.add("SetJustifyV", |_, h, j: String| {
        h.with(|f| f.justify_v = j.to_ascii_uppercase());
        Ok(())
    })?;
    m.add("GetJustifyH", |_, h, ()| {
        Ok(h.read(|f| f.justify_h.clone()).unwrap_or_default())
    })?;
    m.add("GetJustifyV", |_, h, ()| {
        Ok(h.read(|f| f.justify_v.clone()).unwrap_or_default())
    })?;
    m.add("SetWordWrap", |_, h, w: Option<bool>| {
        h.with(|f| f.word_wrap = w.unwrap_or(true));
        Ok(())
    })?;
    m.add("SetShadowColor", |_, h, c: Variadic<Value>| {
        let c = color_args(&c, 1.0);
        h.with(|f| f.shadow = c[3] > 0.0);
        Ok(())
    })?;
    for name in [
        "SetShadowOffset",
        "SetNonSpaceWrap",
        "SetMaxLines",
        "SetIndentedWordWrap",
        "SetSpacing",
    ] {
        m.noop(name)?;
    }
    fn text_size(h: &FrameHandle) -> (f32, f32) {
        h.read(|f| f.measured.unwrap_or_else(|| f.estimated_text_size()))
            .unwrap_or_default()
    }
    m.add("GetStringWidth", |_, h, ()| Ok(num(text_size(h).0)))?;
    m.alias("GetUnboundedStringWidth", "GetStringWidth")?;
    m.add("GetStringHeight", |_, h, ()| Ok(num(text_size(h).1)))?;
    m.add("GetFontString", |lua, h, ()| {
        let id = match h.read(|f| f.font_string).flatten() {
            Some(c) => c,
            None => {
                let (text, size) = h.read(|f| (f.text.clone(), f.font_size)).unwrap_or_default();
                let mut s = h.shared.borrow_mut();
                let c = s.ui.create(FrameKind::FontString, None, Some(h.id));
                if let Some(n) = s.ui.get_mut(c) {
                    n.text = text;
                    n.font_size = size;
                    n.layer = "OVERLAY".into();
                    n.color = [1.0, 0.86, 0.47, 1.0];
                    n.anchors.push(Anchor {
                        point: "CENTER".into(),
                        relative_to: None,
                        relative_point: "CENTER".into(),
                        x: 0.0,
                        y: 0.0,
                    });
                }
                if let Some(n) = s.ui.get_mut(h.id) {
                    n.text.clear();
                    n.font_string = Some(c);
                }
                c
            }
        };
        frame_object(lua, &h.shared, id)
    })?;
    m.add("AddMessage", |lua, h, (text, rest): (Value, Variadic<Value>)| {
        let _ = (lua, &rest);
        let mut line = value_str(&text).unwrap_or_default();
        if rest.len() >= 3 {
            let c = color_args(&rest, 1.0);
            line = format!(
                "|cff{:02x}{:02x}{:02x}{line}|r",
                (c[0].clamp(0.0, 1.0) * 255.0) as u8,
                (c[1].clamp(0.0, 1.0) * 255.0) as u8,
                (c[2].clamp(0.0, 1.0) * 255.0) as u8
            );
        }
        h.shared
            .borrow_mut()
            .commands
            .push(crate::manager::HostCommand::Print(line));
        Ok(())
    })?;

    // Boutons
    for name in [
        "SetNormalTexture",
        "SetPushedTexture",
        "SetHighlightTexture",
        "SetDisabledTexture",
        "SetCheckedTexture",
        "SetThumbTexture",
        "SetStatusBarTexture",
    ] {
        let primary = matches!(name, "SetNormalTexture" | "SetStatusBarTexture");
        m.add(name, move |_, h, (tex, _rest): (Value, MultiValue)| {
            if primary {
                let path = value_str(&tex);
                let src = path.as_deref().and_then(|p| resolve_texture(h, p));
                h.with(|f| {
                    f.texture = path;
                    f.texture_source = src;
                });
            }
            Ok(())
        })?;
    }
    for name in [
        "GetNormalTexture",
        "GetPushedTexture",
        "GetHighlightTexture",
        "GetDisabledTexture",
        "GetCheckedTexture",
        "GetThumbTexture",
        "GetStatusBarTexture",
    ] {
        // Rend la frame elle-même : `SetVertexColor`/`SetTexCoord` s'appliquent à son dessin.
        m.add(name, |lua, h, ()| frame_object(lua, &h.shared, h.id))?;
    }
    m.add("Enable", |_, h, ()| {
        h.with(|f| f.enabled = true);
        Ok(())
    })?;
    m.add("Disable", |_, h, ()| {
        h.with(|f| f.enabled = false);
        Ok(())
    })?;
    m.add("SetEnabled", |_, h, on: Value| {
        h.with(|f| f.enabled = !matches!(on, Value::Nil | Value::Boolean(false)));
        Ok(())
    })?;
    m.add("IsEnabled", |_, h, ()| Ok(h.read(|f| f.enabled).unwrap_or(true)))?;
    m.add("SetChecked", |_, h, c: Value| {
        h.with(|f| f.checked = !matches!(c, Value::Nil | Value::Boolean(false)));
        Ok(())
    })?;
    m.add("GetChecked", |_, h, ()| Ok(h.read(|f| f.checked).unwrap_or(false)))?;
    m.add("Click", |lua, h, button: Option<String>| {
        crate::manager::dispatch_click(lua, &h.shared, h.id, &button.unwrap_or_else(|| "LeftButton".into()));
        Ok(())
    })?;
    for name in ["LockHighlight", "UnlockHighlight", "SetButtonState", "SetMotionScriptsWhileDisabled"] {
        m.noop(name)?;
    }

    // Valeurs (StatusBar, Slider)
    m.add("SetMinMaxValues", |lua, h, (lo, hi): (f32, f32)| {
        h.with(|f| {
            f.min_value = lo;
            f.max_value = hi;
        });
        let v = h.read(|f| f.value).unwrap_or(0.0);
        set_value(lua, h, v, false);
        Ok(())
    })?;
    m.add("GetMinMaxValues", |_, h, ()| {
        let (lo, hi) = h.read(|f| (f.min_value, f.max_value)).unwrap_or((0.0, 1.0));
        Ok((num(lo), num(hi)))
    })?;
    m.add("SetValue", |lua, h, v: f32| {
        set_value(lua, h, v, false);
        Ok(())
    })?;
    m.add("GetValue", |_, h, ()| Ok(num(h.read(|f| f.value).unwrap_or(0.0))))?;
    m.add("SetValueStep", |_, h, s: f32| {
        h.with(|f| f.value_step = s.max(0.0));
        Ok(())
    })?;
    m.add("GetValueStep", |_, h, ()| Ok(num(h.read(|f| f.value_step).unwrap_or(0.0))))?;
    m.add("SetOrientation", |_, h, o: String| {
        h.with(|f| f.orientation = o.to_ascii_uppercase());
        Ok(())
    })?;
    m.add("GetOrientation", |_, h, ()| {
        Ok(h.read(|f| f.orientation.clone()).unwrap_or_default())
    })?;
    for name in ["SetObeyStepOnDrag", "SetStepsPerPage", "SetReverseFill", "SetFillStyle", "SetRotatesTexture"] {
        m.noop(name)?;
    }

    // EditBox
    m.add("SetFocus", |lua, h, ()| {
        let ids: Vec<usize> = {
            let s = h.shared.borrow();
            s.ui.frames.iter().filter(|f| f.focused).map(|f| f.id).collect()
        };
        for id in ids {
            if id != h.id {
                h.shared.borrow_mut().ui.get_mut(id).map(|f| f.focused = false);
                call_script(lua, &h.shared, id, "OnEditFocusLost", ());
            }
        }
        h.with(|f| f.focused = true);
        call_script(lua, &h.shared, h.id, "OnEditFocusGained", ());
        Ok(())
    })?;
    m.add("ClearFocus", |lua, h, ()| {
        if h.read(|f| f.focused).unwrap_or(false) {
            h.with(|f| f.focused = false);
            call_script(lua, &h.shared, h.id, "OnEditFocusLost", ());
        }
        Ok(())
    })?;
    m.add("HasFocus", |_, h, ()| Ok(h.read(|f| f.focused).unwrap_or(false)))?;
    m.add("SetMultiLine", |_, h, on: Option<bool>| {
        h.with(|f| f.multi_line = on.unwrap_or(true));
        Ok(())
    })?;
    m.add("IsMultiLine", |_, h, ()| Ok(h.read(|f| f.multi_line).unwrap_or(false)))?;
    m.add("SetMaxLetters", |_, h, n: usize| {
        h.with(|f| f.max_letters = n);
        Ok(())
    })?;
    m.add("GetMaxLetters", |_, h, ()| Ok(h.read(|f| f.max_letters).unwrap_or(0)))?;
    m.add("SetNumeric", |_, h, on: Option<bool>| {
        h.with(|f| f.numeric = on.unwrap_or(true));
        Ok(())
    })?;
    m.add("IsNumeric", |_, h, ()| Ok(h.read(|f| f.numeric).unwrap_or(false)))?;
    m.add("SetPassword", |_, h, on: Option<bool>| {
        h.with(|f| f.password = on.unwrap_or(true));
        Ok(())
    })?;
    m.add("GetNumber", |_, h, ()| {
        Ok(h.read(|f| f.text.trim().parse::<f64>().unwrap_or(0.0)).unwrap_or(0.0))
    })?;
    m.add("SetNumber", |lua, h, n: f64| {
        set_text(lua, h, if n.fract() == 0.0 { format!("{}", n as i64) } else { n.to_string() }, false);
        Ok(())
    })?;
    m.add("Insert", |lua, h, s: String| {
        let t = h.read(|f| f.text.clone()).unwrap_or_default() + &s;
        set_text(lua, h, t, false);
        Ok(())
    })?;
    for name in [
        "SetAutoFocus",
        "HighlightText",
        "SetCursorPosition",
        "SetTextInsets",
        "AddHistoryLine",
        "SetHistoryLines",
        "SetBlinkSpeed",
        "SetCountInvisibleLetters",
    ] {
        m.noop(name)?;
    }

    // Souris, clavier, déplacement
    m.add("EnableMouse", |_, h, on: Option<bool>| {
        h.with(|f| f.mouse_enabled = on.unwrap_or(true));
        Ok(())
    })?;
    m.add("IsMouseEnabled", |_, h, ()| Ok(h.read(|f| f.mouse_enabled).unwrap_or(false)))?;
    m.alias("SetMouseClickEnabled", "EnableMouse")?;
    m.alias("SetMouseMotionEnabled", "EnableMouse")?;
    m.add("EnableMouseWheel", |_, h, on: Option<bool>| {
        h.with(|f| f.mouse_wheel_enabled = on.unwrap_or(true));
        Ok(())
    })?;
    m.add("IsMouseWheelEnabled", |_, h, ()| {
        Ok(h.read(|f| f.mouse_wheel_enabled).unwrap_or(false))
    })?;
    m.add("EnableKeyboard", |_, h, on: Option<bool>| {
        h.with(|f| f.keyboard_enabled = on.unwrap_or(true));
        Ok(())
    })?;
    m.add("IsKeyboardEnabled", |_, h, ()| {
        Ok(h.read(|f| f.keyboard_enabled).unwrap_or(false))
    })?;
    m.add("SetMovable", |_, h, on: Option<bool>| {
        h.with(|f| f.movable = on.unwrap_or(true));
        Ok(())
    })?;
    m.add("IsMovable", |_, h, ()| Ok(h.read(|f| f.movable).unwrap_or(false)))?;
    m.add("SetClampedToScreen", |_, h, on: Option<bool>| {
        h.with(|f| f.clamped = on.unwrap_or(true));
        Ok(())
    })?;
    m.add("IsClampedToScreen", |_, h, ()| Ok(h.read(|f| f.clamped).unwrap_or(false)))?;
    m.add("RegisterForDrag", |_, h, buttons: Variadic<String>| {
        h.with(|f| f.drag_buttons = buttons.iter().cloned().collect());
        Ok(())
    })?;
    m.add("StartMoving", |_, h, ()| {
        let mut s = h.shared.borrow_mut();
        let cursor = s.host.cursor;
        let rect = s.ui.rects.get(h.id).copied().flatten();
        if let (Some((cx, cy)), Some(r)) = (cursor, rect) {
            s.ui.moving = Some(crate::ui::Moving {
                id: h.id,
                grab: (cx - r.left, cy - r.bottom),
            });
        }
        Ok(())
    })?;
    m.add("StopMovingOrSizing", |_, h, ()| {
        let mut s = h.shared.borrow_mut();
        if s.ui.moving.is_some_and(|mv| mv.id == h.id) {
            s.ui.moving = None;
        }
        Ok(())
    })?;
    m.add("IsMouseOver", |_, h, ()| {
        let s = h.shared.borrow();
        Ok(match (s.host.cursor, s.ui.rects.get(h.id).copied().flatten()) {
            (Some((x, y)), Some(r)) => r.contains(x, y),
            _ => false,
        })
    })?;
    for name in [
        "RegisterForClicks",
        "SetResizable",
        "SetUserPlaced",
        "StartSizing",
        "SetHitRectInsets",
        "SetPropagateKeyboardInput",
        "SetResizeBounds",
        "SetMinResize",
        "SetMaxResize",
        "SetDontSavePosition",
        "SetFixedFrameStrata",
        "SetFixedFrameLevel",
        "SetFlattensRenderLayers",
        "SetIgnoreParentAlpha",
        "SetIgnoreParentScale",
        "SetClipsChildren",
        "SetAttribute",
        "RegisterForMouse",
    ] {
        m.noop(name)?;
    }
    m.t.set(
        "IsUserPlaced",
        lua.create_function(|_, _: MultiValue| Ok(false))?,
    )?;
    m.t.set(
        "GetAttribute",
        lua.create_function(|_, _: MultiValue| Ok(Value::Nil))?,
    )?;

    // Enfants
    fn create_region(
        lua: &Lua,
        h: &FrameHandle,
        kind: FrameKind,
        (name, layer, template): (Option<String>, Option<String>, Option<String>),
    ) -> mlua::Result<Table> {
        let t = create_frame(lua, &h.shared, kind, name, Some(h.id), template.as_deref())?;
        if let Some(id) = frame_id(&Value::Table(t.clone())) {
            let mut s = h.shared.borrow_mut();
            if let Some(f) = s.ui.get_mut(id) {
                if let Some(l) = layer {
                    f.layer = l.to_ascii_uppercase();
                }
            }
        }
        // Les modèles de police WoW (`GameFontNormal`) passés en 3ᵉ argument.
        if let Some(tpl) = template {
            if let Ok(Value::Table(_)) = lua.globals().get::<Value>(tpl.as_str()) {
                let set: Function = t.get("SetFontObject")?;
                set.call::<()>((t.clone(), tpl))?;
            }
        }
        Ok(t)
    }
    m.add("CreateFontString", |lua, h, args: (Option<String>, Option<String>, Option<String>)| {
        create_region(lua, h, FrameKind::FontString, args)
    })?;
    m.add("CreateTexture", |lua, h, args: (Option<String>, Option<String>, Option<String>)| {
        create_region(lua, h, FrameKind::Texture, args)
    })?;
    m.alias("CreateMaskTexture", "CreateTexture")?;
    m.alias("CreateLine", "CreateTexture")?;
    m.add("GetChildren", |lua, h, ()| children(lua, h, false))?;
    m.add("GetRegions", |lua, h, ()| children(lua, h, true))?;
    fn children(lua: &Lua, h: &FrameHandle, regions: bool) -> mlua::Result<Variadic<Value>> {
        let ids: Vec<usize> = {
            let s = h.shared.borrow();
            s.ui.get(h.id)
                .map(|f| {
                    f.children
                        .iter()
                        .copied()
                        .filter(|&c| s.ui.get(c).is_some_and(|n| n.kind.is_region() == regions))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut out = Variadic::new();
        for c in ids {
            out.push(Value::Table(frame_object(lua, &h.shared, c)?));
        }
        Ok(out)
    }
    m.add("GetNumChildren", |_, h, ()| {
        let s = h.shared.borrow();
        Ok(s.ui
            .get(h.id)
            .map(|f| {
                f.children
                    .iter()
                    .filter(|&&c| s.ui.get(c).is_some_and(|n| !n.kind.is_region()))
                    .count()
            })
            .unwrap_or(0))
    })?;
    // ScrollFrame : l'enfant défilant est simplement ancré dans la frame.
    m.add("SetScrollChild", |lua, h, child: Value| {
        if let Some(c) = frame_id(&child) {
            h.shared.borrow_mut().ui.set_parent(c, Some(h.id));
            let t = frame_object(lua, &h.shared, h.id)?;
            t.raw_set("\u{1}scrollChild", child)?;
        }
        Ok(())
    })?;
    m.add("GetScrollChild", |lua, h, ()| {
        frame_object(lua, &h.shared, h.id)?.raw_get::<Value>("\u{1}scrollChild")
    })?;
    for name in ["SetVerticalScroll", "SetHorizontalScroll", "UpdateScrollChildRect"] {
        m.noop(name)?;
    }
    for name in ["GetVerticalScroll", "GetHorizontalScroll", "GetVerticalScrollRange", "GetHorizontalScrollRange"] {
        m.t.set(name, lua.create_function(|_, _: MultiValue| Ok(0))?)?;
    }

    // Scripts et événements
    m.add("SetScript", |lua, h, (handler, func): (String, Option<Function>)| {
        let scripts = scripts_table(lua)?;
        let per_frame: Table = match scripts.get::<Value>(h.id)? {
            Value::Table(t) => t,
            _ => {
                let t = lua.create_table()?;
                scripts.set(h.id, t.clone())?;
                t
            }
        };
        let mut shared = h.shared.borrow_mut();
        match func {
            Some(f) => {
                per_frame.set(handler.as_str(), f)?;
                if let Some(fr) = shared.ui.get_mut(h.id) {
                    fr.scripts.insert(handler.clone());
                }
                if handler == "OnUpdate" {
                    shared.ui.on_update.insert(h.id);
                }
            }
            None => {
                per_frame.set(handler.as_str(), Value::Nil)?;
                if let Some(fr) = shared.ui.get_mut(h.id) {
                    fr.scripts.remove(&handler);
                }
                if handler == "OnUpdate" {
                    shared.ui.on_update.remove(&h.id);
                }
            }
        }
        Ok(())
    })?;
    m.add("GetScript", |lua, h, handler: String| get_script(lua, h.id, &handler))?;
    m.add("HasScript", |_, _h, _handler: String| Ok(true))?;
    m.add("HookScript", |lua, h, (handler, func): (String, Function)| {
        let prev = get_script(lua, h.id, &handler)?;
        let hooked = match prev {
            Some(p) => lua.create_function(move |_, args: MultiValue| {
                p.call::<()>(args.clone())?;
                func.call::<()>(args)
            })?,
            None => func,
        };
        let me = frame_object(lua, &h.shared, h.id)?;
        let set: Function = me.get("SetScript")?;
        set.call::<()>((me, handler, hooked))
    })?;
    m.add("RegisterEvent", |_, h, event: String| {
        h.with(|f| f.events.insert(event));
        Ok(true)
    })?;
    m.add("RegisterUnitEvent", |_, h, (event, _units): (String, MultiValue)| {
        h.with(|f| f.events.insert(event));
        Ok(true)
    })?;
    m.add("UnregisterEvent", |_, h, event: String| {
        h.with(|f| f.events.remove(&event));
        Ok(())
    })?;
    m.add("UnregisterAllEvents", |_, h, ()| {
        h.with(|f| {
            f.events.clear();
            f.all_events = false;
        });
        Ok(())
    })?;
    m.add("RegisterAllEvents", |_, h, ()| {
        h.with(|f| f.all_events = true);
        Ok(())
    })?;
    m.add("IsEventRegistered", |_, h, event: String| {
        Ok(h.read(|f| f.all_events || f.events.contains(&event)).unwrap_or(false))
    })?;

    // Animations : objets factices pour que le code qui en crée ne plante pas.
    m.add("CreateAnimationGroup", |lua, _h, _: MultiValue| {
        let src = r#"
            local g = {}
            local function noop() end
            for _, k in ipairs({"Play","Stop","Pause","Finish","Restart","SetLooping","SetScript","HookScript","SetToFinalAlpha"}) do g[k] = noop end
            g.IsPlaying = function() return false end
            g.CreateAnimation = function()
              local a = {}
              setmetatable(a, { __index = function() return noop end })
              return a
            end
            return g
        "#;
        lua.load(src).set_name("=animation").eval::<Table>()
    })?;

    let meta = lua.create_table()?;
    meta.set("__index", m.t.clone())?;
    meta.set(
        "__tostring",
        lua.create_function(|_, t: Value| {
            Ok(match handle(&t) {
                Some(h) => {
                    let name = h.read(|f| f.name.clone()).flatten();
                    match name {
                        Some(n) => format!("Frame<{n}>"),
                        None => format!("Frame<{}>", h.id),
                    }
                }
                None => "Frame<?>".into(),
            })
        })?,
    )?;
    lua.set_named_registry_value(META_KEY, meta)?;
    Ok(m.t)
}

fn is_point(s: &str) -> bool {
    matches!(
        s.to_ascii_uppercase().as_str(),
        "TOPLEFT" | "TOP" | "TOPRIGHT" | "LEFT" | "CENTER" | "RIGHT" | "BOTTOMLEFT" | "BOTTOM" | "BOTTOMRIGHT"
    )
}
