//! Boucle principale : fenêtre winit, rendu, interface, AddOns et réseau.

use crate::animation::{self, Animator};
use crate::assets::Assets;
use crate::audio::Audio;
use crate::camera::OrbitCamera;
use crate::login_settings::{self, LoginSettings, SceneCamera};
use crate::net::{Credentials, LoginCommand, NetEvent, NetLink};
use crate::objects::WorldObjects;
use crate::render::{DrawItem, Mesh, Renderer};
use crate::scene::{
    box_mesh, entity_meshes, entity_meshes_with_equipment, ground_mesh, item_name_for_tid,
    model_meshes, turn_towards, Entity, Head,
};
use crate::options::ClientOptions;
use crate::ui::{self, UiActions, UiState};
use crate::world::Terrain;
use crate::zones::Zones;
use anyhow::Result;
use archrust_addons::{AddonManager, HostCommand};
use archrust_netclient::{Command, WorldEvent};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

#[path = "app_addons.rs"]
mod addon_glue;

/// Configuration du client.
#[derive(Debug, Clone)]
pub struct Config {
    /// Dossier `Game` du client ; `None` = mode sans client (patchs et substituts seulement).
    pub game_dir: Option<PathBuf>,
    pub addons_dir: PathBuf,
    /// Dossiers de patchs : un dossier avec `patch.toml` (ex. `free-assets/`) ou un dossier de patchs.
    pub patch_dirs: Vec<PathBuf>,
    pub saved_dir: PathBuf,
    pub title: String,
    /// Modèle à afficher au démarrage pour vérifier le rendu (ex. `a0000005.ecl`), facultatif.
    pub preview_model: Option<String>,
    /// Connexion automatique au démarrage : `utilisateur:motdepasse@hôte:port/personnage`.
    pub auto_connect: Option<String>,
    /// Capture d'écran automatique : fichier PNG et délai en secondes, puis le client se ferme.
    pub screenshot: Option<(PathBuf, f32)>,
    /// Commande(s) de discussion (`/goto …`, `/hello`) exécutée(s) dès l'entrée en jeu, séparées par `;`.
    pub exec_on_enter: Option<String>,
    /// Commandes des écrans de sélection et de création (tests), séparées par `;` et jouées
    /// toutes les 1,5 s une fois la scène prête : `créer`, `race Orc`, `classe Berserker`,
    /// `visage 3`, `coiffure 5`, `tourner 90`, `zoom`, `nom Toto`, `valider`, `annuler`,
    /// `choisir 2`, `supprimer Toto`, `entrer`.
    pub login_exec: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Login,
    Connecting,
    /// Authentifié : choix du personnage du compte (la liaison attend `NetLink::choose`).
    CharacterSelect,
    /// Écran de création (modèles de base reçus du serveur).
    CharacterCreate,
    InWorld,
}

struct Gpu {
    window: Arc<Window>,
    renderer: Renderer,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    ground: Arc<Mesh>,
    marker: Arc<Mesh>,
    player_marker: Arc<Mesh>,
}

pub struct App {
    config: Config,
    assets: Arc<Assets>,
    addons: AddonManager,
    gpu: Option<Gpu>,
    ui: UiState,
    /// Fond de l'écran de connexion (`None` = pas encore chargé, `Some(None)` = illisible).
    login_screen: Option<Option<egui::TextureHandle>>,
    /// La connexion en cours passe par l'écran de sélection des personnages.
    choosing_character: bool,
    /// `ini/LoginSettings.txt` (`None` = pas encore lu, `Some(None)` = absent).
    login_settings: Option<Option<LoginSettings>>,
    /// Signature de la scène de connexion construite (phase, personnages ou création).
    scene_signature: Option<String>,
    /// Point autour duquel charger le terrain de la scène de connexion (`None` hors scène).
    scene_focus: Option<Vec3>,
    /// Hauteur du sol (objet) sous chaque personnage de la scène, par CID local.
    scene_floor: HashMap<i32, f32>,
    camera: OrbitCamera,
    phase: Phase,
    net: Option<NetLink>,
    entities: Vec<Entity>,
    items: HashMap<i32, archrust_netclient::world::ItemRecord>,
    player: Option<Entity>,
    /// Animateurs par CID (avec l'identité des maillages pour lesquels ils ont été construits) ;
    /// `None` = entité sans maillage skinné ou sans animation connue.
    animators: HashMap<i32, (usize, Option<Animator>)>,
    player_animator: Option<(usize, Option<Animator>)>,
    preview: Vec<(Arc<Mesh>, Mat4)>,
    terrain: Terrain,
    /// Objets statiques du monde (`ini/obj%05d.ini`).
    objects: WorldObjects,
    /// Le joueur est en marche vers sa destination (un paquet d'arrêt sera envoyé à l'arrivée).
    walking: bool,
    started: Instant,
    screenshot_done: bool,
    last_frame: Instant,
    fps: f32,
    right_drag: bool,
    middle_drag: bool,
    last_cursor: Option<(f64, f64)>,
    keys: [bool; 4],
    exit: bool,
    /// Instant où lancer `--exec` : un peu après l'entrée en jeu, pour que les paquets CHARACTER
    /// déjà reçus (qui remettent la destination du joueur sur sa position) soient traités avant.
    exec_at: Option<Instant>,
    /// File des commandes `--login-exec` restantes et instant de la prochaine.
    login_exec: Vec<String>,
    login_exec_at: Option<Instant>,
    /// Liaison avec les AddOns Lua (interface, souris, CVars), voir `app_addons.rs`.
    addon_glue: addon_glue::AddonGlue,
    /// Musique, ambiance et effets (voir `audio.rs`).
    audio: Audio,
    /// Régions du monde (grilles `.amf`) pour la musique de zone.
    zones: Zones,
    /// Options persistées (`saved-variables/options.toml`) et drapeau « à enregistrer ».
    options: ClientOptions,
    options_dirty: bool,
    /// Musique imposée par un AddOn (`PlayMusic`) : `Some(Some(chemin))` ; `Some(None)` = coupée ;
    /// `None` = musique automatique (écrans ou région).
    music_override: Option<Option<String>>,
    /// Prochaine recherche de la région du joueur (toutes les secondes) et dernière région trouvée.
    zone_check_at: Option<Instant>,
    zone_region: Option<(u16, Option<String>)>,
}

