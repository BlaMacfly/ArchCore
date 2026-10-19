//! Chargement des AddOns, dispatch des événements, commandes slash, minuteurs, souris et clavier,
//! SavedVariables, variables de configuration (CVars).
//!
//! Comme sous WoW, les AddOns se chargent à l'entrée en jeu (le nom du personnage est alors
//! connu, ce qui permet les SavedVariables par personnage) : `ADDON_LOADED` pour chaque AddOn,
//! puis `VARIABLES_LOADED`, puis le moteur émet `PLAYER_LOGIN` et `PLAYER_ENTERING_WORLD`.

use crate::frame::{call_script, frame_object, get_script};
use crate::manifest::Manifest;
use crate::ui::{FrameKind, TextureSource, UiTree};
use mlua::{ChunkMode, Function, IntoLuaMulti, Lua, MultiValue, RegistryKey, Table, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
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

/// Une unité (personnage, PNJ, monstre) telle que les AddOns la voient.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitInfo {
    pub cid: i32,
    pub tid: i32,
    pub name: String,
    pub level: i64,
    pub hp: Option<i64>,
    /// PV maximum : connu du serveur, sinon le plus haut observé.
    pub hp_max: Option<i64>,
    pub mp: Option<i64>,
    pub mp_max: Option<i64>,
    pub position: [f32; 3],
    /// Orientation en degrés (convention Archonia).
    pub facing: f32,
    pub moving: bool,
}

/// Objet de l'inventaire (paquets AgpmItem).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemInfo {
    pub id: i32,
    pub tid: Option<i32>,
    pub name: String,
    pub count: Option<i32>,
    /// 2 = sac, 3 = équipé, 100 = autre (codes du serveur).
    pub status: Option<i32>,
    pub grid: Option<[i32; 3]>,
}

/// Caméra orbitale (angles en degrés).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CameraInfo {
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
    pub target: [f32; 3],
    pub eye: [f32; 3],
}

/// Ce que le jeu expose aux AddOns (mis à jour par le moteur avant chaque image).
#[derive(Debug, Clone, Default)]
pub struct HostState {
    pub time: f64,
    pub framerate: f64,
    pub realm: String,
    pub locale: String,
    pub in_world: bool,
    pub player: Option<UnitInfo>,
    /// Unités en vue (hors joueur), dans l'ordre des jetons `nameplate1`, `nameplate2`…
    pub units: Vec<UnitInfo>,
    /// Cible (CID), choisie par `TargetUnit`/`/target`.
    pub target: Option<i32>,
    pub inventory: Vec<ItemInfo>,
    pub camera: CameraInfo,
    /// Matrice vue × projection (colonnes) pour `WorldToScreen`.
    pub view_proj: [f32; 16],
    /// Taille de l'écran de l'interface (points).
    pub screen: (f32, f32),
    /// Curseur dans le repère de l'interface (origine en bas à gauche).
    pub cursor: Option<(f32, f32)>,
    /// Touches de modification : Maj, Ctrl, Alt.
    pub modifiers: [bool; 3],
    pub sectors: usize,
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
    /// `PlayMusic(chemin)` : musique forcée en boucle ; `None` (`StopMusic`) rend la main à la
    /// musique de zone du client.
    PlayMusic(Option<String>),
    ReloadUi,
    /// Commande `/…` du client ou d'un AddOn (`RunMacroText`, `ArchRust.RunCommand`).
    Slash(String),
    /// Marche vers un point.
    MoveTo([f32; 3]),
    /// Caméra : distance, lacet°, tangage° (`None` = inchangé).
    SetCamera {
        distance: Option<f32>,
        yaw: Option<f32>,
        pitch: Option<f32>,
    },
    /// Capture d'écran (`Screenshot()`), fichier facultatif.
    Screenshot(Option<String>),
    Quit,
}

pub struct Timer {
    pub id: u64,
    pub due: f64,
    pub period: Option<f64>,
    /// Nombre d'appels restants (`NewTicker(…, n)`).
    pub remaining: Option<i64>,
    pub func: RegistryKey,
    /// Objet rendu par `NewTicker`/`NewTimer`, passé au rappel.
    pub handle: Option<RegistryKey>,
    pub cancelled: bool,
}

#[derive(Debug, Clone)]
pub struct LoadedAddon {
    pub manifest: Manifest,
    pub enabled: bool,
    pub loaded: bool,
    pub error: Option<String>,
}

/// Bouton de souris enfoncé sur une frame.
#[derive(Debug, Clone)]
struct Press {
    id: usize,
    button: String,
    start: (f32, f32),
    dragging: bool,
}

/// État partagé entre Rust et les fermetures Lua.
pub struct SharedState {
    pub ui: UiTree,
    pub host: HostState,
    pub commands: Vec<HostCommand>,
    pub timers: Vec<Timer>,
    pub next_timer: u64,
    pub addons: Vec<LoadedAddon>,
    /// AddOns découverts (y compris ceux à la demande).
    pub manifests: Vec<Manifest>,
    pub addons_root: PathBuf,
    pub saved_dir: PathBuf,
    /// AddOn dont les fichiers sont en cours d'exécution (chemins relatifs, `require`).
    pub current: Option<(String, PathBuf)>,
    pub logged_in: bool,
    pub cvars: BTreeMap<String, String>,
    pub cvar_defaults: BTreeMap<String, String>,
    /// Teinte des modèles par CID (`ArchRust.SetUnitTint`).
    pub unit_tints: HashMap<i32, [f32; 4]>,
    /// Couleurs de l'interface native (`ArchRust.SetUIColor`) ; `ui_colors_dirty` à appliquer.
    pub ui_colors: BTreeMap<String, [f32; 4]>,
    pub ui_colors_dirty: bool,
    /// Raccourcis clavier (`ArchRust.SetBinding`) : touche → fonction ou commande.
    pub bindings: HashMap<String, RegistryKey>,
    pub errors: Vec<String>,
    hovered: Option<usize>,
    press: Option<Press>,
    /// Dernier événement du journal de combat (`CombatLogGetCurrentEventInfo`).
    pub combat_log: Vec<CombatValue>,
    started: std::time::Instant,
}

/// Valeur simple du journal de combat.
#[derive(Debug, Clone, PartialEq)]
pub enum CombatValue {
    Nil,
    Bool(bool),
    Num(f64),
    Int(i64),
    Str(String),
}

pub type Shared = Rc<RefCell<SharedState>>;

/// Consigne une erreur Lua : journal, liste `GetErrors`, ligne rouge dans la discussion (une
/// fois par message), gestionnaire d'erreurs de l'AddOn s'il en a posé un.
pub fn report_error(lua: &Lua, shared: &Shared, context: &str, e: &mlua::Error) {
    let msg = format!("{context} : {e}");
    log::error!("addons : {msg}");
    let first_line = msg.lines().next().unwrap_or("").to_string();
    let new = {
        let mut s = shared.borrow_mut();
        let new = !s.errors.contains(&first_line);
        s.errors.push(first_line.clone());
        if s.errors.len() > 100 {
            s.errors.remove(0);
        }
        if new {
            s.commands
                .push(HostCommand::Print(format!("|cffff4040[Lua] {first_line}|r")));
        }
        new
    };
    if new {
        if let Ok(Value::Function(h)) = lua.named_registry_value::<Value>("ARCHRUST_ERRORHANDLER") {
            let _ = h.call::<()>(first_line);
        }
    }
}

