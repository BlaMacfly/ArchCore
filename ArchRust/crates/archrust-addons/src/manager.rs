//! Chargement des AddOns, dispatch des événements, commandes slash, minuteurs, SavedVariables.

use crate::frame::{get_script, FrameHandle};
use crate::manifest::Manifest;
use crate::ui::UiTree;
use mlua::{Function, Lua, LuaOptions, RegistryKey, StdLib, Table, Value, Variadic};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Debug, thiserror::Error)]
pub enum AddonError {
    #[error("Lua : {0}")]
    Lua(#[from] mlua::Error),
    #[error("E/S {0} : {1}")]
    Io(PathBuf, std::io::Error),
    #[error("dépendance manquante pour {addon} : {dep}")]
    MissingDependency { addon: String, dep: String },
}

/// Ce que le jeu expose aux AddOns (mis à jour par le moteur avant chaque image).
#[derive(Debug, Clone, Default)]
pub struct HostState {
    pub time: f64,
    pub framerate: f64,
    pub realm: String,
    pub locale: String,
    pub player_name: String,
    pub player_level: i64,
    pub player_hp: i64,
    pub player_hp_max: i64,
    pub player_mp: i64,
    pub player_mp_max: i64,
    pub player_position: [f32; 3],
    pub target_name: Option<String>,
}

/// Actions demandées par les AddOns, à exécuter par le moteur.
#[derive(Debug, Clone, PartialEq)]
pub enum HostCommand {
    Print(String),
    Chat {
        message: String,
        kind: String,
        target: Option<String>,
    },
    PlaySound(String),
    ReloadUi,
}

pub struct Timer {
    pub due: f64,
    pub period: Option<f64>,
    pub func: RegistryKey,
    pub cancelled: bool,
}

/// État partagé entre Rust et les fermetures Lua.
pub struct SharedState {
    pub ui: UiTree,
    pub host: HostState,
    pub commands: Vec<HostCommand>,
    pub timers: Vec<Timer>,
}

pub type Shared = Rc<RefCell<SharedState>>;

#[derive(Debug, Clone)]
pub struct LoadedAddon {
    pub manifest: Manifest,
    pub enabled: bool,
    pub error: Option<String>,
}

pub struct AddonManager {
    pub lua: Lua,
    pub shared: Shared,
    pub addons: Vec<LoadedAddon>,
    pub saved_dir: PathBuf,
}

impl AddonManager {
    /// Crée l'état Lua (bibliothèques sûres uniquement) et installe l'API.
    pub fn new(saved_dir: impl Into<PathBuf>) -> Result<Self, AddonError> {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE,
            LuaOptions::default(),
        )?;
        let shared: Shared = Rc::new(RefCell::new(SharedState {
            ui: UiTree::new(),
            host: HostState::default(),
            commands: Vec::new(),
            timers: Vec::new(),
        }));
        crate::api::install(&lua, shared.clone())?;
        Ok(Self {
            lua,
            shared,
            addons: Vec::new(),
            saved_dir: saved_dir.into(),
        })
    }

