//! Boucle principale : fenêtre winit, rendu, interface, AddOns et réseau.

use crate::assets::Assets;
use crate::camera::OrbitCamera;
use crate::net::{Credentials, NetEvent, NetLink};
use crate::render::{DrawItem, Mesh, Renderer};
use crate::scene::{box_mesh, entity_meshes, ground_mesh, Entity};
use crate::ui::{self, UiActions, UiState};
use crate::world::Terrain;
use anyhow::Result;
use archrust_addons::{AddonManager, HostCommand};
use archrust_netclient::{Command, WorldEvent};
use glam::{Mat4, Vec3};
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
    pub game_dir: PathBuf,
    pub addons_dir: PathBuf,
    pub patches_dir: PathBuf,
    pub saved_dir: PathBuf,
    pub title: String,
    /// Modèle à afficher au démarrage pour vérifier le rendu (ex. `a0000005.ecl`), facultatif.
    pub preview_model: Option<String>,
    /// Connexion automatique au démarrage : `utilisateur:motdepasse@hôte:port/personnage`.
    pub auto_connect: Option<String>,
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
    player: Option<Entity>,
    preview: Vec<(Arc<Mesh>, Mat4)>,
    terrain: Terrain,
    last_frame: Instant,
    fps: f32,
    right_drag: bool,
    middle_drag: bool,
    last_cursor: Option<(f64, f64)>,
    keys: [bool; 4],
    exit: bool,
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        let assets = Arc::new(Assets::new(&config.game_dir, Some(&config.patches_dir)));
        let mut addons = AddonManager::new(&config.saved_dir).map_err(|e| anyhow::anyhow!("AddOns : {e}"))?;
        addons.set_host(|h| {
            h.realm = "ArchCore".into();
            h.locale = "frFR".into();
        });
        addons.load_dir(&config.addons_dir).map_err(|e| anyhow::anyhow!("AddOns : {e}"))?;
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
            player: None,
            preview: Vec::new(),
            terrain: Terrain::default(),
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
        let attrs = Window::default_attributes().with_title(&self.config.title).with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(event_loop.create_window(attrs)?);
        let mut renderer = Renderer::new(window.clone())?;
        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(egui_ctx.clone(), egui::ViewportId::ROOT, &window, Some(window.scale_factor() as f32), None, None);
        let egui_renderer = egui_wgpu::Renderer::new(&renderer.device, renderer.config.format, None, 1, false);
        let ground = Arc::new(ground_mesh(&renderer, 200_000.0));
        let marker = Arc::new(box_mesh(&renderer, Vec3::new(40.0, 90.0, 40.0), [0.9, 0.8, 0.3, 1.0]));
        let player_marker = Arc::new(box_mesh(&renderer, Vec3::new(40.0, 90.0, 40.0), [0.3, 0.6, 1.0, 1.0]));
        if let Some(model) = &self.config.preview_model {
            match self.assets.clump("character", model) {
                Ok(clump) => {
                    self.preview = crate::scene::build_clump_meshes(&mut renderer, &self.assets, &clump);
                    log::info!("aperçu {model} : {} maillages", self.preview.len());
                }
                Err(e) => log::warn!("aperçu {model} : {e}"),
            }
        }
        self.gpu = Some(Gpu { window, renderer, egui_state, egui_renderer, ground, marker, player_marker });
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
                log::warn!("--connect : forme attendue utilisateur:motdepasse@hôte:port/personnage");
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
                NetEvent::Worlds(w) => self.ui.push_chat(format!("[système] mondes : {}", w.join(", "))),
                NetEvent::Characters(c) => {
                    for ch in c {
                        self.ui.push_chat(format!("[système] personnage {} niveau {} (cid {})", ch.name, ch.level, ch.cid));
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
                let fallback = self.gpu.as_ref().map(|g| g.player_marker.clone());
                let meshes = match (&mut self.gpu, fallback) {
                    (Some(g), Some(fb)) => entity_meshes(&mut g.renderer, &self.assets, tid, &fb).unwrap_or_default(),
                    _ => Vec::new(),
                };
                let pos = self.player.as_ref().map(|p| p.position).unwrap_or(Vec3::ZERO);
                self.player = Some(Entity { cid, tid, name: name.clone(), position: pos, destination: pos, meshes, is_player: true });
                self.addons.set_host(|h| {
                    h.player_name = name;
                    h.player_level = 1;
                });
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
                if self.entities.iter().any(|e| e.cid == c.cid) {
                    return;
                }
                let fallback = self.gpu.as_ref().map(|g| g.marker.clone());
                let meshes = match (&mut self.gpu, fallback) {
                    (Some(g), Some(fb)) if self.entities.len() < 64 => entity_meshes(&mut g.renderer, &self.assets, c.tid, &fb).unwrap_or_default(),
                    (_, Some(fb)) => vec![(fb, Mat4::IDENTITY)],
                    _ => Vec::new(),
                };
                self.entities.push(Entity { cid: c.cid, tid: c.tid, name: c.name, position: c.position, destination: c.destination, meshes, is_player: false });
            }
            WorldEvent::CharacterMoved { cid, position, destination } => {
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
            WorldEvent::Chat { from, message, channel, .. } => {
                let line = format!("[{}] {}", if from.is_empty() { "?" } else { &from }, message);
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
                self.ui.push_chat(format!("[système] {} entités en vue", self.entities.len()));
            }
            WorldEvent::Unhandled { packet_type, op } => log::debug!("paquet non géré 0x{packet_type:02x} op {op:?}"),
            WorldEvent::Disconnected(why) => {
                self.ui.push_chat(format!("[système] déconnecté : {why}"));
                self.ui.status = format!("déconnecté : {why}");
                self.phase = Phase::Login;
                self.net = None;
                self.entities.clear();
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
                self.ui.push_chat(format!("[{}] {}", self.player.as_ref().map(|p| p.name.as_str()).unwrap_or("moi"), text));
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
                        self.ui.push_chat("[système] interface rechargée".to_string());
                    }
                    Err(e) => self.ui.push_chat(format!("[erreur] {e}")),
                }
            }
            "/goto" => {
                let parts: Vec<f32> = arg.split([' ', ',']).filter_map(|s| s.trim().parse().ok()).collect();
                if parts.len() >= 3 {
                    let dest = Vec3::new(parts[0], parts[1], parts[2]);
                    if let (Some(net), Some(p)) = (&self.net, &self.player) {
                        net.send(Command::MoveTo { position: p.position, destination: dest });
                    }
                }
            }
            "/quit" => self.exit = true,
            _ => self.ui.push_chat(format!("[système] commande inconnue : {cmd}")),
        }
    }

    fn apply_addon_commands(&mut self) {
        for cmd in self.addons.drain_commands() {
            match cmd {
                HostCommand::Print(l) => self.ui.push_chat(l),
                HostCommand::Chat { message, .. } => {
                    self.ui.push_chat(format!("[{}] {}", self.player.as_ref().map(|p| p.name.as_str()).unwrap_or("moi"), message));
                    if let Some(net) = &self.net {
                        net.send(Command::Say(message));
                    }
                }
                HostCommand::PlaySound(p) => log::info!("son demandé : {p}"),
                HostCommand::ReloadUi => self.builtin_slash("/reload"),
            }
        }
    }

    fn frame(&mut self) -> Result<()> {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;
        self.fps = if dt > 0.0 { (self.fps * 0.9) + (1.0 / dt as f32) * 0.1 } else { self.fps };
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

        self.addons.set_host(|h| h.framerate = self.fps as f64);
        self.addons.update(dt);
        self.apply_addon_commands();

        let Some(gpu) = &mut self.gpu else { return Ok(()) };
        let raw_input = gpu.egui_state.take_egui_input(&gpu.window);
        let ctx = gpu.egui_state.egui_ctx().clone();
        let mut actions = UiActions::default();
        let phase = self.phase;
        let busy = phase == Phase::Connecting;
        let player_info = self.player.as_ref().map(|p| (p.name.clone(), p.position.to_array()));
        let ents: Vec<(String, [f32; 3])> = self.entities.iter().map(|e| (e.name.clone(), e.position.to_array())).collect();
        let full_output = ctx.run(raw_input, |ctx| {
            if phase != Phase::InWorld {
                ui::login_panel(ctx, &mut self.ui, busy, &mut actions);
            }
            ui::chat_panel(ctx, &mut self.ui, &mut actions);
            ui::console_panel(ctx, &mut self.ui, &mut actions);
            ui::info_panel(ctx, &mut self.ui, self.fps, player_info.as_ref().map(|(n, p)| (n.as_str(), *p)), &ents, &self.addons, self.terrain.loaded_count());
            let tree = &self.addons.shared.borrow().ui;
            ui::draw_addon_frames(ctx, tree, &self.addons, &mut actions);
        });
        gpu.egui_state.handle_platform_output(&gpu.window, full_output.platform_output);
        let paint_jobs = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [gpu.renderer.config.width, gpu.renderer.config.height], pixels_per_point: full_output.pixels_per_point };
        for (id, delta) in &full_output.textures_delta.set {
            gpu.egui_renderer.update_texture(&gpu.renderer.device, &gpu.renderer.queue, *id, delta);
        }

        // Scène 3D : terrain autour du joueur (ou de la caméra), puis entités.
        if self.phase == Phase::InWorld || self.player.is_some() {
            let center = self.player.as_ref().map(|p| p.position).unwrap_or(self.camera.target);
            self.terrain.update(&mut gpu.renderer, &self.assets, center);
        }
        let mut items: Vec<DrawItem> = self.terrain.draw_items();
        if items.is_empty() {
            items.push(DrawItem { mesh: gpu.ground.clone(), model: Mat4::from_translation(Vec3::new(self.camera.target.x, 0.0, self.camera.target.z)), tint: [1.0; 4] });
        }
        for (mesh, local) in &self.preview {
            items.push(DrawItem { mesh: mesh.clone(), model: Mat4::from_translation(self.camera.target - Vec3::Y * 100.0) * *local, tint: [1.0; 4] });
        }
        if let Some(p) = &self.player {
            for (mesh, local) in &p.meshes {
                items.push(DrawItem { mesh: mesh.clone(), model: p.model_matrix() * *local, tint: [1.0; 4] });
            }
        }
        for e in &self.entities {
            for (mesh, local) in &e.meshes {
                items.push(DrawItem { mesh: mesh.clone(), model: e.model_matrix() * *local, tint: [1.0; 4] });
            }
        }
        let view_proj = self.camera.view_proj(gpu.renderer.aspect());
        let eye = self.camera.eye();
        let egui_renderer = &mut gpu.egui_renderer;
        gpu.renderer.render(view_proj, eye, &items, |device, queue, encoder, view| {
            egui_renderer.update_buffers(device, queue, encoder, &paint_jobs, &screen);
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
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
        let consumed = if let Some(gpu) = &mut self.gpu { gpu.egui_state.on_window_event(&gpu.window, &event).consumed } else { false };
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
            WindowEvent::MouseInput { state, button, .. } if !consumed || state == ElementState::Released => match button {
                MouseButton::Right => self.right_drag = state == ElementState::Pressed,
                MouseButton::Middle => self.middle_drag = state == ElementState::Pressed,
                _ => {}
            },
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((lx, ly)) = self.last_cursor {
                    let (dx, dy) = ((position.x - lx) as f32, (position.y - ly) as f32);
                    if self.right_drag {
                        self.camera.rotate(dx, dy);
                    } else if self.middle_drag {
                        self.camera.pan(dy * self.camera.distance * 0.002, -dx * self.camera.distance * 0.002);
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
                    PhysicalKey::Code(KeyCode::Backquote) if down && event.repeat == false => self.ui.console_open = !self.ui.console_open,
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
