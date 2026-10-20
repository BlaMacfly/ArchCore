//! Liaison entre le moteur et les AddOns Lua (sous-module de `app`) : chargement à l'entrée en
//! jeu, état du monde transmis chaque image, commandes des AddOns, CVars appliquées au rendu,
//! discussion filtrée, clavier et souris.

use super::*;
use crate::addon_ui::{AddonUi, AddonUiOutput};
use archrust_addons::{CameraInfo, ItemInfo, UnitInfo};
use winit::keyboard::ModifiersState;

/// CVars du moteur que les AddOns peuvent lire et modifier (`GetCVar` / `SetCVar`).
pub const CVARS: &[(&str, &str, &str)] = &[
    (
        "skyColor",
        "0.35 0.45 0.6",
        "couleur du ciel (fond), « r g b » entre 0 et 1",
    ),
    (
        "cameraFov",
        "55",
        "champ de vision vertical de la caméra, en degrés",
    ),
    (
        "cameraFarClip",
        "200000",
        "distance maximale de rendu de la caméra",
    ),
    (
        "objectDrawDistance",
        "25000",
        "distance de tracé des objets du monde",
    ),
    (
        "lightDirection",
        "0.4 0.8 0.3",
        "direction de la lumière du soleil « x y z »",
    ),
    (
        "ambientColor",
        "0.45 0.45 0.5",
        "lumière ambiante « r g b »",
    ),
    ("uiScale", "1", "échelle de toute l'interface (0,5 à 3)"),
    (
        "nameplateDistance",
        "6000",
        "distance indicative des plaques de nom (lue par les AddOns)",
    ),
    (
        "loginMusic",
        crate::audio::DEFAULT_LOGIN_MUSIC,
        "musique des écrans de connexion, de sélection et de création (chemin client ou patch ; vide = silence)",
    ),
    (
        "worldMusic",
        crate::audio::DEFAULT_WORLD_MUSIC,
        "musique en jeu quand la région du joueur n'en indique pas (RegionTemplate.ini → skyset.ini)",
    ),
    (
        "worldAmbience",
        crate::audio::DEFAULT_WORLD_AMBIENCE,
        "ambiance en jeu (sound/stream/*.mp3 ; vide = aucune)",
    ),
];

/// État de la liaison, gardé dans `App`.
#[derive(Default)]
pub struct AddonGlue {
    pub ui: AddonUi,
    /// L'interface des AddOns a capté la souris à l'image précédente.
    pub pointer_captured: bool,
    pub modifiers: ModifiersState,
    /// Signature de l'inventaire déjà transmis (évite de le reconstruire à chaque image).
    inventory_sig: Option<u64>,
    /// Dernière valeur appliquée de `uiScale`.
    ui_scale: Option<f32>,
    /// Extensions ArchRust négociées avec le serveur monde (gardées pour chaque rechargement).
    pub extensions: Option<archrust_addons::ExtInfo>,
}

fn unit_of(e: &Entity, moving: bool) -> UnitInfo {
    UnitInfo {
        cid: e.cid,
        tid: e.tid,
        name: e.name.clone(),
        level: 1,
        hp: e.hp.map(i64::from),
        hp_max: None,
        mp: None,
        mp_max: None,
        position: e.position.to_array(),
        facing: e.turn_y,
        moving,
    }
}

fn floats(v: &[f32], n: usize) -> Option<Vec<f32>> {
    (v.len() >= n).then(|| v[..n].to_vec())
}