    /// Découvre les AddOns d'un dossier (`addons/<Nom>/<Nom>.toc`).
    pub fn discover(dir: &Path) -> Vec<Manifest> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(dir) else {
            return out;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                match Manifest::load(&p) {
                    Ok(m) => out.push(m),
                    Err(err) => log::debug!("addons : {} ignoré ({err})", p.display()),
                }
            }
        }
        out.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        });
        out
    }

    /// Charge tous les AddOns d'un dossier, dépendances d'abord. Les erreurs d'un AddOn ne
    /// bloquent pas les autres ; elles sont consignées dans `LoadedAddon::error`.
    pub fn load_dir(&mut self, dir: &Path) -> Result<(), AddonError> {
        let manifests = Self::discover(dir);
        let by_name: HashMap<String, Manifest> = manifests
            .iter()
            .map(|m| (m.name.clone(), m.clone()))
            .collect();
        let mut done: HashSet<String> = HashSet::new();
        let mut order: Vec<Manifest> = Vec::new();
        fn visit(
            m: &Manifest,
            by_name: &HashMap<String, Manifest>,
            done: &mut HashSet<String>,
            order: &mut Vec<Manifest>,
            stack: &mut Vec<String>,
        ) -> Result<(), AddonError> {
            if done.contains(&m.name) || stack.contains(&m.name) {
                return Ok(());
            }
            stack.push(m.name.clone());
            for dep in &m.dependencies {
                match by_name.get(dep) {
                    Some(d) => visit(d, by_name, done, order, stack)?,
                    None => {
                        return Err(AddonError::MissingDependency {
                            addon: m.name.clone(),
                            dep: dep.clone(),
                        })
                    }
                }
            }
            for dep in &m.optional_deps {
                if let Some(d) = by_name.get(dep) {
                    visit(d, by_name, done, order, stack)?;
                }
            }
            stack.pop();
            done.insert(m.name.clone());
            order.push(m.clone());
            Ok(())
        }
        for m in &manifests {
            if m.load_on_demand {
                continue;
            }
            if let Err(e) = visit(m, &by_name, &mut done, &mut order, &mut Vec::new()) {
                log::warn!("addons : {e}");
                self.addons.push(LoadedAddon {
                    manifest: m.clone(),
                    enabled: false,
                    error: Some(e.to_string()),
                });
            }
        }
        for m in order {
            self.load_addon(m);
        }
        self.fire_event("ADDONS_LOADED", &[]);
        Ok(())
    }

    /// Charge un AddOn : SavedVariables puis fichiers Lua dans l'ordre du `.toc`.
    pub fn load_addon(&mut self, manifest: Manifest) {
        let mut loaded = LoadedAddon {
            manifest: manifest.clone(),
            enabled: true,
            error: None,
        };
        if let Err(e) = self.restore_saved(&manifest) {
            log::warn!("addons : SavedVariables de {} : {e}", manifest.name);
        }
        let ns = match self.lua.create_table() {
            Ok(t) => t,
            Err(e) => {
                loaded.error = Some(e.to_string());
                self.addons.push(loaded);
                return;
            }
        };
        for file in manifest.lua_files() {
            let src = match std::fs::read(&file) {
                Ok(s) => s,
                Err(e) => {
                    loaded.error = Some(format!("{} : {e}", file.display()));
                    loaded.enabled = false;
                    break;
                }
            };
            let chunk_name = format!(
                "@{}",
                file.strip_prefix(&manifest.dir).unwrap_or(&file).display()
            );
            let result = self
                .lua
                .load(&src)
                .set_name(chunk_name)
                .call::<()>((manifest.name.clone(), ns.clone()));
            if let Err(e) = result {
                log::error!("addons : {} : {e}", file.display());
                loaded.error = Some(e.to_string());
                loaded.enabled = false;
                break;
            }
        }
        for f in &manifest.files {
            if f.extension()
                .map(|e| e.eq_ignore_ascii_case("xml"))
                .unwrap_or(false)
            {
                log::warn!(
                    "addons : {} : les fichiers XML ne sont pas pris en charge ({})",
                    manifest.name,
                    f.display()
                );
            }
        }
        if loaded.enabled {
            self.fire_event(
                "ADDON_LOADED",
                &[Value::String(
                    self.lua.create_string(&manifest.name).unwrap(),
                )],
            );
        }
        self.addons.push(loaded);
    }

    fn saved_path(&self, manifest: &Manifest) -> PathBuf {
        self.saved_dir.join(format!("{}.lua", manifest.name))
    }

    fn restore_saved(&self, manifest: &Manifest) -> Result<(), AddonError> {
        if manifest.saved_variables.is_empty() && manifest.saved_variables_per_character.is_empty()
        {
            return Ok(());
        }
        let path = self.saved_path(manifest);
        match std::fs::read(&path) {
            Ok(src) => {
                self.lua
                    .load(&src)
                    .set_name(format!("@{}", path.display()))
                    .exec()?;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AddonError::Io(path, e)),
        }
    }

    /// Écrit les SavedVariables de tous les AddOns.
    pub fn save_all(&self) -> Result<(), AddonError> {
        std::fs::create_dir_all(&self.saved_dir)
            .map_err(|e| AddonError::Io(self.saved_dir.clone(), e))?;
        for a in &self.addons {
            let m = &a.manifest;
            let mut names = m.saved_variables.clone();
            names.extend(m.saved_variables_per_character.iter().cloned());
            if names.is_empty() {
                continue;
            }
            let src = crate::saved::dump_globals(&self.lua, &names)?;
            let path = self.saved_path(m);
            std::fs::write(&path, src).map_err(|e| AddonError::Io(path, e))?;
        }
        Ok(())
    }

    /// Déclenche un événement pour toutes les frames enregistrées : `OnEvent(self, event, ...)`.
    pub fn fire_event(&self, event: &str, args: &[Value]) {
        let listeners = self.shared.borrow().ui.listeners(event);
        for id in listeners {
            let Ok(Some(f)) = get_script(&self.lua, id, "OnEvent") else {
                continue;
            };
            let me = match self.lua.create_userdata(FrameHandle {
                id,
                shared: self.shared.clone(),
            }) {
                Ok(u) => u,
                Err(_) => continue,
            };
            let mut call_args: Vec<Value> = vec![
                Value::UserData(me),
                Value::String(self.lua.create_string(event).unwrap()),
            ];
            call_args.extend(args.iter().cloned());
            if let Err(e) = f.call::<()>(Variadic::from_iter(call_args)) {
                log::error!("addons : OnEvent {event} (frame {id}) : {e}");
            }
        }
    }

    /// Événement avec des arguments simples (chaînes/nombres).
    pub fn fire_event_str(&self, event: &str, args: &[&str]) {
        let vals: Vec<Value> = args
            .iter()
            .map(|a| Value::String(self.lua.create_string(a).unwrap()))
            .collect();
        self.fire_event(event, &vals);
    }

    /// Une image : `OnUpdate(self, elapsed)` sur les frames visibles, minuteurs échus.
    pub fn update(&self, elapsed: f64) {
        {
            let mut s = self.shared.borrow_mut();
            s.host.time += elapsed;
        }
        let (ids, now) = {
            let s = self.shared.borrow();
            let ids: Vec<usize> =
                s.ui.on_update
                    .iter()
                    .copied()
                    .filter(|&id| s.ui.is_visible(id))
                    .collect();
            (ids, s.host.time)
        };
        for id in ids {
            let Ok(Some(f)) = get_script(&self.lua, id, "OnUpdate") else {
                continue;
            };
            let Ok(me) = self.lua.create_userdata(FrameHandle {
                id,
                shared: self.shared.clone(),
            }) else {
                continue;
            };
            if let Err(e) = f.call::<()>((me, elapsed)) {
                log::error!("addons : OnUpdate (frame {id}) : {e}");
            }
        }
        // Minuteurs
        let due: Vec<usize> = {
            let s = self.shared.borrow();
            s.timers
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.cancelled && t.due <= now)
                .map(|(i, _)| i)
                .collect()
        };
        for i in due {
            let (func, period) = {
                let s = self.shared.borrow();
                let t = &s.timers[i];
                (self.lua.registry_value::<Function>(&t.func).ok(), t.period)
            };
            if let Some(f) = func {
                if let Err(e) = f.call::<()>(()) {
                    log::error!("addons : minuteur : {e}");
                }
            }
            let mut s = self.shared.borrow_mut();
            match period {
                Some(p) => s.timers[i].due = now + p,
                None => s.timers[i].cancelled = true,
            }
        }
        let mut s = self.shared.borrow_mut();
        s.timers.retain(|t| !t.cancelled);
    }

    /// Exécute une commande slash (`/hello args`). Rend `false` si aucune commande ne correspond.
    pub fn run_slash(&self, line: &str) -> bool {
        let line = line.trim();
        if !line.starts_with('/') {
            return false;
        }
        let (cmd, args) = line
            .split_once(' ')
            .map(|(c, a)| (c, a.trim()))
            .unwrap_or((line, ""));
        let g = self.lua.globals();
        let Ok(list) = g.get::<Table>("SlashCmdList") else {
            return false;
        };
        // SLASH_<KEY><n> = "/cmd" → SlashCmdList["<KEY>"]
        for pair in g.pairs::<String, Value>() {
            let Ok((name, value)) = pair else { continue };
            let Some(rest) = name.strip_prefix("SLASH_") else {
                continue;
            };
            let Value::String(s) = value else { continue };
            let Ok(alias) = s.to_str() else { continue };
            if !alias.eq_ignore_ascii_case(cmd) {
                continue;
            }
            let key: String = rest
                .trim_end_matches(|c: char| c.is_ascii_digit())
                .to_string();
            if let Ok(Value::Function(f)) = list.get::<Value>(key.as_str()) {
                if let Err(e) = f.call::<()>((args.to_string(), line.to_string())) {
                    log::error!("addons : /{key} : {e}");
                }
                return true;
            }
        }
        false
    }

    /// Clic sur une frame (par le moteur) : `OnClick(self, button, down)`.
    pub fn click(&self, id: usize, button: &str) {
        if let Ok(Some(f)) = get_script(&self.lua, id, "OnClick") {
            if let Ok(me) = self.lua.create_userdata(FrameHandle {
                id,
                shared: self.shared.clone(),
            }) {
                if let Err(e) = f.call::<()>((me, button.to_string(), false)) {
                    log::error!("addons : OnClick (frame {id}) : {e}");
                }
            }
        }
    }

    /// Survol : `OnEnter` / `OnLeave`.
    pub fn hover(&self, id: usize, entered: bool) {
        let handler = if entered { "OnEnter" } else { "OnLeave" };
        if let Ok(Some(f)) = get_script(&self.lua, id, handler) {
            if let Ok(me) = self.lua.create_userdata(FrameHandle {
                id,
                shared: self.shared.clone(),
            }) {
                let _ = f.call::<()>((me, true));
            }
        }
    }

    /// Récupère et vide les commandes émises par les AddOns.
    pub fn drain_commands(&self) -> Vec<HostCommand> {
        std::mem::take(&mut self.shared.borrow_mut().commands)
    }

    /// Met à jour l'état du jeu visible par les AddOns.
    pub fn set_host(&self, f: impl FnOnce(&mut HostState)) {
        f(&mut self.shared.borrow_mut().host);
    }

    /// Exécute un bout de Lua (console de développement).
    pub fn run_script(&self, src: &str) -> Result<(), AddonError> {
        self.lua.load(src).set_name("@console").exec()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_addon(root: &Path, name: &str, toc: &str, lua: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.toc")), toc).unwrap();
        std::fs::write(dir.join("core.lua"), lua).unwrap();
    }

    #[test]
    fn load_events_slash_timers_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let addons = tmp.path().join("addons");
        make_addon(
            &addons,
            "Hello",
            "## Title: Hello\n## SavedVariables: HelloDB\ncore.lua\n",
            r#"
local name, ns = ...
assert(name == "Hello")
HelloDB = HelloDB or { logins = 0 }
local f = CreateFrame("Frame", "HelloFrame", UIParent)
f:SetSize(200, 50)
f:SetPoint("CENTER", UIParent, "CENTER", 0, 100)
local label = f:CreateFontString(nil, "OVERLAY")
label:SetPoint("CENTER")
f:RegisterEvent("PLAYER_LOGIN")
f:RegisterEvent("CHAT_MSG_SAY")
f:SetScript("OnEvent", function(self, event, ...)
  if event == "PLAYER_LOGIN" then
    HelloDB.logins = HelloDB.logins + 1
    label:SetText("Bonjour " .. UnitName("player"))
  elseif event == "CHAT_MSG_SAY" then
    ns.last = select(1, ...)
  end
end)
ns.ticks = 0
f:SetScript("OnUpdate", function(self, elapsed) ns.ticks = ns.ticks + 1 end)
SLASH_HELLO1 = "/hello"
SLASH_HELLO2 = "/hi"
SlashCmdList["HELLO"] = function(msg) print("slash", msg); SendChatMessage("coucou " .. msg, "SAY") end
C_Timer.After(1.5, function() ns.fired = true end)
ns.frame = f
"#,
        );
        make_addon(
            &addons,
            "Dep",
            "## Title: Dep\n## Dependencies: Hello\ncore.lua\n",
            "DepLoaded = HelloFrame ~= nil\n",
        );

        let mut mgr = AddonManager::new(tmp.path().join("saved")).unwrap();
        mgr.set_host(|h| h.player_name = "Testeur".into());
        mgr.load_dir(&addons).unwrap();
        assert_eq!(mgr.addons.len(), 2);
        assert!(
            mgr.addons.iter().all(|a| a.enabled),
            "{:?}",
            mgr.addons
                .iter()
                .map(|a| a.error.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            mgr.addons[0].manifest.name, "Hello",
            "les dépendances se chargent d'abord"
        );
        assert!(mgr.lua.globals().get::<bool>("DepLoaded").unwrap());

        mgr.fire_event("PLAYER_LOGIN", &[]);
        mgr.fire_event_str("CHAT_MSG_SAY", &["salut", "Autre"]);
        let id = mgr.shared.borrow().ui.by_name("HelloFrame").unwrap();
        let label_id = mgr.shared.borrow().ui.get(id).unwrap().children[0];
        assert_eq!(
            mgr.shared.borrow().ui.get(label_id).unwrap().text,
            "Bonjour Testeur"
        );

        assert!(mgr.run_slash("/hi tout le monde"));
        assert!(!mgr.run_slash("/inconnu"));
        let cmds = mgr.drain_commands();
        assert_eq!(cmds[0], HostCommand::Print("slash tout le monde".into()));
        assert_eq!(
            cmds[1],
            HostCommand::Chat {
                message: "coucou tout le monde".into(),
                kind: "SAY".into(),
                target: None
            }
        );

        mgr.update(1.0);
        mgr.update(1.0);
        let rects = mgr.shared.borrow().ui.layout(1280.0, 720.0);
        assert_eq!(rects[id].unwrap().width, 200.0);

        // Minuteur échu, OnUpdate appelé deux fois : lisible via la table de l'AddOn.
        mgr.run_script("assert(select(2, ...) == nil)").unwrap();
        mgr.save_all().unwrap();
        let saved = std::fs::read_to_string(tmp.path().join("saved/Hello.lua")).unwrap();
        assert!(saved.contains("logins = 1"), "{saved}");

        // Rechargement : la SavedVariable est restaurée avant les fichiers Lua.
        let mut mgr2 = AddonManager::new(tmp.path().join("saved")).unwrap();
        mgr2.load_dir(&addons).unwrap();
        mgr2.fire_event("PLAYER_LOGIN", &[]);
        mgr2.save_all().unwrap();
        let saved = std::fs::read_to_string(tmp.path().join("saved/Hello.lua")).unwrap();
        assert!(saved.contains("logins = 2"), "{saved}");
    }
}