fn safe_component(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Chemin relatif sûr (pas d'absolu, pas de `..`).
pub fn safe_relative(p: &str) -> Option<PathBuf> {
    let path = PathBuf::from(p.replace('\\', "/"));
    if path
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    {
        Some(path)
    } else {
        None
    }
}

const IMAGE_EXTS: [&str; 6] = ["png", "tga", "jpg", "jpeg", "bmp", "dds"];

fn existing_with_ext(p: PathBuf) -> PathBuf {
    if p.is_file() {
        return p;
    }
    for ext in IMAGE_EXTS {
        let c = p.with_extension(ext);
        if c.is_file() {
            return c;
        }
    }
    p
}

/// Résout un chemin de média (texture, police) donné par un AddOn :
/// - `Interface\AddOns\<Nom>\fichier` → fichier de l'AddOn `<Nom>` ;
/// - `Interface\…` (textures de WoW, ex. `Interface\Buttons\WHITE8X8`) → `None` = couleur unie ;
/// - `Fonts\…` → `None` = police du client ;
/// - chemin avec dossier ou extension, pendant le chargement d'un AddOn → relatif à cet AddOn ;
/// - autre → texture du client (`Game`).
pub fn resolve_media(s: &SharedState, path: &str) -> Option<TextureSource> {
    let norm = path.replace('\\', "/");
    let lower = norm.to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    if let Some(rest) = lower.strip_prefix("interface/addons/") {
        let rest = &norm[norm.len() - rest.len()..];
        let rel = safe_relative(rest)?;
        return Some(TextureSource::File(existing_with_ext(s.addons_root.join(rel))));
    }
    if lower.starts_with("interface/") || lower.starts_with("fonts/") {
        return None;
    }
    if norm.contains('/') || Path::new(&norm).extension().is_some() {
        if let Some((_, dir)) = &s.current {
            let rel = safe_relative(&norm)?;
            let p = existing_with_ext(dir.join(rel));
            if p.is_file() {
                return Some(TextureSource::File(p));
            }
        }
        // Chemin relatif à la racine des AddOns (`MonAddOn/media/x.png`).
        if let Some(rel) = safe_relative(&norm) {
            let p = existing_with_ext(s.addons_root.join(&rel));
            if p.is_file() {
                return Some(TextureSource::File(p));
            }
        }
    }
    Some(TextureSource::Game(norm))
}

/// Déclenche un événement : `OnEvent(self, event, ...)` sur les frames enregistrées.
pub fn fire(lua: &Lua, shared: &Shared, event: &str, args: MultiValue) {
    match event {
        "PLAYER_LOGIN" => shared.borrow_mut().logged_in = true,
        "PLAYER_LOGOUT" => shared.borrow_mut().logged_in = false,
        _ => {}
    }
    let listeners = shared.borrow().ui.listeners(event);
    for id in listeners {
        let Ok(Some(f)) = get_script(lua, id, "OnEvent") else {
            continue;
        };
        let result = (|| -> mlua::Result<()> {
            let me = frame_object(lua, shared, id)?;
            let mut all = args.clone();
            all.push_front(Value::String(lua.create_string(event)?));
            all.push_front(Value::Table(me));
            f.call::<()>(all)
        })();
        if let Err(e) = result {
            report_error(lua, shared, &format!("OnEvent {event} (frame {id})"), &e);
        }
    }
}

/// Clic sur une frame : bascule des cases à cocher, puis `OnClick(self, button, down)`.
pub fn dispatch_click(lua: &Lua, shared: &Shared, id: usize, button: &str) {
    let (kind, enabled) = {
        let s = shared.borrow();
        match s.ui.get(id) {
            Some(f) => (f.kind, f.enabled),
            None => return,
        }
    };
    if !enabled {
        return;
    }
    if kind == FrameKind::CheckButton {
        if let Some(f) = shared.borrow_mut().ui.get_mut(id) {
            f.checked = !f.checked;
        }
    }
    call_script(lua, shared, id, "PreClick", (button.to_string(), false));
    call_script(lua, shared, id, "OnClick", (button.to_string(), false));
    call_script(lua, shared, id, "PostClick", (button.to_string(), false));
}

/// Chemin du fichier de SavedVariables d'un AddOn (par compte ou par personnage).
fn saved_path(s: &SharedState, addon: &str, per_character: bool) -> PathBuf {
    if per_character {
        let who = s
            .host
            .player
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "_".into());
        s.saved_dir
            .join(safe_component(&s.host.realm))
            .join(safe_component(&who))
            .join(format!("{}.lua", safe_component(addon)))
    } else {
        s.saved_dir.join(format!("{}.lua", safe_component(addon)))
    }
}

fn restore_saved(lua: &Lua, shared: &Shared, manifest: &Manifest) -> Result<(), AddonError> {
    for per_char in [false, true] {
        let names = if per_char {
            &manifest.saved_variables_per_character
        } else {
            &manifest.saved_variables
        };
        if names.is_empty() {
            continue;
        }
        let path = saved_path(&shared.borrow(), &manifest.name, per_char);
        match std::fs::read(&path) {
            Ok(src) => lua
                .load(&src)
                .set_name(format!("@{}", path.display()))
                .set_mode(ChunkMode::Text)
                .exec()?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(AddonError::Io(path, e)),
        }
    }
    Ok(())
}

/// Charge un AddOn : SavedVariables puis fichiers Lua (et `.xml` : `<Script>`/`<Include>`) dans
/// l'ordre du `.toc`, puis `ADDON_LOADED`.
pub fn load_addon(lua: &Lua, shared: &Shared, manifest: &Manifest) -> LoadedAddon {
    let mut loaded = LoadedAddon {
        manifest: manifest.clone(),
        enabled: true,
        loaded: false,
        error: None,
    };
    if let Err(e) = restore_saved(lua, shared, manifest) {
        log::warn!("addons : SavedVariables de {} : {e}", manifest.name);
    }
    let ns = match lua.create_table() {
        Ok(t) => t,
        Err(e) => {
            loaded.error = Some(e.to_string());
            return loaded;
        }
    };
    let previous = shared
        .borrow_mut()
        .current
        .replace((manifest.name.clone(), manifest.dir.clone()));
    for file in manifest.script_files() {
        let src = match std::fs::read(&file) {
            Ok(s) => s,
            Err(e) => {
                loaded.error = Some(format!("{} : {e}", file.display()));
                loaded.enabled = false;
                break;
            }
        };
        let chunk_name = format!(
            "@{}/{}",
            manifest.name,
            file.strip_prefix(&manifest.dir)
                .unwrap_or(&file)
                .display()
                .to_string()
                .replace('\\', "/")
        );
        let result = lua
            .load(&src)
            .set_name(chunk_name)
            .set_mode(ChunkMode::Text)
            .call::<()>((manifest.name.clone(), ns.clone()));
        if let Err(e) = result {
            report_error(lua, shared, &file.display().to_string(), &e);
            loaded.error = Some(e.to_string());
            loaded.enabled = false;
            break;
        }
    }
    shared.borrow_mut().current = previous;
    loaded.loaded = loaded.enabled;
    if loaded.enabled {
        if let Ok(name) = lua.create_string(&manifest.name) {
            fire(
                lua,
                shared,
                "ADDON_LOADED",
                MultiValue::from_iter([Value::String(name), Value::Boolean(false)]),
            );
        }
    }
    loaded
}

/// Charge un AddOn et ses dépendances (`LoadAddOn`). Rend `(chargé, raison)`.
pub fn load_by_name(lua: &Lua, shared: &Shared, name: &str) -> (bool, Option<String>) {
    fn go(
        lua: &Lua,
        shared: &Shared,
        name: &str,
        stack: &mut Vec<String>,
    ) -> (bool, Option<String>) {
        if let Some(a) = shared
            .borrow()
            .addons
            .iter()
            .find(|a| a.manifest.name.eq_ignore_ascii_case(name))
        {
            return if a.loaded {
                (true, None)
            } else {
                (false, Some("DISABLED".into()))
            };
        }
        let manifest = shared
            .borrow()
            .manifests
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .cloned();
        let Some(m) = manifest else {
            return (false, Some("MISSING".into()));
        };
        if stack.contains(&m.name) {
            return (true, None);
        }
        stack.push(m.name.clone());
        for dep in &m.dependencies {
            let (ok, _) = go(lua, shared, dep, stack);
            if !ok {
                let la = LoadedAddon {
                    manifest: m.clone(),
                    enabled: false,
                    loaded: false,
                    error: Some(format!("dépendance manquante : {dep}")),
                };
                shared.borrow_mut().addons.push(la);
                return (false, Some("DEP_MISSING".into()));
            }
        }
        for dep in &m.optional_deps {
            let exists = shared
                .borrow()
                .manifests
                .iter()
                .any(|x| x.name.eq_ignore_ascii_case(dep));
            if exists {
                go(lua, shared, dep, stack);
            }
        }
        stack.pop();
        let la = load_addon(lua, shared, &m);
        let ok = la.loaded;
        let err = la.error.clone();
        shared.borrow_mut().addons.push(la);
        (ok, if ok { None } else { err.or(Some("CORRUPT".into())) })
    }
    go(lua, shared, name, &mut Vec::new())
}

pub struct AddonManager {
    pub lua: Lua,
    pub shared: Shared,
}

/// Entrée souris d'une image, dans le repère de l'interface (origine en bas à gauche).
#[derive(Debug, Clone, Default)]
pub struct PointerInput {
    pub pos: Option<(f32, f32)>,
    /// Boutons pressés / relâchés (`LeftButton`, `RightButton`, `MiddleButton`).
    pub pressed: Vec<String>,
    pub released: Vec<String>,
    pub wheel: f32,
    /// Une fenêtre native du client est sous le curseur : seules les strates HIGH et plus
    /// (dialogues, info-bulles) reçoivent la souris.
    pub native_over: bool,
}

/// Événement d'une EditBox dessinée par le moteur.
#[derive(Debug, Clone, PartialEq)]
pub enum EditEvent {
    Changed(String),
    Enter,
    Escape,
    Tab,
    FocusGained,
    FocusLost,
}

impl AddonManager {
    /// Crée l'état Lua (bibliothèques sûres uniquement) et installe l'API. Les AddOns ne sont pas
    /// encore chargés (voir [`AddonManager::load_dir`]).
    pub fn new(saved_dir: impl Into<PathBuf>) -> Result<Self, AddonError> {
        let lua = crate::lua_env::new_state()?;
        let shared: Shared = Rc::new(RefCell::new(SharedState {
            ui: UiTree::new(),
            host: HostState {
                realm: "ArchCore".into(),
                locale: "frFR".into(),
                ..Default::default()
            },
            commands: Vec::new(),
            timers: Vec::new(),
            next_timer: 1,
            addons: Vec::new(),
            manifests: Vec::new(),
            addons_root: PathBuf::from("addons"),
            saved_dir: saved_dir.into(),
            current: None,
            logged_in: false,
            cvars: BTreeMap::new(),
            cvar_defaults: BTreeMap::new(),
            unit_tints: HashMap::new(),
            ui_colors: BTreeMap::new(),
            ui_colors_dirty: false,
            bindings: HashMap::new(),
            errors: Vec::new(),
            hovered: None,
            press: None,
            combat_log: Vec::new(),
            started: std::time::Instant::now(),
        }));
        crate::lua_env::install(&lua, shared.clone())?;
        crate::api::install(&lua, shared.clone())?;
        crate::libs::install(&lua)?;
        Ok(Self { lua, shared })
    }

    /// Liste des AddOns chargés (ou en erreur).
    pub fn addons(&self) -> Vec<LoadedAddon> {
        self.shared.borrow().addons.clone()
    }

    /// Les AddOns ont-ils été chargés (entrée en jeu) ?
    pub fn is_loaded(&self) -> bool {
        !self.shared.borrow().addons.is_empty()
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

    /// Charge tous les AddOns d'un dossier, dépendances d'abord (hors `LoadOnDemand`). Les erreurs
    /// d'un AddOn ne bloquent pas les autres ; elles sont consignées dans `LoadedAddon::error`.
    pub fn load_dir(&mut self, dir: &Path) -> Result<(), AddonError> {
        let manifests = Self::discover(dir);
        {
            let mut s = self.shared.borrow_mut();
            s.addons_root = dir.to_path_buf();
            s.manifests = manifests.clone();
        }
        let names: HashSet<String> = manifests.iter().map(|m| m.name.clone()).collect();
        for m in &manifests {
            if m.load_on_demand {
                continue;
            }
            if let Some(dep) = m.dependencies.iter().find(|d| !names.contains(*d)) {
                let e = AddonError::MissingDependency {
                    addon: m.name.clone(),
                    dep: dep.clone(),
                };
                log::warn!("addons : {e}");
                self.shared.borrow_mut().addons.push(LoadedAddon {
                    manifest: m.clone(),
                    enabled: false,
                    loaded: false,
                    error: Some(e.to_string()),
                });
                continue;
            }
            load_by_name(&self.lua, &self.shared, &m.name);
        }
        self.fire_event("VARIABLES_LOADED", &[]);
        self.fire_event("ADDONS_LOADED", &[]);
        Ok(())
    }

    /// Charge un AddOn (et ses dépendances) par son nom, comme `LoadAddOn`.
    pub fn load_addon_by_name(&self, name: &str) -> bool {
        load_by_name(&self.lua, &self.shared, name).0
    }

    /// Écrit les SavedVariables de tous les AddOns.
    pub fn save_all(&self) -> Result<(), AddonError> {
        let s = self.shared.borrow();
        for a in &s.addons {
            if !a.loaded {
                continue;
            }
            let m = &a.manifest;
            for (names, per_char) in [
                (&m.saved_variables, false),
                (&m.saved_variables_per_character, true),
            ] {
                if names.is_empty() {
                    continue;
                }
                let src = crate::saved::dump_globals(&self.lua, names)?;
                let path = saved_path(&s, &m.name, per_char);
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| AddonError::Io(dir.to_path_buf(), e))?;
                }
                std::fs::write(&path, src).map_err(|e| AddonError::Io(path, e))?;
            }
        }
        Ok(())
    }

    /// Fin de session : `PLAYER_LOGOUT` (les AddOns nettoient leurs données) puis sauvegarde.
    pub fn shutdown(&self) {
        if self.shared.borrow().logged_in {
            self.fire_event("PLAYER_LEAVING_WORLD", &[]);
            self.fire_event("PLAYER_LOGOUT", &[]);
        }
        if let Err(e) = self.save_all() {
            log::warn!("addons : sauvegarde : {e}");
        }
    }

    /// Déclenche un événement pour toutes les frames enregistrées : `OnEvent(self, event, ...)`.
    pub fn fire_event(&self, event: &str, args: &[Value]) {
        fire(
            &self.lua,
            &self.shared,
            event,
            MultiValue::from_iter(args.iter().cloned()),
        );
    }

    /// Événement avec des arguments quelconques convertibles en Lua.
    pub fn fire(&self, event: &str, args: impl IntoLuaMulti) {
        match args.into_lua_multi(&self.lua) {
            Ok(a) => fire(&self.lua, &self.shared, event, a),
            Err(e) => log::error!("addons : arguments de {event} : {e}"),
        }
    }

    /// Événement avec des arguments simples (chaînes).
    pub fn fire_event_str(&self, event: &str, args: &[&str]) {
        let vals: Vec<Value> = args
            .iter()
            .filter_map(|a| self.lua.create_string(a).ok().map(Value::String))
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
            let mut ids: Vec<usize> =
                s.ui.on_update
                    .iter()
                    .copied()
                    .filter(|&id| s.ui.is_visible(id))
                    .collect();
            ids.sort_unstable();
            (ids, s.host.time)
        };
        for id in ids {
            call_script(&self.lua, &self.shared, id, "OnUpdate", elapsed);
        }
        // Minuteurs (les rappels peuvent en créer d'autres : on ne traite que ceux échus).
        let due: Vec<u64> = {
            let s = self.shared.borrow();
            s.timers
                .iter()
                .filter(|t| !t.cancelled && t.due <= now)
                .map(|t| t.id)
                .collect()
        };
        for id in due {
            let (func, handle, period) = {
                let s = self.shared.borrow();
                let Some(t) = s.timers.iter().find(|t| t.id == id && !t.cancelled) else {
                    continue;
                };
                (
                    self.lua.registry_value::<Function>(&t.func).ok(),
                    t.handle
                        .as_ref()
                        .and_then(|h| self.lua.registry_value::<Value>(h).ok()),
                    t.period,
                )
            };
            if let Some(f) = func {
                let r = match handle {
                    Some(h) => f.call::<()>(h),
                    None => f.call::<()>(()),
                };
                if let Err(e) = r {
                    report_error(&self.lua, &self.shared, "minuteur", &e);
                }
            }
            let mut s = self.shared.borrow_mut();
            if let Some(t) = s.timers.iter_mut().find(|t| t.id == id) {
                match (period, t.remaining.as_mut()) {
                    (Some(_), Some(n)) if *n <= 1 => t.cancelled = true,
                    (Some(p), Some(n)) => {
                        *n -= 1;
                        t.due = now + p;
                    }
                    (Some(p), None) => t.due = now + p,
                    (None, _) => t.cancelled = true,
                }
            }
        }
        let mut s = self.shared.borrow_mut();
        s.timers.retain(|t| !t.cancelled);
    }

    /// Exécute une commande slash (`/hello args`). Rend `false` si aucune commande ne correspond.
    pub fn run_slash(&self, line: &str) -> bool {
        crate::api::run_slash(&self.lua, line)
    }

    /// Clic sur une frame (par le moteur) : `OnClick(self, button, down)`.
    pub fn click(&self, id: usize, button: &str) {
        dispatch_click(&self.lua, &self.shared, id, button);
    }

    /// Survol : `OnEnter` / `OnLeave`.
    pub fn hover(&self, id: usize, entered: bool) {
        let handler = if entered { "OnEnter" } else { "OnLeave" };
        call_script(&self.lua, &self.shared, id, handler, true);
    }

    /// Frame sous le curseur (la souris est alors pour l'interface, pas pour le monde).
    pub fn hovered_frame(&self) -> Option<usize> {
        let s = self.shared.borrow();
        s.hovered.or(s.press.as_ref().map(|p| p.id))
    }

    /// Souris : survol, clics, glisser, déplacement des frames, molette, curseurs (Slider).
    /// Rend `true` si l'interface des AddOns a capté la souris.
    pub fn pointer(&self, input: &PointerInput) -> bool {
        let lua = &self.lua;
        let sh = &self.shared;
        sh.borrow_mut().host.cursor = input.pos;
        // Frame déplacée (`StartMoving`).
        let moving = sh.borrow().ui.moving;
        if let (Some(mv), Some((x, y))) = (moving, input.pos) {
            sh.borrow_mut()
                .ui
                .place_at(mv.id, x - mv.grab.0, y - mv.grab.1);
        }
        let min_strata = if input.native_over {
            crate::ui::strata_rank("HIGH")
        } else {
            0
        };
        let hit = input
            .pos
            .and_then(|(x, y)| sh.borrow().ui.hit_test_min(x, y, min_strata));
        let old = sh.borrow().hovered;
        if hit != old {
            sh.borrow_mut().hovered = hit;
            if let Some(o) = old {
                call_script(lua, sh, o, "OnLeave", true);
            }
            if let Some(n) = hit {
                call_script(lua, sh, n, "OnEnter", true);
            }
        }
        for b in &input.pressed {
            match (hit, input.pos) {
                (Some(id), Some(pos)) => {
                    sh.borrow_mut().press = Some(Press {
                        id,
                        button: b.clone(),
                        start: pos,
                        dragging: false,
                    });
                    call_script(lua, sh, id, "OnMouseDown", b.clone());
                    let kind = sh.borrow().ui.get(id).map(|f| f.kind);
                    if kind == Some(FrameKind::Slider) {
                        self.slide(id, pos);
                    }
                    if kind == Some(FrameKind::EditBox) {
                        let me = frame_object(lua, sh, id).ok();
                        if let Some(me) = me {
                            if let Ok(f) = me.get::<Function>("SetFocus") {
                                let _ = f.call::<()>(me);
                            }
                        }
                    } else {
                        self.clear_focus();
                    }
                }
                _ => self.clear_focus(),
            }
        }
        // Glisser.
        let press = sh.borrow().press.clone();
        if let (Some(p), Some(pos)) = (&press, input.pos) {
            let moved = (pos.0 - p.start.0).abs() + (pos.1 - p.start.1).abs() > 4.0;
            let (kind, drag) = sh
                .borrow()
                .ui
                .get(p.id)
                .map(|f| {
                    (
                        f.kind,
                        f.drag_buttons
                            .iter()
                            .any(|d| d == &p.button || d == "AnyUp" || d == "AnyDown"),
                    )
                })
                .unwrap_or((FrameKind::Frame, false));
            if kind == FrameKind::Slider {
                self.slide(p.id, pos);
            } else if moved && drag && !p.dragging {
                if let Some(pp) = sh.borrow_mut().press.as_mut() {
                    pp.dragging = true;
                }
                call_script(lua, sh, p.id, "OnDragStart", p.button.clone());
            }
        }
        for b in &input.released {
            let press = sh.borrow().press.clone();
            let Some(p) = press.filter(|p| &p.button == b) else {
                continue;
            };
            sh.borrow_mut().press = None;
            call_script(lua, sh, p.id, "OnMouseUp", b.clone());
            if p.dragging {
                call_script(lua, sh, p.id, "OnDragStop", ());
            } else if hit == Some(p.id) {
                dispatch_click(lua, sh, p.id, b);
            }
            let moving = sh.borrow().ui.moving;
            if moving.is_some_and(|m| m.id == p.id) {
                sh.borrow_mut().ui.moving = None;
            }
        }
        if input.wheel != 0.0 {
            if let Some((x, y)) = input.pos {
                let target = {
                    let s = sh.borrow();
                    s.ui.draw_order().into_iter().rev().find(|&id| {
                        let f = &s.ui.frames[id];
                        f.mouse_wheel_enabled
                            && crate::ui::strata_rank(s.ui.effective_strata(id)) >= min_strata
                            && s.ui.is_visible(id)
                            && s.ui
                                .rects
                                .get(id)
                                .copied()
                                .flatten()
                                .is_some_and(|r| r.contains(x, y))
                    })
                };
                if let Some(id) = target {
                    call_script(lua, sh, id, "OnMouseWheel", input.wheel.signum());
                    return true;
                }
            }
        }
        let s = sh.borrow();
        s.hovered.is_some() || s.press.is_some() || s.ui.moving.is_some()
    }

    fn slide(&self, id: usize, pos: (f32, f32)) {
        let (rect, vertical, lo, hi) = {
            let s = self.shared.borrow();
            let Some(f) = s.ui.get(id) else { return };
            (
                s.ui.rects.get(id).copied().flatten(),
                f.orientation == "VERTICAL",
                f.min_value,
                f.max_value,
            )
        };
        let Some(r) = rect else { return };
        let frac = if vertical {
            1.0 - ((pos.1 - r.bottom) / r.height.max(1.0))
        } else {
            (pos.0 - r.left) / r.width.max(1.0)
        }
        .clamp(0.0, 1.0);
        let v = lo + (hi - lo) * frac;
        if let Ok(me) = frame_object(&self.lua, &self.shared, id) {
            // Passe par la logique de `SetValue` (pas, bornes) puis signale l'action utilisateur.
            let before = self.shared.borrow().ui.get(id).map(|f| f.value);
            let step = self.shared.borrow().ui.get(id).map(|f| f.value_step).unwrap_or(0.0);
            let mut v = v;
            if step > 0.0 {
                v = lo + ((v - lo) / step).round() * step;
            }
            if before.is_some_and(|b| (b - v).abs() > f32::EPSILON) {
                if let Some(f) = self.shared.borrow_mut().ui.get_mut(id) {
                    f.value = v;
                }
                call_script(&self.lua, &self.shared, id, "OnValueChanged", (crate::frame::num(v), true));
            }
            let _ = me;
        }
    }

    fn clear_focus(&self) {
        let focused: Vec<usize> = {
            let s = self.shared.borrow();
            s.ui.frames.iter().filter(|f| f.focused).map(|f| f.id).collect()
        };
        for id in focused {
            if let Some(f) = self.shared.borrow_mut().ui.get_mut(id) {
                f.focused = false;
            }
            call_script(&self.lua, &self.shared, id, "OnEditFocusLost", ());
        }
    }

    /// Une EditBox focalisée attend le clavier (le moteur ne doit pas l'utiliser).
    pub fn wants_keyboard(&self) -> bool {
        self.shared.borrow().ui.frames.iter().any(|f| f.focused)
    }

    /// Événement d'une EditBox dessinée par le moteur.
    pub fn edit_event(&self, id: usize, ev: EditEvent) {
        let lua = &self.lua;
        let sh = &self.shared;
        match ev {
            EditEvent::Changed(text) => {
                if let Some(h) = crate::frame::handle(&Value::Table(
                    match frame_object(lua, sh, id) {
                        Ok(t) => t,
                        Err(_) => return,
                    },
                )) {
                    crate::frame::set_text(lua, &h, text, true);
                }
            }
            EditEvent::Enter => {
                call_script(lua, sh, id, "OnEnterPressed", ());
            }
            EditEvent::Escape => {
                if !call_script(lua, sh, id, "OnEscapePressed", ()) {
                    self.clear_focus();
                }
            }
            EditEvent::Tab => {
                call_script(lua, sh, id, "OnTabPressed", ());
            }
            EditEvent::FocusGained => {
                let was = sh.borrow().ui.get(id).map(|f| f.focused).unwrap_or(false);
                if !was {
                    if let Some(f) = sh.borrow_mut().ui.get_mut(id) {
                        f.focused = true;
                    }
                    call_script(lua, sh, id, "OnEditFocusGained", ());
                }
            }
            EditEvent::FocusLost => {
                let was = sh.borrow().ui.get(id).map(|f| f.focused).unwrap_or(false);
                if was {
                    if let Some(f) = sh.borrow_mut().ui.get_mut(id) {
                        f.focused = false;
                    }
                    call_script(lua, sh, id, "OnEditFocusLost", ());
                }
            }
        }
    }

    /// Touche du clavier (nom WoW : `A`, `F1`, `ESCAPE`, `SHIFT-A`…), quand ni la discussion ni
    /// une EditBox n'ont le clavier. Raccourcis `ArchRust.SetBinding`, puis `OnKeyDown`/`OnKeyUp`
    /// des frames qui ont `EnableKeyboard(true)`. Rend `true` si un raccourci a été exécuté.
    pub fn key(&self, key: &str, down: bool) -> bool {
        let lua = &self.lua;
        let sh = &self.shared;
        let mut handled = false;
        if down {
            let binding = {
                let s = sh.borrow();
                s.bindings
                    .get(key)
                    .and_then(|k| lua.registry_value::<Value>(k).ok())
            };
            match binding {
                Some(Value::Function(f)) => {
                    if let Err(e) = f.call::<()>((key.to_string(), true)) {
                        report_error(lua, sh, &format!("raccourci {key}"), &e);
                    }
                    handled = true;
                }
                Some(Value::String(s)) => {
                    if let Ok(cmd) = s.to_str() {
                        sh.borrow_mut()
                            .commands
                            .push(HostCommand::Slash(cmd.to_string()));
                    }
                    handled = true;
                }
                _ => {}
            }
        }
        let ids: Vec<usize> = {
            let s = sh.borrow();
            s.ui.frames
                .iter()
                .filter(|f| f.keyboard_enabled && s.ui.is_visible(f.id))
                .map(|f| f.id)
                .collect()
        };
        for id in ids {
            call_script(
                lua,
                sh,
                id,
                if down { "OnKeyDown" } else { "OnKeyUp" },
                key.to_string(),
            );
        }
        if down {
            self.fire("ARCHRUST_KEY_DOWN", key.to_string());
        }
        handled
    }

    /// Calcule la mise en page (le moteur fournit la mesure des textes) et la garde pour la souris.
    pub fn layout(
        &self,
        screen_w: f32,
        screen_h: f32,
        measure: &dyn Fn(&crate::ui::FrameNode) -> (f32, f32),
    ) {
        let mut s = self.shared.borrow_mut();
        s.host.screen = (screen_w, screen_h);
        s.ui.screen = (screen_w, screen_h);
        let rects = s.ui.layout_with(screen_w, screen_h, measure);
        s.ui.rects = rects;
    }

    /// Récupère et vide les commandes émises par les AddOns.
    pub fn drain_commands(&self) -> Vec<HostCommand> {
        std::mem::take(&mut self.shared.borrow_mut().commands)
    }

    /// Met à jour l'état du jeu visible par les AddOns.
    pub fn set_host(&self, f: impl FnOnce(&mut HostState)) {
        f(&mut self.shared.borrow_mut().host);
    }

    /// Met à jour joueur, unités en vue et inventaire, et émet les événements correspondants :
    /// `NAME_PLATE_UNIT_REMOVED`/`ADDED`, `UNIT_HEALTH`, `UNIT_POWER_UPDATE`,
    /// `PLAYER_STARTED_MOVING`/`STOPPED_MOVING`, `PLAYER_TARGET_CHANGED`, `BAG_UPDATE`,
    /// `PLAYER_EQUIPMENT_CHANGED`, `UNIT_INVENTORY_CHANGED`.
    pub fn sync_world(
        &self,
        player: Option<UnitInfo>,
        mut units: Vec<UnitInfo>,
        inventory: Option<Vec<ItemInfo>>,
    ) {
        let (old_player, old_units, old_target) = {
            let s = self.shared.borrow();
            (s.host.player.clone(), s.host.units.clone(), s.host.target)
        };
        // PV max : le plus haut observé quand le serveur ne le donne pas.
        let max_of = |cid: i32, hp: Option<i64>, known: Option<i64>| -> Option<i64> {
            let prev = old_units
                .iter()
                .chain(old_player.iter())
                .find(|u| u.cid == cid)
                .and_then(|u| u.hp_max);
            match (known, hp, prev) {
                (Some(k), _, _) => Some(k),
                (None, Some(h), Some(p)) => Some(h.max(p)),
                (None, Some(h), None) => Some(h),
                (None, None, p) => p,
            }
        };
        let player = player.map(|mut p| {
            p.hp_max = max_of(p.cid, p.hp, p.hp_max);
            p
        });
        for u in &mut units {
            u.hp_max = max_of(u.cid, u.hp, u.hp_max);
        }
        // Disparitions (avant la mise à jour, pour que le jeton soit encore lisible).
        for (i, u) in old_units.iter().enumerate() {
            if !units.iter().any(|n| n.cid == u.cid) {
                self.fire(
                    "NAME_PLATE_UNIT_REMOVED",
                    (format!("nameplate{}", i + 1), crate::api::guid_of(u, false)),
                );
            }
        }
        let target_gone = old_target.is_some_and(|t| {
            !units.iter().any(|u| u.cid == t) && player.as_ref().is_none_or(|p| p.cid != t)
        });
        {
            let mut s = self.shared.borrow_mut();
            s.host.player = player.clone();
            s.host.units = units.clone();
            if target_gone {
                s.host.target = None;
            }
        }
        if target_gone {
            self.fire_event("PLAYER_TARGET_CHANGED", &[]);
        }
        let target = self.shared.borrow().host.target;
        for (i, u) in units.iter().enumerate() {
            let token = format!("nameplate{}", i + 1);
            match old_units.iter().find(|o| o.cid == u.cid) {
                None => self.fire(
                    "NAME_PLATE_UNIT_ADDED",
                    (token, crate::api::guid_of(u, false)),
                ),
                Some(o) => {
                    if o.hp != u.hp {
                        self.fire("UNIT_HEALTH", token.clone());
                        if target == Some(u.cid) {
                            self.fire("UNIT_HEALTH", "target");
                        }
                    }
                    if o.mp != u.mp {
                        self.fire("UNIT_POWER_UPDATE", (token, "MANA"));
                    }
                }
            }
        }
        if let (Some(o), Some(p)) = (&old_player, &player) {
            if o.hp != p.hp || o.hp_max != p.hp_max {
                self.fire("UNIT_HEALTH", "player");
                self.fire("UNIT_MAXHEALTH", "player");
            }
            if o.mp != p.mp {
                self.fire("UNIT_POWER_UPDATE", ("player", "MANA"));
            }
            if !o.moving && p.moving {
                self.fire_event("PLAYER_STARTED_MOVING", &[]);
            } else if o.moving && !p.moving {
                self.fire_event("PLAYER_STOPPED_MOVING", &[]);
            }
        }
        if let Some(inv) = inventory {
            let old = self.shared.borrow().host.inventory.clone();
            if old != inv {
                let equipped = |v: &[ItemInfo]| -> Vec<(i32, Option<i32>)> {
                    let mut e: Vec<_> = v
                        .iter()
                        .filter(|i| i.status == Some(3))
                        .map(|i| (i.id, i.tid))
                        .collect();
                    e.sort();
                    e
                };
                let equip_changed = equipped(&old) != equipped(&inv);
                self.shared.borrow_mut().host.inventory = inv;
                self.fire("BAG_UPDATE", 0);
                self.fire_event("BAG_UPDATE_DELAYED", &[]);
                if equip_changed {
                    self.fire_event("PLAYER_EQUIPMENT_CHANGED", &[]);
                }
                self.fire("UNIT_INVENTORY_CHANGED", "player");
            }
        }
    }

    /// Action de combat entre deux CID : `UNIT_COMBAT` et `COMBAT_LOG_EVENT_UNFILTERED`
    /// (`CombatLogGetCurrentEventInfo`).
    pub fn combat(&self, source_cid: i32, dest_cid: i32, delta_hp: i64, remaining_hp: Option<i64>) {
        let (src, dst, player_cid, target) = {
            let s = self.shared.borrow();
            let find = |cid: i32| {
                s.host
                    .player
                    .iter()
                    .chain(s.host.units.iter())
                    .find(|u| u.cid == cid)
                    .cloned()
            };
            (
                find(source_cid),
                find(dest_cid),
                s.host.player.as_ref().map(|p| p.cid),
                s.host.target,
            )
        };
        let guid = |u: &Option<UnitInfo>, cid: i32| match u {
            Some(u) => crate::api::guid_of(u, Some(u.cid) == player_cid),
            None => format!("Unit-0-{cid}"),
        };
        let name = |u: &Option<UnitInfo>| u.as_ref().map(|u| u.name.clone()).unwrap_or_default();
        let sub = if delta_hp < 0 { "SWING_DAMAGE" } else { "SPELL_HEAL" };
        let amount = delta_hp.abs();
        let now = self.shared.borrow().host.time;
        self.shared.borrow_mut().combat_log = vec![
            CombatValue::Num(now),
            CombatValue::Str(sub.into()),
            CombatValue::Bool(false),
            CombatValue::Str(guid(&src, source_cid)),
            CombatValue::Str(name(&src)),
            CombatValue::Num(0.0),
            CombatValue::Num(0.0),
            CombatValue::Str(guid(&dst, dest_cid)),
            CombatValue::Str(name(&dst)),
            CombatValue::Num(0.0),
            CombatValue::Num(0.0),
            CombatValue::Int(amount),
            match remaining_hp {
                Some(h) => CombatValue::Int(h),
                None => CombatValue::Nil,
            },
        ];
        self.fire_event("COMBAT_LOG_EVENT_UNFILTERED", &[]);
        let unit = if Some(dest_cid) == player_cid {
            Some("player".to_string())
        } else if Some(dest_cid) == target {
            Some("target".to_string())
        } else {
            None
        };
        if let Some(unit) = unit {
            let action = if delta_hp < 0 { "WOUND" } else { "HEAL" };
            self.fire("UNIT_COMBAT", (unit, action, "", amount, 1));
        }
    }

    /// Passe un message reçu dans les filtres des AddOns (`ChatFrame_AddMessageEventFilter`).
    /// Rend `None` si un filtre le masque, sinon le message et l'auteur (éventuellement modifiés).
    pub fn filter_chat(&self, event: &str, msg: &str, sender: &str) -> Option<(String, String)> {
        let original = Some((msg.to_string(), sender.to_string()));
        let Ok(ar) = self.lua.globals().get::<Table>("ArchRust") else {
            return original;
        };
        let Ok(f) = ar.get::<Function>("FilterChat") else {
            return original;
        };
        match f.call::<(bool, Option<String>, Option<String>)>((event, msg, sender)) {
            Ok((true, _, _)) => None,
            Ok((false, m, s)) => Some((
                m.unwrap_or_else(|| msg.to_string()),
                s.unwrap_or_else(|| sender.to_string()),
            )),
            Err(e) => {
                report_error(&self.lua, &self.shared, "filtre de discussion", &e);
                original
            }
        }
    }

    /// Message tapé par le joueur : passe par la fonction Lua `SendChatMessage` (que les AddOns
    /// peuvent surcharger ou accrocher). Rend `false` si les AddOns ne sont pas chargés.
    pub fn send_chat(&self, text: &str, kind: &str) -> bool {
        if !self.is_loaded() {
            return false;
        }
        match self.lua.globals().get::<Function>("SendChatMessage") {
            Ok(f) => {
                if let Err(e) = f.call::<()>((text, kind)) {
                    report_error(&self.lua, &self.shared, "SendChatMessage", &e);
                }
                true
            }
            Err(_) => false,
        }
    }

    /// Variable de configuration (CVar) lue par le moteur.
    pub fn cvar(&self, name: &str) -> Option<String> {
        let s = self.shared.borrow();
        s.cvars
            .get(name)
            .or_else(|| s.cvar_defaults.get(name))
            .cloned()
    }

    pub fn cvar_f32(&self, name: &str) -> Option<f32> {
        self.cvar(name).and_then(|v| v.trim().parse().ok())
    }

    /// Liste de nombres (`"0.2 0.3 0.5"`).
    pub fn cvar_floats(&self, name: &str) -> Vec<f32> {
        self.cvar(name)
            .map(|v| {
                v.split([' ', ','])
                    .filter_map(|x| x.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Déclare une CVar du moteur avec sa valeur par défaut.
    pub fn register_cvar(&self, name: &str, default: &str) {
        self.shared
            .borrow_mut()
            .cvar_defaults
            .insert(name.to_string(), default.to_string());
    }

    /// Exécute un bout de Lua (console de développement).
    pub fn run_script(&self, src: &str) -> Result<(), AddonError> {
        self.lua
            .load(src)
            .set_name("@console")
            .set_mode(ChunkMode::Text)
            .exec()?;
        Ok(())
    }

    /// Évalue une expression Lua et rend sa valeur sous forme de texte (tests, console).
    pub fn eval_string(&self, src: &str) -> Result<String, AddonError> {
        let v: Value = self
            .lua
            .load(src)
            .set_name("@eval")
            .set_mode(ChunkMode::Text)
            .eval()?;
        let tostring: Function = self.lua.globals().get("tostring")?;
        Ok(tostring.call::<String>(v)?)
    }

    /// Teinte demandée pour le modèle d'un CID.
    pub fn unit_tint(&self, cid: i32) -> Option<[f32; 4]> {
        self.shared.borrow().unit_tints.get(&cid).copied()
    }

    /// Couleurs de l'interface native à appliquer (une fois après chaque changement).
    pub fn take_ui_colors(&self) -> Option<BTreeMap<String, [f32; 4]>> {
        let mut s = self.shared.borrow_mut();
        if s.ui_colors_dirty {
            s.ui_colors_dirty = false;
            Some(s.ui_colors.clone())
        } else {
            None
        }
    }

    /// Les frames natives (fenêtre de discussion…) : visibilité et alpha voulus par les AddOns.
    pub fn builtin_frame(&self, name: &str) -> (bool, f32) {
        let s = self.shared.borrow();
        match s.ui.by_name(name) {
            Some(id) => (s.ui.is_visible(id), s.ui.effective_alpha(id)),
            None => (true, 1.0),
        }
    }

    /// Erreurs Lua récentes.
    pub fn errors(&self) -> Vec<String> {
        self.shared.borrow().errors.clone()
    }

    /// Secondes depuis la création de l'état (`debugprofilestop`).
    pub fn uptime(&self) -> f64 {
        self.shared.borrow().started.elapsed().as_secs_f64()
    }
}

/// Instant de création (pour `debugprofilestop`, `GetTimePreciseSec`).
pub fn uptime(shared: &Shared) -> f64 {
    shared.borrow().started.elapsed().as_secs_f64()
}

/// Table Lua de l'AddOn en cours (rien hors chargement).
pub fn current_addon(shared: &Shared) -> Option<(String, PathBuf)> {
    shared.borrow().current.clone()
}

#[allow(dead_code)]
fn _assert_table(_: Table) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_addon(root: &Path, name: &str, toc: &str, lua: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.toc")), toc).unwrap();
        std::fs::write(dir.join("core.lua"), lua).unwrap();
    }

    fn player(name: &str) -> UnitInfo {
        UnitInfo {
            cid: 1,
            tid: 96,
            name: name.into(),
            level: 1,
            hp: Some(100),
            ..Default::default()
        }
    }

    #[test]
    fn load_events_slash_timers_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let addons = tmp.path().join("addons");
        make_addon(
            &addons,
            "Hello",
            "## Title: Hello\n## SavedVariables: HelloDB\n## SavedVariablesPerCharacter: HelloCharDB\ncore.lua\n",
            r#"
local name, ns = ...
assert(name == "Hello")
HelloDB = HelloDB or { logins = 0 }
HelloCharDB = HelloCharDB or { seen = 0 }
local f = CreateFrame("Frame", "HelloFrame", UIParent)
f:SetSize(200, 50)
f:SetPoint("CENTER", UIParent, "CENTER", 0, 100)
f.label = f:CreateFontString(nil, "OVERLAY")
f.label:SetPoint("CENTER")
f:RegisterEvent("PLAYER_LOGIN")
f:RegisterEvent("CHAT_MSG_SAY")
f:SetScript("OnEvent", function(self, event, ...)
  assert(self == HelloFrame and self.label, "même table de frame")
  if event == "PLAYER_LOGIN" then
    HelloDB.logins = HelloDB.logins + 1
    HelloCharDB.seen = HelloCharDB.seen + 1
    self.label:SetText("Bonjour " .. UnitName("player"))
  elseif event == "CHAT_MSG_SAY" then
    ns.last = select(1, ...)
  end
end)
ns.ticks = 0
f:SetScript("OnUpdate", function(self, elapsed) ns.ticks = ns.ticks + 1 end)
SLASH_HELLO1 = "/hello"
SLASH_HELLO2 = "/hi"
SlashCmdList["HELLO"] = function(msg) print("slash", msg); SendChatMessage("coucou " .. msg, "SAY") end
C_Timer.After(1.5, function() HelloFired = true end)
local n = 0
HelloTicker = C_Timer.NewTicker(0.5, function(t) n = n + 1; HelloTicks = n end, 3)
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
        mgr.set_host(|h| h.player = Some(player("Testeur")));
        mgr.load_dir(&addons).unwrap();
        let list = mgr.addons();
        assert_eq!(list.len(), 2);
        assert!(
            list.iter().all(|a| a.enabled),
            "{:?}",
            list.iter().map(|a| a.error.clone()).collect::<Vec<_>>()
        );
        assert_eq!(list[1].manifest.name, "Dep", "les dépendances se chargent d'abord");
        assert!(mgr.lua.globals().get::<bool>("DepLoaded").unwrap());

        mgr.fire_event("PLAYER_LOGIN", &[]);
        mgr.fire_event_str("CHAT_MSG_SAY", &["salut", "Autre"]);
        assert!(mgr.errors().is_empty(), "{:?}", mgr.errors());
        let id = mgr.shared.borrow().ui.by_name("HelloFrame").unwrap();
        let label_id = mgr.shared.borrow().ui.get(id).unwrap().children[0];
        assert_eq!(
            mgr.shared.borrow().ui.get(label_id).unwrap().text,
            "Bonjour Testeur"
        );

        assert!(mgr.run_slash("/hi tout le monde"));
        assert!(mgr.run_slash("/HELLO maj"));
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

        for _ in 0..4 {
            mgr.update(0.5);
        }
        let g = mgr.lua.globals();
        assert!(g.get::<bool>("HelloFired").unwrap());
        assert_eq!(g.get::<i64>("HelloTicks").unwrap(), 3, "NewTicker limité à 3 appels");
        let rects = mgr.shared.borrow().ui.layout(1280.0, 720.0);
        assert_eq!(rects[id].unwrap().width, 200.0);

        mgr.shutdown();
        let saved = std::fs::read_to_string(tmp.path().join("saved/Hello.lua")).unwrap();
        assert!(saved.contains("logins = 1"), "{saved}");
        let per_char =
            std::fs::read_to_string(tmp.path().join("saved/ArchCore/Testeur/Hello.lua")).unwrap();
        assert!(per_char.contains("seen = 1"), "{per_char}");

        // Rechargement : les SavedVariables sont restaurées avant les fichiers Lua.
        let mut mgr2 = AddonManager::new(tmp.path().join("saved")).unwrap();
        mgr2.set_host(|h| h.player = Some(player("Testeur")));
        mgr2.load_dir(&addons).unwrap();
        mgr2.fire_event("PLAYER_LOGIN", &[]);
        mgr2.save_all().unwrap();
        let saved = std::fs::read_to_string(tmp.path().join("saved/Hello.lua")).unwrap();
        assert!(saved.contains("logins = 2"), "{saved}");
    }

    #[test]
    fn world_sync_fires_unit_events() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = AddonManager::new(tmp.path()).unwrap();
        mgr.run_script(
            r#"
            Seen = {}
            local f = CreateFrame("Frame")
            for _, e in ipairs({"UNIT_HEALTH","NAME_PLATE_UNIT_ADDED","NAME_PLATE_UNIT_REMOVED","PLAYER_TARGET_CHANGED","BAG_UPDATE","PLAYER_STARTED_MOVING","COMBAT_LOG_EVENT_UNFILTERED"}) do f:RegisterEvent(e) end
            f:SetScript("OnEvent", function(self, e, a, b)
              table.insert(Seen, e .. ":" .. tostring(a))
              if e == "COMBAT_LOG_EVENT_UNFILTERED" then
                local _, sub, _, _, src, _, _, _, dst, _, _, amount = CombatLogGetCurrentEventInfo()
                LastCombat = sub .. " " .. src .. ">" .. dst .. " " .. amount
              end
            end)
        "#,
        )
        .unwrap();
        let mob = UnitInfo {
            cid: 7,
            tid: 300,
            name: "Loup".into(),
            hp: Some(50),
            position: [100.0, 0.0, 200.0],
            ..Default::default()
        };
        mgr.sync_world(Some(player("Moi")), vec![mob.clone()], Some(vec![]));
        mgr.run_script("assert(TargetUnit('nameplate1')); assert(UnitName('target') == 'Loup')")
            .unwrap();
        let mut hurt = mob.clone();
        hurt.hp = Some(20);
        let mut me = player("Moi");
        me.moving = true;
        mgr.sync_world(Some(me.clone()), vec![hurt], None);
        assert_eq!(mgr.eval_string("UnitHealthMax('target')").unwrap(), "50");
        assert_eq!(mgr.eval_string("UnitHealth('target')").unwrap(), "20");
        mgr.combat(1, 7, -30, Some(20));
        mgr.sync_world(Some(me), vec![], None);
        let seen = mgr.eval_string("table.concat(Seen, ',')").unwrap();
        assert!(seen.contains("NAME_PLATE_UNIT_ADDED:nameplate1"), "{seen}");
        assert!(seen.contains("UNIT_HEALTH:target"), "{seen}");
        assert!(seen.contains("PLAYER_STARTED_MOVING"), "{seen}");
        assert!(seen.contains("NAME_PLATE_UNIT_REMOVED:nameplate1"), "{seen}");
        assert!(seen.contains("PLAYER_TARGET_CHANGED"), "{seen}");
        assert_eq!(
            mgr.eval_string("LastCombat").unwrap(),
            "SWING_DAMAGE Moi>Loup 30"
        );
        assert_eq!(mgr.eval_string("tostring(UnitExists('target'))").unwrap(), "false");
    }

    #[test]
    fn mouse_click_drag_and_slider() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = AddonManager::new(tmp.path()).unwrap();
        mgr.run_script(
            r#"
            Clicks = 0
            local b = CreateFrame("Button", "TestButton", UIParent)
            b:SetSize(100, 40)
            b:SetPoint("BOTTOMLEFT", 10, 10)
            b:SetScript("OnClick", function(self, button) Clicks = Clicks + 1; LastButton = button end)
            b:SetScript("OnEnter", function() Hover = true end)
            local f = CreateFrame("Frame", "TestMover", UIParent)
            f:SetSize(100, 100)
            f:SetPoint("BOTTOMLEFT", 300, 300)
            f:EnableMouse(true)
            f:SetMovable(true)
            f:RegisterForDrag("LeftButton")
            f:SetScript("OnDragStart", f.StartMoving)
            f:SetScript("OnDragStop", f.StopMovingOrSizing)
            local s = CreateFrame("Slider", "TestSlider", UIParent)
            s:SetSize(200, 20)
            s:SetPoint("BOTTOMLEFT", 10, 500)
            s:SetMinMaxValues(0, 10)
            s:SetValueStep(1)
            s:SetScript("OnValueChanged", function(self, v, user) SliderValue = v; SliderUser = user end)
        "#,
        )
        .unwrap();
        mgr.layout(1280.0, 720.0, &|f| f.estimated_text_size());
        let step = |pos: (f32, f32), pressed: &[&str], released: &[&str]| {
            mgr.pointer(&PointerInput {
                pos: Some(pos),
                pressed: pressed.iter().map(|s| s.to_string()).collect(),
                released: released.iter().map(|s| s.to_string()).collect(),
                wheel: 0.0,
                native_over: false,
            });
            mgr.layout(1280.0, 720.0, &|f| f.estimated_text_size());
        };
        step((50.0, 30.0), &["RightButton"], &[]);
        step((50.0, 30.0), &[], &["RightButton"]);
        assert_eq!(mgr.eval_string("Clicks").unwrap(), "1");
        assert_eq!(mgr.eval_string("LastButton").unwrap(), "RightButton");
        assert_eq!(mgr.eval_string("tostring(Hover)").unwrap(), "true");
        // Glisser la frame de 50 px vers la droite.
        step((320.0, 320.0), &["LeftButton"], &[]);
        step((345.0, 320.0), &[], &[]);
        step((370.0, 320.0), &[], &[]);
        step((370.0, 320.0), &[], &["LeftButton"]);
        // StartMoving garde l'écart curseur–frame au moment du départ du glisser (comme WoW).
        assert_eq!(mgr.eval_string("TestMover:GetLeft()").unwrap(), "325");
        // Curseur : 75 % de la largeur → 7,5 arrondi au pas → 8.
        step((160.0, 510.0), &["LeftButton"], &[]);
        step((160.0, 510.0), &[], &["LeftButton"]);
        assert_eq!(mgr.eval_string("SliderValue").unwrap(), "8");
        assert_eq!(mgr.eval_string("tostring(SliderUser)").unwrap(), "true");
    }
}