/// Nom WoW d'une touche (`A`, `F1`, `ESCAPE`…), précédé de `ALT-`, `CTRL-`, `SHIFT-`.
pub fn wow_key_name(code: KeyCode, mods: ModifiersState) -> Option<String> {
    use KeyCode::*;
    let base: String = match code {
        KeyA => "A".into(),
        KeyB => "B".into(),
        KeyC => "C".into(),
        KeyD => "D".into(),
        KeyE => "E".into(),
        KeyF => "F".into(),
        KeyG => "G".into(),
        KeyH => "H".into(),
        KeyI => "I".into(),
        KeyJ => "J".into(),
        KeyK => "K".into(),
        KeyL => "L".into(),
        KeyM => "M".into(),
        KeyN => "N".into(),
        KeyO => "O".into(),
        KeyP => "P".into(),
        KeyQ => "Q".into(),
        KeyR => "R".into(),
        KeyS => "S".into(),
        KeyT => "T".into(),
        KeyU => "U".into(),
        KeyV => "V".into(),
        KeyW => "W".into(),
        KeyX => "X".into(),
        KeyY => "Y".into(),
        KeyZ => "Z".into(),
        Digit0 => "0".into(),
        Digit1 => "1".into(),
        Digit2 => "2".into(),
        Digit3 => "3".into(),
        Digit4 => "4".into(),
        Digit5 => "5".into(),
        Digit6 => "6".into(),
        Digit7 => "7".into(),
        Digit8 => "8".into(),
        Digit9 => "9".into(),
        F1 => "F1".into(),
        F2 => "F2".into(),
        F3 => "F3".into(),
        F4 => "F4".into(),
        F5 => "F5".into(),
        F6 => "F6".into(),
        F7 => "F7".into(),
        F8 => "F8".into(),
        F9 => "F9".into(),
        F10 => "F10".into(),
        F11 => "F11".into(),
        F12 => "F12".into(),
        Escape => "ESCAPE".into(),
        Enter | NumpadEnter => "ENTER".into(),
        Space => "SPACE".into(),
        Tab => "TAB".into(),
        Backspace => "BACKSPACE".into(),
        Delete => "DELETE".into(),
        Insert => "INSERT".into(),
        Home => "HOME".into(),
        End => "END".into(),
        PageUp => "PAGEUP".into(),
        PageDown => "PAGEDOWN".into(),
        ArrowUp => "UP".into(),
        ArrowDown => "DOWN".into(),
        ArrowLeft => "LEFT".into(),
        ArrowRight => "RIGHT".into(),
        Minus => "-".into(),
        Equal => "=".into(),
        Numpad0 => "NUMPAD0".into(),
        Numpad1 => "NUMPAD1".into(),
        Numpad2 => "NUMPAD2".into(),
        Numpad3 => "NUMPAD3".into(),
        Numpad4 => "NUMPAD4".into(),
        Numpad5 => "NUMPAD5".into(),
        Numpad6 => "NUMPAD6".into(),
        Numpad7 => "NUMPAD7".into(),
        Numpad8 => "NUMPAD8".into(),
        Numpad9 => "NUMPAD9".into(),
        _ => return None,
    };
    let mut name = String::new();
    if mods.alt_key() {
        name.push_str("ALT-");
    }
    if mods.control_key() {
        name.push_str("CTRL-");
    }
    if mods.shift_key() {
        name.push_str("SHIFT-");
    }
    name.push_str(&base);
    Some(name)
}

impl App {
    /// Nouvel état des AddOns (sans AddOn chargé) avec les CVars du moteur.
    pub(super) fn new_addon_manager(config: &Config) -> Result<AddonManager> {
        let m =
            AddonManager::new(&config.saved_dir).map_err(|e| anyhow::anyhow!("AddOns : {e}"))?;
        m.set_host(|h| {
            h.realm = "ArchCore".into();
            h.locale = "frFR".into();
        });
        for (name, default, _) in CVARS {
            m.register_cvar(name, default);
        }
        Ok(m)
    }

    /// Joueur, unités en vue et inventaire → AddOns (événements UNIT_*, NAME_PLATE_*, BAG_UPDATE…).
    pub(super) fn sync_addons_world(&mut self) {
        let player = self
            .player
            .as_ref()
            .map(|p| unit_of(p, self.walking || Self::is_moving(p)));
        let units: Vec<UnitInfo> = self
            .entities
            .iter()
            .map(|e| unit_of(e, Self::is_moving(e)))
            .collect();
        // Inventaire : reconstruit seulement s'il a changé.
        let sig = self.items.values().fold(self.items.len() as u64, |acc, i| {
            acc.wrapping_mul(31)
                .wrapping_add(i.id as u64)
                .wrapping_add((i.count.unwrap_or(0) as u64) << 20)
                .wrapping_add((i.status.unwrap_or(0) as u64) << 40)
                .wrapping_add(i.grid.map_or(0, |g| {
                    (g[0] as i64 * 10_000 + g[1] as i64 * 100 + g[2] as i64) as u64
                }))
        });
        let inventory = (self.addon_glue.inventory_sig != Some(sig)).then(|| {
            self.addon_glue.inventory_sig = Some(sig);
            let mut v: Vec<ItemInfo> = self
                .items
                .values()
                .map(|i| ItemInfo {
                    id: i.id,
                    tid: i.template_id,
                    name: i
                        .template_id
                        .and_then(|tid| item_name_for_tid(&self.assets, tid))
                        .unwrap_or_else(|| format!("Objet {}", i.template_id.unwrap_or_default())),
                    count: i.count,
                    status: i.status.map(i32::from),
                    grid: i.grid.map(|g| g.map(i32::from)),
                })
                .collect();
            v.sort_by_key(|i| i.id);
            v
        });
        self.addons.sync_world(player, units, inventory);
    }