/// Résumé visible d'une variation de PV observée entre deux actions réseau.
fn combat_hp_line(actor: &str, target: &str, previous: i32, current: i32) -> Option<String> {
    let delta = current as i64 - previous as i64;
    (delta != 0).then(|| format!("[combat] {actor} → {target} : {delta:+} PV (reste {current})"))
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        let assets = Arc::new(Assets::new(config.game_dir.clone(), &config.patch_dirs));
        // Comme sous WoW, les AddOns se chargent à l'entrée en jeu (voir `enter_world_addons`).
        let addons = Self::new_addon_manager(&config)?;
        log::info!(
            "{} AddOn(s) trouvé(s) dans {}",
            AddonManager::discover(&config.addons_dir).len(),
            config.addons_dir.display()
        );
        // Options persistées, puis périphérique audio (muet s'il n'y en a pas).
        let options = crate::options::load(&config.saved_dir);
        let audio = Audio::new(
            assets.clone(),
            Some(config.addons_dir.clone()),
            options.audio.clone(),
        );
        // Serveur de connexion : `realmlist.wtf` (à côté de l'exécutable, dossier courant, client).
        let mut ui = UiState::default();
        // Compte enregistré (case « Enregistrer le compte »).
        if let Some(saved) = crate::account::load(&config.saved_dir) {
            log::info!("compte enregistré : {}", saved.user);
            ui.user = saved.user;
            ui.password = saved.password;
            ui.character = saved.character;
            ui.remember = true;
        }
        match crate::realmlist::find(&crate::realmlist::search_dirs(config.game_dir.as_deref())) {
            Some(r) => {
                log::info!("serveur {}:{} ({})", r.host, r.port, r.source.display());
                ui.host = r.host;
                ui.port = r.port.to_string();
            }
            None => log::info!(
                "pas de {} : serveur {}:{} par défaut",
                crate::realmlist::FILE_NAME,
                ui.host,
                ui.port
            ),
        }
        Ok(Self {
            config,
            assets,
            addons,
            gpu: None,
            ui,
            login_screen: None,
            choosing_character: false,
            login_settings: None,
            scene_signature: None,
            scene_focus: None,
            scene_floor: HashMap::new(),
            camera: OrbitCamera::default(),
            phase: Phase::Login,
            net: None,
            entities: Vec::new(),
            items: HashMap::new(),
            player: None,
            animators: HashMap::new(),
            player_animator: None,
            preview: Vec::new(),
            terrain: Terrain::default(),
            objects: WorldObjects::new(3),
            walking: false,
            started: Instant::now(),
            screenshot_done: false,
            last_frame: Instant::now(),
            fps: 0.0,
            right_drag: false,
            middle_drag: false,
            last_cursor: None,
            keys: [false; 4],
            exit: false,
            exec_at: None,
            login_exec: Vec::new(),
            login_exec_at: None,
            addon_glue: Default::default(),
            audio,
            zones: Zones::default(),
            options,
            options_dirty: false,
            music_override: None,
            zone_check_at: None,
            zone_region: None,
        })
    }

    /// Musique et ambiance voulues selon l'écran, la région du joueur et les AddOns, puis fondus.
    fn update_audio(&mut self, dt: f32) {
        let in_world = self.phase == Phase::InWorld;
        // Région du joueur, revue toutes les secondes (et à chaque entrée en jeu).
        if in_world {
            let due = self.zone_check_at.map_or(true, |t| Instant::now() >= t);
            if due {
                self.zone_check_at = Some(Instant::now() + std::time::Duration::from_secs(1));
                let found = self.player.as_ref().and_then(|p| {
                    self.zones.music_at(
                        &self.assets,
                        p.position.x,
                        p.position.z,
                        crate::zones::local_hour(),
                    )
                });
                if found.as_ref().map(|f| f.0) != self.zone_region.as_ref().map(|f| f.0) {
                    if let Some((region, music)) = &found {
                        log::info!(
                            "région {region} ({}) : musique {}",
                            Zones::region_name(&self.assets, *region).unwrap_or_default(),
                            music.as_deref().unwrap_or("aucune dans skyset.ini")
                        );
                    }
                }
                self.zone_region = found;
            }
        } else {
            self.zone_check_at = None;
            self.zone_region = None;
        }
        let cvar = |name: &str| {
            self.addons
                .cvar(name)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let music = match &self.music_override {
            Some(forced) => forced.clone(),
            None if in_world => self
                .zone_region
                .as_ref()
                .and_then(|(_, m)| m.clone())
                .or_else(|| cvar("worldMusic")),
            None => cvar("loginMusic"),
        };
        let ambience = if in_world { cvar("worldAmbience") } else { None };
        self.audio.play_music(music.as_deref());
        self.audio.play_ambience(ambience.as_deref());
        self.audio.update(dt);
    }

    /// Enregistre les options si elles ont changé et que l'utilisateur a lâché les curseurs.
    fn save_options_if_needed(&mut self, force: bool) {
        if !self.options_dirty {
            return;
        }
        let pointer_busy = self
            .gpu
            .as_ref()
            .map(|g| g.egui_state.egui_ctx().is_using_pointer())
            .unwrap_or(false);
        if force || !pointer_busy {
            match crate::options::save(&self.config.saved_dir, &self.options) {
                Ok(()) => self.options_dirty = false,
                Err(e) => {
                    log::warn!("options : {e:#}");
                    self.options_dirty = false;
                }
            }
        }
    }

    fn init_gpu(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let mut attrs = Window::default_attributes()
            .with_title(&self.config.title)
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        // Icône de fenêtre : le logo officiel d'ArchRust (64×64), intégré à l'exécutable.
        const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/window-icon.png");
        if let Ok(img) = image::load_from_memory(WINDOW_ICON_PNG) {
            let img = img.to_rgba8();
            let (w, h) = img.dimensions();
            if let Ok(icon) = winit::window::Icon::from_rgba(img.into_raw(), w, h) {
                attrs = attrs.with_window_icon(Some(icon));
            }
        }
        let window = Arc::new(event_loop.create_window(attrs)?);
        let mut renderer = Renderer::new(window.clone())?;
        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let egui_renderer =
            egui_wgpu::Renderer::new(&renderer.device, renderer.config.format, None, 1, false);
        let ground = Arc::new(ground_mesh(&renderer, 200_000.0));
        let marker = Arc::new(box_mesh(
            &renderer,
            Vec3::new(40.0, 90.0, 40.0),
            [0.9, 0.8, 0.3, 1.0],
        ));
        let player_marker = Arc::new(box_mesh(
            &renderer,
            Vec3::new(40.0, 90.0, 40.0),
            [0.3, 0.6, 1.0, 1.0],
        ));
        if let Some(model) = &self.config.preview_model {
            match model_meshes(&mut renderer, &self.assets, "character", model) {
                Ok(meshes) => {
                    self.preview = meshes;
                    log::info!("aperçu {model} : {} maillages", self.preview.len());
                    // Cadrage : près du modèle (à l'origine), décalé pour qu'il ne soit pas sous le panneau de connexion.
                    self.camera.target = Vec3::new(0.0, 100.0, 0.0);
                    self.camera.distance = 600.0;
                    self.camera.pan(0.0, 300.0);
                }
                Err(e) => log::warn!("aperçu {model} : {e:#}"),
            }
        }
        // Sans client ni connexion automatique : un personnage local (humain par défaut, TID 96) à
        // l'origine, pour voir les modèles des patchs (`[[models]]` glTF) sans serveur.
        if !self.assets.has_client() && self.config.auto_connect.is_none() && self.player.is_none()
        {
            let meshes = entity_meshes(
                &mut renderer,
                &self.assets,
                Self::LOCAL_PREVIEW_TID,
                Head::default(),
                &player_marker,
            )
            .unwrap_or_default();
            log::info!(
                "mode sans client : personnage local (TID {}) affiché à l'origine, {} maillage(s)",
                Self::LOCAL_PREVIEW_TID,
                meshes.len()
            );
            self.player = Some(Entity {
                cid: 0,
                tid: Self::LOCAL_PREVIEW_TID,
                name: "Aperçu".into(),
                hp: None,
                position: Vec3::ZERO,
                destination: Vec3::ZERO,
                turn_y: 0.0,
                head: Head::default(),
                meshes,
                equipment: Vec::new(),
                is_player: true,
            });
            // Cadrage : près du personnage, décalé pour qu'il ne soit pas sous le panneau de connexion.
            self.camera.target = Vec3::new(0.0, 100.0, 0.0);
            self.camera.distance = 600.0;
            self.camera.pan(0.0, 300.0);
        }
        self.gpu = Some(Gpu {
            window,
            renderer,
            egui_state,
            egui_renderer,
            ground,
            marker,
            player_marker,
        });
        if let Some(spec) = self.config.auto_connect.clone() {
            // utilisateur:motdepasse@hôte:port/personnage ; sans hôte (`test:test@/Testeur`), le
            // serveur du `realmlist.wtf` (déjà dans l'écran de connexion) est gardé.
            if let Some((creds, rest)) = spec.split_once('@') {
                let (user, pass) = creds.split_once(':').unwrap_or((creds, ""));
                // Sans `/personnage`, la connexion passe par l'écran de sélection.
                let (hostport, character) = match rest.split_once('/') {
                    Some((h, c)) if !c.trim().is_empty() => (h, Some(c.trim().to_string())),
                    Some((h, _)) => (h, None),
                    None => (rest, None),
                };
                self.ui.user = user.to_string();
                self.ui.password = pass.to_string();
                if !hostport.trim().is_empty() {
                    let (host, port) = hostport.split_once(':').unwrap_or((hostport, "11002"));
                    self.ui.host = host.to_string();
                    self.ui.port = port.to_string();
                }
                if let Some(name) = &character {
                    self.ui.character = name.clone();
                }
                self.start_connection(character);
            } else {
                log::warn!(
                    "--connect : forme attendue utilisateur:motdepasse@hôte:port/personnage"
                );
            }
        }
        Ok(())
    }

    /// `LoginSettings.txt`, lu une fois.
    fn login_settings(&mut self) -> Option<&LoginSettings> {
        if self.login_settings.is_none() {
            let s = login_settings::load(&self.assets);
            match &s {
                Some(s) => log::info!(
                    "scène de connexion : division {:?}, {} emplacements",
                    s.division,
                    s.select_spots.len()
                ),
                None => log::warn!("ini/LoginSettings.txt absent ou illisible"),
            }
            self.login_settings = Some(s);
        }
        self.login_settings.as_ref().and_then(|s| s.as_ref())
    }

    /// Point autour duquel charger le terrain de la scène de connexion.
    fn scene_center(&mut self) -> Option<Vec3> {
        let phase = self.phase;
        let race = self
            .ui
            .creation
            .as_ref()
            .map(|c| c.race.to_ascii_lowercase());
        let in_scene = matches!(phase, Phase::CharacterSelect | Phase::CharacterCreate)
            || (phase == Phase::Connecting && self.choosing_character);
        if !in_scene {
            return None;
        }
        let s = self.login_settings()?;
        match (phase, race) {
            (Phase::CharacterCreate, Some(r)) => s.create_spots.get(&r).map(|p| p.pos),
            _ => s.select_spots.first().map(|p| p.pos),
        }
    }

    /// Caméra fixe de la scène : sélection, création par race, zoom par race et classe.
    fn scene_camera(&mut self) -> Option<SceneCamera> {
        let phase = self.phase;
        let creation = self.ui.creation.as_ref().map(|c| {
            (
                c.race.to_ascii_lowercase(),
                c.current()
                    .map(|t| login_settings::class_kind(&t.class_name).to_ascii_lowercase())
                    .unwrap_or_default(),
                c.zoom,
            )
        });
        let s = self.login_settings()?;
        match (phase, creation) {
            (Phase::CharacterCreate, Some((race, class, zoom))) => {
                let zoomed = zoom
                    .then(|| s.camera_create_zoom.get(&(race.clone(), class)).copied())
                    .flatten();
                zoomed.or_else(|| s.camera_create.get(&race).copied())
            }
            _ => s.camera_select,
        }
    }

    /// Réapplique la caméra de la scène de connexion (après les CVars des AddOns).
    fn reapply_scene_camera(&mut self) {
        let in_scene = matches!(self.phase, Phase::CharacterSelect | Phase::CharacterCreate)
            || (self.phase == Phase::Connecting
                && self.choosing_character
                && !self.ui.characters.is_empty());
        if in_scene {
            if let Some(cam) = self.scene_camera() {
                self.apply_scene_camera(cam);
            }
        }
    }

    /// Applique la caméra fixe à la caméra orbitale : cible à 600 unités devant l'œil.
    fn apply_scene_camera(&mut self, cam: SceneCamera) {
        let forward = (cam.look_at - cam.pos).normalize_or(Vec3::X);
        let back = -forward;
        let distance = 600.0;
        self.camera.target = cam.pos + forward * distance;
        self.camera.distance = distance;
        self.camera.pitch = back.y.clamp(-1.0, 1.0).asin();
        self.camera.yaw = back.z.atan2(back.x);
        let aspect = self
            .gpu
            .as_ref()
            .map(|g| g.renderer.aspect())
            .unwrap_or(16.0 / 9.0);
        self.camera.fov_y = cam.fov_y(aspect);
    }

    /// Construit ou met à jour la scène de connexion : caméra, personnages du compte aux
    /// emplacements (`CharacterSelect_Position`), personnage choisi qui avance de son
    /// `CharacterSelect_MoveOffSet`, aperçu de création (`CharacterCreate_Position`).
    /// Joue la prochaine commande `--login-exec` quand la scène est prête.
    fn run_login_exec(&mut self) {
        if self.login_exec.is_empty() {
            if let Some(list) = self.config.login_exec.take() {
                self.login_exec = list
                    .split(';')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .rev()
                    .collect();
            }
        }
        let ready = matches!(self.phase, Phase::CharacterSelect | Phase::CharacterCreate)
            && self.terrain.loaded_count() >= 9;
        if !ready || self.login_exec.is_empty() {
            return;
        }
        let now = Instant::now();
        if self.login_exec_at.is_some_and(|t| now < t) {
            return;
        }
        if self.login_exec_at.is_none() {
            self.login_exec_at = Some(now + std::time::Duration::from_millis(1500));
            return;
        }
        self.login_exec_at = Some(now + std::time::Duration::from_millis(1500));
        let Some(cmd) = self.login_exec.pop() else {
            return;
        };
        log::info!("--login-exec : {cmd}");
        let (verb, arg) = cmd.split_once(' ').unwrap_or((cmd.as_str(), ""));
        let arg = arg.trim().to_string();
        let mut actions = UiActions::default();
        match verb {
            "créer" | "creer" => actions.open_creation = true,
            "valider" => {
                if let Some(c) = &self.ui.creation {
                    if let Some(t) = c.current() {
                        actions.create_submit = Some((t.tid, c.name.clone(), c.face, c.hair));
                    }
                }
            }
            "annuler" => actions.cancel_creation = true,
            "entrer" => {
                actions.enter_character = self
                    .ui
                    .selected_character
                    .and_then(|i| self.ui.characters.get(i))
                    .map(|c| c.name.clone())
            }
            "supprimer" => actions.delete_character = Some(arg.clone()),
            "choisir" => {
                if let Ok(n) = arg.parse::<usize>() {
                    if n >= 1 && n <= self.ui.characters.len() {
                        self.ui.selected_character = Some(n - 1);
                    }
                }
            }
            _ => {
                if let Some(c) = self.ui.creation.as_mut() {
                    match verb {
                        "race" => {
                            if let Some(r) = login_settings::RACES
                                .into_iter()
                                .find(|r| r.eq_ignore_ascii_case(&arg))
                            {
                                c.select_race(r);
                            }
                        }
                        "classe" => {
                            if let Some(i) = c.templates.iter().position(|t| {
                                t.race == c.race && t.class_name.eq_ignore_ascii_case(&arg)
                            }) {
                                c.template = i;
                            }
                        }
                        "visage" => c.face = arg.parse().unwrap_or(c.face),
                        "coiffure" => c.hair = arg.parse().unwrap_or(c.hair),
                        "tourner" => c.rotate += arg.parse::<f32>().unwrap_or(0.0),
                        "zoom" => c.zoom = !c.zoom,
                        "nom" => c.name = arg.clone(),
                        _ => log::warn!("--login-exec : commande inconnue {cmd}"),
                    }
                }
            }
        }
        self.handle_actions(actions);
    }

    fn update_login_scene(&mut self) {
        self.run_login_exec();
        self.scene_focus = self.scene_center();
        let in_scene = matches!(self.phase, Phase::CharacterSelect | Phase::CharacterCreate)
            || (self.phase == Phase::Connecting
                && self.choosing_character
                && !self.ui.characters.is_empty());
        if !in_scene {
            return;
        }
        if let Some(cam) = self.scene_camera() {
            self.apply_scene_camera(cam);
        }
        let Some(settings) = self.login_settings().cloned() else {
            return;
        };
        // Signature : ce qui impose de recharger les modèles.
        let signature = match (self.phase, &self.ui.creation) {
            (Phase::CharacterCreate, Some(c)) => format!(
                "creation:{}:{}:{}",
                c.current().map(|t| t.tid).unwrap_or(0),
                c.face,
                c.hair
            ),
            _ => format!(
                "select:{}",
                self.ui
                    .characters
                    .iter()
                    .map(|c| format!("{}/{}/{}/{}", c.name, c.tid, c.face, c.hair))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        };
        if self.scene_signature.as_deref() != Some(signature.as_str()) {
            self.scene_signature = Some(signature);
            self.scene_floor.clear();
            self.build_login_scene(&settings);
        }
        // Sol réel sous chaque personnage : le dallage (objet) de la scène, sous la hauteur de
        // `LoginSettings.txt` ; calculé dès que les objets du secteur sont lus.
        for e in self.entities.iter_mut().filter(|e| e.cid < 0) {
            if !self.scene_floor.contains_key(&e.cid) {
                if let Some(h) = self.objects.floor_height(
                    &self.assets,
                    e.position.x,
                    e.position.z,
                    e.position.y + 50.0,
                ) {
                    log::debug!("scène : sol à {h:.1} sous {}", e.name);
                    self.scene_floor.insert(e.cid, h);
                }
            }
            if let Some(h) = self.scene_floor.get(&e.cid) {
                e.position.y = *h;
                e.destination.y = *h;
            }
        }
        // Rotation de l'aperçu de création ; pas en avant du personnage choisi.
        if self.phase == Phase::CharacterCreate {
            let (race, rotate) = self
                .ui
                .creation
                .as_ref()
                .map(|c| (c.race.to_ascii_lowercase(), c.rotate))
                .unwrap_or_default();
            if let (Some(spot), Some(e)) = (
                settings.create_spots.get(&race),
                self.entities
                    .iter_mut()
                    .find(|e| e.cid == Self::CREATION_CID),
            ) {
                e.turn_y = spot.rotate + rotate;
            }
            return;
        }
        let selected = self.ui.selected_character;
        for (i, ch) in self
            .ui
            .characters
            .iter()
            .enumerate()
            .take(settings.select_spots.len())
        {
            let spot = settings.select_spots[i];
            let Some(e) = self
                .entities
                .iter_mut()
                .find(|e| e.cid == Self::slot_cid(i))
            else {
                continue;
            };
            let (race, class) = login_settings::template_identity(&self.assets, ch.tid);
            let offset = settings
                .move_offsets
                .get(&(
                    race.to_ascii_lowercase(),
                    login_settings::class_kind(&class).to_ascii_lowercase(),
                ))
                .copied()
                .unwrap_or(0.0);
            let rad = spot.rotate.to_radians();
            let forward = Vec3::new(rad.sin(), 0.0, rad.cos());
            let target = if selected == Some(i) {
                spot.pos + forward * offset
            } else {
                spot.pos
            };
            e.destination = Vec3::new(target.x, e.position.y, target.z);
            if !Self::is_moving(e) {
                e.turn_y = spot.rotate;
            }
        }
    }

    const CREATION_CID: i32 = -100;

    /// CID local d'un emplacement de la scène de sélection (négatif : jamais un CID serveur).
    fn slot_cid(slot: usize) -> i32 {
        -1 - slot as i32
    }

    fn build_login_scene(&mut self, settings: &LoginSettings) {
        self.entities.retain(|e| e.cid >= 0);
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let fallback = gpu.marker.clone();
        let mut spawned = Vec::new();
        if self.phase == Phase::CharacterCreate {
            if let Some(c) = &self.ui.creation {
                let spot = settings
                    .create_spots
                    .get(&c.race.to_ascii_lowercase())
                    .copied();
                if let (Some(t), Some(spot)) = (c.current(), spot) {
                    let head = Head {
                        face: c.face,
                        hair: c.hair,
                    };
                    let meshes =
                        entity_meshes(&mut gpu.renderer, &self.assets, t.tid, head, &fallback)
                            .unwrap_or_default();
                    spawned.push((
                        Self::CREATION_CID,
                        t.tid,
                        t.class_name.clone(),
                        head,
                        spot,
                        meshes,
                    ));
                }
            }
        } else {
            for (i, ch) in self
                .ui
                .characters
                .iter()
                .enumerate()
                .take(settings.select_spots.len())
            {
                let head = Head {
                    face: ch.face,
                    hair: ch.hair,
                };
                let meshes =
                    entity_meshes(&mut gpu.renderer, &self.assets, ch.tid, head, &fallback)
                        .unwrap_or_default();
                spawned.push((
                    Self::slot_cid(i),
                    ch.tid,
                    ch.name.clone(),
                    head,
                    settings.select_spots[i],
                    meshes,
                ));
            }
        }
        for (cid, tid, name, head, spot, meshes) in spawned {
            self.entities.push(Entity {
                cid,
                tid,
                name,
                hp: None,
                position: spot.pos,
                destination: spot.pos,
                turn_y: spot.rotate,
                head,
                meshes,
                equipment: Vec::new(),
                is_player: false,
            });
        }
    }

    /// Quitte la scène de connexion : personnages de la scène retirés, caméra normale.
    fn leave_login_scene(&mut self) {
        self.entities.retain(|e| e.cid >= 0);
        self.scene_signature = None;
        self.scene_floor.clear();
        // Caméra orbitale par défaut, autour du joueur s'il est déjà là.
        let target = self
            .player
            .as_ref()
            .map(|p| p.position + Vec3::new(0.0, 100.0, 0.0))
            .unwrap_or(self.camera.target);
        self.camera = OrbitCamera {
            target,
            ..OrbitCamera::default()
        };
    }

    /// Clic dans la scène de sélection : personnage le plus proche du rayon de la souris.
    fn click_scene_character(&mut self) {
        let (Some(gpu), Some((cx, cy))) = (&self.gpu, self.last_cursor) else {
            return;
        };
        let (w, h) = (
            gpu.renderer.config.width as f32,
            gpu.renderer.config.height as f32,
        );
        let inv = self.camera.view_proj(gpu.renderer.aspect()).inverse();
        let ndc = glam::Vec2::new(cx as f32 / w * 2.0 - 1.0, 1.0 - cy as f32 / h * 2.0);
        let near = inv.project_point3(ndc.extend(0.0));
        let far = inv.project_point3(ndc.extend(1.0));
        let dir = (far - near).normalize_or_zero();
        let mut best: Option<(usize, f32)> = None;
        for i in 0..self.ui.characters.len() {
            let Some(e) = self.entities.iter().find(|e| e.cid == Self::slot_cid(i)) else {
                continue;
            };
            // Distance du rayon à l'axe du personnage, à mi-hauteur.
            let center = e.position + Vec3::Y * 90.0;
            let t = (center - near).dot(dir).max(0.0);
            let d = (near + dir * t).distance(center);
            if d < 90.0 && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        if let Some((i, _)) = best {
            self.ui.selected_character = Some(i);
        }
    }

    /// Lance la connexion ; `character` = entrée directe, `None` = écran de sélection.
    fn start_connection(&mut self, character: Option<String>) {
        let port = self.ui.port.trim().parse().unwrap_or(11002);
        self.choosing_character = character.is_none();
        self.ui.characters.clear();
        self.ui.selected_character = None;
        let creds = Credentials {
            host: self.ui.host.trim().to_string(),
            port,
            user: self.ui.user.trim().to_string(),
            password: self.ui.password.clone(),
            character,
            create_if_missing: self.ui.create_if_missing,
        };
        self.ui.status = "connexion…".into();
        self.net = Some(NetLink::start(creds));
        self.phase = Phase::Connecting;
    }

    fn poll_net(&mut self) {
        let Some(net) = &self.net else { return };
        let mut events = Vec::new();
        while let Ok(e) = net.events.try_recv() {
            events.push(e);
        }
        for e in events {
            match e {
                NetEvent::Status(s) => {
                    self.ui.status = s.clone();
                    self.ui.push_chat(format!("[système] {s}"));
                }
                NetEvent::Error(e) => {
                    self.ui.status = format!("erreur : {e}");
                    self.ui.push_chat(format!("[erreur] {e}"));
                    self.ui.characters.clear();
                    self.ui.creation = None;
                    self.leave_login_scene();
                    self.phase = Phase::Login;
                    self.net = None;
                    return;
                }
                NetEvent::Worlds(w) => self
                    .ui
                    .push_chat(format!("[système] mondes : {}", w.join(", "))),
                NetEvent::Characters(c) => {
                    for ch in &c {
                        self.ui.push_chat(format!(
                            "[système] personnage {} niveau {} (cid {})",
                            ch.name, ch.level, ch.cid
                        ));
                    }
                    // Présélection : le dernier personnage joué, sinon le premier.
                    self.ui.selected_character = c
                        .iter()
                        .position(|ch| ch.name.eq_ignore_ascii_case(self.ui.character.trim()))
                        .or(if c.is_empty() { None } else { Some(0) });
                    self.ui.characters = c;
                    if self.choosing_character {
                        self.phase = Phase::CharacterSelect;
                        self.ui.status.clear();
                    }
                }
                NetEvent::CreationTemplates(bases) => {
                    let templates: Vec<ui::CreationTemplate> = bases
                        .iter()
                        .map(|b| {
                            let (race, class_name) =
                                login_settings::template_identity(&self.assets, b.tid);
                            let (face_count, hair_count) =
                                crate::scene::head_counts(&self.assets, b.tid);
                            ui::CreationTemplate {
                                tid: b.tid,
                                class_name: if class_name.is_empty() {
                                    b.name.clone()
                                } else {
                                    class_name
                                },
                                race,
                                face_count,
                                hair_count,
                            }
                        })
                        .collect();
                    log::info!("création : {} modèle(s) de base", templates.len());
                    let mut creation = ui::CreationUi {
                        templates,
                        ..Default::default()
                    };
                    if let Some(first) = creation.templates.first().map(|t| t.race) {
                        creation.select_race(first);
                    }
                    self.ui.creation = Some(creation);
                    self.ui.status.clear();
                    self.phase = Phase::CharacterCreate;
                }
                NetEvent::CharacterCreated(c) => {
                    log::info!("personnage créé : {} (TID {})", c.name, c.tid);
                    self.ui.character = c.name.clone();
                    self.ui.characters.push(c);
                    self.ui.selected_character = Some(self.ui.characters.len() - 1);
                    self.ui.creation = None;
                    self.ui.status.clear();
                    self.phase = Phase::CharacterSelect;
                }
                NetEvent::CharacterDeleted(name) => {
                    self.ui.characters.retain(|c| c.name != name);
                    self.ui.selected_character = if self.ui.characters.is_empty() {
                        None
                    } else {
                        Some(0)
                    };
                    self.ui.status = format!("{name} supprimé");
                }
                NetEvent::Refused(msg) => {
                    self.ui.status = format!("refusé : {msg}");
                }
                NetEvent::World(ev) => self.on_world_event(ev),
            }
        }
    }

    fn on_world_event(&mut self, ev: WorldEvent) {
        match ev {
            WorldEvent::Connected => {}
            WorldEvent::SelfCharacter {
                cid,
                tid,
                name,
                face,
                hair,
            } => {
                // Les paquets CHARACTER ultérieurs (mises à jour partielles) n'ont pas toujours le TID :
                // on garde le modèle déjà chargé, et on ignore ceux d'un autre CID.
                // Le personnage local du mode sans client (cid 0) est remplacé par le vrai.
                let mut head = Head {
                    face: face.unwrap_or(0),
                    hair: hair.unwrap_or(0),
                };
                let mut tid = tid;
                if let Some(p) = self.player.as_mut().filter(|p| p.cid != 0) {
                    if p.cid != cid {
                        return;
                    }
                    head = Head {
                        face: face.unwrap_or(p.head.face),
                        hair: hair.unwrap_or(p.head.hair),
                    };
                    if tid == 0 || tid == p.tid {
                        if !name.is_empty() {
                            p.name = name.clone();
                        }
                        if head == p.head {
                            return;
                        }
                        // Même modèle, autre visage ou coiffure : on recharge.
                        tid = p.tid;
                    }
                }
                let name = if name.is_empty() {
                    self.player
                        .as_ref()
                        .map(|p| p.name.clone())
                        .unwrap_or_default()
                } else {
                    name
                };
                let fallback = self.gpu.as_ref().map(|g| g.player_marker.clone());
                let meshes = match (&mut self.gpu, fallback) {
                    (Some(g), Some(fb)) => {
                        entity_meshes(&mut g.renderer, &self.assets, tid, head, &fb)
                            .unwrap_or_default()
                    }
                    _ => Vec::new(),
                };
                let pos = self
                    .player
                    .as_ref()
                    .map(|p| p.position)
                    .unwrap_or(Vec3::ZERO);
                let hp = self.player.as_ref().and_then(|p| p.hp);
                let turn_y = self.player.as_ref().map_or(0.0, |p| p.turn_y);
                log::info!(
                    "joueur {name} (cid {cid}, tid {tid}) : {} maillage(s)",
                    meshes.len()
                );
                self.player = Some(Entity {
                    cid,
                    tid,
                    name: name.clone(),
                    hp,
                    position: pos,
                    destination: pos,
                    turn_y,
                    head,
                    meshes,
                    equipment: Vec::new(),
                    is_player: true,
                });
                self.sync_addons_world();
            }
            WorldEvent::CharacterUpdated {
                cid,
                position,
                destination,
                factors,
            } => {
                let hp = factors.and_then(|f| f.hp);
                if let Some(player) = self.player.as_mut().filter(|p| p.cid == cid) {
                    if let Some(position) = position {
                        player.position = position;
                    }
                    if let Some(destination) = destination {
                        player.destination = destination;
                    }
                    if hp.is_some() {
                        player.hp = hp;
                    }
                } else if let Some(entity) = self.entities.iter_mut().find(|e| e.cid == cid) {
                    if let Some(position) = position {
                        entity.position = position;
                    }
                    if let Some(destination) = destination {
                        entity.destination = destination;
                    }
                    if hp.is_some() {
                        entity.hp = hp;
                    }
                }
            }
            WorldEvent::CharacterRemoved { cid, for_view } => {
                if self.player.as_ref().is_some_and(|player| player.cid == cid) {
                    log::debug!(
                        "paquet retrait personnage local ignoré (cid {cid}, vue={for_view})"
                    );
                } else {
                    self.entities.retain(|entity| entity.cid != cid);
                    log::debug!("personnage {cid} retiré du monde (vue={for_view})");
                }
            }
            WorldEvent::ItemChanged(item) => {
                if item.operation == 1
                    || item
                        .status
                        .is_some_and(|status| !matches!(status, 2 | 3 | 100))
                {
                    self.items.remove(&item.id);
                } else {
                    let current = self.items.entry(item.id).or_insert_with(|| item.clone());
                    current.operation = item.operation;
                    if item.template_id.is_some() {
                        current.template_id = item.template_id;
                    }
                    if item.owner_cid.is_some() {
                        current.owner_cid = item.owner_cid;
                    }
                    if item.count.is_some() {
                        current.count = item.count;
                    }
                    if item.status.is_some() {
                        current.status = item.status;
                    }
                    if item.grid.is_some() {
                        current.grid = item.grid;
                    }
                }
            }
            WorldEvent::ItemsSnapshot(items) => {
                self.items = items
                    .into_iter()
                    .filter(|item| matches!(item.status, Some(2 | 3 | 100)))
                    .map(|item| (item.id, item))
                    .collect();
            }
            WorldEvent::LoadingPosition(p) => {
                if let Some(pl) = &mut self.player {
                    pl.position = p;
                    pl.destination = p;
                }
                self.camera.target = p + Vec3::new(0.0, 100.0, 0.0);
            }
            WorldEvent::GameTime(t) => log::debug!("heure du jeu {t}"),
            WorldEvent::CharacterSeen(c) => {
                if let Some(index) = self.entities.iter().position(|e| e.cid == c.cid) {
                    if self.entities[index].equipment != c.equipment {
                        if let Some(gpu) = self.gpu.as_mut() {
                            let fallback = gpu.marker.clone();
                            let head = self.entities[index].head;
                            if let Ok(meshes) = entity_meshes_with_equipment(
                                &mut gpu.renderer,
                                &self.assets,
                                c.tid,
                                head,
                                &c.equipment,
                                &fallback,
                            ) {
                                self.entities[index].meshes = meshes;
                                self.entities[index].equipment = c.equipment;
                            }
                        }
                    }
                    return;
                }
                let fallback = self.gpu.as_ref().map(|g| g.marker.clone());
                let meshes = match (&mut self.gpu, fallback) {
                    (Some(g), Some(fb)) if self.entities.len() < 64 => {
                        entity_meshes_with_equipment(
                            &mut g.renderer,
                            &self.assets,
                            c.tid,
                            Head::default(),
                            &c.equipment,
                            &fb,
                        )
                        .unwrap_or_default()
                    }
                    (_, Some(fb)) => vec![(fb, Mat4::IDENTITY)],
                    _ => Vec::new(),
                };
                self.entities.push(Entity {
                    cid: c.cid,
                    tid: c.tid,
                    name: c.name,
                    hp: None,
                    position: c.position,
                    destination: c.destination,
                    // Orientation de la vue ; sinon, vers la destination s'il marche déjà.
                    turn_y: c
                        .turn_y
                        .or_else(|| turn_towards(c.position, c.destination))
                        .unwrap_or(0.0),
                    head: Head::default(),
                    meshes,
                    equipment: c.equipment,
                    is_player: false,
                });
            }
            WorldEvent::CharacterMoved {
                cid,
                position,
                destination,
            } => {
                if let Some(e) = self.entities.iter_mut().find(|e| e.cid == cid) {
                    e.position = position;
                    e.destination = destination;
                } else if self.player.as_ref().map(|p| p.cid == cid).unwrap_or(false) {
                    if let Some(p) = &mut self.player {
                        p.position = position;
                        p.destination = destination;
                    }
                }
            }
            WorldEvent::CharacterAction {
                cid,
                target_cid,
                result,
                factor_packet,
                factors,
                target_hp,
                combo,
                force_attack,
                additional_effect,
                hit_index,
            } => {
                let target_hp = factors.as_ref().and_then(|f| f.hp).or(target_hp);
                let actor = self
                    .player
                    .as_ref()
                    .filter(|p| p.cid == cid)
                    .map(|p| p.name.clone())
                    .or_else(|| {
                        self.entities
                            .iter()
                            .find(|e| e.cid == cid)
                            .map(|e| e.name.clone())
                    })
                    .unwrap_or_else(|| format!("CID {cid}"));
                let (target, previous_hp) = self
                    .player
                    .as_ref()
                    .filter(|p| p.cid == target_cid)
                    .map(|p| (p.name.clone(), p.hp))
                    .or_else(|| {
                        self.entities
                            .iter()
                            .find(|e| e.cid == target_cid)
                            .map(|e| (e.name.clone(), e.hp))
                    })
                    .unwrap_or_else(|| (format!("CID {target_cid}"), None));
                if let Some(hp) = target_hp {
                    if let Some(player) = self.player.as_mut().filter(|p| p.cid == target_cid) {
                        player.hp = Some(hp);
                    } else if let Some(target_entity) =
                        self.entities.iter_mut().find(|e| e.cid == target_cid)
                    {
                        target_entity.hp = Some(hp);
                    }
                    if let Some(previous) = previous_hp {
                        if let Some(line) = combat_hp_line(&actor, &target, previous, hp) {
                            self.ui.push_chat(line);
                        }
                        if previous != hp {
                            self.addons.combat(
                                cid,
                                target_cid,
                                hp as i64 - previous as i64,
                                Some(hp as i64),
                            );
                        }
                    }
                }
                log::debug!(
                    "action 0x41 : {actor} ({cid}) → {target_cid}, résultat={result:?}, hp cible={target_hp:?}, dégâts normaux/magiques/eau/feu/terre/air/poison/foudre/glace/héroïques={:?}, combo={combo:?}, attaque forcée={force_attack:?}, effet={additional_effect:?}, hit={hit_index:?}, facteurs={} octets",
                    factors.as_ref().map(|f| [f.normal_damage, f.magic_damage, f.water_damage, f.fire_damage, f.earth_damage, f.air_damage, f.poison_damage, f.lightning_damage, f.ice_damage, f.heroic_damage]),
                    factor_packet.as_ref().map_or(0, Vec::len)
                );
            }
            WorldEvent::Chat {
                from,
                message,
                channel,
                ..
            } => {
                let event = match channel {
                    1 => "CHAT_MSG_WHISPER",
                    2 => "CHAT_MSG_PARTY",
                    _ => "CHAT_MSG_SAY",
                };
                // Filtres des AddOns, fenêtre de discussion, puis événement CHAT_MSG_*.
                self.addon_chat_received(event, &message, &from);
            }
            WorldEvent::SystemMessage(m) => self.addon_chat_received("CHAT_MSG_SYSTEM", &m, ""),
            WorldEvent::EnteredWorld => {
                self.leave_login_scene();
                self.phase = Phase::InWorld;
                self.ui.status = "en jeu".into();
                // Chargement des AddOns (comme sous WoW), PLAYER_LOGIN, PLAYER_ENTERING_WORLD.
                self.enter_world_addons();
                self.ui
                    .push_chat(format!("[système] {} entités en vue", self.entities.len()));
                if self.config.exec_on_enter.is_some() {
                    self.exec_at = Some(Instant::now() + std::time::Duration::from_millis(1500));
                }
            }
            WorldEvent::Unhandled { packet_type, op } => {
                log::debug!("paquet non géré 0x{packet_type:02x} op {op:?}")
            }
            WorldEvent::Disconnected(why) => {
                self.ui.push_chat(format!("[système] déconnecté : {why}"));
                self.ui.status = format!("déconnecté : {why}");
                self.phase = Phase::Login;
                self.net = None;
                self.entities.clear();
                if let Some(player) = &mut self.player {
                    player.hp = None;
                }
                self.leave_world_addons();
            }
        }
    }

    fn handle_actions(&mut self, actions: UiActions) {
        if actions.connect && self.phase == Phase::Login {
            if self.ui.remember {
                let account = crate::account::SavedAccount {
                    user: self.ui.user.trim().to_string(),
                    password: self.ui.password.clone(),
                    character: self.ui.character.trim().to_string(),
                };
                if let Err(e) = crate::account::save(&self.config.saved_dir, &account) {
                    log::warn!("enregistrement du compte : {e:#}");
                    self.ui.status = format!("compte non enregistré : {e}");
                }
            } else {
                crate::account::forget(&self.config.saved_dir);
            }
            self.start_connection(None);
        }
        if self.phase == Phase::CharacterCreate {
            if let Some((tid, name, face, hair)) = actions.create_submit {
                self.ui.status = format!("création de {name}…");
                if let Some(net) = &self.net {
                    net.choose(LoginCommand::Create {
                        tid,
                        name,
                        face,
                        hair,
                    });
                }
            } else if actions.cancel_creation {
                self.ui.creation = None;
                self.ui.status.clear();
                self.phase = Phase::CharacterSelect;
            }
        }
        if self.phase == Phase::CharacterSelect {
            if actions.open_creation {
                self.ui.status = "écran de création…".into();
                if let Some(net) = &self.net {
                    net.choose(LoginCommand::OpenCreation);
                }
            }
            if let Some(name) = actions.delete_character.clone() {
                if let Some(net) = &self.net {
                    net.choose(LoginCommand::Delete(name));
                }
            }
            if let Some(name) = actions.enter_character.clone() {
                let choice = LoginCommand::Enter(name.clone());
                self.ui.character = name.clone();
                if self.ui.remember {
                    let account = crate::account::SavedAccount {
                        user: self.ui.user.trim().to_string(),
                        password: self.ui.password.clone(),
                        character: name.clone(),
                    };
                    if let Err(e) = crate::account::save(&self.config.saved_dir, &account) {
                        log::warn!("enregistrement du compte : {e:#}");
                    }
                }
                if let Some(net) = &self.net {
                    net.choose(choice);
                }
                self.phase = Phase::Connecting;
            } else if actions.back_to_login {
                // Fermer la liaison : la session réseau voit son canal de choix fermé et s'arrête.
                self.net = None;
                self.ui.characters.clear();
                self.ui.status.clear();
                self.leave_login_scene();
                self.phase = Phase::Login;
            }
        }
        if let Some(text) = actions.chat {
            if text.starts_with('/') {
                if !self.addons.run_slash(&text) {
                    self.builtin_slash(&text);
                }
            } else {
                // Passe par `SendChatMessage` des AddOns (surchargeable, accrochable).
                self.send_typed_chat(text);
            }
        }
        if let Some(src) = actions.console {
            if let Err(e) = self.addons.run_script(&src) {
                self.ui.push_chat(format!("[lua] {e}"));
            }
        }
        for id in actions.clicked_frames {
            self.addons.click(id, "LeftButton");
        }
    }

    fn builtin_slash(&mut self, text: &str) {
        let (cmd, arg) = text.split_once(' ').unwrap_or((text, ""));
        match cmd {
            "/reload" | "/rl" => self.reload_addons(),
            // /music <chemin> | off | auto : musique forcée (comme PlayMusic / StopMusic des AddOns).
            "/music" => {
                let arg = arg.trim();
                let line = match arg {
                    "" => format!(
                        "[système] musique : {} ; ambiance : {} ; usage : /music <chemin>|off|auto",
                        self.audio.current_music().unwrap_or("aucune"),
                        self.audio.current_ambience().unwrap_or("aucune")
                    ),
                    "auto" => {
                        self.music_override = None;
                        "[système] musique automatique".to_string()
                    }
                    "off" | "stop" => {
                        self.music_override = Some(None);
                        "[système] musique coupée".to_string()
                    }
                    path => {
                        self.music_override = Some(Some(path.to_string()));
                        format!("[système] musique : {path}")
                    }
                };
                self.ui.push_chat(line);
            }
            // /sound <chemin> : joue un effet (sound/ui/x.wav, sd0000.wav, fichier d'AddOn).
            "/sound" => {
                let arg = arg.trim();
                let line = if arg.is_empty() {
                    "[système] usage : /sound <chemin>".to_string()
                } else if self.audio.play_effect(arg) {
                    format!("[système] son : {arg}")
                } else {
                    format!("[système] son {arg} non joué (introuvable, muet ou sans sortie audio)")
                };
                self.ui.push_chat(line);
            }
            "/goto" => {
                let parts: Vec<f32> = arg
                    .split([' ', ','])
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                if parts.len() >= 3 {
                    let mut dest = Vec3::new(parts[0], parts[1], parts[2]);
                    if let Some(h) = self.terrain.height_at(dest.x, dest.z) {
                        dest.y = h;
                    }
                    log::debug!(
                        "/goto {dest:?} (joueur présent : {}, réseau : {})",
                        self.player.is_some(),
                        self.net.is_some()
                    );
                    if let (Some(net), Some(p)) = (&self.net, &mut self.player) {
                        net.send(Command::MoveTo {
                            position: p.position,
                            destination: dest,
                        });
                        p.destination = dest;
                        self.walking = true;
                    }
                } else {
                    self.ui
                        .push_chat("[système] usage : /goto x y z".to_string());
                }
            }
            "/cam" => {
                // /cam distance [lacet°] [tangage°] : cadre la caméra orbitale (tests et captures).
                let parts: Vec<f32> = arg
                    .split([' ', ','])
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                match parts.as_slice() {
                    [d, rest @ ..] => {
                        self.camera.distance = d.clamp(100.0, 50_000.0);
                        if let Some(yaw) = rest.first() {
                            self.camera.yaw = yaw.to_radians();
                        }
                        if let Some(pitch) = rest.get(1) {
                            self.camera.pitch = pitch.to_radians().clamp(-1.2, 1.5);
                        }
                        self.ui.push_chat(format!(
                            "[système] caméra : distance {:.0}, lacet {:.0}°, tangage {:.0}°",
                            self.camera.distance,
                            self.camera.yaw.to_degrees(),
                            self.camera.pitch.to_degrees()
                        ));
                    }
                    _ => self.ui.push_chat(
                        "[système] usage : /cam distance [lacet°] [tangage°]".to_string(),
                    ),
                }
            }
            "/quit" => self.exit = true,
            _ => self
                .ui
                .push_chat(format!("[système] commande inconnue : {cmd}")),
        }
    }

    /// TID du personnage local affiché en mode sans client (humain par défaut, `a0000060.ecl`).
    const LOCAL_PREVIEW_TID: i32 = 96;

    /// Vitesse de marche (unités du monde par seconde). `Movement=2000` dans charactertemplate.ini
    /// est exprimé pour un pas de 100 ms chez le client ; on prend 1 200 u/s en attendant mieux.
    const WALK_SPEED: f32 = 1200.0;

    /// Distance de tracé des objets du monde (unités ArchLord), en deçà du brouillard.
    const OBJECT_DRAW_DISTANCE: f32 = 25_000.0;

    /// Avance une entité vers sa destination et la pose sur le terrain.
    fn step_entity(terrain: &Terrain, e: &mut Entity, dt: f32) {
        let to = Vec3::new(
            e.destination.x - e.position.x,
            0.0,
            e.destination.z - e.position.z,
        );
        let dist = to.length();
        if dist > 1.0 {
            // Archonia tourne le personnage vers sa destination à chaque déplacement
            // (MoveCharacter, `FUN_140168070`) et garde l'angle à l'arrêt.
            if let Some(turn) = turn_towards(e.position, e.destination) {
                e.turn_y = turn;
            }
            let step = (Self::WALK_SPEED * dt).min(dist);
            e.position += to / dist * step;
        }
        // Les personnages de la scène de connexion (CID négatifs) gardent la hauteur de
        // `LoginSettings.txt` : ils se tiennent sur le sol d'un bâtiment (salle du trône de la
        // division 1817), pas sur le terrain extérieur, plus bas.
        if e.cid < 0 {
            return;
        }
        if let Some(h) = terrain.height_at(e.position.x, e.position.z) {
            e.position.y = h;
        }
    }

    /// Simulation locale : déplacements, hauteur du sol, caméra qui suit le joueur.
    fn simulate(&mut self, dt: f32) {
        for e in self.entities.iter_mut() {
            Self::step_entity(&self.terrain, e, dt);
        }
        if let Some(p) = &mut self.player {
            let before = p.position;
            Self::step_entity(&self.terrain, p, dt);
            if before != p.position {
                self.camera.target = p.position + Vec3::new(0.0, 100.0, 0.0);
            }
            let arrived = (p.destination.x - p.position.x).abs() <= 1.0
                && (p.destination.z - p.position.z).abs() <= 1.0;
            if self.walking && arrived {
                self.walking = false;
                if let Some(net) = &self.net {
                    net.send(Command::Stop {
                        position: p.position,
                    });
                }
                log::debug!("arrivé en {:?}", p.position);
            }
        }
        // `--exec` : plusieurs commandes possibles, séparées par `;` (ex. `/cam 300 90 10; /goto x y z`).
        if self.exec_at.is_some_and(|t| Instant::now() >= t) {
            self.exec_at = None;
            if let Some(cmds) = self.config.exec_on_enter.clone() {
                for cmd in cmds.split(';').map(str::trim).filter(|c| !c.is_empty()) {
                    self.handle_actions(UiActions {
                        chat: Some(cmd.to_string()),
                        ..Default::default()
                    });
                }
            }
        }
        self.animate(dt);
    }

    /// Distance au-delà de laquelle les personnages ne sont plus animés (unités ArchLord).
    const ANIMATION_RANGE: f32 = 8000.0;

    fn is_moving(e: &Entity) -> bool {
        (e.destination.x - e.position.x).abs() > 1.0 || (e.destination.z - e.position.z).abs() > 1.0
    }

    /// Animation des personnages : attente ou course selon le déplacement, sommets réécrits à
    /// 30 Hz au plus, seulement près de la caméra. Les animateurs sont reconstruits quand les
    /// maillages d'une entité changent (équipement, modèle).
    fn animate(&mut self, dt: f32) {
        let Some(gpu) = &self.gpu else { return };
        let focus = self.camera.target;
        let seed = |cid: i32| (cid as u32).wrapping_mul(0x9e37_79b9) | 1;
        if let Some(p) = &self.player {
            let sig = animation::signature(&p.meshes);
            if self.player_animator.as_ref().map(|(s, _)| *s) != Some(sig) {
                let a = Animator::new(&self.assets, p.tid, &p.meshes, seed(p.cid));
                if let Some(a) = &a {
                    log::info!("joueur TID {} animé ({:?})", p.tid, a.clip_name());
                }
                self.player_animator = Some((sig, a));
            }
            if let Some((_, Some(a))) = &mut self.player_animator {
                a.update(&self.assets, &gpu.renderer, dt, Self::is_moving(p), 30.0);
            }
        }
        self.animators
            .retain(|cid, _| self.entities.iter().any(|e| e.cid == *cid));
        for e in &self.entities {
            let sig = animation::signature(&e.meshes);
            let slot = self.animators.entry(e.cid).or_insert((usize::MAX, None));
            if slot.0 != sig {
                *slot = (
                    sig,
                    Animator::new(&self.assets, e.tid, &e.meshes, seed(e.cid)),
                );
            }
            let near = (e.position - focus).length() < Self::ANIMATION_RANGE;
            if let (Some(a), true) = (&mut slot.1, near) {
                a.update(&self.assets, &gpu.renderer, dt, Self::is_moving(e), 30.0);
            }
        }
    }

    /// Clic gauche au sol : rayon caméra → terrain, destination du joueur et paquet 0x40.
    fn click_ground(&mut self) {
        let (Some(gpu), Some((cx, cy)), Some(player)) =
            (&self.gpu, self.last_cursor, self.player.as_ref())
        else {
            return;
        };
        let (w, h) = (
            gpu.renderer.config.width as f32,
            gpu.renderer.config.height as f32,
        );
        let ndc = Vec3::new(
            (cx as f32 / w) * 2.0 - 1.0,
            1.0 - (cy as f32 / h) * 2.0,
            0.0,
        );
        let inv = self.camera.view_proj(gpu.renderer.aspect()).inverse();
        let near = inv.project_point3(ndc);
        let far = inv.project_point3(Vec3::new(ndc.x, ndc.y, 1.0));
        let dir = (far - near).normalize_or_zero();
        let Some(hit) = self.terrain.raycast(near, dir, self.camera.far) else {
            return;
        };
        let from = player.position;
        if let Some(p) = &mut self.player {
            p.destination = hit;
        }
        self.walking = true;
        log::debug!("destination {:?} → {:?}", from, hit);
        if let Some(net) = &self.net {
            net.send(Command::MoveTo {
                position: from,
                destination: hit,
            });
        }
    }

    fn frame(&mut self) -> Result<()> {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;
        self.fps = if dt > 0.0 {
            (self.fps * 0.9) + (1.0 / dt as f32) * 0.1
        } else {
            self.fps
        };
        self.poll_net();

        // Déplacement clavier (ZQSD / WASD) de la caméra.
        let speed = (self.camera.distance * 0.8 * dt as f32).max(1.0);
        if self.keys[0] {
            self.camera.pan(speed, 0.0);
        }
        if self.keys[1] {
            self.camera.pan(-speed, 0.0);
        }
        if self.keys[2] {
            self.camera.pan(0.0, -speed);
        }
        if self.keys[3] {
            self.camera.pan(0.0, speed);
        }
        self.update_login_scene();
        self.simulate(dt as f32);

        // AddOns : état du monde, OnUpdate et minuteurs, commandes, CVars.
        self.sync_addons_frame();
        self.addons.update(dt);
        self.apply_addon_commands();
        self.apply_addon_cvars();
        // La scène de connexion garde sa caméra fixe (`LoginSettings.txt`), CVars comprises.
        self.reapply_scene_camera();
        let object_distance = self.object_draw_distance();
        self.update_audio(dt as f32);
        self.save_options_if_needed(false);

        let Some(gpu) = &mut self.gpu else {
            return Ok(());
        };
        let raw_input = gpu.egui_state.take_egui_input(&gpu.window);
        let ctx = gpu.egui_state.egui_ctx().clone();
        if let Some(colors) = self.addons.take_ui_colors() {
            self.addon_glue.ui.apply_ui_colors(&ctx, &colors);
        }
        let mut addon_out = crate::addon_ui::AddonUiOutput::default();
        let mut actions = UiActions::default();
        let phase = self.phase;
        let busy = phase == Phase::Connecting;
        let choosing = self.choosing_character;
        // La scène 3D remplace l'image une fois son terrain chargé.
        let scene_ready = self.terrain.loaded_count() >= 9;
        let player_info = self
            .player
            .as_ref()
            .map(|p| (p.name.clone(), p.position.to_array(), p.hp));
        let ents: Vec<(String, [f32; 3], Option<i32>)> = self
            .entities
            .iter()
            .map(|e| (e.name.clone(), e.position.to_array(), e.hp))
            .collect();
        let inventory: Vec<_> = self
            .items
            .values()
            .cloned()
            .map(|item| {
                let name = item
                    .template_id
                    .and_then(|tid| item_name_for_tid(&self.assets, tid))
                    .unwrap_or_else(|| {
                        format!("Objet TID {}", item.template_id.unwrap_or_default())
                    });
                (item, name)
            })
            .collect();
        let login_screen = self
            .login_screen
            .get_or_insert_with(|| ui::load_login_screen(&ctx))
            .clone();
        let full_output = ctx.run(raw_input, |ctx| {
            if phase != Phase::InWorld {
                // Écran de connexion : image plein écran, panneau de connexion ou de sélection
                // du personnage, console seule.
                let in_scene = phase == Phase::CharacterSelect
                    || phase == Phase::CharacterCreate
                    || (busy && choosing && !self.ui.characters.is_empty());
                if !in_scene || !scene_ready {
                    if let Some(tex) = &login_screen {
                        ui::login_background(ctx, tex);
                    }
                }
                if phase == Phase::CharacterCreate {
                    ui::character_create_panel(ctx, &mut self.ui, busy, &mut actions);
                } else if in_scene {
                    ui::character_select_panel(ctx, &mut self.ui, busy, &mut actions);
                } else {
                    ui::login_panel(ctx, &mut self.ui, busy, &mut actions);
                }
            } else {
                // Les panneaux natifs sont des frames pilotables par les AddOns
                // (`ChatFrame1:Hide()`, `ArchRustInfoFrame:Hide()`…).
                if self.addons.builtin_frame("ChatFrame1").0 {
                    ui::chat_panel(ctx, &mut self.ui, &mut actions);
                }
                if self.addons.builtin_frame("ArchRustInfoFrame").0 {
                    ui::info_panel(
                        ctx,
                        &mut self.ui,
                        self.fps,
                        player_info.as_ref().map(|(n, p, hp)| (n.as_str(), *p, *hp)),
                        &ents,
                        &self.addons,
                        self.terrain.loaded_count(),
                    );
                }
                if self.addons.builtin_frame("ArchRustInventoryButton").0 {
                    ui::inventory_panel(ctx, &mut self.ui, &inventory);
                }
                // Comme dans WoW, l'interface des AddOns n'apparaît qu'en jeu.
                self.addon_glue
                    .ui
                    .draw(ctx, &self.addons, &self.assets, &mut addon_out);
            }
            ui::console_panel(ctx, &mut self.ui, &mut actions);
            let status = ui::AudioStatus {
                active: self.audio.is_active(),
                music: self.audio.current_music().map(str::to_string),
                ambience: self.audio.current_ambience().map(str::to_string),
                region: self.zone_region.as_ref().map(|(r, _)| {
                    (
                        *r,
                        Zones::region_name(&self.assets, *r).unwrap_or_default(),
                    )
                }),
            };
            if ui::options_panel(ctx, &mut self.ui, &mut self.options.audio, &status) {
                self.audio.set_settings(&self.options.audio);
                self.options_dirty = true;
            }
        });
        gpu.egui_state
            .handle_platform_output(&gpu.window, full_output.platform_output);
        let paint_jobs = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [gpu.renderer.config.width, gpu.renderer.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        for (id, delta) in &full_output.textures_delta.set {
            gpu.egui_renderer
                .update_texture(&gpu.renderer.device, &gpu.renderer.queue, *id, delta);
        }

        // Scène 3D : terrain autour du joueur (ou de la caméra), puis entités.
        let scene_center = if self.phase == Phase::InWorld {
            None
        } else {
            self.scene_focus
        };
        if self.phase == Phase::InWorld || self.player.is_some() || scene_center.is_some() {
            let center = scene_center.unwrap_or_else(|| {
                self.player
                    .as_ref()
                    .map(|p| p.position)
                    .unwrap_or(self.camera.target)
            });
            self.terrain.update(&mut gpu.renderer, &self.assets, center);
            self.objects.update(&mut gpu.renderer, &self.assets, center);
        }
        let ground = self.terrain.ground_meshes();
        let mut items: Vec<DrawItem> = self.terrain.draw_items();
        items.extend(self.objects.draw_items(self.camera.eye(), object_distance));
        if items.is_empty() && ground.is_empty() {
            items.push(DrawItem {
                mesh: gpu.ground.clone(),
                model: Mat4::from_translation(Vec3::new(
                    self.camera.target.x,
                    0.0,
                    self.camera.target.z,
                )),
                tint: [1.0; 4],
            });
        }
        // L'aperçu (`--preview`) est posé à l'origine, comme le personnage local du mode sans client.
        for (mesh, local) in &self.preview {
            items.push(DrawItem {
                mesh: mesh.clone(),
                model: *local,
                tint: [1.0; 4],
            });
        }
        if let Some(p) = &self.player {
            for (mesh, local) in &p.meshes {
                items.push(DrawItem {
                    mesh: mesh.clone(),
                    model: p.model_matrix() * *local,
                    tint: self.addons.unit_tint(p.cid).unwrap_or([1.0; 4]),
                });
            }
        }
        for e in &self.entities {
            for (mesh, local) in &e.meshes {
                items.push(DrawItem {
                    mesh: mesh.clone(),
                    model: e.model_matrix() * *local,
                    tint: self.addons.unit_tint(e.cid).unwrap_or([1.0; 4]),
                });
            }
        }
        let view_proj = self.camera.view_proj(gpu.renderer.aspect());
        let eye = self.camera.eye();
        if let Some((_, after)) = &self.config.screenshot {
            if !self.screenshot_done && self.started.elapsed().as_secs_f32() >= *after {
                gpu.renderer.capture_next = true;
            }
        }
        let egui_renderer = &mut gpu.egui_renderer;
        gpu.renderer.render(
            view_proj,
            eye,
            &ground,
            &items,
            |device, queue, encoder, view| {
                egui_renderer.update_buffers(device, queue, encoder, &paint_jobs, &screen);
                let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                let mut pass = pass.forget_lifetime();
                egui_renderer.render(&mut pass, &paint_jobs, &screen);
            },
        )?;
        for id in &full_output.textures_delta.free {
            gpu.egui_renderer.free_texture(id);
        }
        if let Some((w, h, rgba)) = gpu.renderer.take_capture() {
            if let Some(path) = self.addon_glue.ui.screenshot.take() {
                // `Screenshot()` d'un AddOn : le client continue.
                match image::RgbaImage::from_raw(w, h, rgba).map(|img| img.save(&path)) {
                    Some(Ok(())) => self
                        .ui
                        .push_chat(format!("[système] capture : {}", path.display())),
                    other => log::error!("capture : {other:?}"),
                }
            } else if let Some((path, _)) = &self.config.screenshot {
                match image::RgbaImage::from_raw(w, h, rgba).map(|img| img.save(path)) {
                    Some(Ok(())) => log::info!(
                        "capture enregistrée : {} ({w}x{h}, {} secteurs, {} entités)",
                        path.display(),
                        self.terrain.loaded_count(),
                        self.entities.len()
                    ),
                    other => log::error!("capture : {other:?}"),
                }
                self.screenshot_done = true;
                self.exit = true;
            }
        }
        self.after_addon_ui(addon_out);
        self.handle_actions(actions);
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_none() {
            if let Err(e) = self.init_gpu(event_loop) {
                log::error!("initialisation graphique : {e:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let consumed = if let Some(gpu) = &mut self.gpu {
            gpu.egui_state.on_window_event(&gpu.window, &event).consumed
        } else {
            false
        };
        match event {
            WindowEvent::CloseRequested => {
                self.shutdown_addons();
                self.save_options_if_needed(true);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.frame() {
                    log::error!("rendu : {e:#}");
                }
                if self.exit {
                    self.shutdown_addons();
                    self.save_options_if_needed(true);
                    event_loop.exit();
                }
                if let Some(gpu) = &self.gpu {
                    gpu.window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. }
                if !consumed || state == ElementState::Released =>
            {
                // Un clic sur une frame d'AddOn ne traverse pas vers le monde.
                let on_addon = self.addon_glue.pointer_captured;
                match button {
                    MouseButton::Right => {
                        self.right_drag = state == ElementState::Pressed && !on_addon
                    }
                    MouseButton::Middle => {
                        self.middle_drag = state == ElementState::Pressed && !on_addon
                    }
                    MouseButton::Left
                        if state == ElementState::Pressed
                            && self.phase == Phase::InWorld
                            && !on_addon =>
                    {
                        self.click_ground()
                    }
                    MouseButton::Left
                        if state == ElementState::Pressed
                            && self.phase == Phase::CharacterSelect =>
                    {
                        self.click_scene_character()
                    }
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((lx, ly)) = self.last_cursor {
                    let (dx, dy) = ((position.x - lx) as f32, (position.y - ly) as f32);
                    if self.right_drag {
                        self.camera.rotate(dx, dy);
                    } else if self.middle_drag {
                        self.camera.pan(
                            dy * self.camera.distance * 0.002,
                            -dx * self.camera.distance * 0.002,
                        );
                    }
                }
                self.last_cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseWheel { delta, .. }
                if !consumed && !self.addon_glue.pointer_captured =>
            {
                let d = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.camera.zoom(d);
            }
            WindowEvent::ModifiersChanged(m) => self.addon_glue.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } if !consumed => {
                let down = event.state == ElementState::Pressed;
                // Raccourcis et OnKeyDown des AddOns (`ArchRust.SetBinding`) d'abord.
                if let PhysicalKey::Code(code) = event.physical_key {
                    if !event.repeat && self.addon_key(code, down) {
                        return;
                    }
                }
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::KeyW) => self.keys[0] = down,
                    PhysicalKey::Code(KeyCode::KeyS) => self.keys[1] = down,
                    PhysicalKey::Code(KeyCode::KeyA) => self.keys[2] = down,
                    PhysicalKey::Code(KeyCode::KeyD) => self.keys[3] = down,
                    PhysicalKey::Code(KeyCode::Backquote) if down && event.repeat == false => {
                        self.ui.console_open = !self.ui.console_open
                    }
                    // Échap : ferme la console ou les options, sinon ouvre les options.
                    PhysicalKey::Code(KeyCode::Escape) if down && !event.repeat => {
                        if self.ui.console_open {
                            self.ui.console_open = false;
                        } else {
                            self.ui.show_options = !self.ui.show_options;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(gpu) = &self.gpu {
            gpu.window.request_redraw();
        }
    }
}

/// Lance le client.
pub fn run(config: Config) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(config)?;
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::combat_hp_line;

    #[test]
    fn combat_log_reports_hp_loss_heal_and_ignores_unchanged_values() {
        assert_eq!(
            combat_hp_line("A", "B", 100, 73).as_deref(),
            Some("[combat] A → B : -27 PV (reste 73)")
        );
        assert_eq!(
            combat_hp_line("A", "B", 73, 100).as_deref(),
            Some("[combat] A → B : +27 PV (reste 100)")
        );
        assert_eq!(combat_hp_line("A", "B", 100, 100), None);
    }
}
