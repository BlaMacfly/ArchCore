//! Objet `Frame` exposé à Lua (userdata) : méthodes façon WoW qui modifient l'[`UiTree`] partagé
//! et conservent les scripts dans le registre Lua.

use crate::manager::Shared;
use crate::ui::{Anchor, FrameKind};
use mlua::{AnyUserData, Function, Lua, MetaMethod, UserData, UserDataMethods, Value, Variadic};

/// Poignée Lua sur une frame.
#[derive(Clone)]
pub struct FrameHandle {
    pub id: usize,
    pub shared: Shared,
}

/// Nom de la table du registre qui conserve les scripts : `SCRIPTS[id][handler] = fonction`.
pub const SCRIPTS_KEY: &str = "ARCHRUST_SCRIPTS";

pub fn scripts_table(lua: &Lua) -> mlua::Result<mlua::Table> {
    match lua.named_registry_value::<Value>(SCRIPTS_KEY)? {
        Value::Table(t) => Ok(t),
        _ => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(SCRIPTS_KEY, t.clone())?;
            Ok(t)
        }
    }
}

/// Fonction de script d'une frame, si définie.
pub fn get_script(lua: &Lua, id: usize, handler: &str) -> mlua::Result<Option<Function>> {
    let scripts = scripts_table(lua)?;
    let Value::Table(per_frame) = scripts.get::<Value>(id)? else { return Ok(None) };
    match per_frame.get::<Value>(handler)? {
        Value::Function(f) => Ok(Some(f)),
        _ => Ok(None),
    }
}

fn frame_id(v: &Value) -> Option<usize> {
    match v {
        Value::UserData(ud) => ud.borrow::<FrameHandle>().ok().map(|f| f.id),
        _ => None,
    }
}