    /// Caméra, matrice de projection, secteurs chargés, touches de modification → AddOns.
    pub(super) fn sync_addons_frame(&mut self) {
        let aspect = self
            .gpu
            .as_ref()
            .map_or(16.0 / 9.0, |g| g.renderer.aspect());
        let cam = CameraInfo {
            distance: self.camera.distance,
            yaw: self.camera.yaw.to_degrees(),
            pitch: self.camera.pitch.to_degrees(),
            fov: self.camera.fov_y.to_degrees(),
            target: self.camera.target.to_array(),
            eye: self.camera.eye().to_array(),
        };
        let vp = self.camera.view_proj(aspect).to_cols_array();
        let sectors = self.terrain.loaded_count();
        let fps = self.fps as f64;
        let in_world = self.phase == Phase::InWorld;
        let m = self.addon_glue.modifiers;
        self.addons.set_host(|h| {
            h.camera = cam;
            h.view_proj = vp;
            h.sectors = sectors;
            h.framerate = fps;
            h.in_world = in_world;
            h.modifiers = [m.shift_key(), m.control_key(), m.alt_key()];
        });
        if in_world && self.addons.is_loaded() {
            self.sync_addons_world();
        }
    }

    /// Entrée en jeu : chargement des AddOns (une fois), puis `PLAYER_LOGIN` et
    /// `PLAYER_ENTERING_WORLD`.
    pub(super) fn enter_world_addons(&mut self) {
        let first = !self.addons.is_loaded();
        if first {
            self.addon_glue.inventory_sig = None;
            self.sync_addons_frame();
            self.sync_addons_world();
            if let Err(e) = self.addons.load_dir(&self.config.addons_dir) {
                log::error!("AddOns : {e}");
            }
            let list = self.addons.addons();
            log::info!(
                "{} AddOn(s) chargé(s) : {}",
                list.iter().filter(|a| a.loaded).count(),
                list.iter()
                    .map(|a| format!(
                        "{}{}",
                        a.manifest.name,
                        if a.loaded { "" } else { " (erreur)" }
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            // Extensions ArchRust déjà négociées (HELLO_ACK arrive avant l'entrée en jeu).
            self.addons.set_extensions(self.addon_glue.extensions.clone());
            self.addons.fire_event("PLAYER_LOGIN", &[]);
        }
        self.addons.fire("PLAYER_ENTERING_WORLD", (first, !first));
        self.apply_addon_commands();
    }

    /// Sortie du monde : `PLAYER_LOGOUT`, sauvegarde, état Lua neuf pour la prochaine entrée.
    pub(super) fn leave_world_addons(&mut self) {
        self.music_override = None;
        self.addons.shutdown();
        match Self::new_addon_manager(&self.config) {
            Ok(m) => self.addons = m,
            Err(e) => log::error!("{e:#}"),
        }
        self.addon_glue.inventory_sig = None;
        // Déconnexion (plus de liaison réseau) : l'accord d'extensions ne vaut plus. Un `/reload`
        // le garde.
        if self.net.is_none() {
            self.addon_glue.extensions = None;
        }
    }

    /// `/reload` : comme sous WoW, `PLAYER_LOGOUT` + sauvegarde, puis rechargement complet.
    pub(super) fn reload_addons(&mut self) {
        self.leave_world_addons();
        if self.phase == Phase::InWorld {
            self.enter_world_addons();
        }
        self.ui
            .push_chat("[système] interface rechargée".to_string());
    }

    /// Fin du programme : sauvegarde des AddOns.
    pub(super) fn shutdown_addons(&mut self) {
        self.addons.shutdown();
    }

    /// Message reçu : filtres des AddOns, fenêtre de discussion, événement `CHAT_MSG_*`.
    pub(super) fn addon_chat_received(&mut self, event: &str, message: &str, from: &str) {
        let Some((msg, who)) = self.addons.filter_chat(event, message, from) else {
            return;
        };
        let line = if event == "CHAT_MSG_SYSTEM" {
            format!("[système] {msg}")
        } else {
            format!("[{}] {}", if who.is_empty() { "?" } else { &who }, msg)
        };
        self.ui.push_chat(line);
        self.addons
            .fire_event_str(event, &[message, from, "", "", from]);
    }

    /// Texte tapé dans la discussion (hors commande) : via `SendChatMessage` des AddOns si possible.
    pub(super) fn send_typed_chat(&mut self, text: String) {
        if !self.addons.send_chat(&text, "SAY") {
            self.say(text);
        }
        self.apply_addon_commands();
    }

    fn say(&mut self, message: String) {
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

    /// Exécute les actions demandées par les AddOns.
    pub(super) fn apply_addon_commands(&mut self) {
        for cmd in self.addons.drain_commands() {
            match cmd {
                HostCommand::Print(l) => self.ui.push_chat(l),
                HostCommand::Chat { message, kind, .. } => {
                    if kind != "SAY" {
                        log::debug!("canal {kind} non géré par le serveur : envoyé en SAY");
                    }
                    self.say(message);
                }
                HostCommand::PlaySound(p) => {
                    // `PlaySound(1234)` façon WoW : identifiants de kits inconnus ici.
                    if p.trim().parse::<f64>().is_ok() {
                        log::debug!("son WoW n° {p} ignoré (pas de table de kits sonores)");
                    } else {
                        self.audio.play_effect(&p);
                    }
                }
                HostCommand::PlayMusic(path) => {
                    self.music_override = path.map(|p| (!p.trim().is_empty()).then_some(p));
                }
                HostCommand::ReloadUi => self.reload_addons(),
                HostCommand::Slash(line) => {
                    if !self.addons.run_slash(&line) {
                        self.builtin_slash(&line);
                    }
                }
                HostCommand::MoveTo([x, y, z]) => {
                    self.builtin_slash(&format!("/goto {x} {y} {z}"));
                }
                HostCommand::SetCamera {
                    distance,
                    yaw,
                    pitch,
                } => {
                    if let Some(d) = distance {
                        self.camera.distance = d.clamp(100.0, 50_000.0);
                    }
                    if let Some(y) = yaw {
                        self.camera.yaw = y.to_radians();
                    }
                    if let Some(p) = pitch {
                        self.camera.pitch = p.to_radians().clamp(-1.2, 1.5);
                    }
                }
                HostCommand::Screenshot(file) => {
                    let path = file
                        .filter(|f| !f.contains("..") && !f.contains(':'))
                        .map(|f| self.config.saved_dir.join("Screenshots").join(f))
                        .unwrap_or_else(|| {
                            self.config.saved_dir.join("Screenshots").join(format!(
                                "archrust-{}.png",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0)
                            ))
                        });
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    self.addon_glue.ui.screenshot = Some(path);
                    if let Some(g) = &mut self.gpu {
                        g.renderer.capture_next = true;
                    }
                }
                HostCommand::Quit => self.exit = true,
                HostCommand::AddonSend {
                    request_id,
                    prefix,
                    channel,
                    target,
                    payload,
                } => {
                    if let Some(net) = &self.net {
                        net.send(Command::AddonSend {
                            request_id,
                            prefix,
                            channel,
                            target,
                            payload,
                        });
                    }
                }
                HostCommand::RequestTexts {
                    request_id,
                    kind,
                    ids,
                } => {
                    if let Some(net) = &self.net {
                        net.send(Command::RequestTexts {
                            request_id,
                            kind,
                            ids,
                        });
                    }
                }
            }
        }
    }

    /// CVars appliquées au rendu et à l'interface.
    pub(super) fn apply_addon_cvars(&mut self) {
        let sky = self.addons.cvar_floats("skyColor");
        let fov = self.addons.cvar_f32("cameraFov");
        let far = self.addons.cvar_f32("cameraFarClip");
        let light = self.addons.cvar_floats("lightDirection");
        let ambient = self.addons.cvar_floats("ambientColor");
        let scale = self.addons.cvar_f32("uiScale");
        // Caméra et rendu : seulement en jeu (les écrans de sélection et de création ont leur
        // propre caméra, LoginSettings.txt). L'échelle de l'interface vaut partout.
        let in_world = self.phase == Phase::InWorld;
        if let Some(f) = fov.filter(|_| in_world) {
            self.camera.fov_y = f.clamp(20.0, 120.0).to_radians();
        }
        if let Some(f) = far.filter(|_| in_world) {
            self.camera.far = f.clamp(1_000.0, 2_000_000.0);
        }
        if let Some(g) = &mut self.gpu {
            if let Some(c) = floats(&sky, 3).filter(|_| in_world) {
                g.renderer.clear_color = wgpu::Color {
                    r: c[0].clamp(0.0, 1.0) as f64,
                    g: c[1].clamp(0.0, 1.0) as f64,
                    b: c[2].clamp(0.0, 1.0) as f64,
                    a: 1.0,
                };
            }
            if let Some(l) = floats(&light, 3).filter(|_| in_world) {
                let v = Vec3::new(l[0], l[1], l[2]);
                if v.length_squared() > 1e-6 {
                    g.renderer.light_dir = v;
                }
            }
            if let Some(a) = floats(&ambient, 3).filter(|_| in_world) {
                g.renderer.ambient = [a[0], a[1], a[2], 1.0];
            }
            if let Some(s) = scale.map(|s| s.clamp(0.5, 3.0)) {
                if self.addon_glue.ui_scale != Some(s) {
                    self.addon_glue.ui_scale = Some(s);
                    g.egui_state.egui_ctx().set_zoom_factor(s);
                }
            }
        }
    }

    /// Distance de tracé des objets (CVar `objectDrawDistance`).
    pub(super) fn object_draw_distance(&self) -> f32 {
        self.addons
            .cvar_f32("objectDrawDistance")
            .unwrap_or(Self::OBJECT_DRAW_DISTANCE)
            .clamp(1_000.0, 500_000.0)
    }

    /// Après l'interface : souris et EditBox des AddOns.
    pub(super) fn after_addon_ui(&mut self, out: AddonUiOutput) {
        self.addon_glue.pointer_captured = self.addons.pointer(&out.pointer);
        for (id, ev) in out.edits {
            self.addons.edit_event(id, ev);
        }
        self.apply_addon_commands();
    }

    /// Touche (hors saisie de texte) : raccourcis et `OnKeyDown` des AddOns.
    pub(super) fn addon_key(&mut self, code: KeyCode, down: bool) -> bool {
        if self.phase != Phase::InWorld || self.addons.wants_keyboard() {
            return false;
        }
        let Some(name) = wow_key_name(code, self.addon_glue.modifiers) else {
            return false;
        };
        let handled = self.addons.key(&name, down);
        self.apply_addon_commands();
        handled
    }
}

impl App {
    /// Extensions ArchRust (paquet 0xAC) : accord du serveur monde, messages d'AddOn reçus ou
    /// refusés.
    pub(super) fn addon_world_event(&mut self, ev: WorldEvent) {
        match ev {
            WorldEvent::ExtensionsReady(e) => {
                if let Some(l) = &e.language {
                    self.ui.server_language = Some((l.clone(), e.languages.clone()));
                }
                let info = archrust_addons::ExtInfo {
                    software: e.software,
                    major: e.major,
                    minor: e.minor,
                    capabilities: e.capabilities,
                    max_payload: e.max_payload,
                    rate_burst: e.rate_burst,
                    rate_per_sec: e.rate_per_sec,
                    language: e.language,
                    languages: e.languages,
                };
                self.addon_glue.extensions = Some(info.clone());
                if self.addons.is_loaded() {
                    self.addons.set_extensions(Some(info));
                }
            }
            WorldEvent::LanguageChanged {
                request_id,
                language,
                languages,
            } => {
                self.ui.server_language = Some((language.clone(), languages.clone()));
                if request_id == crate::net::LANGUAGE_REQUEST_ID {
                    self.ui.push_chat(format!("[langue] le serveur applique {language}"));
                    self.ui.status = format!("langue : {language}");
                }
                if let Some(i) = self.addon_glue.extensions.as_mut() {
                    i.language = Some(language.clone());
                    i.languages = languages.clone();
                }
                if self.addons.is_loaded() {
                    self.addons.set_language(&language, &languages);
                    self.addons
                        .fire("ARCHCORE_LANGUAGE_CHANGED", (language, languages.join(";")));
                }
            }
            WorldEvent::Texts {
                request_id,
                language,
                kind,
                texts,
            } => {
                if self.addons.is_loaded() {
                    let map: std::collections::HashMap<u32, String> = texts.into_iter().collect();
                    self.addons
                        .fire("ARCHCORE_TEXTS", (request_id, language, kind, map));
                }
            }
            WorldEvent::AddonMessage {
                prefix,
                channel,
                target,
                sender_cid,
                sender,
                payload,
            } => {
                self.addons.addon_message(
                    &prefix,
                    channel,
                    target.as_deref(),
                    &sender,
                    sender_cid,
                    &payload,
                );
            }
            WorldEvent::AddonMessageFailed { request_id, result } => {
                self.addons.addon_message_failed(request_id, result);
            }
            _ => {}
        }
        self.apply_addon_commands();
    }
}
