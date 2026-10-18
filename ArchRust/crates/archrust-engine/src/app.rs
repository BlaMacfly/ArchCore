//! Boucle principale : fenêtre winit, rendu, interface, AddOns et réseau.

use crate::assets::Assets;
use crate::camera::OrbitCamera;
use crate::net::{Credentials, NetEvent, NetLink};
use crate::render::{DrawItem, Mesh, Renderer};
use crate::scene::{
    box_mesh, entity_meshes, entity_meshes_with_equipment, ground_mesh, item_name_for_tid,
    model_meshes, Entity,
};
use crate::ui::{self, UiActions, UiState};
use crate::world::Terrain;
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
    /// Commande de discussion (`/goto …`, `/hello`) exécutée dès l'entrée en jeu.
    pub exec_on_enter: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Login,
    Connecting,
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
    camera: OrbitCamera,
    phase: Phase,
    net: Option<NetLink>,
    entities: Vec<Entity>,
    items: HashMap<i32, archrust_netclient::world::ItemRecord>,
    player: Option<Entity>,
    preview: Vec<(Arc<Mesh>, Mat4)>,
    terrain: Terrain,
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
}

/// Résumé visible d'une variation de PV observée entre deux actions réseau.
fn combat_hp_line(actor: &str, target: &str, previous: i32, current: i32) -> Option<String> {
    let delta = current as i64 - previous as i64;
    (delta != 0).then(|| format!("[combat] {actor} → {target} : {delta:+} PV (reste {current})"))
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        let assets = Arc::new(Assets::new(config.game_dir.clone(), &config.patch_dirs));
        let mut addons =
            AddonManager::new(&config.saved_dir).map_err(|e| anyhow::anyhow!("AddOns : {e}"))?;
        addons.set_host(|h| {
            h.realm = "ArchCore".into();
            h.locale = "frFR".into();
        });
        addons
            .load_dir(&config.addons_dir)
            .map_err(|e| anyhow::anyhow!("AddOns : {e}"))?;
        log::info!("{} AddOn(s) chargé(s)", addons.addons.len());
        Ok(Self {
            config,
            assets,
            addons,
            gpu: None,
            ui: UiState::default(),
            camera: OrbitCamera::default(),
            phase: Phase::Login,
            net: None,
            entities: Vec::new(),
            items: HashMap::new(),
            player: None,
            preview: Vec::new(),
            terrain: Terrain::default(),
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
        })
    }

    fn init_gpu(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let mut attrs = Window::default_attributes()
            .with_title(&self.config.title)
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        // Icône de fenêtre : le logo du projet s'il est présent à côté de l'exécutable ou du dépôt.
        for candidate in ["docs/assets/archrust-logo.png", "archrust-logo.png"] {
            if let Ok(img) = image::open(candidate) {
                let img = img
                    .resize_exact(64, 64, image::imageops::FilterType::Lanczos3)
                    .to_rgba8();
                if let Ok(icon) = winit::window::Icon::from_rgba(img.into_raw(), 64, 64) {
                    attrs = attrs.with_window_icon(Some(icon));
                    break;
                }
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
            // utilisateur:motdepasse@hôte:port/personnage
            if let Some((creds, rest)) = spec.split_once('@') {
                let (user, pass) = creds.split_once(':').unwrap_or((creds, ""));
                let (hostport, character) = rest.split_once('/').unwrap_or((rest, "Testeur"));
                let (host, port) = hostport.split_once(':').unwrap_or((hostport, "11002"));
                self.ui.user = user.to_string();
                self.ui.password = pass.to_string();
                self.ui.host = host.to_string();
                self.ui.port = port.to_string();
                self.ui.character = character.to_string();
                self.start_connection();
            } else {
                log::warn!(
                    "--connect : forme attendue utilisateur:motdepasse@hôte:port/personnage"
                );
            }
        }
        Ok(())
    }

    fn start_connection(&mut self) {
        let port = self.ui.port.trim().parse().unwrap_or(11002);
        let creds = Credentials {
            host: self.ui.host.trim().to_string(),
            port,
            user: self.ui.user.trim().to_string(),
            password: self.ui.password.clone(),
            character: self.ui.character.trim().to_string(),
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
                    self.phase = Phase::Login;
                    self.net = None;
                    return;
                }
                NetEvent::Worlds(w) => self
                    .ui
                    .push_chat(format!("[système] mondes : {}", w.join(", "))),
                NetEvent::Characters(c) => {
                    for ch in c {
                        self.ui.push_chat(format!(
                            "[système] personnage {} niveau {} (cid {})",
                            ch.name, ch.level, ch.cid
                        ));
                    }
                }
                NetEvent::World(ev) => self.on_world_event(ev),
            }
        }
    }

    fn on_world_event(&mut self, ev: WorldEvent) {
        match ev {
            WorldEvent::Connected => {}
            WorldEvent::SelfCharacter { cid, tid, name } => {
                // Les paquets CHARACTER ultérieurs (mises à jour partielles) n'ont pas toujours le TID :
                // on garde le modèle déjà chargé, et on ignore ceux d'un autre CID.
                // Le personnage local du mode sans client (cid 0) est remplacé par le vrai.
                if let Some(p) = self.player.as_mut().filter(|p| p.cid != 0) {
                    if p.cid != cid {
                        return;
                    }
                    if tid == 0 || tid == p.tid {
                        if !name.is_empty() {
                            p.name = name;
                        }
                        return;
                    }
                }
                let fallback = self.gpu.as_ref().map(|g| g.player_marker.clone());
                let meshes = match (&mut self.gpu, fallback) {
                    (Some(g), Some(fb)) => {
                        entity_meshes(&mut g.renderer, &self.assets, tid, &fb).unwrap_or_default()
                    }
                    _ => Vec::new(),
                };
                let pos = self
                    .player
                    .as_ref()
                    .map(|p| p.position)
                    .unwrap_or(Vec3::ZERO);
                let hp = self.player.as_ref().and_then(|p| p.hp);
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
                    meshes,
                    equipment: Vec::new(),
                    is_player: true,
                });
                self.addons.set_host(|h| {
                    h.player_name = name;
                    h.player_level = 1;
                });
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
                self.addons.set_host(|h| h.player_position = p.to_array());
            }
            WorldEvent::GameTime(t) => log::debug!("heure du jeu {t}"),
            WorldEvent::CharacterSeen(c) => {
                if let Some(index) = self.entities.iter().position(|e| e.cid == c.cid) {
                    if self.entities[index].equipment != c.equipment {
                        if let Some(gpu) = self.gpu.as_mut() {
                            let fallback = gpu.marker.clone();
                            if let Ok(meshes) = entity_meshes_with_equipment(
                                &mut gpu.renderer,
                                &self.assets,
                                c.tid,
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
                let line = format!(
                    "[{}] {}",
                    if from.is_empty() { "?" } else { &from },
                    message
                );
                self.ui.push_chat(line);
                let event = match channel {
                    1 => "CHAT_MSG_WHISPER",
                    2 => "CHAT_MSG_PARTY",
                    _ => "CHAT_MSG_SAY",
                };
                self.addons.fire_event_str(event, &[&message, &from]);
            }
            WorldEvent::SystemMessage(m) => self.ui.push_chat(format!("[système] {m}")),
            WorldEvent::EnteredWorld => {
                self.phase = Phase::InWorld;
                self.ui.status = "en jeu".into();
                self.addons.fire_event("PLAYER_LOGIN", &[]);
                self.addons.fire_event("PLAYER_ENTERING_WORLD", &[]);
                self.ui
                    .push_chat(format!("[système] {} entités en vue", self.entities.len()));
                if let Some(cmd) = self.config.exec_on_enter.clone() {
                    self.handle_actions(UiActions {
                        chat: Some(cmd),
                        ..Default::default()
                    });
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
                self.addons.fire_event("PLAYER_LOGOUT", &[]);
            }
        }
    }

    fn handle_actions(&mut self, actions: UiActions) {
        if actions.connect && self.phase == Phase::Login {
            self.start_connection();
        }
        if let Some(text) = actions.chat {
            if text.starts_with('/') {
                if !self.addons.run_slash(&text) {
                    self.builtin_slash(&text);
                }
            } else {
                self.ui.push_chat(format!(
                    "[{}] {}",
                    self.player
                        .as_ref()
                        .map(|p| p.name.as_str())
                        .unwrap_or("moi"),
                    text
                ));
                if let Some(net) = &self.net {
                    net.send(Command::Say(text));
                }
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
            "/reload" | "/rl" => {
                let _ = self.addons.save_all();
                match AddonManager::new(&self.config.saved_dir) {
                    Ok(mut m) => {
                        let _ = m.load_dir(&self.config.addons_dir);
                        self.addons = m;
                        self.ui
                            .push_chat("[système] interface rechargée".to_string());
                    }
                    Err(e) => self.ui.push_chat(format!("[erreur] {e}")),
                }
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

    fn apply_addon_commands(&mut self) {
        for cmd in self.addons.drain_commands() {
            match cmd {
                HostCommand::Print(l) => self.ui.push_chat(l),
                HostCommand::Chat { message, .. } => {
                    self.ui.push_chat(format!(
                        "[{}] {}",
                        self.player
                            .as_ref()
                            .map(|p| p.name.as_str())
                            .unwrap_or("moi"),
                        message
                    ));
                    if let Some(net) = &self.net {
                        net.send(Command::Say(message));
                    }
                }
                HostCommand::PlaySound(p) => log::info!("son demandé : {p}"),
                HostCommand::ReloadUi => self.builtin_slash("/reload"),
            }
        }
    }

    /// TID du personnage local affiché en mode sans client (humain par défaut, `a0000060.ecl`).
    const LOCAL_PREVIEW_TID: i32 = 96;

    /// Vitesse de marche (unités du monde par seconde). `Movement=2000` dans charactertemplate.ini
    /// est exprimé pour un pas de 100 ms chez le client ; on prend 1 200 u/s en attendant mieux.
    const WALK_SPEED: f32 = 1200.0;

    /// Avance une entité vers sa destination et la pose sur le terrain.
    fn step_entity(terrain: &Terrain, e: &mut Entity, dt: f32) {
        let to = Vec3::new(
            e.destination.x - e.position.x,
            0.0,
            e.destination.z - e.position.z,
        );
        let dist = to.length();
        if dist > 1.0 {
            let step = (Self::WALK_SPEED * dt).min(dist);
            e.position += to / dist * step;
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
                let pos = p.position.to_array();
                self.addons.set_host(|h| h.player_position = pos);
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
        self.simulate(dt as f32);

        self.addons.set_host(|h| h.framerate = self.fps as f64);
        self.addons.update(dt);
        self.apply_addon_commands();

        let Some(gpu) = &mut self.gpu else {
            return Ok(());
        };
        let raw_input = gpu.egui_state.take_egui_input(&gpu.window);
        let ctx = gpu.egui_state.egui_ctx().clone();
        let mut actions = UiActions::default();
        let phase = self.phase;
        let busy = phase == Phase::Connecting;
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
        let full_output = ctx.run(raw_input, |ctx| {
            if phase != Phase::InWorld {
                ui::login_panel(ctx, &mut self.ui, busy, &mut actions);
            }
            ui::chat_panel(ctx, &mut self.ui, &mut actions);
            ui::console_panel(ctx, &mut self.ui, &mut actions);
            ui::info_panel(
                ctx,
                &mut self.ui,
                self.fps,
                player_info.as_ref().map(|(n, p, hp)| (n.as_str(), *p, *hp)),
                &ents,
                &self.addons,
                self.terrain.loaded_count(),
            );
            ui::inventory_panel(ctx, &mut self.ui, &inventory);
            let tree = &self.addons.shared.borrow().ui;
            ui::draw_addon_frames(ctx, tree, &self.addons, &mut actions);
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
        if self.phase == Phase::InWorld || self.player.is_some() {
            let center = self
                .player
                .as_ref()
                .map(|p| p.position)
                .unwrap_or(self.camera.target);
            self.terrain.update(&mut gpu.renderer, &self.assets, center);
        }
        let mut items: Vec<DrawItem> = self.terrain.draw_items();
        if items.is_empty() {
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
                    tint: [1.0; 4],
                });
            }
        }
        for e in &self.entities {
            for (mesh, local) in &e.meshes {
                items.push(DrawItem {
                    mesh: mesh.clone(),
                    model: e.model_matrix() * *local,
                    tint: [1.0; 4],
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
        gpu.renderer
            .render(view_proj, eye, &items, |device, queue, encoder, view| {
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
            })?;
        for id in &full_output.textures_delta.free {
            gpu.egui_renderer.free_texture(id);
        }
        if let Some((w, h, rgba)) = gpu.renderer.take_capture() {
            if let Some((path, _)) = &self.config.screenshot {
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
                let _ = self.addons.save_all();
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
                    let _ = self.addons.save_all();
                    event_loop.exit();
                }
                if let Some(gpu) = &self.gpu {
                    gpu.window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. }
                if !consumed || state == ElementState::Released =>
            {
                match button {
                    MouseButton::Right => self.right_drag = state == ElementState::Pressed,
                    MouseButton::Middle => self.middle_drag = state == ElementState::Pressed,
                    MouseButton::Left
                        if state == ElementState::Pressed && self.phase == Phase::InWorld =>
                    {
                        self.click_ground()
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
            WindowEvent::MouseWheel { delta, .. } if !consumed => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.camera.zoom(d);
            }
            WindowEvent::KeyboardInput { event, .. } if !consumed => {
                let down = event.state == ElementState::Pressed;
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::KeyW) => self.keys[0] = down,
                    PhysicalKey::Code(KeyCode::KeyS) => self.keys[1] = down,
                    PhysicalKey::Code(KeyCode::KeyA) => self.keys[2] = down,
                    PhysicalKey::Code(KeyCode::KeyD) => self.keys[3] = down,
                    PhysicalKey::Code(KeyCode::Backquote) if down && event.repeat == false => {
                        self.ui.console_open = !self.ui.console_open
                    }
                    PhysicalKey::Code(KeyCode::Escape) if down => self.ui.console_open = false,
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