impl UserData for FrameHandle {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            let ui = this.shared.borrow();
            Ok(format!("Frame<{}>", ui.ui.get(this.id).and_then(|f| f.name.clone()).unwrap_or_else(|| this.id.to_string())))
        });
        m.add_meta_method(MetaMethod::Eq, |_, this, other: AnyUserData| Ok(other.borrow::<FrameHandle>().map(|o| o.id == this.id).unwrap_or(false)));

        m.add_method("GetName", |_, this, ()| Ok(this.shared.borrow().ui.get(this.id).and_then(|f| f.name.clone())));
        m.add_method("GetObjectType", |_, this, ()| {
            let kind = this.shared.borrow().ui.get(this.id).map(|f| f.kind);
            Ok(match kind {
                Some(FrameKind::Button) => "Button",
                Some(FrameKind::FontString) => "FontString",
                Some(FrameKind::Texture) => "Texture",
                Some(FrameKind::EditBox) => "EditBox",
                Some(FrameKind::StatusBar) => "StatusBar",
                Some(FrameKind::CheckButton) => "CheckButton",
                Some(FrameKind::Slider) => "Slider",
                _ => "Frame",
            })
        });
        m.add_method("GetParent", |lua, this, ()| {
            let parent = this.shared.borrow().ui.get(this.id).and_then(|f| f.parent);
            match parent {
                Some(p) => Ok(Value::UserData(lua.create_userdata(FrameHandle { id: p, shared: this.shared.clone() })?)),
                None => Ok(Value::Nil),
            }
        });
        m.add_method("SetParent", |_, this, parent: Value| {
            let p = frame_id(&parent);
            this.shared.borrow_mut().ui.set_parent(this.id, p);
            Ok(())
        });

        // Visibilité
        m.add_method("Show", |_, this, ()| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.shown = true);
            Ok(())
        });
        m.add_method("Hide", |_, this, ()| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.shown = false);
            Ok(())
        });
        m.add_method("SetShown", |_, this, shown: bool| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.shown = shown);
            Ok(())
        });
        m.add_method("IsShown", |_, this, ()| Ok(this.shared.borrow().ui.get(this.id).map(|f| f.shown).unwrap_or(false)));
        m.add_method("IsVisible", |_, this, ()| Ok(this.shared.borrow().ui.is_visible(this.id)));

        // Géométrie
        m.add_method("SetSize", |_, this, (w, h): (f32, f32)| {
            if let Some(f) = this.shared.borrow_mut().ui.get_mut(this.id) {
                f.width = w;
                f.height = h;
            }
            Ok(())
        });
        m.add_method("SetWidth", |_, this, w: f32| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.width = w);
            Ok(())
        });
        m.add_method("SetHeight", |_, this, h: f32| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.height = h);
            Ok(())
        });
        m.add_method("GetWidth", |_, this, ()| Ok(this.shared.borrow().ui.get(this.id).map(|f| f.width).unwrap_or(0.0)));
        m.add_method("GetHeight", |_, this, ()| Ok(this.shared.borrow().ui.get(this.id).map(|f| f.height).unwrap_or(0.0)));
        m.add_method("GetSize", |_, this, ()| {
            let ui = this.shared.borrow();
            let f = ui.ui.get(this.id);
            Ok((f.map(|f| f.width).unwrap_or(0.0), f.map(|f| f.height).unwrap_or(0.0)))
        });
        // SetPoint(point [, relativeTo] [, relativePoint] [, x, y])
        m.add_method("SetPoint", |_, this, args: Variadic<Value>| {
            let mut it = args.into_iter();
            let point = match it.next() {
                Some(Value::String(s)) => s.to_str()?.to_ascii_uppercase(),
                _ => return Err(mlua::Error::RuntimeError("SetPoint : point attendu".into())),
            };
            let mut relative_to = None;
            let mut relative_point = point.clone();
            let mut nums: Vec<f32> = Vec::new();
            for v in it {
                match v {
                    Value::UserData(_) => relative_to = frame_id(&v),
                    Value::String(s) => {
                        let s = s.to_str()?.to_string();
                        if nums.is_empty() {
                            // Un nom de frame ou un point relatif.
                            let by_name = this.shared.borrow().ui.by_name(&s);
                            if let Some(id) = by_name {
                                relative_to = Some(id);
                            } else {
                                relative_point = s.to_ascii_uppercase();
                            }
                        }
                    }
                    Value::Integer(i) => nums.push(i as f32),
                    Value::Number(n) => nums.push(n as f32),
                    Value::Nil => {}
                    _ => {}
                }
            }
            let anchor = Anchor { point, relative_to, relative_point, x: nums.first().copied().unwrap_or(0.0), y: nums.get(1).copied().unwrap_or(0.0) };
            if let Some(f) = this.shared.borrow_mut().ui.get_mut(this.id) {
                f.anchors.retain(|a| a.point != anchor.point);
                f.anchors.push(anchor);
            }
            Ok(())
        });
        m.add_method("ClearAllPoints", |_, this, ()| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.anchors.clear());
            Ok(())
        });
        m.add_method("SetAllPoints", |_, this, _rel: Variadic<Value>| {
            if let Some(f) = this.shared.borrow_mut().ui.get_mut(this.id) {
                f.anchors = vec![
                    Anchor { point: "TOPLEFT".into(), relative_to: None, relative_point: "TOPLEFT".into(), x: 0.0, y: 0.0 },
                    Anchor { point: "BOTTOMRIGHT".into(), relative_to: None, relative_point: "BOTTOMRIGHT".into(), x: 0.0, y: 0.0 },
                ];
                f.width = 0.0;
                f.height = 0.0;
            }
            Ok(())
        });
        m.add_method("GetPoint", |lua, this, _idx: Option<i64>| {
            let ui = this.shared.borrow();
            let Some(a) = ui.ui.get(this.id).and_then(|f| f.anchors.first().cloned()) else { return Ok(Variadic::new()) };
            let rel = match a.relative_to {
                Some(r) => Value::UserData(lua.create_userdata(FrameHandle { id: r, shared: this.shared.clone() })?),
                None => Value::Nil,
            };
            Ok(Variadic::from_iter([Value::String(lua.create_string(&a.point)?), rel, Value::String(lua.create_string(&a.relative_point)?), Value::Number(a.x as f64), Value::Number(a.y as f64)]))
        });

        // Apparence
        m.add_method("SetAlpha", |_, this, a: f32| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.alpha = a.clamp(0.0, 1.0));
            Ok(())
        });
        m.add_method("GetAlpha", |_, this, ()| Ok(this.shared.borrow().ui.get(this.id).map(|f| f.alpha).unwrap_or(1.0)));
        m.add_method("SetFrameStrata", |_, this, s: String| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.strata = s.to_ascii_uppercase());
            Ok(())
        });
        m.add_method("GetFrameStrata", |_, this, ()| Ok(this.shared.borrow().ui.get(this.id).map(|f| f.strata.clone()).unwrap_or_default()));
        m.add_method("SetFrameLevel", |_, this, l: i32| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.level = l);
            Ok(())
        });
        m.add_method("GetFrameLevel", |_, this, ()| Ok(this.shared.borrow().ui.get(this.id).map(|f| f.level).unwrap_or(0)));
        m.add_method("SetBackdropColor", |_, this, (r, g, b, a): (f32, f32, f32, Option<f32>)| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.color = [r, g, b, a.unwrap_or(1.0)]);
            Ok(())
        });
        m.add_method("SetColorTexture", |_, this, (r, g, b, a): (f32, f32, f32, Option<f32>)| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.color = [r, g, b, a.unwrap_or(1.0)]);
            Ok(())
        });
        m.add_method("SetVertexColor", |_, this, (r, g, b, a): (f32, f32, f32, Option<f32>)| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.color = [r, g, b, a.unwrap_or(1.0)]);
            Ok(())
        });
        m.add_method("SetTextColor", |_, this, (r, g, b, a): (f32, f32, f32, Option<f32>)| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.color = [r, g, b, a.unwrap_or(1.0)]);
            Ok(())
        });
        m.add_method("SetTexture", |_, this, path: Option<String>| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.texture = path);
            Ok(())
        });
        m.add_method("GetTexture", |_, this, ()| Ok(this.shared.borrow().ui.get(this.id).and_then(|f| f.texture.clone())));
        m.add_method("SetText", |_, this, text: Option<String>| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.text = text.unwrap_or_default());
            Ok(())
        });
        m.add_method("GetText", |_, this, ()| Ok(this.shared.borrow().ui.get(this.id).map(|f| f.text.clone()).unwrap_or_default()));
        m.add_method("SetFormattedText", |_, this, args: Variadic<Value>| {
            let mut it = args.into_iter();
            let fmt = match it.next() {
                Some(Value::String(s)) => s.to_str()?.to_string(),
                _ => String::new(),
            };
            let text = crate::api::lua_format(&fmt, it.collect());
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.text = text);
            Ok(())
        });
        m.add_method("SetFont", |_, this, (_path, size): (Option<String>, Option<f32>)| {
            if let Some(size) = size {
                this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.font_size = size);
            }
            Ok(())
        });
        m.add_method("SetFontObject", |_, _this, _o: Value| Ok(()));
        m.add_method("SetJustifyH", |_, this, j: String| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.justify_h = j.to_ascii_uppercase());
            Ok(())
        });
        m.add_method("GetStringWidth", |_, this, ()| {
            let ui = this.shared.borrow();
            let f = ui.ui.get(this.id);
            Ok(f.map(|f| f.text.chars().count() as f32 * f.font_size * 0.55).unwrap_or(0.0))
        });

        // Valeurs (StatusBar, Slider, CheckButton)
        m.add_method("SetMinMaxValues", |_, this, (lo, hi): (f32, f32)| {
            if let Some(f) = this.shared.borrow_mut().ui.get_mut(this.id) {
                f.min_value = lo;
                f.max_value = hi;
            }
            Ok(())
        });
        m.add_method("GetMinMaxValues", |_, this, ()| {
            let ui = this.shared.borrow();
            let f = ui.ui.get(this.id);
            Ok((f.map(|f| f.min_value).unwrap_or(0.0), f.map(|f| f.max_value).unwrap_or(1.0)))
        });
        m.add_method("SetValue", |_, this, v: f32| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.value = v);
            Ok(())
        });
        m.add_method("GetValue", |_, this, ()| Ok(this.shared.borrow().ui.get(this.id).map(|f| f.value).unwrap_or(0.0)));
        m.add_method("SetStatusBarColor", |_, this, (r, g, b, a): (f32, f32, f32, Option<f32>)| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.color = [r, g, b, a.unwrap_or(1.0)]);
            Ok(())
        });
        m.add_method("SetChecked", |_, this, c: bool| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.checked = c);
            Ok(())
        });
        m.add_method("GetChecked", |_, this, ()| Ok(this.shared.borrow().ui.get(this.id).map(|f| f.checked).unwrap_or(false)));

        // Interaction
        m.add_method("EnableMouse", |_, this, on: Option<bool>| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.mouse_enabled = on.unwrap_or(true));
            Ok(())
        });
        m.add_method("SetMovable", |_, this, on: Option<bool>| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.movable = on.unwrap_or(true));
            Ok(())
        });
        m.add_method("RegisterForDrag", |_, _this, _b: Variadic<Value>| Ok(()));
        m.add_method("RegisterForClicks", |_, _this, _b: Variadic<Value>| Ok(()));
        m.add_method("StartMoving", |_, _this, ()| Ok(()));
        m.add_method("StopMovingOrSizing", |_, _this, ()| Ok(()));
        m.add_method("Raise", |_, _this, ()| Ok(()));
        m.add_method("SetClampedToScreen", |_, _this, _b: bool| Ok(()));
        m.add_method("SetBackdrop", |_, _this, _t: Value| Ok(()));
        m.add_method("SetBackdropBorderColor", |_, _this, _c: Variadic<Value>| Ok(()));
        m.add_method("SetScale", |_, _this, _s: f32| Ok(()));
        m.add_method("SetID", |_, _this, _i: i64| Ok(()));

        // Enfants
        m.add_method("CreateFontString", |lua, this, (name, _layer, _template): (Option<String>, Option<String>, Option<String>)| {
            let id = this.shared.borrow_mut().ui.create(FrameKind::FontString, name.clone(), Some(this.id));
            let h = FrameHandle { id, shared: this.shared.clone() };
            if let Some(n) = name {
                lua.globals().set(n, lua.create_userdata(h.clone())?)?;
            }
            lua.create_userdata(h)
        });
        m.add_method("CreateTexture", |lua, this, (name, _layer, _template): (Option<String>, Option<String>, Option<String>)| {
            let id = this.shared.borrow_mut().ui.create(FrameKind::Texture, name.clone(), Some(this.id));
            let h = FrameHandle { id, shared: this.shared.clone() };
            if let Some(n) = name {
                lua.globals().set(n, lua.create_userdata(h.clone())?)?;
            }
            lua.create_userdata(h)
        });
        m.add_method("GetChildren", |lua, this, ()| {
            let children = this.shared.borrow().ui.get(this.id).map(|f| f.children.clone()).unwrap_or_default();
            let mut out = Variadic::new();
            for c in children {
                out.push(Value::UserData(lua.create_userdata(FrameHandle { id: c, shared: this.shared.clone() })?));
            }
            Ok(out)
        });

        // Scripts et événements
        m.add_method("SetScript", |lua, this, (handler, func): (String, Option<Function>)| {
            let scripts = scripts_table(lua)?;
            let per_frame: mlua::Table = match scripts.get::<Value>(this.id)? {
                Value::Table(t) => t,
                _ => {
                    let t = lua.create_table()?;
                    scripts.set(this.id, t.clone())?;
                    t
                }
            };
            let mut shared = this.shared.borrow_mut();
            match func {
                Some(f) => {
                    per_frame.set(handler.as_str(), f)?;
                    if let Some(fr) = shared.ui.get_mut(this.id) {
                        fr.scripts.insert(handler.clone());
                    }
                    if handler == "OnUpdate" {
                        shared.ui.on_update.insert(this.id);
                    }
                }
                None => {
                    per_frame.set(handler.as_str(), Value::Nil)?;
                    if let Some(fr) = shared.ui.get_mut(this.id) {
                        fr.scripts.remove(&handler);
                    }
                    if handler == "OnUpdate" {
                        shared.ui.on_update.remove(&this.id);
                    }
                }
            }
            Ok(())
        });
        m.add_method("GetScript", |lua, this, handler: String| Ok(get_script(lua, this.id, &handler)?));
        m.add_method("HasScript", |_, _this, _handler: String| Ok(true));
        m.add_method("HookScript", |lua, this, (handler, func): (String, Function)| {
            let prev = get_script(lua, this.id, &handler)?;
            let scripts = scripts_table(lua)?;
            let per_frame: mlua::Table = match scripts.get::<Value>(this.id)? {
                Value::Table(t) => t,
                _ => {
                    let t = lua.create_table()?;
                    scripts.set(this.id, t.clone())?;
                    t
                }
            };
            let hooked = match prev {
                Some(p) => lua.create_function(move |_, args: Variadic<Value>| {
                    p.call::<()>(args.clone())?;
                    func.call::<()>(args)
                })?,
                None => func,
            };
            per_frame.set(handler.as_str(), hooked)?;
            if let Some(fr) = this.shared.borrow_mut().ui.get_mut(this.id) {
                fr.scripts.insert(handler);
            }
            Ok(())
        });
        m.add_method("RegisterEvent", |_, this, event: String| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.events.insert(event));
            Ok(())
        });
        m.add_method("UnregisterEvent", |_, this, event: String| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.events.remove(&event));
            Ok(())
        });
        m.add_method("UnregisterAllEvents", |_, this, ()| {
            this.shared.borrow_mut().ui.get_mut(this.id).map(|f| f.events.clear());
            Ok(())
        });
        m.add_method("IsEventRegistered", |_, this, event: String| Ok(this.shared.borrow().ui.get(this.id).map(|f| f.events.contains(&event)).unwrap_or(false)));
        m.add_method("Click", |lua, this, button: Option<String>| {
            if let Some(f) = get_script(lua, this.id, "OnClick")? {
                let me = lua.create_userdata(this.clone())?;
                f.call::<()>((me, button.unwrap_or_else(|| "LeftButton".into()), false))?;
            }
            Ok(())
        });
    }
}
